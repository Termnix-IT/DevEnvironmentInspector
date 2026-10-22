use inspector_core::{PathExposure, project_store::JsonProjectRepository, projects::*};
use inspector_desktop::projects::{DesktopProjects, RegisterProjectRequest};
use inspector_scanner::projects::{MetadataProjectScanner, service::ProjectService};
use std::{fs, sync::Arc};

#[tokio::test]
async fn desktop_accepts_only_folder_selection_tokens_and_exports_exact_revision() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("日本語 project");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("package.json"),
        br#"{"dependencies":{"example":"^1.0.0"}}"#,
    )
    .unwrap();
    let service = ProjectService::new(
        Arc::new(JsonProjectRepository {
            path: directory.path().join("state/projects.json"),
        }),
        Arc::new(MetadataProjectScanner::default()),
        Some(directory.path().into()),
    );
    let desktop = DesktopProjects::new(Some(service.clone()), Some(directory.path().into()));
    let request = |token: String| RegisterProjectRequest {
        name: "sample".into(),
        root_selection: token,
        extra_venv_selection: None,
        mcp_allowed: false,
        paths: PathExposure::Omit,
    };
    assert_eq!(
        desktop
            .register(request(root.to_string_lossy().into()))
            .unwrap_err(),
        ProjectError::InvalidProjectInput
    );
    let selected = desktop
        .remember_selection(&root, PathExposure::Omit)
        .unwrap();
    assert!(selected.path.is_none());
    let project = desktop.register(request(selected.token.clone())).unwrap();
    assert_eq!(
        desktop.register(request(selected.token)).unwrap_err(),
        ProjectError::InvalidProjectInput
    );
    assert!(
        service
            .cached(&project.id, PathExposure::Omit, false)
            .unwrap()
            .snapshot
            .is_none()
    );
    let scanned = service
        .inspect(&project.id, PathExposure::Omit, false, true)
        .await
        .unwrap();
    assert!(
        desktop
            .export_view(&project.id, scanned.scan_revision.unwrap() + 1)
            .is_err()
    );
    let view = desktop
        .export_view(&project.id, scanned.scan_revision.unwrap())
        .unwrap();
    let output = directory.path().join("export.json");
    desktop
        .write_export(&view, &output, PathExposure::Omit)
        .unwrap();
    let data = fs::read_to_string(&output).unwrap();
    assert!(!data.contains("日本語 project"));
    let restored: ProjectView = serde_json::from_str(&data).unwrap();
    assert!(restored.project.root.is_none());
    assert_eq!(restored.scan_revision, scanned.scan_revision);
    assert!(serde_json::from_value::<RegisterProjectRequest>(serde_json::json!({"name":"sample","root_selection":"x","root":"C:/outside","extra_venv_selection":null,"mcp_allowed":false,"paths":"omit"})).is_err());
}

#[test]
fn shared_frontend_fixture_matches_rust_contract_and_export_projection() {
    let view: ProjectView =
        serde_json::from_str(include_str!("../../src/test/fixtures/projects.json")).unwrap();
    assert!(view.snapshot.as_ref().unwrap().validate());
    let omitted = view.project(PathExposure::Omit, None);
    let output = serde_json::to_string(&omitted).unwrap();
    assert!(!output.contains("<HOME>"));
    assert!(!output.contains("sample-api/node_modules"));
}
