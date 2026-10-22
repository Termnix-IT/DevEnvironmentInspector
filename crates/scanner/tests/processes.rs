#![cfg(all(windows, feature = "test-fixtures"))]

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use inspector_core::{KnownCommand, VersionSource, VersionStatus};
use inspector_scanner::{NativeVersionRunner, PathEnvironment, VersionRunner, engine::ProbeLimits};

fn fixture(mode: &str, name: &str) -> (tempfile::TempDir, PathBuf, NativeVersionRunner) {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("日本語 and spaces & percent%");
    fs::create_dir(&parent).unwrap();
    let path = parent.join(format!("{name}.exe"));
    fs::copy(env!("CARGO_BIN_EXE_dei-probe-fixture"), &path).unwrap();
    fs::write(path.with_extension("mode"), mode).unwrap();
    let runner = NativeVersionRunner::new(&PathEnvironment::new(vec![parent], Some(".EXE;.CMD")));
    (directory, path, runner)
}

fn limits() -> ProbeLimits {
    ProbeLimits {
        timeout: Duration::from_secs(2),
        output_bytes_per_stream: 16 * 1024,
    }
}

#[tokio::test]
async fn native_execution_handles_unicode_spaces_and_shell_metacharacters() {
    let (_directory, path, runner) = fixture("", "node");
    let result = runner
        .probe(KnownCommand::Node, &path, None, limits())
        .await;
    assert_eq!(result.status, VersionStatus::Success);
    assert_eq!(result.version.as_deref(), Some("1.2.3"));
    assert_eq!(result.source, Some(VersionSource::NativeExecutable));
}

#[tokio::test]
async fn nonzero_unrecognized_and_stderr_are_handled_without_raw_output() {
    for (mode, name, known, expected) in [
        (
            "nonzero",
            "node",
            KnownCommand::Node,
            VersionStatus::NonzeroExit,
        ),
        (
            "unrecognized",
            "node",
            KnownCommand::Node,
            VersionStatus::UnrecognizedOutput,
        ),
        ("stderr", "java", KnownCommand::Java, VersionStatus::Success),
    ] {
        let (_directory, path, runner) = fixture(mode, name);
        let result = runner.probe(known, &path, None, limits()).await;
        assert_eq!(result.status, expected);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("PRIVATE_OUTPUT")
        );
        if mode == "nonzero" {
            assert_eq!(result.exit_code, Some(7));
        }
    }
}

#[tokio::test]
async fn timeout_terminates_descendants_and_does_not_hang_on_inherited_pipes() {
    let (_directory, path, runner) = fixture("child", "node");
    let start = Instant::now();
    let result = runner
        .probe(
            KnownCommand::Node,
            &path,
            None,
            ProbeLimits {
                timeout: Duration::from_secs(1),
                ..limits()
            },
        )
        .await;
    assert_eq!(result.status, VersionStatus::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(4));
    let heartbeat = path.with_extension("heartbeat");
    let before = fs::read(&heartbeat).expect("子プロセスが実際に起動したことを確認");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        fs::read(&heartbeat).unwrap(),
        before,
        "終了後に子プロセスが動き続けていないこと"
    );
}

#[tokio::test]
async fn output_limit_terminates_noisy_process() {
    let (_directory, path, runner) = fixture("overflow", "node");
    let result = runner
        .probe(KnownCommand::Node, &path, None, limits())
        .await;
    assert_eq!(result.status, VersionStatus::OutputLimitExceeded);
}

#[tokio::test]
async fn npm_wrapper_uses_fixed_entrypoint_without_evaluating_shell() {
    let (_directory, node, runner) = fixture("wrapper", "node");
    let parent = node.parent().unwrap();
    let entrypoint = parent.join("node_modules/npm/bin/npm-cli.js");
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(&entrypoint, b"fixture only").unwrap();
    let wrapper = parent.join("npm.cmd");
    let sentinel = parent.join("must-not-exist");
    fs::write(&wrapper, format!("@echo off\r\necho unexpected > \"{}\"\r\n\"%_prog%\"  \"%dp0%\\node_modules\\npm\\bin\\npm-cli.js\" %*\r\n", sentinel.display())).unwrap();
    let result = runner
        .probe(KnownCommand::Npm, &wrapper, None, limits())
        .await;
    assert_eq!(result.status, VersionStatus::Success);
    assert_eq!(result.source, Some(VersionSource::NpmEntrypoint));
    assert!(!sentinel.exists());
    fs::write(&wrapper, b"@echo off\r\nnode arbitrary.js %*\r\n").unwrap();
    assert_eq!(
        runner
            .probe(KnownCommand::Npm, &wrapper, None, limits())
            .await
            .status,
        VersionStatus::UnsupportedWrapper
    );
}

#[tokio::test]
async fn mismatched_filename_and_missing_or_invalid_program_fail_safely() {
    let (_directory, path, runner) = fixture("", "node");
    assert_eq!(
        runner
            .probe(KnownCommand::Git, &path, None, limits())
            .await
            .status,
        VersionStatus::SpawnFailed
    );
    fs::write(&path, b"not a native executable").unwrap();
    assert_eq!(
        runner
            .probe(KnownCommand::Node, &path, None, limits())
            .await
            .status,
        VersionStatus::SpawnFailed
    );
}

#[test]
fn cli_json_is_private_and_environment_secrets_are_not_inherited() {
    let (directory, path, _) = fixture("environment", "node");
    let output_path = directory.path().join("scan.json");
    let output = Command::new(env!("CARGO_BIN_EXE_dei"))
        .args(["scan", "--path-only", "--probe-versions", "--output"])
        .arg(&output_path)
        .env("PATH", path.parent().unwrap())
        .env("PATHEXT", ".EXE")
        .env("DEI_TEST_SECRET", "test-only-not-a-real-secret")
        .env("NODE_OPTIONS", "test-only-not-an-option")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["tools"][0]["installations"][0]["version"]["status"],
        "success"
    );
    assert_eq!(
        value["tools"][0]["installations"][0]["executable_path"],
        serde_json::Value::Null
    );
    assert_eq!(fs::read(&output_path).unwrap(), output.stdout);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("test-only"));
    assert!(output.stderr.is_empty());
}
