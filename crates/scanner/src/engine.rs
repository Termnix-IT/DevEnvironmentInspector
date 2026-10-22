use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    pin::Pin,
    time::{Duration, Instant},
};

use inspector_core::{
    AccessStatus, Catalog, DetectionStatus, Diagnostic, DiagnosticCode, EvidenceClass,
    Installation, KnownCommand, PathExposure, Registration, SCHEMA_VERSION, ScanSnapshot, Severity,
    TargetOs, ToolScan, VersionResult, VersionSource, VersionStatus,
};
use thiserror::Error;
use time::OffsetDateTime;

use crate::{
    path_provider::{is_local_absolute, path_key},
    provider::DiscoveryProvider,
};

#[derive(Debug, Clone, Copy)]
pub struct ProbeLimits {
    pub timeout: Duration,
    pub output_bytes_per_stream: usize,
}

pub type ProbeFuture<'a> = Pin<Box<dyn Future<Output = VersionResult> + Send + 'a>>;

pub trait VersionRunner: Send + Sync {
    fn probe<'a>(
        &'a self,
        command: KnownCommand,
        path: &'a Path,
        node: Option<&'a Path>,
        limits: ProbeLimits,
    ) -> ProbeFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub probe_versions: bool,
    pub per_probe_timeout: Duration,
    pub scan_timeout: Duration,
    pub max_version_probes: usize,
    pub max_candidates_per_tool: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            probe_versions: false,
            per_probe_timeout: Duration::from_secs(2),
            scan_timeout: Duration::from_secs(30),
            max_version_probes: 64,
            max_candidates_per_tool: 16,
        }
    }
}

#[derive(Debug, Error)]
#[error("スキャン制限が許可範囲外です")]
pub struct InvalidScanOptions;

pub struct ScanEngine {
    catalog: Catalog,
    providers: Vec<Box<dyn DiscoveryProvider>>,
    runner: Box<dyn VersionRunner>,
}

fn diag(
    code: DiagnosticCode,
    class: EvidenceClass,
    severity: Severity,
    id: Option<&str>,
) -> Diagnostic {
    Diagnostic {
        code,
        class,
        severity,
        provider: None,
        installation_id: id.map(str::to_string),
    }
}

impl ScanEngine {
    pub fn new(
        catalog: Catalog,
        providers: Vec<Box<dyn DiscoveryProvider>>,
        runner: Box<dyn VersionRunner>,
    ) -> Self {
        Self {
            catalog,
            providers,
            runner,
        }
    }

    pub async fn scan(&self, options: &ScanOptions) -> Result<ScanSnapshot, InvalidScanOptions> {
        if options.per_probe_timeout.is_zero()
            || options.per_probe_timeout > Duration::from_secs(5)
            || options.scan_timeout.is_zero()
            || options.scan_timeout > Duration::from_secs(60)
            || !(1..=64).contains(&options.max_version_probes)
            || !(1..=16).contains(&options.max_candidates_per_tool)
        {
            return Err(InvalidScanOptions);
        }
        let started_at = OffsetDateTime::now_utc();
        let deadline = Instant::now() + options.scan_timeout;
        let mut snapshot = ScanSnapshot {
            schema_version: SCHEMA_VERSION,
            target_os: TargetOs::Windows,
            started_at,
            completed_at: started_at,
            providers: Vec::new(),
            path_exposure: PathExposure::Full,
            tools: self
                .catalog
                .tools
                .iter()
                .cloned()
                .map(|tool| ToolScan {
                    tool,
                    status: DetectionStatus::NotFound,
                    installations: Vec::new(),
                    registrations: Vec::new(),
                    diagnostics: Vec::new(),
                })
                .collect(),
            diagnostics: Vec::new(),
        };
        let tool_indices: HashMap<_, _> = snapshot
            .tools
            .iter()
            .enumerate()
            .map(|(i, t)| (t.tool.id.clone(), i))
            .collect();
        let mut incomplete = vec![false; snapshot.tools.len()];
        let mut registration_indices = HashMap::new();
        if self.providers.is_empty() {
            incomplete.fill(true);
        }
        for provider in &self.providers {
            if Instant::now() >= deadline {
                incomplete.fill(true);
                snapshot.diagnostics.push(diag(
                    DiagnosticCode::ScanDeadlineReached,
                    EvidenceClass::Fact,
                    Severity::Warning,
                    None,
                ));
                break;
            }
            snapshot.providers.push(provider.kind());
            let discovery = match provider.discover(&self.catalog) {
                Ok(discovery) => discovery,
                Err(_) => {
                    incomplete.fill(true);
                    let mut failure = diag(
                        DiagnosticCode::ProviderFailed,
                        EvidenceClass::Fact,
                        Severity::Error,
                        None,
                    );
                    failure.provider = Some(provider.kind());
                    snapshot.diagnostics.push(failure);
                    continue;
                }
            };
            if discovery.incomplete {
                incomplete.fill(true);
            }
            for id in discovery.incomplete_tools {
                if let Some(&index) = tool_indices.get(&id) {
                    incomplete[index] = true;
                }
            }
            snapshot.diagnostics.extend(discovery.diagnostics);
            for observation in discovery.registrations {
                let Some(&index) = tool_indices.get(&observation.tool_id) else {
                    continue;
                };
                if observation
                    .install_location
                    .as_deref()
                    .is_some_and(|p| !is_local_absolute(p))
                {
                    incomplete[index] = true;
                    continue;
                }
                let tool = &mut snapshot.tools[index];
                let key = (index, observation.identity);
                if let Some(&existing_index) = registration_indices.get(&key) {
                    let existing: &mut Registration = &mut tool.registrations[existing_index];
                    if !existing.sources.contains(&observation.evidence) {
                        existing.sources.push(observation.evidence);
                    }
                    existing.package_id = existing.package_id.take().or(observation.package_id);
                    existing.install_location = existing
                        .install_location
                        .take()
                        .or(observation.install_location);
                } else {
                    registration_indices.insert(key, tool.registrations.len());
                    tool.registrations.push(Registration {
                        id: String::new(),
                        package_id: observation.package_id,
                        install_location: observation.install_location,
                        sources: vec![observation.evidence],
                        detected_at: started_at,
                    });
                }
            }
            for candidate in discovery.candidates {
                let Some(&index) = tool_indices.get(&candidate.tool_id) else {
                    continue;
                };
                let tool = &mut snapshot.tools[index];
                // プロバイダーを追加してもカタログ外コマンド・相対パスは実行境界に渡さない。
                if !tool.tool.commands.contains(&candidate.command)
                    || !is_local_absolute(&candidate.path)
                {
                    incomplete[index] = true;
                    continue;
                }
                let existing = tool.installations.iter_mut().find(|i| {
                    i.command == candidate.command
                        && i.executable_path
                            .as_deref()
                            .is_some_and(|p| path_key(p) == path_key(&candidate.path))
                });
                if let Some(existing) = existing {
                    existing.on_path |= candidate.on_path;
                    existing.path_order = existing
                        .path_order
                        .into_iter()
                        .chain(candidate.path_order)
                        .min();
                    if candidate.access == AccessStatus::Readable {
                        existing.access = AccessStatus::Readable;
                    }
                    if !existing.sources.contains(&candidate.evidence) {
                        existing.sources.push(candidate.evidence);
                    }
                } else {
                    // 優先順位は全プロバイダー統合後に確定するため、この時点では上位候補を切り捨てない。
                    tool.installations.push(Installation {
                        id: String::new(),
                        command: candidate.command,
                        executable_path: Some(candidate.path),
                        on_path: candidate.on_path,
                        path_order: candidate.path_order,
                        access: candidate.access,
                        install_method: None,
                        version: VersionResult::status(VersionStatus::NotRequested),
                        sources: vec![candidate.evidence],
                        detected_at: started_at,
                    });
                }
            }
        }
        for (index, tool) in snapshot.tools.iter_mut().enumerate() {
            if tool.registrations.len() > 16 {
                tool.registrations.truncate(16);
                incomplete[index] = true;
                tool.diagnostics.push(diag(
                    DiagnosticCode::MetadataLimitReached,
                    EvidenceClass::Fact,
                    Severity::Warning,
                    None,
                ));
            }
            for (n, registration) in tool.registrations.iter_mut().enumerate() {
                registration.id = format!("{}-registration-{}", tool.tool.id, n + 1);
            }
            tool.installations.sort_by_key(|i| {
                (
                    tool.tool
                        .commands
                        .iter()
                        .position(|c| *c == i.command)
                        .unwrap_or(usize::MAX),
                    !i.on_path,
                    i.path_order.unwrap_or(usize::MAX),
                    i.executable_path.as_deref().map(path_key),
                )
            });
            if tool.installations.len() > options.max_candidates_per_tool {
                tool.installations.truncate(options.max_candidates_per_tool);
                incomplete[index] = true;
                tool.diagnostics.push(diag(
                    DiagnosticCode::CandidateLimitReached,
                    EvidenceClass::Fact,
                    Severity::Warning,
                    None,
                ));
            }
            for (n, installation) in tool.installations.iter_mut().enumerate() {
                installation.id = format!("{}-{}", tool.tool.id, n + 1);
                let methods: Vec<_> = installation
                    .sources
                    .iter()
                    .filter_map(|s| s.install_method)
                    .collect();
                installation.install_method = methods
                    .first()
                    .copied()
                    .filter(|m| methods.iter().all(|other| other == m));
            }
        }
        let node = snapshot
            .tools
            .iter()
            .flat_map(|t| &t.installations)
            .find(|i| i.command == KnownCommand::Node && i.on_path)
            .filter(|i| i.access == AccessStatus::Readable)
            .and_then(|i| i.executable_path.clone());
        let mut probes = 0;
        for tool in &mut snapshot.tools {
            for installation in &mut tool.installations {
                if !options.probe_versions {
                    continue;
                }
                let status = if Instant::now() >= deadline {
                    Some(VersionStatus::ScanDeadlineReached)
                } else if probes >= options.max_version_probes {
                    Some(VersionStatus::ProbeLimitReached)
                } else if installation.access == AccessStatus::Denied {
                    Some(VersionStatus::AccessDenied)
                } else if installation.access != AccessStatus::Readable {
                    Some(VersionStatus::SpawnFailed)
                } else {
                    None
                };
                if let Some(status) = status {
                    installation.version = VersionResult::status(status);
                    continue;
                }
                if let Some(path) = &installation.executable_path {
                    probes += 1;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    installation.version = self
                        .runner
                        .probe(
                            installation.command,
                            path,
                            node.as_deref(),
                            ProbeLimits {
                                timeout: options.per_probe_timeout.min(remaining),
                                output_bytes_per_stream: 16 * 1024,
                            },
                        )
                        .await;
                }
            }
        }
        for (index, tool) in snapshot.tools.iter_mut().enumerate() {
            tool.status = if incomplete[index] {
                DetectionStatus::Incomplete
            } else if !tool.has_evidence() {
                DetectionStatus::NotFound
            } else {
                DetectionStatus::Detected
            };
            diagnose(tool);
        }
        snapshot.completed_at = OffsetDateTime::now_utc();
        Ok(snapshot)
    }
}

fn diagnose(tool: &mut ToolScan) {
    if !tool.registrations.is_empty() {
        if tool.installations.is_empty() {
            tool.diagnostics.push(diag(
                DiagnosticCode::RegisteredExecutableUnresolved,
                EvidenceClass::Caution,
                Severity::Warning,
                None,
            ));
        }
        if !tool.installations.iter().any(|i| i.on_path) {
            tool.diagnostics.push(diag(
                DiagnosticCode::RegisteredToolNotOnPath,
                EvidenceClass::Caution,
                Severity::Info,
                None,
            ));
        }
        if tool.registrations.iter().any(|r| {
            let versions: Vec<_> = r
                .sources
                .iter()
                .filter_map(|s| s.reported_version.as_ref())
                .collect();
            versions
                .first()
                .is_some_and(|first| versions.iter().any(|v| v != first))
        }) {
            tool.diagnostics.push(diag(
                DiagnosticCode::ConflictingRegistrationEvidence,
                EvidenceClass::Caution,
                Severity::Warning,
                None,
            ));
        }
    }
    for &command in &tool.tool.commands {
        let candidates: Vec<_> = tool
            .installations
            .iter()
            .filter(|i| i.command == command && i.on_path)
            .collect();
        if !candidates.is_empty() {
            tool.diagnostics.push(diag(
                DiagnosticCode::PathCandidateFound,
                EvidenceClass::Fact,
                Severity::Info,
                None,
            ));
        }
        if candidates.len() > 1 {
            tool.diagnostics.push(diag(
                DiagnosticCode::MultipleCommandCandidates,
                EvidenceClass::Fact,
                Severity::Info,
                None,
            ));
        }
        if let Some(primary) = candidates.first() {
            let available = primary.version.status == VersionStatus::Success
                && primary.version.source == Some(VersionSource::NativeExecutable);
            tool.diagnostics.push(diag(
                if available {
                    DiagnosticCode::CliAvailable
                } else {
                    DiagnosticCode::AvailabilityUnverified
                },
                if available {
                    EvidenceClass::Fact
                } else {
                    EvidenceClass::Caution
                },
                Severity::Info,
                Some(&primary.id),
            ));
        }
    }
    for installation in &tool.installations {
        if installation.access == AccessStatus::Denied {
            tool.diagnostics.push(diag(
                DiagnosticCode::AccessDenied,
                EvidenceClass::Fact,
                Severity::Warning,
                Some(&installation.id),
            ));
        }
        if !matches!(
            installation.version.status,
            VersionStatus::Success | VersionStatus::NotRequested
        ) {
            tool.diagnostics.push(diag(
                DiagnosticCode::VersionProbeFailed,
                EvidenceClass::Fact,
                Severity::Warning,
                Some(&installation.id),
            ));
        }
        if installation.version.source == Some(VersionSource::NpmEntrypoint) {
            tool.diagnostics.push(diag(
                DiagnosticCode::WrapperNotExecuted,
                EvidenceClass::Caution,
                Severity::Info,
                Some(&installation.id),
            ));
        }
        let versions: Vec<_> = installation
            .sources
            .iter()
            .filter_map(|s| s.reported_version.as_deref())
            .chain(installation.version.version.as_deref())
            .collect();
        if let Some(code) = version_evidence_code(installation.command, &versions) {
            let formatting = code == DiagnosticCode::VersionFormatDifference;
            tool.diagnostics.push(diag(
                code,
                if formatting {
                    EvidenceClass::Caution
                } else {
                    EvidenceClass::Fact
                },
                if formatting {
                    Severity::Info
                } else {
                    Severity::Warning
                },
                Some(&installation.id),
            ));
        }
    }
}

fn version_evidence_code(command: KnownCommand, versions: &[&str]) -> Option<DiagnosticCode> {
    let first = versions.first()?;
    if versions.iter().all(|v| v == first) {
        return None;
    }
    // 元の申告値を変更せず、既知の表記差だけで矛盾と確定することを避ける。
    let key = |version: &str| {
        let version = if command == KnownCommand::Git {
            version
                .split_once(".windows.")
                .filter(|(_, build)| !build.is_empty() && build.bytes().all(|b| b.is_ascii_digit()))
                .map_or(version, |(base, _)| base)
        } else {
            version
        };
        let mut parts: Vec<_> = version.split('.').collect();
        if parts.len() > 1
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            while parts.len() > 1 && parts.last() == Some(&"0") {
                parts.pop();
            }
            parts.join(".")
        } else {
            version.to_string()
        }
    };
    Some(if versions.iter().all(|v| key(v) == key(first)) {
        DiagnosticCode::VersionFormatDifference
    } else {
        DiagnosticCode::ConflictingVersionEvidence
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_format_differences_preserve_uncertainty_without_conflict() {
        assert_eq!(
            version_evidence_code(KnownCommand::Java, &["21.0.6", "21.0.6.0"]),
            Some(DiagnosticCode::VersionFormatDifference)
        );
        assert_eq!(
            version_evidence_code(KnownCommand::Git, &["2.51.0", "2.51.0.windows.1"]),
            Some(DiagnosticCode::VersionFormatDifference)
        );
        assert_eq!(
            version_evidence_code(KnownCommand::Java, &["21.0.6", "21.0.7"]),
            Some(DiagnosticCode::ConflictingVersionEvidence)
        );
        assert_eq!(
            version_evidence_code(KnownCommand::Node, &["24.1.0", "24.1.0"]),
            None
        );
    }
}
