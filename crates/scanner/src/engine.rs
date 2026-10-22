use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    pin::Pin,
    time::{Duration, Instant},
};

use inspector_core::{
    AccessStatus, Catalog, DetectionStatus, Diagnostic, DiagnosticCode, EvidenceClass,
    Installation, KnownCommand, PathExposure, SCHEMA_VERSION, ScanSnapshot, Severity, TargetOs,
    ToolScan, VersionResult, VersionSource, VersionStatus,
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
            } else if tool.installations.is_empty() {
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
            .filter_map(|s| s.reported_version.as_ref())
            .chain(installation.version.version.as_ref())
            .collect();
        if versions
            .first()
            .is_some_and(|first| versions.iter().any(|other| first != other))
        {
            tool.diagnostics.push(diag(
                DiagnosticCode::ConflictingVersionEvidence,
                EvidenceClass::Fact,
                Severity::Warning,
                Some(&installation.id),
            ));
        }
    }
}
