use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use inspector_core::*;
use inspector_scanner::{
    engine::{ProbeFuture, ProbeLimits},
    path_provider::FileSystem,
    provider::*,
    *,
};

struct MockFs(HashMap<PathBuf, Result<Option<AccessStatus>, io::ErrorKind>>);
impl FileSystem for MockFs {
    fn inspect(&self, path: &Path) -> io::Result<Option<AccessStatus>> {
        self.0
            .get(path)
            .copied()
            .unwrap_or(Err(io::ErrorKind::NotFound))
            .map_err(io::Error::from)
    }
}

struct MockRunner {
    calls: Arc<AtomicUsize>,
    result: VersionResult,
}
impl VersionRunner for MockRunner {
    fn probe<'a>(
        &'a self,
        _: KnownCommand,
        _: &'a Path,
        _: Option<&'a Path>,
        _: ProbeLimits,
    ) -> ProbeFuture<'a> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { self.result.clone() })
    }
}

struct MockProvider {
    kind: ProviderKind,
    discovery: Discovery,
}
impl DiscoveryProvider for MockProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }
    fn discover(&self, _: &Catalog) -> Result<Discovery, ProviderFailure> {
        Ok(self.discovery.clone())
    }
}

fn runner(status: VersionStatus) -> (Box<dyn VersionRunner>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Box::new(MockRunner {
            calls: calls.clone(),
            result: VersionResult {
                status,
                version: (status == VersionStatus::Success).then(|| "1.2.3".into()),
                exit_code: Some(0),
                source: Some(VersionSource::NativeExecutable),
            },
        }),
        calls,
    )
}

fn candidate(path: PathBuf, provider: ProviderKind, reported_version: Option<&str>) -> Candidate {
    Candidate {
        tool_id: "node".into(),
        command: KnownCommand::Node,
        path,
        on_path: provider == ProviderKind::Path,
        path_order: (provider == ProviderKind::Path).then_some(0),
        access: AccessStatus::Readable,
        evidence: SourceEvidence {
            provider,
            reported_version: reported_version.map(str::to_string),
            install_method: None,
        },
    }
}

#[tokio::test]
async fn absent_tools_do_not_trigger_execution() {
    let directory = tempfile::tempdir().unwrap();
    let provider = PathProvider {
        environment: PathEnvironment::new(vec![directory.path().to_owned()], None),
        file_system: MockFs(HashMap::new()),
    };
    let (runner, calls) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions {
        probe_versions: true,
        ..ScanOptions::default()
    })
    .await
    .unwrap();
    assert_eq!(data.tools.len(), 18);
    assert!(
        data.tools
            .iter()
            .all(|t| t.status == DetectionStatus::NotFound)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn multiple_candidates_preserve_path_precedence_without_warning() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("空白 first");
    let second = directory.path().join("second");
    let provider = PathProvider {
        environment: PathEnvironment::new(
            vec![first.clone(), second.clone(), first.clone()],
            Some(".CMD;.EXE"),
        ),
        file_system: MockFs(HashMap::from([
            (first.join("node.EXE"), Ok(Some(AccessStatus::Readable))),
            (second.join("node.EXE"), Ok(Some(AccessStatus::Readable))),
            (first.join("npm.CMD"), Ok(Some(AccessStatus::Readable))),
            (first.join("npm.EXE"), Ok(Some(AccessStatus::Readable))),
        ])),
    };
    let (runner, calls) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions::default())
    .await
    .unwrap();
    let node = &data.tools[0];
    assert_eq!(node.installations.len(), 2);
    assert_eq!(
        node.installations[0].executable_path,
        Some(first.join("node.EXE"))
    );
    assert!(node.diagnostics.iter().any(
        |d| d.code == DiagnosticCode::MultipleCommandCandidates && d.severity == Severity::Info
    ));
    assert!(
        node.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Info)
    );
    let npm = data.tools.iter().find(|t| t.tool.id == "npm").unwrap();
    assert_eq!(
        npm.installations[0].executable_path,
        Some(first.join("npm.CMD"))
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn access_denied_is_recorded_without_running_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let provider = PathProvider {
        environment: PathEnvironment::new(vec![directory.path().to_owned()], None),
        file_system: MockFs(HashMap::from([(
            directory.path().join("node.EXE"),
            Ok(Some(AccessStatus::Denied)),
        )])),
    };
    let (runner, calls) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions {
        probe_versions: true,
        ..ScanOptions::default()
    })
    .await
    .unwrap();
    assert_eq!(data.tools[0].installations[0].access, AccessStatus::Denied);
    assert_eq!(
        data.tools[0].installations[0].version.status,
        VersionStatus::AccessDenied
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn merges_evidence_and_reports_only_same_installation_version_conflicts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.exe");
    let providers: Vec<Box<dyn DiscoveryProvider>> = vec![
        Box::new(MockProvider {
            kind: ProviderKind::Path,
            discovery: Discovery {
                candidates: vec![candidate(path.clone(), ProviderKind::Path, None)],
                ..Discovery::default()
            },
        }),
        Box::new(MockProvider {
            kind: ProviderKind::Registry,
            discovery: Discovery {
                candidates: vec![candidate(path, ProviderKind::Registry, Some("9.0.0"))],
                ..Discovery::default()
            },
        }),
    ];
    let (runner, _) = runner(VersionStatus::Success);
    let data = ScanEngine::new(Catalog::bundled().unwrap(), providers, runner)
        .scan(&ScanOptions {
            probe_versions: true,
            ..ScanOptions::default()
        })
        .await
        .unwrap();
    let node = &data.tools[0];
    assert_eq!(node.installations.len(), 1);
    assert_eq!(node.installations[0].sources.len(), 2);
    assert_eq!(node.installations[0].install_method, None);
    assert!(
        node.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingVersionEvidence
                && d.class == EvidenceClass::Fact)
    );
}

#[tokio::test]
async fn shadowed_success_does_not_mark_unusable_primary_as_available() {
    let directory = tempfile::tempdir().unwrap();
    let mut primary = candidate(
        directory.path().join("first/node.exe"),
        ProviderKind::Path,
        None,
    );
    primary.access = AccessStatus::Denied;
    let mut secondary = candidate(
        directory.path().join("second/node.exe"),
        ProviderKind::Path,
        None,
    );
    secondary.path_order = Some(4);
    let provider = MockProvider {
        kind: ProviderKind::Path,
        discovery: Discovery {
            candidates: vec![secondary, primary],
            ..Discovery::default()
        },
    };
    let (runner, _) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions {
        probe_versions: true,
        ..ScanOptions::default()
    })
    .await
    .unwrap();
    assert!(
        !data.tools[0]
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::CliAvailable)
    );
}

#[tokio::test]
async fn caps_candidates_and_version_probes_and_rejects_unbounded_options() {
    let directory = tempfile::tempdir().unwrap();
    let candidates = (0..20)
        .map(|i| {
            let mut c = candidate(
                directory.path().join(format!("{i}/node.exe")),
                ProviderKind::Path,
                None,
            );
            c.path_order = Some(i);
            c
        })
        .collect();
    let provider = MockProvider {
        kind: ProviderKind::Path,
        discovery: Discovery {
            candidates,
            ..Discovery::default()
        },
    };
    let (runner, calls) = runner(VersionStatus::TimedOut);
    let engine = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    );
    let data = engine
        .scan(&ScanOptions {
            probe_versions: true,
            max_version_probes: 1,
            max_candidates_per_tool: 3,
            ..ScanOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(data.tools[0].installations.len(), 3);
    assert_eq!(data.tools[0].status, DetectionStatus::Incomplete);
    assert_eq!(
        data.tools[0].installations[1].version.status,
        VersionStatus::ProbeLimitReached
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        engine
            .scan(&ScanOptions {
                max_version_probes: 999,
                ..ScanOptions::default()
            })
            .await
            .is_err()
    );
}

#[test]
fn relative_empty_network_and_excess_path_entries_are_skipped() {
    let directory = tempfile::tempdir().unwrap();
    let mut paths = vec![
        PathBuf::new(),
        PathBuf::from("relative"),
        PathBuf::from(r"\\server\share"),
    ];
    paths.extend((0..300).map(|i| directory.path().join(i.to_string())));
    let env = PathEnvironment::new(paths, Some(".EXE;.PS1;.exe"));
    assert!(env.directories.len() <= 256);
    assert_eq!(env.extensions, [".EXE"]);
    assert!(
        env.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::PathEntriesSkipped)
    );
    assert!(
        env.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnsupportedPathExtension)
    );
}

#[tokio::test]
async fn inaccessible_metadata_does_not_invent_an_installation() {
    let directory = tempfile::tempdir().unwrap();
    let provider = PathProvider {
        environment: PathEnvironment::new(vec![directory.path().to_owned()], Some(".EXE")),
        file_system: MockFs(HashMap::from([(
            directory.path().join("node.EXE"),
            Err(io::ErrorKind::PermissionDenied),
        )])),
    };
    let (runner, calls) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions::default())
    .await
    .unwrap();
    assert_eq!(data.tools[0].status, DetectionStatus::Incomplete);
    assert!(data.tools[0].installations.is_empty());
    assert!(
        data.tools
            .iter()
            .skip(1)
            .all(|tool| tool.status == DetectionStatus::NotFound)
    );
    assert!(
        data.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AccessDenied)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

struct SlowRunner;
impl VersionRunner for SlowRunner {
    fn probe<'a>(
        &'a self,
        _: KnownCommand,
        _: &'a Path,
        _: Option<&'a Path>,
        limits: ProbeLimits,
    ) -> ProbeFuture<'a> {
        Box::pin(async move {
            tokio::time::sleep(limits.timeout).await;
            VersionResult::status(VersionStatus::TimedOut)
        })
    }
}

#[tokio::test]
async fn total_deadline_prevents_further_probes() {
    let directory = tempfile::tempdir().unwrap();
    let provider = MockProvider {
        kind: ProviderKind::Path,
        discovery: Discovery {
            candidates: vec![
                candidate(
                    directory.path().join("a/node.exe"),
                    ProviderKind::Path,
                    None,
                ),
                candidate(
                    directory.path().join("b/node.exe"),
                    ProviderKind::Path,
                    None,
                ),
            ],
            ..Discovery::default()
        },
    };
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        Box::new(SlowRunner),
    )
    .scan(&ScanOptions {
        probe_versions: true,
        scan_timeout: std::time::Duration::from_millis(40),
        ..ScanOptions::default()
    })
    .await
    .unwrap();
    assert_eq!(
        data.tools[0].installations[0].version.status,
        VersionStatus::TimedOut
    );
    assert_eq!(
        data.tools[0].installations[1].version.status,
        VersionStatus::ScanDeadlineReached
    );
}

#[tokio::test]
async fn multiple_installation_versions_are_not_a_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let provider = MockProvider {
        kind: ProviderKind::Registry,
        discovery: Discovery {
            candidates: vec![
                candidate(
                    directory.path().join("a/node.exe"),
                    ProviderKind::Registry,
                    Some("20.0.0"),
                ),
                candidate(
                    directory.path().join("b/node.exe"),
                    ProviderKind::Registry,
                    Some("22.0.0"),
                ),
            ],
            ..Discovery::default()
        },
    };
    let (runner, _) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(provider)],
        runner,
    )
    .scan(&ScanOptions::default())
    .await
    .unwrap();
    assert_eq!(data.tools[0].installations.len(), 2);
    assert!(
        !data.tools[0]
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingVersionEvidence)
    );
}

struct BrokenProvider;
impl DiscoveryProvider for BrokenProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Registry
    }
    fn discover(&self, _: &Catalog) -> Result<Discovery, ProviderFailure> {
        Err(ProviderFailure)
    }
}

#[tokio::test]
async fn provider_failure_does_not_claim_tools_are_absent() {
    let (runner, _) = runner(VersionStatus::Success);
    let data = ScanEngine::new(
        Catalog::bundled().unwrap(),
        vec![Box::new(BrokenProvider)],
        runner,
    )
    .scan(&ScanOptions::default())
    .await
    .unwrap();
    assert!(
        data.tools
            .iter()
            .all(|t| t.status == DetectionStatus::Incomplete)
    );
    assert!(
        data.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ProviderFailed
                && d.provider == Some(ProviderKind::Registry))
    );
}
