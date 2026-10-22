use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use inspector_core::*;
use inspector_mcp_server::{
    InspectionService, InspectorServer, ServerPolicy,
    backend::{EnvironmentScanner, ScanFailure},
};
use time::OffsetDateTime;

#[derive(Default)]
pub struct MockScanner {
    pub calls: AtomicUsize,
    pub version_calls: AtomicUsize,
    pub blocking: AtomicBool,
    pub fail: AtomicBool,
}

impl EnvironmentScanner for MockScanner {
    fn scan(&self, catalog: Catalog, probes: bool) -> Result<ScanSnapshot, ScanFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.blocking.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return Err(ScanFailure::Failed);
            }
            thread::sleep(Duration::from_millis(5));
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(ScanFailure::Failed);
        }
        if probes {
            self.version_calls.fetch_add(1, Ordering::SeqCst);
        }
        let now = OffsetDateTime::now_utc();
        let tools = catalog
            .tools
            .into_iter()
            .map(|tool| {
                let installations = if tool.id == "node" {
                    vec![Installation {
                        id: "node-1".into(),
                        command: KnownCommand::Node,
                        executable_path: Some(PathBuf::from("C:/Fixture User/開発 tools/node.exe")),
                        on_path: true,
                        path_order: Some(0),
                        access: AccessStatus::Readable,
                        install_method: None,
                        version: if probes {
                            VersionResult {
                                status: VersionStatus::Success,
                                version: Some("1.2.3".into()),
                                exit_code: Some(0),
                                source: Some(VersionSource::NativeExecutable),
                            }
                        } else {
                            VersionResult::status(VersionStatus::NotRequested)
                        },
                        sources: vec![SourceEvidence {
                            provider: ProviderKind::Path,
                            reported_version: None,
                            install_method: None,
                        }],
                        detected_at: now,
                    }]
                } else {
                    vec![]
                };
                ToolScan {
                    status: if installations.is_empty() {
                        DetectionStatus::NotFound
                    } else {
                        DetectionStatus::Detected
                    },
                    tool,
                    installations,
                    diagnostics: vec![],
                }
            })
            .collect();
        Ok(ScanSnapshot {
            schema_version: SCHEMA_VERSION,
            target_os: TargetOs::Windows,
            started_at: now,
            completed_at: now,
            providers: vec![ProviderKind::Path],
            path_exposure: PathExposure::Full,
            tools,
            diagnostics: vec![],
        })
    }
}

#[allow(dead_code)]
pub fn server(policy: ServerPolicy) -> (InspectorServer, InspectionService, Arc<MockScanner>) {
    let backend = Arc::new(MockScanner::default());
    let service = InspectionService::new(Catalog::bundled().unwrap(), backend.clone(), policy);
    (InspectorServer::new(service.clone()), service, backend)
}
