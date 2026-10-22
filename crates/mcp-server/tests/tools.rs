mod common;

use inspector_core::{Catalog, PathExposure};
use inspector_mcp_server::{
    InspectionService, InspectorServer, ServerPolicy,
    service::{MIN_SCAN_INTERVAL, ServiceError},
};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{CallToolResult, ErrorCode},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

async fn invoke(server: &InspectorServer, name: &str, args: Value) -> CallToolResult {
    server
        .invoke(name, args.as_object().cloned())
        .await
        .unwrap()
}

fn body(result: &CallToolResult) -> &Value {
    result.structured_content.as_ref().unwrap()
}

#[tokio::test]
async fn discovers_eight_tools_with_strict_schemas_and_annotations() {
    let (server, _, backend) = common::server(ServerPolicy::default());
    let tools = server.tools();
    assert_eq!(
        tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(),
        [
            "list_tools",
            "get_tool_info",
            "check_command",
            "get_environment_summary",
            "scan_environment",
            "list_projects",
            "get_project_environment",
            "list_project_packages"
        ]
    );
    for tool in &tools {
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
    }
    assert!(
        tools[2].input_schema["properties"]["command"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value != "cmd")
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    let package_schema = &tools[7].input_schema["properties"];
    assert_eq!(package_schema["limit"]["minimum"], 1);
    assert_eq!(package_schema["limit"]["maximum"], 200);
    assert_eq!(package_schema["offset"]["maximum"], 512);
    assert_eq!(
        package_schema["environment_id"]["enum"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let (enabled, _, _) = common::server(ServerPolicy {
        allow_version_probes: true,
        ..ServerPolicy::default()
    });
    assert_eq!(
        enabled.tools()[4]
            .annotations
            .as_ref()
            .unwrap()
            .read_only_hint,
        Some(false)
    );
}

#[tokio::test]
async fn list_details_command_and_summary_share_one_passive_snapshot() {
    let (server, _, backend) = common::server(ServerPolicy::default());
    let list = invoke(&server, "list_tools", json!({})).await;
    let details = invoke(&server, "get_tool_info", json!({"tool_id":"node"})).await;
    let command = invoke(&server, "check_command", json!({"command":"node"})).await;
    let summary = invoke(&server, "get_environment_summary", json!({})).await;
    assert_eq!(body(&list)["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        body(&list)["scanned_tool_ids"].as_array().unwrap().len(),
        18
    );
    assert_eq!(body(&details)["tool"]["tool"]["id"], "node");
    assert_eq!(body(&command)["primary_candidate_id"], "node-1");
    assert_eq!(body(&summary)["detected_tool_count"], 1);
    assert_eq!(body(&summary)["installation_count"], 1);
    for result in [&list, &details, &command, &summary] {
        assert_eq!(body(result)["completed_at"], body(&list)["completed_at"]);
        assert_eq!(body(result)["path_exposure"], "omit");
        let serialized = serde_json::to_string(result).unwrap();
        assert!(!serialized.contains("Fixture User"));
        assert!(!serialized.contains("node.exe"));
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.version_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rejects_unknown_inputs_without_scanning_or_reflecting_sensitive_values() {
    let (server, _, backend) = common::server(ServerPolicy::default());
    for (name, args, code) in [
        (
            "check_command",
            json!({"command":"cmd /c PRIVATE_TEST_VALUE"}),
            "unknown_command",
        ),
        (
            "check_command",
            json!({"command":"C:/PRIVATE_TEST_VALUE/node.exe"}),
            "unknown_command",
        ),
        (
            "get_tool_info",
            json!({"tool_id":"PRIVATE_TEST_VALUE"}),
            "unknown_tool",
        ),
        ("scan_environment", json!({"tool_ids":[]}), "invalid_scope"),
        (
            "scan_environment",
            json!({"tool_ids":["node","node"]}),
            "invalid_scope",
        ),
        (
            "scan_environment",
            json!({"tool_ids":["PRIVATE_TEST_VALUE"]}),
            "invalid_scope",
        ),
        (
            "scan_environment",
            json!({"probe_versions":true}),
            "version_probes_disabled",
        ),
    ] {
        let result = invoke(&server, name, args).await;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(body(&result)["error"]["code"], code);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("PRIVATE_TEST_VALUE")
        );
    }
    let error = server.invoke("PRIVATE_TEST_VALUE", None).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND);
    assert!(!error.message.contains("PRIVATE_TEST_VALUE"));
    for (name, args) in [
        ("list_tools", json!({"PRIVATE_TEST_VALUE":true})),
        ("check_command", json!({"command":0})),
        ("scan_environment", json!({"paths":"full"})),
    ] {
        let error = server
            .invoke(name, args.as_object().cloned())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(!error.message.contains("PRIVATE_TEST_VALUE"));
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn subset_scan_has_explicit_scope_and_does_not_claim_unscanned_tools_are_absent() {
    let (server, _, _) = common::server(ServerPolicy::default());
    let scan = invoke(&server, "scan_environment", json!({"tool_ids":["node"]})).await;
    assert_eq!(body(&scan)["scanned_tool_ids"], json!(["node"]));
    assert_eq!(body(&scan)["tools"].as_array().unwrap().len(), 1);
    let absent = invoke(&server, "get_tool_info", json!({"tool_id":"git"})).await;
    assert_eq!(body(&absent)["error"]["code"], "tool_not_scanned");
    let command = invoke(&server, "check_command", json!({"command":"git"})).await;
    assert_eq!(body(&command)["error"]["code"], "tool_not_scanned");
    let summary = invoke(&server, "get_environment_summary", json!({})).await;
    assert_eq!(body(&summary)["catalog_tool_count"], 18);
    assert_eq!(body(&summary)["scanned_tool_count"], 1);
}

#[tokio::test]
async fn versions_require_both_startup_permission_and_explicit_request() {
    for requested in [false, true] {
        let (server, _, backend) = common::server(ServerPolicy {
            allow_version_probes: true,
            ..ServerPolicy::default()
        });
        let result = invoke(
            &server,
            "scan_environment",
            json!({"tool_ids":["node"], "probe_versions":requested}),
        )
        .await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            backend.version_calls.load(Ordering::SeqCst),
            usize::from(requested)
        );
    }
}

#[tokio::test]
async fn rate_limits_refresh_but_cached_reads_continue() {
    let (server, _, backend) = common::server(ServerPolicy::default());
    invoke(&server, "list_tools", json!({})).await;
    let result = invoke(&server, "scan_environment", json!({})).await;
    assert_eq!(body(&result)["error"]["code"], "scan_rate_limited");
    assert_eq!(
        invoke(&server, "list_tools", json!({})).await.is_error,
        Some(false)
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn canceled_request_holds_the_single_scan_slot_until_worker_finishes() {
    let (server, service, backend) = common::server(ServerPolicy::default());
    backend.blocking.store(true, Ordering::SeqCst);
    let pending_service = service.clone();
    let pending = tokio::spawn(async move { pending_service.refresh(None, false).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while backend.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    let error = service.refresh(None, false).await.unwrap_err();
    assert!(matches!(error, ServiceError::Busy));
    backend.blocking.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service.snapshot().await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        invoke(&server, "list_tools", json!({})).await.is_error,
        Some(false)
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_refresh_keeps_the_previous_timestamped_snapshot() {
    let (_, service, backend) = common::server(ServerPolicy::default());
    let initial = service.snapshot().await.unwrap();
    backend.fail.store(true, Ordering::SeqCst);
    tokio::time::sleep(MIN_SCAN_INTERVAL + Duration::from_millis(10)).await;
    assert!(matches!(
        service.refresh(None, false).await,
        Err(ServiceError::ScanFailed)
    ));
    assert_eq!(*service.snapshot().await.unwrap(), *initial);
}

#[tokio::test]
async fn exposure_is_fixed_by_startup_policy_for_every_response() {
    for paths in [PathExposure::Full, PathExposure::RedactHome] {
        let (server, _, _) = common::server(ServerPolicy {
            path_only: false,
            paths,
            home: Some(PathBuf::from("C:/Fixture User")),
            allow_version_probes: false,
            projects_file: None,
        });
        let result = invoke(&server, "get_tool_info", json!({"tool_id":"node"})).await;
        let path = body(&result)["tool"]["installations"][0]["executable_path"]
            .as_str()
            .unwrap();
        assert!(path.ends_with("node.exe"));
        assert_eq!(path.contains("Fixture User"), paths == PathExposure::Full);
    }
}

#[tokio::test]
async fn official_sdk_client_reads_list_and_details() {
    let backend = Arc::new(common::MockScanner::default());
    let service = InspectionService::new(
        Catalog::bundled().unwrap(),
        backend,
        ServerPolicy::default(),
    );
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        InspectorServer::new(service)
            .serve(server_io)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });
    let client = ().serve(client_io).await.unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 8);
    let response = client
        .call_tool(
            rmcp::model::CallToolRequestParams::new("get_tool_info")
                .with_arguments(json!({"tool_id":"node"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(
        response.structured_content.unwrap()["tool"]["tool"]["id"],
        "node"
    );
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn official_sdk_supports_modern_discovery_without_legacy_initialize() {
    let (server, _, _) = common::server(ServerPolicy::default());
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_io)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });
    let client = ()
        .serve_with_lifecycle(
            client_io,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        client.peer_info().unwrap().protocol_version,
        rmcp::model::ProtocolVersion::V_2026_07_28
    );
    assert_eq!(client.list_all_tools().await.unwrap().len(), 8);
    let summary = client
        .call_tool(rmcp::model::CallToolRequestParams::new(
            "get_environment_summary",
        ))
        .await
        .unwrap();
    assert_eq!(
        summary.structured_content.unwrap()["catalog_tool_count"],
        18
    );
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), server_task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn sdk_lists_metadata_only_tools_without_claiming_a_command_is_available() {
    use inspector_core::*;
    use inspector_mcp_server::backend::{EnvironmentScanner, ScanFailure};
    struct MetadataScanner;
    impl EnvironmentScanner for MetadataScanner {
        fn scan(&self, catalog: Catalog, probes: bool) -> Result<ScanSnapshot, ScanFailure> {
            let mut data = common::MockScanner::default().scan(catalog, probes)?;
            let node = data.tools.iter_mut().find(|t| t.tool.id == "node").unwrap();
            node.installations.clear();
            node.registrations.push(Registration {
                id: "node-registration-1".into(),
                package_id: Some("OpenJS.NodeJS".into()),
                install_location: Some("C:/Fixture User/開発 tools".into()),
                sources: vec![SourceEvidence {
                    provider: ProviderKind::Winget,
                    reported_version: Some("24.1.0".into()),
                    install_method: Some(InstallMethod::Winget),
                }],
                detected_at: data.started_at,
            });
            Ok(data)
        }
    }
    let service = InspectionService::new(
        Catalog::bundled().unwrap(),
        Arc::new(MetadataScanner),
        ServerPolicy::default(),
    );
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        InspectorServer::new(service)
            .serve(server_io)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });
    let client = ().serve(client_io).await.unwrap();
    for (name, args) in [
        ("list_tools", json!({})),
        ("get_tool_info", json!({"tool_id":"node"})),
        ("check_command", json!({"command":"node"})),
        ("get_environment_summary", json!({})),
    ] {
        let result = client
            .call_tool(
                rmcp::model::CallToolRequestParams::new(name)
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        let value = result.structured_content.unwrap();
        assert!(!value.to_string().contains("Fixture User"));
        match name {
            "list_tools" => {
                assert_eq!(value["tools"].as_array().unwrap().len(), 1);
                assert_eq!(
                    value["tools"][0]["registrations"][0]["install_location"],
                    Value::Null
                );
            }
            "get_tool_info" => assert_eq!(
                value["tool"]["registrations"][0]["sources"][0]["reported_version"],
                "24.1.0"
            ),
            "check_command" => {
                assert_eq!(value["primary_candidate_id"], Value::Null);
                assert!(value["candidates"].as_array().unwrap().is_empty());
            }
            _ => {
                assert_eq!(value["installation_count"], 0);
                assert_eq!(value["registration_count"], 1);
                assert_eq!(value["detected_tool_count"], 1);
            }
        }
    }
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}
