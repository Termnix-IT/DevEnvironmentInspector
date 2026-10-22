import type { ExportOutcome, PathExposure } from './contracts';

export interface ProjectSummary {
  id: string; name: string; root: string | null; extra_venv: string | null;
  mcp_allowed: boolean; revision: number; created_at: string;
}
export type ProjectFinding = 'access_denied' | 'read_failed' | 'metadata_invalid' | 'read_limit_reached' | 'scan_deadline_reached' | 'linked_path_skipped' | 'unsupported_layout' | 'interpreter_not_found' | 'system_packages_not_inspected' | 'non_version_specifier_redacted' | 'dependency_mismatch';
export type ProjectSource = 'venv_config' | 'python_distribution' | 'package_manifest' | 'npm_lock' | 'npm_installed';
export interface ProjectPackage {
  name: string;
  declarations: { kind: 'dependency' | 'development' | 'optional' | 'peer'; requirement: string | null }[];
  locked_version: string | null; installed_version: string | null;
  presence: 'metadata_present' | 'not_found' | 'unverified';
  lock_matches: boolean | null; declaration_satisfied: boolean | null; sources: ProjectSource[];
}
export interface ProjectEnvironment {
  id: string; label: string; kind: 'python_venv' | 'node_npm'; status: 'detected' | 'incomplete' | 'missing'; path: string | null;
  reported_runtime_version: string | null; declared_runtime_requirement: string | null; interpreter_present: boolean | null;
  system_site_packages: boolean | null; packages: ProjectPackage[]; findings: ProjectFinding[];
}
export interface ProjectSnapshot { schema_version: 1; started_at: string; completed_at: string; path_exposure: PathExposure; environments: ProjectEnvironment[]; findings: ProjectFinding[] }
export interface ProjectView { project: ProjectSummary; scan_revision: number | null; snapshot: ProjectSnapshot | null }
export interface FolderSelection { token: string; path: string | null }
export interface RegisterProjectRequest { name: string; root_selection: string; extra_venv_selection: string | null; mcp_allowed: boolean; paths: PathExposure }
export interface ProjectApi {
  list(paths: PathExposure): Promise<ProjectSummary[]>;
  snapshot(request: { project_id: string; paths: PathExposure }): Promise<ProjectView>;
  scan(request: { project_id: string; paths: PathExposure }): Promise<ProjectView>;
  chooseFolder(request: { kind: 'project' | 'venv'; paths: PathExposure }): Promise<FolderSelection | null>;
  register(request: RegisterProjectRequest): Promise<ProjectSummary>;
  update(request: { project_id: string; name: string; mcp_allowed: boolean; paths: PathExposure }): Promise<ProjectSummary>;
  remove(request: { project_id: string }): Promise<void>;
  export(request: { project_id: string; scan_revision: number; paths: PathExposure }): Promise<ExportOutcome>;
}
