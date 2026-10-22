use std::{
    fs,
    path::{Path, PathBuf},
};

use inspector_core::{
    storage::{JsonRepository, SnapshotRepository, StorageError},
    *,
};
use time::OffsetDateTime;

fn snapshot(path: &Path) -> ScanSnapshot {
    let now = OffsetDateTime::now_utc();
    ScanSnapshot {
        schema_version: SCHEMA_VERSION,
        target_os: TargetOs::Windows,
        started_at: now,
        completed_at: now,
        providers: vec![ProviderKind::Path],
        path_exposure: PathExposure::Full,
        tools: vec![ToolScan {
            tool: Catalog::bundled().unwrap().tools.remove(0),
            status: DetectionStatus::Detected,
            registrations: vec![],
            installations: vec![Installation {
                id: "node-1".into(),
                command: KnownCommand::Node,
                executable_path: Some(path.to_owned()),
                on_path: true,
                path_order: Some(0),
                access: AccessStatus::Readable,
                install_method: None,
                version: VersionResult::status(VersionStatus::NotRequested),
                sources: vec![SourceEvidence {
                    provider: ProviderKind::Path,
                    reported_version: None,
                    install_method: None,
                }],
                detected_at: now,
            }],
            diagnostics: vec![],
        }],
        diagnostics: vec![],
    }
}

#[test]
fn json_roundtrip_and_atomic_replacement_in_unicode_directory() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("保存 テスト");
    fs::create_dir(&parent).unwrap();
    let path = parent.join("snapshot.json");
    let repository = JsonRepository { path: &path };
    let mut expected = snapshot(&parent.join("node.exe"));
    repository
        .save(&expected, PathExposure::Full, None)
        .unwrap();
    assert_eq!(repository.load().unwrap(), expected);
    expected.tools[0].installations[0].version = VersionResult {
        status: VersionStatus::Success,
        version: Some("1.2.3".into()),
        exit_code: Some(0),
        source: Some(VersionSource::NativeExecutable),
    };
    repository
        .save(&expected, PathExposure::Full, None)
        .unwrap();
    assert_eq!(repository.load().unwrap(), expected);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
}

#[test]
fn rejects_corrupt_future_and_oversized_json_without_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    let repository = JsonRepository { path: &path };
    fs::write(&path, b"{truncated").unwrap();
    assert!(matches!(repository.load(), Err(StorageError::Json(_))));
    assert_eq!(fs::read(&path).unwrap(), b"{truncated");
    fs::write(&path, br#"{"schema_version":999}"#).unwrap();
    assert!(matches!(
        repository.load(),
        Err(StorageError::UnsupportedSchema)
    ));
    fs::write(&path, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    assert!(matches!(repository.load(), Err(StorageError::TooLarge)));
}

#[test]
fn failed_replacement_preserves_original_and_cleans_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("existing-directory");
    fs::create_dir(&target).unwrap();
    let sentinel = target.join("keep.txt");
    fs::write(&sentinel, b"original").unwrap();
    assert!(
        JsonRepository { path: &target }
            .save(&snapshot(&target), PathExposure::Omit, None)
            .is_err()
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"original");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn path_exposure_is_explicit_and_does_not_mutate_original() {
    let home = PathBuf::from(if cfg!(windows) {
        r"C:\Users\fixture"
    } else {
        "/home/fixture"
    });
    let path = home.join("開発 tools").join("node.exe");
    let original = snapshot(&path);
    let omitted = original.project(PathExposure::Omit, Some(&home));
    assert_eq!(omitted.tools[0].installations[0].executable_path, None);
    assert!(!serde_json::to_string(&omitted).unwrap().contains("fixture"));
    let redacted = original.project(PathExposure::RedactHome, Some(&home));
    assert_eq!(
        redacted.tools[0].installations[0].executable_path,
        Some(PathBuf::from("<HOME>").join("開発 tools/node.exe"))
    );
    assert_eq!(
        original.tools[0].installations[0].executable_path,
        Some(path)
    );
    assert!(
        original.project(PathExposure::RedactHome, None).tools[0].installations[0]
            .executable_path
            .is_none()
    );
    let sibling = snapshot(&home.with_file_name("fixture-other").join("node.exe"));
    assert_eq!(
        sibling.project(PathExposure::RedactHome, Some(&home)),
        ScanSnapshot {
            path_exposure: PathExposure::RedactHome,
            ..sibling
        }
    );
}

#[test]
fn invalid_schema_save_preserves_existing_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    fs::write(&path, b"existing").unwrap();
    let mut data = snapshot(&path);
    data.schema_version = 999;
    assert!(matches!(
        JsonRepository { path: &path }.save(&data, PathExposure::Omit, None),
        Err(StorageError::UnsupportedSchema)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"existing");
}

#[test]
fn rejects_structurally_valid_json_with_inconsistent_results() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    let repository = JsonRepository { path: &path };
    let mut data = snapshot(&directory.path().join("node.exe"));
    data.tools[0].status = DetectionStatus::NotFound;
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(matches!(repository.load(), Err(StorageError::InvalidData)));
    data.tools[0].status = DetectionStatus::Detected;
    data.tools[0].installations[0].version.status = VersionStatus::Success;
    assert!(matches!(
        repository.save(&data, PathExposure::Omit, None),
        Err(StorageError::InvalidData)
    ));
}

#[test]
fn migrates_schema_one_in_memory_without_rewriting_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.json");
    let data = snapshot(&dir.path().join("node.exe"));
    let mut value = serde_json::to_value(&data).unwrap();
    value["schema_version"] = 1.into();
    value["tools"][0]
        .as_object_mut()
        .unwrap()
        .remove("registrations");
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    let migrated = JsonRepository { path: &path }.load().unwrap();
    assert_eq!(migrated, data);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn metadata_only_registration_roundtrips_and_obeys_path_projection() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Fixture User");
    let mut data = snapshot(&home.join("node.exe"));
    data.tools[0].installations.clear();
    data.tools[0].registrations.push(Registration {
        id: "node-registration-1".into(),
        package_id: Some("OpenJS.NodeJS".into()),
        install_location: Some(home.join("開発 tools")),
        sources: vec![SourceEvidence {
            provider: ProviderKind::Winget,
            reported_version: Some("24.1.0".into()),
            install_method: Some(InstallMethod::Winget),
        }],
        detected_at: data.started_at,
    });
    assert!(data.validate());
    let redacted = data.project(PathExposure::RedactHome, Some(&home));
    assert_eq!(
        redacted.tools[0].registrations[0].install_location,
        Some(PathBuf::from("<HOME>/開発 tools"))
    );
    let omitted = data.project(PathExposure::Omit, None);
    assert!(omitted.validate());
    assert!(
        !serde_json::to_string(&omitted)
            .unwrap()
            .contains("Fixture User")
    );
    let path = dir.path().join("snapshot.json");
    let repo = JsonRepository { path: &path };
    repo.save(&data, PathExposure::Omit, None).unwrap();
    assert_eq!(repo.load().unwrap(), omitted);
}
