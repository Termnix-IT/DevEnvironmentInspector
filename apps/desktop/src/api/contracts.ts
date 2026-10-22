// Rustのserde境界（保存スキーマ2）。画面部品はTauriへ直接依存しない。
export type PathExposure = 'full' | 'redact_home' | 'omit';
export type Category = 'runtime' | 'ai_cli' | 'development' | 'package_manager';
export type ProviderKind = 'path' | 'installed_applications' | 'winget' | 'npm_global' | 'registry' | 'known_directory';
export type InstallMethod = 'winget' | 'npm' | 'installer' | 'portable';
export type DetectionStatus = 'not_found' | 'detected' | 'incomplete';
export type VersionStatus = 'not_requested' | 'success' | 'nonzero_exit' | 'timed_out' | 'access_denied' | 'spawn_failed' | 'unrecognized_output' | 'output_limit_exceeded' | 'unsupported_wrapper' | 'probe_limit_reached' | 'scan_deadline_reached' | 'unsupported_platform' | 'cleanup_failed';
export type DiagnosticCode = 'path_candidate_found' | 'multiple_command_candidates' | 'access_denied' | 'version_probe_failed' | 'cli_available' | 'availability_unverified' | 'wrapper_not_executed' | 'conflicting_version_evidence' | 'version_format_difference' | 'path_entries_skipped' | 'unsupported_path_extension' | 'candidate_limit_reached' | 'provider_failed' | 'scan_deadline_reached' | 'metadata_invalid' | 'metadata_limit_reached' | 'registered_executable_unresolved' | 'registered_tool_not_on_path' | 'conflicting_registration_evidence';

export interface ToolDefinition { id: string; name: string; category: Category; commands: string[] }
export interface Catalog { schema_version: 1; tools: ToolDefinition[] }
export interface SourceEvidence { provider: ProviderKind; reported_version: string | null; install_method: InstallMethod | null }
export interface Diagnostic { code: DiagnosticCode; class: 'fact' | 'caution'; severity: 'info' | 'warning' | 'error'; provider: ProviderKind | null; installation_id: string | null }
export interface Installation {
  id: string;
  command: string;
  executable_path: string | null;
  on_path: boolean;
  path_order: number | null;
  access: 'readable' | 'denied' | 'unknown';
  install_method: InstallMethod | null;
  version: { status: VersionStatus; version: string | null; exit_code: number | null; source: 'native_executable' | 'npm_entrypoint' | null };
  sources: SourceEvidence[];
  detected_at: string;
}
export interface Registration { id: string; package_id: string | null; install_location: string | null; sources: SourceEvidence[]; detected_at: string }
export interface ToolScan { tool: ToolDefinition; status: DetectionStatus; installations: Installation[]; registrations: Registration[]; diagnostics: Diagnostic[] }
export interface ScanSnapshot {
  schema_version: 2;
  target_os: 'windows' | 'linux' | 'macos';
  started_at: string;
  completed_at: string;
  providers: ProviderKind[];
  path_exposure: PathExposure;
  tools: ToolScan[];
  diagnostics: Diagnostic[];
}
export interface SnapshotView { revision: number; snapshot: ScanSnapshot }
export interface ScanRequest { probe_versions: boolean; path_only: boolean; paths: PathExposure }
export interface ExportRequest { revision: number; paths: PathExposure }
export type ExportOutcome = 'saved' | 'cancelled';
export interface InspectorApi {
  catalog(): Promise<Catalog>;
  snapshot(paths: PathExposure): Promise<SnapshotView | null>;
  scan(request: ScanRequest): Promise<SnapshotView>;
  export(request: ExportRequest): Promise<ExportOutcome>;
}
