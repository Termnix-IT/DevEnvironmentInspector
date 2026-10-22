use inspector_core::*;
use inspector_scanner::{
    RealFileSystem, ScanEngine, ScanOptions,
    engine::{ProbeFuture, ProbeLimits, VersionRunner},
    known_provider::{KnownDirectoryProvider, KnownLocation},
    metadata::MetadataFileSystem,
    npm_provider::NpmGlobalProvider,
    path_provider::{FileSystem, PathEnvironment, PathProvider},
    provider::DiscoveryProvider,
    registry_provider::*,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct RegistryFixture {
    inventory: RegistryInventory,
    calls: Arc<AtomicUsize>,
}
impl RegistrySource for RegistryFixture {
    fn read(&self) -> RegistryInventory {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inventory.clone()
    }
}
struct NoExecution;
impl VersionRunner for NoExecution {
    fn probe<'a>(
        &'a self,
        _: KnownCommand,
        _: &'a Path,
        _: Option<&'a Path>,
        _: ProbeLimits,
    ) -> ProbeFuture<'a> {
        panic!("受動スキャンでプロセスを起動しない")
    }
}
fn registry(inventory: RegistryInventory) -> (Vec<Box<dyn DiscoveryProvider>>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        RegistryProvider::providers(
            Arc::new(RegistryFixture {
                inventory,
                calls: calls.clone(),
            }),
            Arc::new(RealFileSystem),
        ),
        calls,
    )
}
async fn scan(providers: Vec<Box<dyn DiscoveryProvider>>) -> ScanSnapshot {
    let snapshot = ScanEngine::new(
        Catalog::bundled().unwrap(),
        providers,
        Box::new(NoExecution),
    )
    .scan(&ScanOptions::default())
    .await
    .unwrap();
    assert!(snapshot.validate());
    snapshot
}
fn tool<'a>(data: &'a ScanSnapshot, id: &str) -> &'a ToolScan {
    data.tools.iter().find(|t| t.tool.id == id).unwrap()
}
fn file(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn product(location: &Path) -> RegisteredProduct {
    RegisteredProduct {
        identity: "fixture-arp-key".into(),
        display_name: Some("Node.js".into()),
        version: Some("24.1.0".into()),
        install_location: Some(location.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn explicit_winget_marker_merges_with_arp_and_path_without_guessing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("開発 tools");
    file(&root.join("node.exe"), b"not executed");
    let mut app = product(&root);
    app.winget_id = Some("OpenJS.NodeJS.LTS".into());
    let (mut providers, calls) = registry(RegistryInventory {
        applications: vec![app],
        ..Default::default()
    });
    providers.insert(
        0,
        Box::new(PathProvider {
            environment: PathEnvironment::new(vec![root], Some(".EXE")),
            file_system: RealFileSystem,
        }),
    );
    let data = scan(providers).await;
    let node = tool(&data, "node");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(node.installations.len(), 1);
    assert_eq!(node.registrations.len(), 1);
    assert_eq!(node.registrations[0].sources.len(), 2);
    assert_eq!(node.installations[0].sources.len(), 3);
    assert!(node.installations[0].on_path);
    assert_eq!(
        node.installations[0].install_method,
        Some(InstallMethod::Winget)
    );
    assert_eq!(
        node.installations[0].version.status,
        VersionStatus::NotRequested
    );
    assert_eq!(
        node.registrations[0].package_id.as_deref(),
        Some("OpenJS.NodeJS.LTS")
    );
}

#[tokio::test]
async fn registered_product_without_executable_stays_metadata_only() {
    let dir = tempfile::tempdir().unwrap();
    let (providers, _) = registry(RegistryInventory {
        applications: vec![product(dir.path())],
        ..Default::default()
    });
    let data = scan(providers).await;
    let node = tool(&data, "node");
    assert_eq!(node.status, DetectionStatus::Detected);
    assert!(node.installations.is_empty());
    assert_eq!(node.registrations.len(), 1);
    assert_eq!(node.registrations[0].sources[0].install_method, None);
    assert!(
        node.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::RegisteredExecutableUnresolved
                && d.class == EvidenceClass::Caution)
    );
    assert!(
        !node
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::CliAvailable)
    );
}

#[tokio::test]
async fn matching_product_name_alone_never_proves_winget_installation() {
    let dir = tempfile::tempdir().unwrap();
    file(&dir.path().join("node.exe"), b"not executed");
    let (providers, _) = registry(RegistryInventory {
        applications: vec![product(dir.path())],
        ..Default::default()
    });
    let data = scan(providers).await;
    let node = tool(&data, "node");
    assert_eq!(node.installations[0].install_method, None);
    assert_eq!(node.installations[0].sources.len(), 1);
    assert_eq!(
        node.installations[0].sources[0].provider,
        ProviderKind::InstalledApplications
    );
}

#[tokio::test]
async fn portable_target_is_used_only_for_a_known_winget_id_and_command() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("日本語 folder/terraform.exe");
    file(&target, b"not executed");
    let app = RegisteredProduct {
        identity: "portable".into(),
        winget_id: Some("Hashicorp.Terraform".into()),
        portable_target: Some(target),
        version: Some("1.13.0".into()),
        ..Default::default()
    };
    let mut unknown = app.clone();
    unknown.winget_id = Some("unknown.package".into());
    unknown.identity = "unknown".into();
    let (providers, _) = registry(RegistryInventory {
        applications: vec![app, unknown],
        ..Default::default()
    });
    let data = scan(providers).await;
    assert_eq!(tool(&data, "terraform").installations.len(), 1);
    assert_eq!(tool(&data, "terraform").registrations.len(), 1);
}

#[tokio::test]
async fn version_difference_between_separate_installations_is_not_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old");
    let new = dir.path().join("new");
    file(&old.join("node.exe"), b"old");
    file(&new.join("node.exe"), b"new");
    let mut second = product(&new);
    second.identity = "second".into();
    second.version = Some("25.0.0".into());
    let (providers, _) = registry(RegistryInventory {
        applications: vec![product(&old), second],
        ..Default::default()
    });
    let data = scan(providers).await;
    let node = tool(&data, "node");
    assert_eq!(node.installations.len(), 2);
    assert!(
        !node
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingVersionEvidence)
    );
}

#[tokio::test]
async fn contradictions_for_the_same_executable_are_retained() {
    let dir = tempfile::tempdir().unwrap();
    file(&dir.path().join("node.exe"), b"not executed");
    let (providers, _) = registry(RegistryInventory {
        applications: vec![product(dir.path())],
        runtimes: vec![RegisteredRuntime {
            identity: "other-source".into(),
            tool_id: "node".into(),
            version: Some("25.0.0".into()),
            install_location: Some(dir.path().into()),
            executable: Some(dir.path().join("node.exe")),
        }],
        ..Default::default()
    });
    let data = scan(providers).await;
    assert_eq!(tool(&data, "node").installations.len(), 1);
    assert!(
        tool(&data, "node")
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingVersionEvidence)
    );
}

#[tokio::test]
async fn invalid_metadata_and_registry_read_failures_are_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = product(dir.path());
    app.version = Some("PRIVATE_TEST_VALUE\nsecret".into());
    let (providers, _) = registry(RegistryInventory {
        applications: vec![app],
        incomplete: true,
        ..Default::default()
    });
    let data = scan(providers).await;
    assert_eq!(tool(&data, "node").status, DetectionStatus::Incomplete);
    assert_eq!(
        tool(&data, "node").registrations[0].sources[0].reported_version,
        None
    );
    assert!(
        !serde_json::to_string(&data)
            .unwrap()
            .contains("PRIVATE_TEST_VALUE")
    );
    assert!(
        data.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ProviderFailed)
    );
}

fn npm(prefix: &Path) -> NpmGlobalProvider {
    NpmGlobalProvider {
        prefixes: vec![prefix.into(), prefix.into()],
        file_system: RealFileSystem,
    }
}
#[tokio::test]
async fn npm_reads_known_packages_and_native_bins_without_executing_shims() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("開発 global");
    let root = prefix.join("node_modules/@anthropic-ai/claude-code");
    file(&root.join("package.json"), br#"{"name":"@anthropic-ai/claude-code","version":"2.1.0","bin":{"claude":"./bin/claude.exe"}}"#);
    file(&root.join("bin/claude.exe"), b"not executable");
    file(
        &prefix.join("claude.cmd"),
        br#"@echo off
"%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe" %*
"#,
    );
    file(&prefix.join(".npmrc"), b"PRIVATE_TEST_VALUE=secret");
    let data = scan(vec![Box::new(npm(&prefix))]).await;
    let claude = tool(&data, "claude");
    assert_eq!(claude.registrations.len(), 1);
    assert_eq!(claude.installations.len(), 2);
    assert!(
        claude
            .installations
            .iter()
            .all(|i| i.install_method == Some(InstallMethod::Npm) && !i.on_path)
    );
    assert!(
        !serde_json::to_string(&data)
            .unwrap()
            .contains("PRIVATE_TEST_VALUE")
    );
}

#[tokio::test]
async fn npm_js_metadata_is_kept_when_a_wrapper_is_missing_or_unrelated() {
    let dir = tempfile::tempdir().unwrap();
    file(
        &dir.path().join("node_modules/@openai/codex/package.json"),
        br#"{"name":"@openai/codex","version":"0.159.2","bin":{"codex":"bin/codex.js"}}"#,
    );
    file(&dir.path().join("codex.cmd"), b"@echo unrelated wrapper");
    let data = scan(vec![Box::new(npm(dir.path()))]).await;
    assert_eq!(tool(&data, "codex").registrations.len(), 1);
    assert!(tool(&data, "codex").installations.is_empty());
}

#[tokio::test]
async fn npm_rejects_traversal_wrong_names_oversized_and_corrupt_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node_modules/pnpm/package.json");
    for bytes in [
        br#"{"name":"pnpm","version":"10.0.0","bin":"../../pnpm.exe"}"#.to_vec(),
        br#"{"name":"other","version":"10.0.0","bin":"pnpm.exe"}"#.to_vec(),
        br#"{"name":"pnpm","version":"PRIVATE_TEST_VALUE","bin":"pnpm.exe"}"#.to_vec(),
        b"{truncated".to_vec(),
        vec![b' '; 256 * 1024 + 1],
    ] {
        file(&path, &bytes);
        let data = scan(vec![Box::new(npm(dir.path()))]).await;
        let pnpm = tool(&data, "pnpm");
        assert_eq!(pnpm.status, DetectionStatus::Incomplete);
        assert!(pnpm.installations.is_empty());
        assert!(
            data.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::MetadataInvalid)
        );
        assert!(
            !serde_json::to_string(&data)
                .unwrap()
                .contains("PRIVATE_TEST_VALUE")
        );
    }
}

struct DeniedFs;
impl FileSystem for DeniedFs {
    fn inspect(&self, _: &Path) -> io::Result<Option<AccessStatus>> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}
impl MetadataFileSystem for DeniedFs {
    fn read_bounded(&self, _: &Path, _: usize) -> io::Result<Vec<u8>> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
    fn child_directories(&self, _: &Path, _: usize) -> io::Result<(Vec<PathBuf>, bool)> {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}
#[tokio::test]
async fn metadata_permission_failures_do_not_create_executable_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let data = scan(vec![Box::new(NpmGlobalProvider {
        prefixes: vec![dir.path().into()],
        file_system: DeniedFs,
    })])
    .await;
    assert_eq!(tool(&data, "pnpm").status, DetectionStatus::Incomplete);
    assert!(tool(&data, "pnpm").installations.is_empty());
    assert!(
        data.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AccessDenied)
    );
}

#[tokio::test]
async fn known_directories_stop_at_one_level_and_keep_install_method_unknown() {
    let dir = tempfile::tempdir().unwrap();
    file(&dir.path().join("Python314/python.exe"), b"not executed");
    file(&dir.path().join("Other/python.exe"), b"excluded");
    file(&dir.path().join("Python315/nested/python.exe"), b"excluded");
    let data = scan(vec![Box::new(KnownDirectoryProvider {
        locations: vec![KnownLocation {
            tool_id: "python".into(),
            root: dir.path().into(),
            child_prefix: Some("Python3".into()),
            executable_directory: "".into(),
        }],
        file_system: RealFileSystem,
    })])
    .await;
    assert_eq!(tool(&data, "python").installations.len(), 1);
    assert_eq!(tool(&data, "python").installations[0].install_method, None);
}

#[tokio::test]
async fn empty_metadata_sources_report_not_found_without_execution() {
    let dir = tempfile::tempdir().unwrap();
    let (mut providers, _) = registry(RegistryInventory::default());
    providers.push(Box::new(npm(dir.path())));
    let data = scan(providers).await;
    assert!(
        data.tools
            .iter()
            .all(|t| t.status == DetectionStatus::NotFound)
    );
}

#[tokio::test]
async fn registry_name_and_explicit_package_disagreement_is_not_silently_merged() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = product(dir.path());
    app.winget_id = Some("Git.Git".into());
    let (providers, _) = registry(RegistryInventory {
        applications: vec![app],
        ..Default::default()
    });
    let data = scan(providers).await;
    assert!(tool(&data, "node").registrations.is_empty());
    assert_eq!(tool(&data, "node").status, DetectionStatus::Incomplete);
    assert_eq!(tool(&data, "git").registrations.len(), 1);
}

#[tokio::test]
async fn metadata_limits_keep_partial_results_and_mark_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let applications = (0..20)
        .map(|n| {
            let mut app = product(dir.path());
            app.identity = format!("app-{n}");
            app
        })
        .collect();
    let (providers, _) = registry(RegistryInventory {
        applications,
        ..Default::default()
    });
    let data = scan(providers).await;
    let node = tool(&data, "node");
    assert_eq!(node.status, DetectionStatus::Incomplete);
    assert_eq!(node.registrations.len(), 16);
    assert!(
        node.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::MetadataLimitReached)
    );
    for n in 0..129 {
        fs::create_dir(dir.path().join(format!("Python3{n}"))).unwrap();
    }
    let provider = KnownDirectoryProvider {
        locations: vec![KnownLocation {
            tool_id: "python".into(),
            root: dir.path().into(),
            child_prefix: Some("Python3".into()),
            executable_directory: "".into(),
        }],
        file_system: RealFileSystem,
    };
    let discovery = provider.discover(&Catalog::bundled().unwrap()).unwrap();
    assert!(discovery.incomplete_tools.contains(&"python".into()));
    assert!(
        discovery
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::MetadataLimitReached)
    );
}
