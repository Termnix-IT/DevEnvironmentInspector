mod common;
use inspector_core::{
    Catalog, PathExposure,
    project_store::{JsonProjectRepository, ProjectRepository},
    projects::NewProject,
};
use inspector_mcp_server::{InspectionService, InspectorServer, ServerPolicy};
use inspector_scanner::projects::{MetadataProjectScanner, service::ProjectService};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
async fn call(server: &InspectorServer, name: &str, args: Value) -> Value {
    server
        .invoke(name, args.as_object().cloned())
        .await
        .unwrap()
        .structured_content
        .unwrap()
}

#[tokio::test]
async fn registered_opt_in_projects_only_and_bounded_package_pages() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("private project");
    fs::create_dir(&root).unwrap();
    write(
        &root,
        "package.json",
        br#"{"dependencies":{"alpha":"^1.0.0","beta":"2.0.0"}}"#,
    );
    write(
        &root,
        "node_modules/alpha/package.json",
        br#"{"name":"alpha","version":"1.2.0"}"#,
    );
    let repository = Arc::new(JsonProjectRepository {
        path: temporary.path().join("registry/projects.json"),
    });
    let service = ProjectService::new(
        repository.clone(),
        Arc::new(MetadataProjectScanner::default()),
        Some(temporary.path().into()),
    );
    let registration = service
        .register("sample".into(), &root, None, true, PathExposure::Omit)
        .unwrap();
    let hidden = repository
        .register(NewProject {
            name: "DO_NOT_PUBLISH".into(),
            root: temporary.path().into(),
            extra_venv: None,
            mcp_allowed: false,
        })
        .unwrap();
    let server = InspectorServer::new(InspectionService::new_with_projects(
        Catalog::bundled().unwrap(),
        Arc::new(common::MockScanner::default()),
        ServerPolicy::default(),
        Some(service),
    ));
    let list = call(&server, "list_projects", json!({})).await;
    assert_eq!(list["projects"].as_array().unwrap().len(), 1);
    assert!(list["projects"][0]["root"].is_null());
    assert!(!list.to_string().contains("DO_NOT_PUBLISH"));
    let metadata = call(
        &server,
        "get_project_environment",
        json!({"project_id":registration.id}),
    )
    .await;
    assert_eq!(metadata["snapshot"]["environments"][0]["package_count"], 2);
    assert!(
        metadata["snapshot"]["environments"][0]
            .get("packages")
            .is_none()
    );
    let revision = metadata["scan_revision"].as_u64().unwrap();
    let page=call(&server,"list_project_packages",json!({"project_id":registration.id,"environment_id":"node-npm","limit":1,"scan_revision":revision})).await;
    assert_eq!(page["total"], 2);
    assert_eq!(page["packages"].as_array().unwrap().len(), 1);
    assert_eq!(page["next_offset"], 1);
    assert_eq!(page["completed_at"], metadata["snapshot"]["completed_at"]);
    assert!(!metadata.to_string().contains("private project"));
    let changed=call(&server,"list_project_packages",json!({"project_id":registration.id,"environment_id":"node-npm","scan_revision":revision+1})).await;
    assert_eq!(changed["error"]["code"], "project_snapshot_changed");
    let denied = call(
        &server,
        "get_project_environment",
        json!({"project_id":hidden.id}),
    )
    .await;
    assert_eq!(denied["error"]["code"], "project_not_found");
    repository
        .update(&registration.id, "sample", false)
        .unwrap();
    let revoked = call(
        &server,
        "get_project_environment",
        json!({"project_id":registration.id}),
    )
    .await;
    assert_eq!(revoked["error"]["code"], "project_not_found");
    assert!(
        call(&server, "list_projects", json!({})).await["projects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for args in [
        json!({"project_id":"project-1","path":"C:/anything"}),
        json!({"command":"pip list"}),
    ] {
        assert!(
            server
                .invoke("get_project_environment", args.as_object().cloned())
                .await
                .is_err()
        );
    }
    assert!(
        server
            .invoke(
                "list_project_packages",
                json!({"project_id":"project-1","environment_id":"node-npm","limit":201})
                    .as_object()
                    .cloned()
            )
            .await
            .is_err()
    );
}
