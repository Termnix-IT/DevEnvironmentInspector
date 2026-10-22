use inspector_core::{
    storage::{JsonRepository, SnapshotRepository},
    *,
};
use inspector_desktop::service::*;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn fixture() -> ScanSnapshot {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../src/test/fixtures/snapshot.json")).unwrap();
    serde_json::from_value(value["snapshot"].clone()).unwrap()
}
#[derive(Default)]
struct MockScanner {
    calls: AtomicUsize,
    probe_calls: AtomicUsize,
    path_only_calls: AtomicUsize,
    fail: AtomicBool,
    blocking: AtomicBool,
    invalid: AtomicBool,
}
impl DesktopScanner for MockScanner {
    fn scan(&self, _: Catalog, request: ScanRequest) -> Result<ScanSnapshot, DesktopError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if request.probe_versions {
            self.probe_calls.fetch_add(1, Ordering::SeqCst);
        }
        if request.path_only {
            self.path_only_calls.fetch_add(1, Ordering::SeqCst);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.blocking.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return Err(DesktopError::ScanFailed);
            }
            thread::sleep(Duration::from_millis(5));
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(DesktopError::ScanFailed);
        }
        let mut data = fixture();
        data.path_exposure = PathExposure::Full;
        for tool in &mut data.tools {
            for installation in &mut tool.installations {
                installation.executable_path = installation.executable_path.as_ref().map(|p| {
                    PathBuf::from(p.to_string_lossy().replace("<HOME>", "C:/Fixture User"))
                });
            }
            for registration in &mut tool.registrations {
                registration.install_location = registration.install_location.as_ref().map(|p| {
                    PathBuf::from(p.to_string_lossy().replace("<HOME>", "C:/Fixture User"))
                });
            }
        }
        if self.invalid.load(Ordering::SeqCst) {
            data.schema_version = 999;
        }
        Ok(data)
    }
}
fn service() -> (DesktopService, Arc<MockScanner>) {
    let scanner = Arc::new(MockScanner::default());
    (
        DesktopService::new(
            Catalog::bundled().unwrap(),
            scanner.clone(),
            Some("C:/Fixture User".into()),
        ),
        scanner,
    )
}
fn passive() -> ScanRequest {
    ScanRequest {
        probe_versions: false,
        path_only: false,
        paths: PathExposure::RedactHome,
    }
}

#[test]
fn ui_fixture_is_a_valid_rust_snapshot_and_commands_reject_extra_fields() {
    assert!(fixture().validate());
    assert!(serde_json::from_value::<ScanRequest>(serde_json::json!({"probe_versions":false,"path_only":false,"paths":"omit","exec":"PRIVATE_TEST_VALUE"})).is_err());
    assert!(
        serde_json::from_value::<ExportRequest>(
            serde_json::json!({"revision":1,"paths":"omit","path":"PRIVATE_TEST_VALUE"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ScanRequest>(
            serde_json::json!({"probe_versions":false,"path_only":false,"paths":"unknown"})
        )
        .is_err()
    );
}

#[test]
fn initial_reads_never_trigger_a_scan_or_a_file_write() {
    let (service, scanner) = service();
    assert_eq!(service.catalog().tools.len(), 18);
    assert!(service.snapshot(PathExposure::Full).unwrap().is_none());
    assert!(matches!(
        service.begin_export(0),
        Err(DesktopError::NoSnapshot)
    ));
    assert_eq!(scanner.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn passive_scan_and_cached_projection_keep_full_paths_inside_rust() {
    let (service, scanner) = service();
    let result = service.scan(passive()).await.unwrap();
    assert_eq!(result.revision, 1);
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("Fixture User")
    );
    assert_eq!(scanner.probe_calls.load(Ordering::SeqCst), 0);
    let full = service.snapshot(PathExposure::Full).unwrap().unwrap();
    assert!(
        serde_json::to_string(&full)
            .unwrap()
            .contains("Fixture User")
    );
    let omitted = service.snapshot(PathExposure::Omit).unwrap().unwrap();
    assert!(omitted.snapshot.validate());
    assert!(
        omitted.snapshot.tools[0].installations[0]
            .executable_path
            .is_none()
    );
    assert!(
        omitted.snapshot.tools[1].registrations[0]
            .install_location
            .is_none()
    );
    assert_eq!(scanner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn explicit_probe_and_scope_options_are_forwarded_and_rate_limited() {
    let (service, scanner) = service();
    service
        .scan(ScanRequest {
            probe_versions: true,
            path_only: true,
            paths: PathExposure::Omit,
        })
        .await
        .unwrap();
    assert_eq!(scanner.probe_calls.load(Ordering::SeqCst), 1);
    assert_eq!(scanner.path_only_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        service.scan(passive()).await,
        Err(DesktopError::ScanRateLimited)
    ));
    assert_eq!(scanner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_rescan_retains_the_previous_revision_and_observation_time() {
    let (service, scanner) = service();
    let initial = service.scan(passive()).await.unwrap();
    scanner.fail.store(true, Ordering::SeqCst);
    tokio::time::sleep(MIN_SCAN_INTERVAL + Duration::from_millis(10)).await;
    assert!(matches!(
        service.scan(passive()).await,
        Err(DesktopError::ScanFailed)
    ));
    let latest = service.snapshot(PathExposure::RedactHome).unwrap().unwrap();
    assert_eq!(latest.revision, initial.revision);
    assert_eq!(latest.snapshot, initial.snapshot);
}

#[tokio::test]
async fn invalid_backend_data_is_not_cached_or_exported() {
    let (service, scanner) = service();
    scanner.invalid.store(true, Ordering::SeqCst);
    assert!(matches!(
        service.scan(passive()).await,
        Err(DesktopError::ScanFailed)
    ));
    assert!(service.snapshot(PathExposure::Full).unwrap().is_none());
}

#[tokio::test]
async fn canceled_ipc_keeps_the_scan_slot_until_the_worker_completes() {
    let (service, scanner) = service();
    scanner.blocking.store(true, Ordering::SeqCst);
    let other = service.clone();
    let request = tokio::spawn(async move { other.scan(passive()).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while scanner.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(matches!(
        service.scan(passive()).await,
        Err(DesktopError::ScanInProgress)
    ));
    scanner.blocking.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.snapshot(PathExposure::Omit).unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(scanner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn export_uses_a_frozen_snapshot_and_never_accepts_a_frontend_destination() {
    let (service, _) = service();
    service.scan(passive()).await.unwrap();
    assert!(matches!(
        service.begin_export(999),
        Err(DesktopError::StaleSnapshot)
    ));
    let (frozen, permit) = service.begin_export(1).unwrap();
    assert!(matches!(
        service.begin_export(1),
        Err(DesktopError::ExportInProgress)
    ));
    tokio::time::sleep(MIN_SCAN_INTERVAL + Duration::from_millis(10)).await;
    assert_eq!(service.scan(passive()).await.unwrap().revision, 2);
    assert!(matches!(
        service.begin_export(1),
        Err(DesktopError::ExportInProgress)
    ));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("保存 テスト.json");
    service
        .write_export(&frozen, &path, PathExposure::Omit)
        .unwrap();
    let exported = JsonRepository { path: &path }.load().unwrap();
    assert_eq!(exported.completed_at, frozen.completed_at);
    assert!(!fs::read_to_string(&path).unwrap().contains("Fixture User"));
    assert!(exported.tools[0].installations[0].executable_path.is_none());
    assert!(
        exported.tools[1].registrations[0]
            .install_location
            .is_none()
    );
    drop(permit);
    assert!(matches!(
        service.begin_export(1),
        Err(DesktopError::StaleSnapshot)
    ));
    assert!(service.begin_export(2).is_ok());
}

#[tokio::test]
async fn export_failures_are_sanitized_and_do_not_discard_the_cache() {
    let (service, _) = service();
    service.scan(passive()).await.unwrap();
    let (snapshot, _permit) = service.begin_export(1).unwrap();
    assert_eq!(
        service.write_export(
            &snapshot,
            &PathBuf::from("relative.json"),
            PathExposure::Full
        ),
        Err(DesktopError::InvalidDestination)
    );
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        service.write_export(
            &snapshot,
            &dir.path().join("output.txt"),
            PathExposure::Full
        ),
        Err(DesktopError::InvalidDestination)
    );
    let directory_target = dir.path().join("folder.json");
    fs::create_dir(&directory_target).unwrap();
    assert_eq!(
        service.write_export(&snapshot, &directory_target, PathExposure::Full),
        Err(DesktopError::ExportFailed)
    );
    assert_eq!(
        serde_json::to_string(&DesktopError::ExportFailed).unwrap(),
        r#"{"code":"export_failed"}"#
    );
    assert!(service.snapshot(PathExposure::Omit).unwrap().is_some());
}
