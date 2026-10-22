use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use inspector_core::{KnownCommand, VersionResult, VersionSource, VersionStatus};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

use crate::{
    PathEnvironment,
    engine::{ProbeFuture, ProbeLimits, VersionRunner},
    path_provider::is_local_absolute,
    version::parse_version,
};

pub struct NativeVersionRunner {
    environment: Vec<(OsString, OsString)>,
}

impl NativeVersionRunner {
    pub fn new(path: &PathEnvironment) -> Self {
        // 認証情報、NODE_OPTIONS、JAVA_TOOL_OPTIONS等を子プロセスへ引き継がない。
        let keys = [
            "SystemRoot",
            "WINDIR",
            "SystemDrive",
            "USERPROFILE",
            "HOME",
            "LOCALAPPDATA",
            "APPDATA",
            "TEMP",
            "TMP",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "RUSTUP_HOME",
            "CARGO_HOME",
        ];
        let mut environment: Vec<_> = keys
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|v| (OsString::from(key), v)))
            .collect();
        if let Ok(value) = std::env::join_paths(&path.directories) {
            environment.push(("PATH".into(), value));
        }
        for (key, value) in [
            ("CI", "1"),
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
            ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
            ("AWS_EC2_METADATA_DISABLED", "true"),
            ("CHECKPOINT_DISABLE", "1"),
            ("UV_OFFLINE", "1"),
            ("COREPACK_ENABLE_NETWORK", "0"),
            ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0"),
            ("npm_config_offline", "true"),
            ("npm_config_update_notifier", "false"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("RUSTUP_AUTO_INSTALL", "0"),
        ] {
            environment.push((key.into(), value.into()));
        }
        Self { environment }
    }

    async fn run(
        &self,
        command: KnownCommand,
        path: &Path,
        node: Option<&Path>,
        limits: ProbeLimits,
    ) -> VersionResult {
        if limits.timeout.is_zero()
            || limits.timeout > Duration::from_secs(5)
            || limits.output_bytes_per_stream == 0
            || limits.output_bytes_per_stream > 16 * 1024
        {
            return VersionResult::status(VersionStatus::SpawnFailed);
        }
        let plan = match prepare(command, path, node) {
            Ok(plan) => plan,
            Err(status) => return VersionResult::status(status),
        };
        let directory = match tempfile::tempdir() {
            Ok(directory) => directory,
            Err(_) => return VersionResult::status(VersionStatus::SpawnFailed),
        };
        let mut process = Command::new(&plan.program);
        process
            .args(&plan.prefix_args)
            .args(command.version_args())
            .env_clear()
            .envs(self.environment.iter().cloned())
            .current_dir(directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut wrapped = CommandWrap::from(process);
        wrapped.wrap(KillOnDrop);
        #[cfg(windows)]
        {
            use process_wrap::tokio::{CreationFlags, JobObject};
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            wrapped.wrap(CreationFlags(CREATE_NO_WINDOW));
            wrapped.wrap(JobObject);
        }
        #[cfg(unix)]
        {
            wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
        }
        let mut child = match wrapped.spawn() {
            Ok(child) => child,
            Err(error) => return VersionResult::status(io_status(&error)),
        };
        let Some(stdout) = child.stdout().take() else {
            return VersionResult::status(VersionStatus::SpawnFailed);
        };
        let Some(stderr) = child.stderr().take() else {
            return VersionResult::status(VersionStatus::SpawnFailed);
        };
        let collected = timeout(limits.timeout, async {
            tokio::try_join!(
                async { child.wait().await.map_err(|_| VersionStatus::SpawnFailed) },
                read_bounded(stdout, limits.output_bytes_per_stream),
                read_bounded(stderr, limits.output_bytes_per_stream),
            )
        })
        .await;
        let result = match collected {
            Ok(Ok((status, stdout, stderr))) => {
                let version = status
                    .success()
                    .then(|| parse_version(command, &stdout, &stderr))
                    .flatten();
                VersionResult {
                    status: if !status.success() {
                        VersionStatus::NonzeroExit
                    } else if version.is_some() {
                        VersionStatus::Success
                    } else {
                        VersionStatus::UnrecognizedOutput
                    },
                    version,
                    exit_code: status.code(),
                    source: Some(plan.source),
                }
            }
            failure => {
                // kill対象はWindowsではJob Object全体。終了待ちにも上限を設ける。
                let started = child.start_kill();
                let reaped = timeout(Duration::from_secs(1), child.wait()).await;
                let status = if started.is_err() || !matches!(reaped, Ok(Ok(_))) {
                    VersionStatus::CleanupFailed
                } else {
                    match failure {
                        Err(_) => VersionStatus::TimedOut,
                        Ok(Err(status)) => status,
                        _ => unreachable!(),
                    }
                };
                VersionResult {
                    source: Some(plan.source),
                    ..VersionResult::status(status)
                }
            }
        };
        // kill-on-dropを有効にしたJob Objectを閉じ、残存子プロセスも終了する。
        drop(child);
        result
    }
}

impl VersionRunner for NativeVersionRunner {
    fn probe<'a>(
        &'a self,
        command: KnownCommand,
        path: &'a Path,
        node: Option<&'a Path>,
        limits: ProbeLimits,
    ) -> ProbeFuture<'a> {
        Box::pin(self.run(command, path, node, limits))
    }
}

struct ExecutionPlan {
    program: PathBuf,
    prefix_args: Vec<OsString>,
    source: VersionSource,
}

fn io_status(error: &io::Error) -> VersionStatus {
    if error.kind() == io::ErrorKind::PermissionDenied {
        VersionStatus::AccessDenied
    } else {
        VersionStatus::SpawnFailed
    }
}

fn prepare(
    command: KnownCommand,
    path: &Path,
    node: Option<&Path>,
) -> Result<ExecutionPlan, VersionStatus> {
    if !is_local_absolute(path) {
        return Err(VersionStatus::SpawnFailed);
    }
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(VersionStatus::SpawnFailed)?;
    if !filename.eq_ignore_ascii_case(command.as_str()) {
        return Err(VersionStatus::SpawnFailed);
    }
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com") {
        return Ok(ExecutionPlan {
            program: path.to_owned(),
            prefix_args: Vec::new(),
            source: VersionSource::NativeExecutable,
        });
    }
    if !extension.eq_ignore_ascii_case("cmd") {
        return Err(VersionStatus::UnsupportedWrapper);
    }
    let relative = command
        .npm_entrypoint()
        .ok_or(VersionStatus::UnsupportedWrapper)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| io_status(&e))?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| io_status(&e))?;
    if bytes.len() > 8192 {
        return Err(VersionStatus::UnsupportedWrapper);
    }
    let contents = std::str::from_utf8(&bytes).map_err(|_| VersionStatus::UnsupportedWrapper)?;
    let target = format!("\"%dp0%\\{}\"", relative.replace('/', "\\"));
    // シェル文を評価せず、固定のnode起動先に一致する参照だけを認識する。
    let recognized = contents.lines().any(|line| {
        line.split_once("\"%_prog%\"")
            .is_some_and(|(_, args)| args.trim() == format!("{target} %*"))
    });
    if !recognized {
        return Err(VersionStatus::UnsupportedWrapper);
    }
    let parent = path.parent().ok_or(VersionStatus::UnsupportedWrapper)?;
    let entrypoint = parent.join(relative);
    if !entrypoint.is_file() {
        return Err(VersionStatus::UnsupportedWrapper);
    }
    let adjacent_node = parent.join("node.exe");
    let runtime = if adjacent_node.is_file() {
        adjacent_node
    } else {
        node.map(Path::to_owned)
            .ok_or(VersionStatus::UnsupportedWrapper)?
    };
    if !is_local_absolute(&runtime)
        || !runtime
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("node.exe"))
    {
        return Err(VersionStatus::UnsupportedWrapper);
    }
    Ok(ExecutionPlan {
        program: runtime,
        prefix_args: vec![entrypoint.into_os_string()],
        source: VersionSource::NpmEntrypoint,
    })
}

async fn read_bounded(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, VersionStatus> {
    let mut bytes = Vec::with_capacity(limit.min(4096));
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| VersionStatus::SpawnFailed)?;
    if bytes.len() > limit {
        Err(VersionStatus::OutputLimitExceeded)
    } else {
        Ok(bytes)
    }
}
