use inspector_core::{
    PathExposure,
    project_store::{JsonProjectRepository, ProjectRepository},
    projects::*,
};
use inspector_scanner::projects::{
    MetadataProjectScanner, ProjectScanner, filesystem::*, service::ProjectService,
};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn registration(root: &Path) -> ProjectRegistration {
    ProjectRegistration {
        id: "project-1".into(),
        name: "sample".into(),
        root: root.into(),
        extra_venv: None,
        mcp_allowed: true,
        revision: 1,
        created_at: time::OffsetDateTime::now_utc(),
    }
}
fn scan(root: &Path) -> ProjectSnapshot {
    MetadataProjectScanner::default()
        .scan(&registration(root))
        .unwrap()
}

#[test]
fn missing_tools_and_dependencies_are_not_invented() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = scan(directory.path());
    assert!(snapshot.environments.is_empty());
    assert!(snapshot.validate());
}
#[test]
fn venv_reads_unicode_paths_and_distribution_headers_without_executing_python() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("日本語 tools");
    fs::create_dir(&root).unwrap();
    write(&root,".venv/pyvenv.cfg",b"home = SECRET_HOME\nversion = 3.13.7\ninclude-system-site-packages = false\ncommand = PRIVATE_COMMAND\n");
    write(&root, ".venv/Scripts/python.exe", b"not executable");
    write(
        &root,
        ".venv/Lib/site-packages/example-1.2.dist-info/METADATA",
        b"Metadata-Version: 2.4\nName: example\nVersion: 1.2\n\nPRIVATE_BODY",
    );
    write(
        &root,
        ".venv/Lib/site-packages/secret.pth",
        b"raise RuntimeError('must not execute')",
    );
    let snapshot = scan(&root);
    let environment = &snapshot.environments[0];
    assert_eq!(
        environment.reported_runtime_version.as_deref(),
        Some("3.13.7")
    );
    assert_eq!(environment.interpreter_present, Some(true));
    assert_eq!(environment.packages.len(), 1);
    assert_eq!(
        environment.packages[0].installed_version.as_deref(),
        Some("1.2")
    );
    let serialized = serde_json::to_string(&snapshot.project(PathExposure::Omit, None)).unwrap();
    for secret in [
        "PRIVATE_BODY",
        "SECRET_HOME",
        "PRIVATE_COMMAND",
        "日本語 tools",
    ] {
        assert!(!serialized.contains(secret));
    }
}
#[test]
fn malformed_and_shared_venv_data_is_incomplete() {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        "venv/pyvenv.cfg",
        b"version = 3.13.7\ninclude-system-site-packages = true",
    );
    write(
        directory.path(),
        "venv/Lib/site-packages/example-1.dist-info/METADATA",
        b"Name: example\nVersion: 1.0\nVersion: 2.0\n",
    );
    let snapshot = scan(directory.path());
    let environment = &snapshot.environments[0];
    assert_eq!(environment.status, ProjectEnvironmentStatus::Incomplete);
    assert!(
        environment
            .findings
            .contains(&ProjectFinding::SystemPackagesNotInspected)
    );
    assert!(
        environment
            .findings
            .contains(&ProjectFinding::InterpreterNotFound)
    );
    assert!(
        environment
            .findings
            .contains(&ProjectFinding::MetadataInvalid)
    );
    assert!(environment.packages.is_empty());
}
#[test]
fn npm_declarations_locks_and_placement_are_distinct_and_secrets_are_redacted() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(root,"package.json",br#"{"engines":{"node":">=22"},"dependencies":{"@example/pkg":"^1.0.0","absent":"~2.0.0","private":"https://user:SECRET@host/path?token=SECRET"},"scripts":{"postinstall":"PRIVATE_SCRIPT"}}"#);
    write(root,"package-lock.json",br#"{"lockfileVersion":3,"packages":{"node_modules/@example/pkg":{"version":"1.2.0","resolved":"SECRET_URL"},"node_modules/absent":{"version":"2.0.1"},"../escape":{"version":"8.0.0"}}}"#);
    write(
        root,
        "node_modules/@example/pkg/package.json",
        br#"{"name":"@example/pkg","version":"1.1.0","scripts":{"install":"PRIVATE_SCRIPT"}}"#,
    );
    write(
        root,
        "node_modules/indirect/package.json",
        br#"{"name":"indirect","version":"3.0.0"}"#,
    );
    write(root, "node_modules/.bin/node.cmd", b"must not execute");
    let snapshot = scan(root);
    let environment = &snapshot.environments[0];
    let package = environment
        .packages
        .iter()
        .find(|p| p.name == "@example/pkg")
        .unwrap();
    assert_eq!(package.locked_version.as_deref(), Some("1.2.0"));
    assert_eq!(package.installed_version.as_deref(), Some("1.1.0"));
    assert_eq!(package.lock_matches, Some(false));
    assert_eq!(package.declaration_satisfied, Some(true));
    assert_eq!(
        environment
            .packages
            .iter()
            .find(|p| p.name == "absent")
            .unwrap()
            .presence,
        PackagePresence::NotFound
    );
    assert!(environment.packages.iter().any(|p| p.name == "indirect"));
    assert!(
        environment
            .packages
            .iter()
            .find(|p| p.name == "private")
            .unwrap()
            .declarations[0]
            .requirement
            .is_none()
    );
    let output = serde_json::to_string(&snapshot).unwrap();
    for value in ["SECRET", "PRIVATE_SCRIPT", "escape", "node.cmd"] {
        assert!(!output.contains(value));
    }
}
#[test]
fn unsupported_lock_and_oversized_metadata_are_reported() {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        "package-lock.json",
        br#"{"lockfileVersion":1}"#,
    );
    write(directory.path(), "package.json", vec![b' '; 256 * 1024 + 1]);
    let snapshot = scan(directory.path());
    let findings = &snapshot.environments[0].findings;
    assert!(findings.contains(&ProjectFinding::ReadLimitReached));
    assert!(findings.contains(&ProjectFinding::UnsupportedLayout));
}

struct DeniedFs;
struct DeniedDirectory;
impl ProjectFileSystem for DeniedFs {
    fn open(&self, _: &Path) -> Result<Box<dyn ProjectDirectory>, ProjectFinding> {
        Ok(Box::new(DeniedDirectory))
    }
}
impl ProjectDirectory for DeniedDirectory {
    fn read(&self, _: &Path, _: usize) -> Result<Option<Vec<u8>>, ProjectFinding> {
        Err(ProjectFinding::AccessDenied)
    }
    fn directories(&self, _: &Path, _: usize) -> Result<DirectoryEntries, ProjectFinding> {
        Err(ProjectFinding::AccessDenied)
    }
    fn regular_file(&self, _: &Path) -> Result<bool, ProjectFinding> {
        Err(ProjectFinding::AccessDenied)
    }
}
#[test]
fn denied_access_remains_unverified() {
    let snapshot = MetadataProjectScanner {
        file_system: Arc::new(DeniedFs),
    }
    .scan(&registration(Path::new("C:/fixture")))
    .unwrap();
    assert!(
        snapshot
            .environments
            .iter()
            .all(|e| e.status == ProjectEnvironmentStatus::Incomplete)
    );
    assert!(
        snapshot
            .environments
            .iter()
            .all(|e| e.findings.contains(&ProjectFinding::AccessDenied))
    );
}

#[cfg(windows)]
#[test]
fn junction_cannot_read_metadata_outside_registered_project() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    write(
        &root,
        "package.json",
        br#"{"dependencies":{"outside":"1.0.0"}}"#,
    );
    write(
        &outside,
        "outside/package.json",
        br#"{"name":"outside","version":"1.0.0"}"#,
    );
    use std::os::windows::process::CommandExt;
    let powershell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output=std::process::Command::new(powershell).creation_flags(0x08000000).args(["-NoProfile","-NonInteractive","-Command","try { New-Item -ItemType Junction -Path $env:DEI_TEST_LINK -Target $env:DEI_TEST_TARGET -ErrorAction Stop | Out-Null } catch { [Console]::Error.WriteLine($_.FullyQualifiedErrorId); exit 1 }"])
        .env("DEI_TEST_LINK",root.join("node_modules")).env("DEI_TEST_TARGET",&outside).output().unwrap();
    assert!(
        output.status.success(),
        "junction fixture setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = scan(&root);
    let environment = snapshot
        .environments
        .iter()
        .find(|e| e.kind == ProjectEnvironmentKind::NodeNpm)
        .unwrap();
    assert!(environment.packages[0].installed_version.is_none());
    assert!(environment.findings.iter().any(|e| matches!(
        e,
        ProjectFinding::LinkedPathSkipped | ProjectFinding::AccessDenied
    )));
}

struct BlockingScanner {
    block: AtomicBool,
    fail: AtomicBool,
    entered: AtomicBool,
}
impl ProjectScanner for BlockingScanner {
    fn scan(&self, project: &ProjectRegistration) -> Result<ProjectSnapshot, ProjectError> {
        self.entered.store(true, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while self.block.load(Ordering::SeqCst) {
            if std::time::Instant::now() > deadline {
                return Err(ProjectError::ProjectScanFailed);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(ProjectError::ProjectScanFailed);
        }
        MetadataProjectScanner::default().scan(project)
    }
}
#[tokio::test]
async fn cache_is_private_and_permission_revocation_invalidates_inflight_reads() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    fs::create_dir(&root).unwrap();
    let repository = Arc::new(JsonProjectRepository {
        path: directory.path().join("state/projects.json"),
    });
    let scanner = Arc::new(BlockingScanner {
        block: AtomicBool::new(true),
        fail: AtomicBool::new(false),
        entered: AtomicBool::new(false),
    });
    let service = ProjectService::new(repository.clone(), scanner.clone(), None);
    let project = service
        .register("sample".into(), &root, None, true, PathExposure::Omit)
        .unwrap();
    let copy = service.clone();
    let id = project.id.clone();
    let running =
        tokio::spawn(async move { copy.inspect(&id, PathExposure::Omit, true, false).await });
    tokio::time::sleep(Duration::from_millis(40)).await;
    repository.update(&project.id, "sample", false).unwrap();
    scanner.block.store(false, Ordering::SeqCst);
    assert!(matches!(
        running.await.unwrap(),
        Err(ProjectError::ProjectNotFound)
    ));
    assert!(service.list(PathExposure::Omit, true).unwrap().is_empty());
    assert!(
        service
            .cached(&project.id, PathExposure::Omit, false)
            .unwrap()
            .snapshot
            .is_none()
    );
}

#[tokio::test]
async fn cancellation_keeps_scan_slot_and_failed_rescan_keeps_observation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    fs::create_dir(&root).unwrap();
    let repository = Arc::new(JsonProjectRepository {
        path: directory.path().join("state/projects.json"),
    });
    let scanner = Arc::new(BlockingScanner {
        block: AtomicBool::new(true),
        fail: AtomicBool::new(false),
        entered: AtomicBool::new(false),
    });
    let service = ProjectService::new(repository, scanner.clone(), None);
    let project = service
        .register("sample".into(), &root, None, false, PathExposure::Omit)
        .unwrap();
    let copy = service.clone();
    let id = project.id.clone();
    let request =
        tokio::spawn(async move { copy.inspect(&id, PathExposure::Omit, false, true).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !scanner.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    request.abort();
    assert!(matches!(
        service
            .inspect(&project.id, PathExposure::Omit, false, true)
            .await,
        Err(ProjectError::ProjectScanInProgress)
    ));
    scanner.block.store(false, Ordering::SeqCst);
    let original = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let view = service
                .cached(&project.id, PathExposure::Omit, false)
                .unwrap();
            if view.snapshot.is_some() {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        service
            .inspect(&project.id, PathExposure::Omit, false, true)
            .await,
        Err(ProjectError::ProjectScanRateLimited)
    ));
    tokio::time::sleep(Duration::from_millis(2050)).await;
    scanner.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        service
            .inspect(&project.id, PathExposure::Omit, false, true)
            .await,
        Err(ProjectError::ProjectScanFailed)
    ));
    let retained = service
        .cached(&project.id, PathExposure::Omit, false)
        .unwrap();
    assert_eq!(retained.scan_revision, original.scan_revision);
    assert_eq!(
        retained.snapshot.unwrap().completed_at,
        original.snapshot.unwrap().completed_at
    );
}
