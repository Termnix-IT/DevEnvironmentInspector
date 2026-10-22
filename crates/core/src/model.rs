use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{Catalog, KnownCommand, ToolDefinition};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOs {
    Windows,
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Path,
    InstalledApplications,
    Winget,
    NpmGlobal,
    Registry,
    KnownDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Winget,
    Npm,
    Installer,
    Portable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub provider: ProviderKind,
    pub reported_version: Option<String>,
    pub install_method: Option<InstallMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessStatus {
    Readable,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    NotRequested,
    Success,
    NonzeroExit,
    TimedOut,
    AccessDenied,
    SpawnFailed,
    UnrecognizedOutput,
    OutputLimitExceeded,
    UnsupportedWrapper,
    ProbeLimitReached,
    ScanDeadlineReached,
    UnsupportedPlatform,
    CleanupFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionSource {
    NativeExecutable,
    NpmEntrypoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionResult {
    pub status: VersionStatus,
    pub version: Option<String>,
    pub exit_code: Option<i32>,
    pub source: Option<VersionSource>,
}

impl VersionResult {
    pub fn status(status: VersionStatus) -> Self {
        Self {
            status,
            version: None,
            exit_code: None,
            source: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    /// スキャン内の識別子。永続的なインストールIDではない。
    pub id: String,
    pub command: KnownCommand,
    pub executable_path: Option<PathBuf>,
    pub on_path: bool,
    pub path_order: Option<usize>,
    pub access: AccessStatus,
    pub install_method: Option<InstallMethod>,
    pub version: VersionResult,
    pub sources: Vec<SourceEvidence>,
    #[serde(with = "time::serde::rfc3339")]
    pub detected_at: OffsetDateTime,
}

/// 製品登録は実行ファイルの存在・CLIの利用可否を保証しない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub id: String,
    pub package_id: Option<String>,
    pub install_location: Option<PathBuf>,
    pub sources: Vec<SourceEvidence>,
    #[serde(with = "time::serde::rfc3339")]
    pub detected_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionStatus {
    NotFound,
    Detected,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Fact,
    Caution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    PathCandidateFound,
    MultipleCommandCandidates,
    AccessDenied,
    VersionProbeFailed,
    CliAvailable,
    AvailabilityUnverified,
    WrapperNotExecuted,
    ConflictingVersionEvidence,
    VersionFormatDifference,
    PathEntriesSkipped,
    UnsupportedPathExtension,
    CandidateLimitReached,
    ProviderFailed,
    ScanDeadlineReached,
    MetadataInvalid,
    MetadataLimitReached,
    RegisteredExecutableUnresolved,
    RegisteredToolNotOnPath,
    ConflictingRegistrationEvidence,
}

/// 自由形式の外部出力は保持しない。表示文言はUI側でcodeから生成できる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub class: EvidenceClass,
    pub severity: Severity,
    pub provider: Option<ProviderKind>,
    pub installation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolScan {
    pub tool: ToolDefinition,
    pub status: DetectionStatus,
    pub installations: Vec<Installation>,
    #[serde(default)]
    pub registrations: Vec<Registration>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ToolScan {
    pub fn has_evidence(&self) -> bool {
        !self.installations.is_empty() || !self.registrations.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathExposure {
    Full,
    RedactHome,
    Omit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub schema_version: u32,
    pub target_os: TargetOs,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,
    pub providers: Vec<ProviderKind>,
    pub path_exposure: PathExposure,
    pub tools: Vec<ToolScan>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ScanSnapshot {
    /// 保存データを観測結果として扱う前に基本的な整合性を検証する。
    pub fn validate(&self) -> bool {
        if self.schema_version != SCHEMA_VERSION
            || self.completed_at < self.started_at
            || (Catalog {
                schema_version: 1,
                tools: self.tools.iter().map(|t| t.tool.clone()).collect(),
            })
            .validate()
            .is_err()
        {
            return false;
        }
        let mut ids = HashSet::new();
        for tool in &self.tools {
            if (tool.status == DetectionStatus::NotFound && tool.has_evidence())
                || (tool.status == DetectionStatus::Detected && !tool.has_evidence())
                || tool.installations.len() > 16
                || tool.registrations.len() > 16
            {
                return false;
            }
            let mut local_ids = HashSet::new();
            for registration in &tool.registrations {
                if registration.id.is_empty()
                    || !ids.insert(&registration.id)
                    || registration.sources.is_empty()
                    || (self.path_exposure == PathExposure::Omit
                        && registration.install_location.is_some())
                {
                    return false;
                }
            }
            for installation in &tool.installations {
                if installation.id.is_empty()
                    || !ids.insert(&installation.id)
                    || !local_ids.insert(installation.id.as_str())
                    || !tool.tool.commands.contains(&installation.command)
                    || installation.sources.is_empty()
                    || (self.path_exposure == PathExposure::Omit
                        && installation.executable_path.is_some())
                    || (installation.version.status == VersionStatus::Success
                        && (installation.version.version.is_none()
                            || installation.version.exit_code != Some(0)
                            || installation.version.source.is_none()))
                    || (installation.version.status != VersionStatus::Success
                        && installation.version.version.is_some())
                {
                    return false;
                }
            }
            if tool.diagnostics.iter().any(|d| {
                d.installation_id
                    .as_deref()
                    .is_some_and(|id| !local_ids.contains(id))
            }) {
                return false;
            }
        }
        true
    }

    /// API応答・保存の共通公開境界。元のスナップショットは変更しない。
    pub fn project(&self, exposure: PathExposure, home: Option<&Path>) -> Self {
        let mut result = self.clone();
        result.path_exposure = exposure;
        for installation in result.tools.iter_mut().flat_map(|t| &mut t.installations) {
            installation.executable_path = match exposure {
                PathExposure::Full => installation.executable_path.take(),
                PathExposure::Omit => None,
                PathExposure::RedactHome => installation
                    .executable_path
                    .as_deref()
                    .and_then(|path| redact_home(path, home)),
            };
        }
        for registration in result.tools.iter_mut().flat_map(|t| &mut t.registrations) {
            registration.install_location = match exposure {
                PathExposure::Full => registration.install_location.take(),
                PathExposure::Omit => None,
                PathExposure::RedactHome => registration
                    .install_location
                    .as_deref()
                    .and_then(|path| redact_home(path, home)),
            };
        }
        result
    }
}

pub(crate) fn redact_home(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    // home未指定なら安全側に倒して省略する。文字列の部分一致は使用しない。
    let home = home?;
    let parts: Vec<_> = path.components().collect();
    let home_parts: Vec<_> = home.components().collect();
    if home_parts.is_empty() {
        return None;
    }
    let starts_with_home = parts.len() >= home_parts.len()
        && parts.iter().zip(&home_parts).all(|(a, b)| {
            a.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
        });
    if starts_with_home {
        let mut redacted = PathBuf::from("<HOME>");
        for part in &parts[home_parts.len()..] {
            redacted.push(part);
        }
        Some(redacted)
    } else {
        Some(path.to_owned())
    }
}
