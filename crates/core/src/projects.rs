//! 登録プロジェクトの情報。PC全体のツールスナップショットとは独立したスキーマ。
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

use crate::{PathExposure, model::redact_home};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROJECTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ProjectError {
    #[error("プロジェクトの登録情報を読み書きできませんでした")]
    ProjectStorageUnavailable,
    #[error("プロジェクトの登録情報が破損しているか未対応です。既存データは保持しました")]
    ProjectStorageInvalid,
    #[error("別の登録操作を処理中です。再度お試しください")]
    ProjectStorageBusy,
    #[error("プロジェクトの指定が不正です")]
    InvalidProjectInput,
    #[error("選択したフォルダーを使用できません。ローカルの通常のフォルダーを指定してください")]
    InvalidProjectFolder,
    #[error("このフォルダーは既に登録されています")]
    ProjectAlreadyRegistered,
    #[error("登録できるプロジェクト数の上限に到達しました")]
    ProjectLimitReached,
    #[error("登録された参照可能なプロジェクトが見つかりません")]
    ProjectNotFound,
    #[error("別のプロジェクトスキャンを処理中です")]
    ProjectScanInProgress,
    #[error("スキャン完了から2秒以上待って再試行してください")]
    ProjectScanRateLimited,
    #[error("プロジェクトスキャンを完了できませんでした。前回結果は保持しています")]
    ProjectScanFailed,
    #[error("登録情報またはスキャン結果が変更されました。再取得してください")]
    ProjectSnapshotChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRegistration {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub extra_venv: Option<PathBuf>,
    pub mcp_allowed: bool,
    pub revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub struct NewProject {
    pub name: String,
    pub root: PathBuf,
    pub extra_venv: Option<PathBuf>,
    pub mcp_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRegistry {
    pub schema_version: u32,
    pub next_id: u64,
    pub projects: Vec<ProjectRegistration>,
}
impl Default for ProjectRegistry {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            next_id: 1,
            projects: Vec::new(),
        }
    }
}

pub fn valid_project_id(id: &str) -> bool {
    id.strip_prefix("project-")
        .is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
}
pub fn valid_project_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name == name.trim()
        && name.len() <= 240
        && !name.chars().any(char::is_control)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub root: Option<PathBuf>,
    pub extra_venv: Option<PathBuf>,
    pub mcp_allowed: bool,
    pub revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub fn expose_project_path(
    path: &Path,
    exposure: PathExposure,
    home: Option<&Path>,
) -> Option<PathBuf> {
    match exposure {
        PathExposure::Full => Some(path.to_owned()),
        PathExposure::Omit => None,
        PathExposure::RedactHome => redact_home(path, home),
    }
}
impl ProjectRegistration {
    pub fn summary(&self, exposure: PathExposure, home: Option<&Path>) -> ProjectSummary {
        ProjectSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            root: expose_project_path(&self.root, exposure, home),
            extra_venv: self
                .extra_venv
                .as_deref()
                .and_then(|p| expose_project_path(p, exposure, home)),
            mcp_allowed: self.mcp_allowed,
            revision: self.revision,
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectEnvironmentKind {
    PythonVenv,
    NodeNpm,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectEnvironmentStatus {
    Detected,
    Incomplete,
    Missing,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSource {
    VenvConfig,
    PythonDistribution,
    PackageManifest,
    NpmLock,
    NpmInstalled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackagePresence {
    MetadataPresent,
    NotFound,
    Unverified,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Dependency,
    Development,
    Optional,
    Peer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyDeclaration {
    pub kind: DependencyKind,
    /// バージョン範囲以外（URL・パス等）は反射せずnullにする。
    pub requirement: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPackage {
    pub name: String,
    pub declarations: Vec<DependencyDeclaration>,
    pub locked_version: Option<String>,
    pub installed_version: Option<String>,
    pub presence: PackagePresence,
    pub lock_matches: Option<bool>,
    pub declaration_satisfied: Option<bool>,
    pub sources: Vec<ProjectSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFinding {
    AccessDenied,
    ReadFailed,
    MetadataInvalid,
    ReadLimitReached,
    ScanDeadlineReached,
    LinkedPathSkipped,
    UnsupportedLayout,
    InterpreterNotFound,
    SystemPackagesNotInspected,
    NonVersionSpecifierRedacted,
    DependencyMismatch,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEnvironment {
    pub id: String,
    pub label: String,
    pub kind: ProjectEnvironmentKind,
    pub status: ProjectEnvironmentStatus,
    pub path: Option<PathBuf>,
    pub reported_runtime_version: Option<String>,
    pub declared_runtime_requirement: Option<String>,
    pub interpreter_present: Option<bool>,
    pub system_site_packages: Option<bool>,
    pub packages: Vec<ProjectPackage>,
    pub findings: Vec<ProjectFinding>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub schema_version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,
    pub path_exposure: PathExposure,
    pub environments: Vec<ProjectEnvironment>,
    pub findings: Vec<ProjectFinding>,
}
impl ProjectSnapshot {
    pub fn validate(&self) -> bool {
        self.schema_version == PROJECT_SCHEMA_VERSION
            && self.completed_at >= self.started_at
            && self.environments.len() <= 4
            && self.findings.len() <= 16
            && self.environments.iter().all(|environment| {
                [
                    "python-dot-venv",
                    "python-venv",
                    "python-external",
                    "node-npm",
                ]
                .contains(&environment.id.as_str())
                    && environment.packages.len() <= 512
                    && environment.findings.len() <= 16
                    && environment
                        .packages
                        .iter()
                        .all(|p| p.name.len() <= 214 && !p.name.is_empty())
            })
    }
    pub fn project(&self, exposure: PathExposure, home: Option<&Path>) -> Self {
        let mut result = self.clone();
        result.path_exposure = exposure;
        for environment in &mut result.environments {
            environment.path = environment
                .path
                .as_deref()
                .and_then(|p| expose_project_path(p, exposure, home));
        }
        result
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectView {
    pub project: ProjectSummary,
    pub scan_revision: Option<u64>,
    pub snapshot: Option<ProjectSnapshot>,
}
impl ProjectView {
    pub fn project(&self, exposure: PathExposure, home: Option<&Path>) -> Self {
        let mut result = self.clone();
        result.project.root = result
            .project
            .root
            .as_deref()
            .and_then(|p| expose_project_path(p, exposure, home));
        result.project.extra_venv = result
            .project
            .extra_venv
            .as_deref()
            .and_then(|p| expose_project_path(p, exposure, home));
        result.snapshot = result.snapshot.as_ref().map(|s| s.project(exposure, home));
        result
    }
}
