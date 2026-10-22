use inspector_core::{Category, EvidenceClass, ScanSnapshot, Severity};
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CustomRequest, CustomResult,
        ErrorCode, ErrorData, Implementation, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
    },
    schemars::{self, JsonSchema},
    service::RequestContext,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use crate::service::{InspectionService, ServiceError};
use inspector_core::projects::{ProjectError, ProjectView};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ToolArgs {
    /// ID from the bundled tool catalog, for example node, python, or rust.
    tool_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CommandArgs {
    /// Exact known command name. Shell expressions, paths, and arguments are not accepted.
    command: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScanArgs {
    /// Optional nonempty subset of catalog tool IDs. Omit to scan the entire catalog.
    tool_ids: Option<Vec<String>>,
    /// Requires the server to have been started with --allow-version-probes.
    #[serde(default)]
    probe_versions: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectArgs {
    /// ID returned by list_projects. Filesystem paths are not accepted.
    project_id: String,
    /// Refresh only this registered project. Does not execute any process.
    #[serde(default)]
    refresh: bool,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectPackagesArgs {
    project_id: String,
    /// Environment ID returned by get_project_environment.
    environment_id: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_page_size")]
    limit: usize,
    /// Supply the previous scan_revision when paging to reject changed snapshots.
    scan_revision: Option<u64>,
}
fn default_page_size() -> usize {
    100
}

#[derive(Clone)]
pub struct InspectorServer {
    service: InspectionService,
}

impl InspectorServer {
    pub fn new(service: InspectionService) -> Self {
        Self { service }
    }

    pub fn tools(&self) -> Vec<Tool> {
        let mut package_schema = schema::<ProjectPackagesArgs>();
        package_schema["properties"]["limit"]["minimum"] = json!(1);
        package_schema["properties"]["limit"]["maximum"] = json!(200);
        package_schema["properties"]["offset"]["maximum"] = json!(512);
        package_schema["properties"]["environment_id"]["enum"] = json!([
            "python-dot-venv",
            "python-venv",
            "python-external",
            "node-npm"
        ]);
        let mut command_schema = schema::<CommandArgs>();
        command_schema["properties"]["command"]["enum"] = json!(
            self.service
                .catalog()
                .tools
                .iter()
                .flat_map(|t| t.commands.iter().map(|c| c.as_str()))
                .collect::<Vec<_>>()
        );
        vec![
            definition(
                "list_tools",
                "List detected development tools from the latest scan. Missing tools are excluded; scanned_tool_ids describes the scan scope. The first read performs a passive scan.",
                schema::<EmptyArgs>(),
                false,
                false,
            ),
            definition(
                "get_tool_info",
                "Get a known development tool's candidates, versions, sources and diagnostics from the latest scan.",
                schema::<ToolArgs>(),
                false,
                false,
            ),
            definition(
                "check_command",
                "Inspect cached resolution candidates for one known command. This never executes a command or shell expression.",
                command_schema,
                false,
                false,
            ),
            definition(
                "get_environment_summary",
                "Get scan timestamps, scope, category counts and fact/caution diagnostics from the latest scan.",
                schema::<EmptyArgs>(),
                false,
                false,
            ),
            definition(
                "scan_environment",
                "Refresh the bounded catalog scan. At most one scan runs, with a two-second cooldown. Replaces the latest scan scope. Version probes require both startup permission and probe_versions=true.",
                schema::<ScanArgs>(),
                true,
                self.service.policy().allow_version_probes,
            ),
            definition(
                "list_projects",
                "List explicitly registered projects allowed for MCP. Does not scan directories. Paths follow server startup policy.",
                schema::<EmptyArgs>(),
                false,
                false,
            ),
            definition(
                "get_project_environment",
                "Read Python venv and npm metadata for one allowed registered project. The first read scans only known metadata; refresh=true performs a bounded rescan. No scripts, interpreters, activation, or network requests are run. Returns environment IDs and package counts.",
                schema::<ProjectArgs>(),
                true,
                false,
            ),
            definition(
                "list_project_packages",
                "Read a page of observed packages for an allowed registered project environment. Declared, locked, and installed metadata are separate. Specify scan_revision for consistent pagination. Never executes project code.",
                package_schema,
                false,
                false,
            ),
        ]
    }

    /// SDKがプロトコルを担当し、ここではツール引数と業務エラーを分類する。
    pub async fn invoke(
        &self,
        name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let args = Value::Object(arguments.unwrap_or_default());
        if matches!(
            name,
            "list_projects" | "get_project_environment" | "list_project_packages"
        ) {
            return self.invoke_projects(name, args).await;
        }
        let result = match name {
            "list_tools" => {
                decode::<EmptyArgs>(args)?;
                self.list_detected().await
            }
            "get_tool_info" => {
                let args: ToolArgs = decode(args)?;
                self.tool_info(&args.tool_id).await
            }
            "check_command" => {
                let args: CommandArgs = decode(args)?;
                self.command_info(&args.command).await
            }
            "get_environment_summary" => {
                decode::<EmptyArgs>(args)?;
                self.summary().await
            }
            "scan_environment" => {
                let args: ScanArgs = decode(args)?;
                match self
                    .service
                    .refresh(args.tool_ids, args.probe_versions)
                    .await
                {
                    Ok(snapshot) => snapshot_json(&snapshot),
                    Err(error) => Err(error),
                }
            }
            _ => return Err(unsupported_method()),
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(
                json!({ "error": { "code": error.code(), "message": error.to_string() } }),
            ),
        })
    }

    async fn invoke_projects(&self, name: &str, args: Value) -> Result<CallToolResult, ErrorData> {
        let result = match name {
            "list_projects" => {
                decode::<EmptyArgs>(args)?;
                self.service
                    .projects()
                    .and_then(|service| service.list(self.service.policy().paths, true))
                    .and_then(|projects| {
                        serde_json::to_value(projects)
                            .map(|projects| json!({"projects":projects}))
                            .map_err(|_| ProjectError::ProjectScanFailed)
                    })
            }
            "get_project_environment" => {
                let args: ProjectArgs = decode(args)?;
                match self.service.projects() {
                    Ok(service) => service
                        .inspect(
                            &args.project_id,
                            self.service.policy().paths,
                            true,
                            args.refresh,
                        )
                        .await
                        .and_then(project_metadata),
                    Err(error) => Err(error),
                }
            }
            "list_project_packages" => {
                let args: ProjectPackagesArgs = decode(args)?;
                if args.limit == 0
                    || args.limit > 200
                    || args.offset > 512
                    || ![
                        "python-dot-venv",
                        "python-venv",
                        "python-external",
                        "node-npm",
                    ]
                    .contains(&args.environment_id.as_str())
                {
                    return Err(ErrorData::invalid_params(
                        "Invalid project package page",
                        None,
                    ));
                }
                match self.service.projects() {
                    Ok(service) => service
                        .inspect(&args.project_id, self.service.policy().paths, true, false)
                        .await
                        .and_then(|view| project_package_page(view, args)),
                    Err(error) => Err(error),
                }
            }
            _ => unreachable!(),
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => {
                let mut value = serde_json::to_value(error)
                    .unwrap_or_else(|_| json!({"code":"project_scan_failed"}));
                value["message"] = json!(error.to_string());
                CallToolResult::structured_error(json!({"error":value}))
            }
        })
    }

    async fn list_detected(&self) -> Result<Value, ServiceError> {
        let snapshot = self.service.snapshot().await?;
        let mut value = metadata(&snapshot)?;
        value["tools"] = serde_json::to_value(
            snapshot
                .tools
                .iter()
                .filter(|t| t.has_evidence())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| ServiceError::Unavailable)?;
        Ok(value)
    }

    async fn tool_info(&self, id: &str) -> Result<Value, ServiceError> {
        self.service.require_tool(id)?;
        let snapshot = self.service.snapshot().await?;
        let tool = snapshot
            .tools
            .iter()
            .find(|t| t.tool.id == id)
            .ok_or(ServiceError::NotScanned)?;
        let mut value = metadata(&snapshot)?;
        value["tool"] = serde_json::to_value(tool).map_err(|_| ServiceError::Unavailable)?;
        Ok(value)
    }

    async fn command_info(&self, command: &str) -> Result<Value, ServiceError> {
        let id = self.service.tool_id_for_command(command)?;
        let snapshot = self.service.snapshot().await?;
        let tool = snapshot
            .tools
            .iter()
            .find(|t| t.tool.id == id)
            .ok_or(ServiceError::NotScanned)?;
        let candidates: Vec<_> = tool
            .installations
            .iter()
            .filter(|i| i.command.as_str() == command)
            .collect();
        let mut value = metadata(&snapshot)?;
        value["tool_id"] = json!(id);
        value["command"] = json!(command);
        value["primary_candidate_id"] = json!(candidates.iter().find(|i| i.on_path).map(|i| &i.id));
        value["candidates"] =
            serde_json::to_value(candidates).map_err(|_| ServiceError::Unavailable)?;
        value["tool_diagnostics"] =
            serde_json::to_value(&tool.diagnostics).map_err(|_| ServiceError::Unavailable)?;
        Ok(value)
    }

    async fn summary(&self) -> Result<Value, ServiceError> {
        let snapshot = self.service.snapshot().await?;
        let mut value = metadata(&snapshot)?;
        let categories: Vec<_> = [Category::Runtime, Category::AiCli, Category::Development, Category::PackageManager].into_iter().map(|category| {
            let tools: Vec<_> = snapshot.tools.iter().filter(|t| t.tool.category == category).collect();
            json!({ "category": category, "scanned_tools": tools.len(), "detected_tools": tools.iter().filter(|t| t.has_evidence()).count(), "installations": tools.iter().map(|t| t.installations.len()).sum::<usize>(), "registrations": tools.iter().map(|t| t.registrations.len()).sum::<usize>() })
        }).collect();
        let diagnostics = snapshot
            .diagnostics
            .iter()
            .map(|d| (None, d))
            .chain(
                snapshot
                    .tools
                    .iter()
                    .flat_map(|t| t.diagnostics.iter().map(move |d| (Some(&t.tool.id), d))),
            )
            .collect::<Vec<_>>();
        value["catalog_tool_count"] = json!(self.service.catalog().tools.len());
        value["scanned_tool_count"] = json!(snapshot.tools.len());
        value["detected_tool_count"] =
            json!(snapshot.tools.iter().filter(|t| t.has_evidence()).count());
        value["installation_count"] = json!(
            snapshot
                .tools
                .iter()
                .map(|t| t.installations.len())
                .sum::<usize>()
        );
        value["registration_count"] = json!(
            snapshot
                .tools
                .iter()
                .map(|t| t.registrations.len())
                .sum::<usize>()
        );
        value["warning_count"] = json!(
            diagnostics
                .iter()
                .filter(|(_, d)| d.severity == Severity::Warning)
                .count()
        );
        value["error_count"] = json!(
            diagnostics
                .iter()
                .filter(|(_, d)| d.severity == Severity::Error)
                .count()
        );
        value["caution_count"] = json!(
            diagnostics
                .iter()
                .filter(|(_, d)| d.class == EvidenceClass::Caution)
                .count()
        );
        value["categories"] = json!(categories);
        value["findings"] = json!(
            diagnostics
                .into_iter()
                .map(
                    |(tool_id, diagnostic)| json!({ "tool_id": tool_id, "diagnostic": diagnostic })
                )
                .collect::<Vec<_>>()
        );
        Ok(value)
    }
}

fn project_metadata(view: ProjectView) -> Result<Value, ProjectError> {
    let mut value = serde_json::to_value(view).map_err(|_| ProjectError::ProjectScanFailed)?;
    if let Some(environments) = value["snapshot"]["environments"].as_array_mut() {
        for environment in environments {
            let count = environment["packages"].as_array().map_or(0, Vec::len);
            if let Some(object) = environment.as_object_mut() {
                object.remove("packages");
                object.insert("package_count".into(), json!(count));
            }
        }
    }
    Ok(value)
}

fn project_package_page(
    view: ProjectView,
    args: ProjectPackagesArgs,
) -> Result<Value, ProjectError> {
    if args
        .scan_revision
        .is_some_and(|revision| Some(revision) != view.scan_revision)
    {
        return Err(ProjectError::ProjectSnapshotChanged);
    }
    let snapshot = view.snapshot.ok_or(ProjectError::ProjectScanFailed)?;
    let environment = snapshot
        .environments
        .iter()
        .find(|environment| environment.id == args.environment_id)
        .ok_or(ProjectError::ProjectNotFound)?;
    if args.offset > environment.packages.len() {
        return Err(ProjectError::InvalidProjectInput);
    }
    let end = (args.offset + args.limit).min(environment.packages.len());
    let timestamp = |value: time::OffsetDateTime| {
        value
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| ProjectError::ProjectScanFailed)
    };
    Ok(json!({
        "project_id": view.project.id,
        "environment_id": environment.id,
        "scan_revision": view.scan_revision,
        "started_at": timestamp(snapshot.started_at)?,
        "completed_at": timestamp(snapshot.completed_at)?,
        "path_exposure": snapshot.path_exposure,
        "offset": args.offset,
        "total": environment.packages.len(),
        "next_offset": (end < environment.packages.len()).then_some(end),
        "packages": &environment.packages[args.offset..end],
    }))
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ErrorData> {
    // serdeの詳細エラーには入力値が含まれ得るため、外部へ返さない。
    serde_json::from_value(value).map_err(|_| {
        ErrorData::invalid_params(
            "Invalid tool arguments; follow the advertised input schema",
            None,
        )
    })
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| json!({ "type": "object", "additionalProperties": false }))
}

fn definition(
    name: &'static str,
    description: &'static str,
    schema: Value,
    scan: bool,
    allow_probes: bool,
) -> Tool {
    let annotations = ToolAnnotations::new()
        .read_only(!scan || !allow_probes)
        .destructive(false)
        .idempotent(!scan)
        .open_world(scan && allow_probes);
    Tool::new(
        name,
        description,
        schema.as_object().cloned().unwrap_or_default(),
    )
    .with_annotations(annotations)
}

fn snapshot_json(snapshot: &ScanSnapshot) -> Result<Value, ServiceError> {
    let mut value = serde_json::to_value(snapshot).map_err(|_| ServiceError::Unavailable)?;
    value["scanned_tool_ids"] = json!(
        snapshot
            .tools
            .iter()
            .map(|t| &t.tool.id)
            .collect::<Vec<_>>()
    );
    Ok(value)
}

fn metadata(snapshot: &ScanSnapshot) -> Result<Value, ServiceError> {
    let mut value = snapshot_json(snapshot)?;
    if let Some(map) = value.as_object_mut() {
        map.remove("tools");
    }
    Ok(value)
}

fn unsupported_method() -> ErrorData {
    ErrorData::new(
        ErrorCode::METHOD_NOT_FOUND,
        "Unsupported MCP tool or method",
        None,
    )
}

impl ServerHandler for InspectorServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("dev-environment-inspector", env!("CARGO_PKG_VERSION")))
            .with_instructions("Inspect local development tools through the bundled catalog and explicitly registered MCP-allowed projects. Project tools accept IDs, never arbitrary paths or commands, and only read bounded metadata without running project code. Results include scope and timestamps; not_found does not mean uninstalled. Paths are controlled at server startup. check_command never executes commands. Refresh operations may return retryable busy or cooldown errors.")
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "This server has a single tool page",
                None,
            ));
        }
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.input_responses.is_some() || request.request_state.is_some() {
            return Err(ErrorData::invalid_params(
                "This server does not accept multi-round tool arguments",
                None,
            ));
        }
        self.invoke(&request.name, request.arguments)
            .await
            .map(Into::into)
    }

    async fn on_custom_request(
        &self,
        _: CustomRequest,
        _: RequestContext<RoleServer>,
    ) -> Result<CustomResult, ErrorData> {
        Err(unsupported_method())
    }
}
