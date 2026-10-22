use inspector_core::{
    PathExposure,
    project_store::{JsonProjectRepository, ProjectRepository},
    projects::*,
};
use std::{fs, path::PathBuf};

fn new(root: PathBuf) -> NewProject {
    NewProject {
        name: "sample".into(),
        root,
        extra_venv: None,
        mcp_allowed: false,
    }
}
#[test]
fn registration_is_atomic_persistent_and_revocable_without_touching_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("開発 tools");
    fs::create_dir(&root).unwrap();
    let repository = JsonProjectRepository {
        path: temp.path().join("state/projects.json"),
    };
    assert!(repository.load().unwrap().projects.is_empty());
    assert!(!repository.path.exists());
    let project = repository.register(new(root.clone())).unwrap();
    assert!(!project.mcp_allowed);
    let updated = repository.update(&project.id, "renamed", true).unwrap();
    assert_eq!(updated.revision, 2);
    assert!(repository.load().unwrap().projects[0].mcp_allowed);
    assert_eq!(
        repository.register(new(root.clone())).unwrap_err(),
        ProjectError::ProjectAlreadyRegistered
    );
    repository.remove(&project.id).unwrap();
    assert!(root.is_dir());
    assert!(repository.load().unwrap().projects.is_empty());
    let later = repository.register(new(root)).unwrap();
    assert_ne!(project.id, later.id);
}
#[test]
fn corrupt_or_newer_registry_is_never_replaced_by_a_write() {
    let temp = tempfile::tempdir().unwrap();
    let repository = JsonProjectRepository {
        path: temp.path().join("projects.json"),
    };
    for bytes in [
        b"broken".as_slice(),
        br#"{"schema_version":999,"next_id":1,"projects":[]}"#,
    ] {
        fs::write(&repository.path, bytes).unwrap();
        assert_eq!(
            repository
                .register(new(temp.path().to_owned()))
                .unwrap_err(),
            ProjectError::ProjectStorageInvalid
        );
        assert_eq!(fs::read(&repository.path).unwrap(), bytes);
    }
}
#[test]
fn concurrent_writer_cannot_overwrite_another_transaction() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("projects.json");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(temp.path().join("projects.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let repository = JsonProjectRepository { path };
    assert_eq!(
        repository
            .register(new(temp.path().to_owned()))
            .unwrap_err(),
        ProjectError::ProjectStorageBusy
    );
}
#[test]
fn projection_does_not_leak_home_through_project_or_environment_paths() {
    let now = time::OffsetDateTime::now_utc();
    let home = PathBuf::from("C:/Fixture User");
    let registration = ProjectRegistration {
        id: "project-1".into(),
        name: "sample".into(),
        root: home.join("開発 tools"),
        extra_venv: Some(home.join("venvs/python")),
        mcp_allowed: true,
        revision: 1,
        created_at: now,
    };
    let omitted = registration.summary(PathExposure::Omit, Some(&home));
    assert!(omitted.root.is_none() && omitted.extra_venv.is_none());
    let redacted =
        serde_json::to_string(&registration.summary(PathExposure::RedactHome, Some(&home)))
            .unwrap();
    assert!(redacted.contains("<HOME>"));
    assert!(!redacted.contains("Fixture User"));
    assert!(
        registration
            .summary(PathExposure::RedactHome, None)
            .root
            .is_none()
    );
}
#[test]
fn ids_names_and_schema_are_validated() {
    assert!(!valid_project_id("../../outside"));
    assert!(!valid_project_name("line\nname"));
    let temp = tempfile::tempdir().unwrap();
    let repository = JsonProjectRepository {
        path: temp.path().join("projects.json"),
    };
    assert_eq!(
        repository.register(new("relative".into())).unwrap_err(),
        ProjectError::InvalidProjectInput
    );
    let mut registry = ProjectRegistry::default();
    registry.projects.push(ProjectRegistration {
        id: "project-99999999999999999999".into(),
        name: "sample".into(),
        root: temp.path().into(),
        extra_venv: None,
        mcp_allowed: false,
        revision: 1,
        created_at: time::OffsetDateTime::now_utc(),
    });
    fs::write(&repository.path, serde_json::to_vec(&registry).unwrap()).unwrap();
    assert_eq!(
        repository.load().unwrap_err(),
        ProjectError::ProjectStorageInvalid
    );
}
