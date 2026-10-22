//! 登録したプロジェクト内の既知メタデータを読み取る。プロセスは起動しない。
pub mod filesystem;
mod node;
mod python;
pub mod service;

use filesystem::{NativeProjectFileSystem, ProjectDirectory, ProjectFileSystem};
use inspector_core::{PathExposure, projects::*};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

pub const MAX_PROJECT_PACKAGES: usize = 512;
const MAX_FILES: usize = 1600;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;
const SCAN_BUDGET: Duration = Duration::from_secs(8);

pub trait ProjectScanner: Send + Sync + 'static {
    fn scan(&self, project: &ProjectRegistration) -> Result<ProjectSnapshot, ProjectError>;
}
pub struct MetadataProjectScanner {
    pub file_system: Arc<dyn ProjectFileSystem>,
}
impl Default for MetadataProjectScanner {
    fn default() -> Self {
        Self {
            file_system: Arc::new(NativeProjectFileSystem),
        }
    }
}
struct Budget {
    deadline: Instant,
    files: usize,
    bytes: usize,
    entries: usize,
}
impl Budget {
    fn check(&mut self) -> Result<(), ProjectFinding> {
        if Instant::now() >= self.deadline {
            return Err(ProjectFinding::ScanDeadlineReached);
        }
        if self.files == 0 || self.bytes == 0 {
            return Err(ProjectFinding::ReadLimitReached);
        }
        self.files -= 1;
        Ok(())
    }
}
struct Reader<'a> {
    dir: &'a dyn ProjectDirectory,
    budget: &'a mut Budget,
    findings: Vec<ProjectFinding>,
}
impl Reader<'_> {
    fn note(&mut self, finding: ProjectFinding) {
        if !self.findings.contains(&finding) {
            self.findings.push(finding);
        }
    }
    fn bytes(&mut self, path: &Path, limit: usize) -> Result<Option<Vec<u8>>, ProjectFinding> {
        self.budget.check()?;
        let bytes = self.dir.read(path, limit.min(self.budget.bytes))?;
        if let Some(value) = &bytes {
            self.budget.bytes = self.budget.bytes.saturating_sub(value.len());
        }
        Ok(bytes)
    }
    fn read(&mut self, path: &Path, limit: usize) -> Option<Vec<u8>> {
        match self.bytes(path, limit) {
            Ok(value) => value,
            Err(error) => {
                self.note(error);
                None
            }
        }
    }
    fn text(&mut self, path: &Path, limit: usize) -> Option<String> {
        let bytes = self.read(path, limit)?;
        match String::from_utf8(bytes) {
            Ok(value) => Some(value),
            Err(_) => {
                self.note(ProjectFinding::MetadataInvalid);
                None
            }
        }
    }
    fn directories(&mut self, path: &Path) -> Vec<String> {
        if let Err(error) = self.budget.check() {
            self.note(error);
            return Vec::new();
        }
        if self.budget.entries == 0 {
            self.note(ProjectFinding::ReadLimitReached);
            return Vec::new();
        }
        match self.dir.directories(path, self.budget.entries) {
            Ok(entries) => {
                // 列挙結果だけでなく、ファイル等も含む上限到達は未完了として残す。
                self.budget.entries = self.budget.entries.saturating_sub(entries.visited);
                if entries.truncated {
                    self.budget.entries = 0;
                    self.note(ProjectFinding::ReadLimitReached);
                }
                if entries.links_skipped {
                    self.note(ProjectFinding::LinkedPathSkipped);
                }
                entries.names
            }
            Err(error) => {
                self.note(error);
                Vec::new()
            }
        }
    }
    fn regular_file(&mut self, path: &Path) -> Option<bool> {
        if let Err(error) = self.budget.check() {
            self.note(error);
            return None;
        }
        match self.dir.regular_file(path) {
            Ok(value) => Some(value),
            Err(error) => {
                self.note(error);
                None
            }
        }
    }
}

impl ProjectScanner for MetadataProjectScanner {
    fn scan(&self, project: &ProjectRegistration) -> Result<ProjectSnapshot, ProjectError> {
        let started_at = OffsetDateTime::now_utc();
        let mut snapshot = ProjectSnapshot {
            schema_version: PROJECT_SCHEMA_VERSION,
            started_at,
            completed_at: started_at,
            path_exposure: PathExposure::Full,
            environments: Vec::new(),
            findings: Vec::new(),
        };
        let mut budget = Budget {
            deadline: Instant::now() + SCAN_BUDGET,
            files: MAX_FILES,
            bytes: MAX_BYTES,
            entries: MAX_ENTRIES,
        };
        match self.file_system.open(&project.root) {
            Ok(directory) => {
                for (relative, id) in [(".venv", "python-dot-venv"), ("venv", "python-venv")] {
                    if let Some(environment) = python::inspect(
                        directory.as_ref(),
                        &mut budget,
                        Path::new(relative),
                        &project.root.join(relative),
                        id,
                        relative,
                    ) {
                        snapshot.environments.push(environment);
                    }
                }
                if let Some(environment) =
                    node::inspect(directory.as_ref(), &mut budget, &project.root)
                {
                    snapshot.environments.push(environment);
                }
            }
            Err(error) => snapshot.findings.push(error),
        }
        if let Some(root) = &project.extra_venv {
            // プロジェクト内の既知候補と同じ外部venvは重複させない。
            if ![project.root.join(".venv"), project.root.join("venv")]
                .iter()
                .any(|p| inspector_core::project_store::same_path(p, root))
            {
                match self.file_system.open(root) {
                    Ok(directory) => {
                        let environment = python::inspect(
                            directory.as_ref(),
                            &mut budget,
                            Path::new("."),
                            root,
                            "python-external",
                            "追加venv",
                        )
                        .unwrap_or_else(|| missing_environment(root));
                        snapshot.environments.push(environment);
                    }
                    Err(error) => {
                        let mut environment = missing_environment(root);
                        environment.findings.push(error);
                        snapshot.environments.push(environment);
                    }
                }
            }
        }
        snapshot.completed_at = OffsetDateTime::now_utc();
        Ok(snapshot)
    }
}
fn missing_environment(path: &Path) -> ProjectEnvironment {
    ProjectEnvironment {
        id: "python-external".into(),
        label: "追加venv".into(),
        kind: ProjectEnvironmentKind::PythonVenv,
        status: ProjectEnvironmentStatus::Missing,
        path: Some(path.to_owned()),
        reported_runtime_version: None,
        declared_runtime_requirement: None,
        interpreter_present: None,
        system_site_packages: None,
        packages: Vec::new(),
        findings: Vec::new(),
    }
}
fn empty_environment(
    id: &str,
    label: &str,
    kind: ProjectEnvironmentKind,
    path: PathBuf,
) -> ProjectEnvironment {
    ProjectEnvironment {
        id: id.into(),
        label: label.into(),
        kind,
        status: ProjectEnvironmentStatus::Detected,
        path: Some(path),
        reported_runtime_version: None,
        declared_runtime_requirement: None,
        interpreter_present: None,
        system_site_packages: None,
        packages: Vec::new(),
        findings: Vec::new(),
    }
}
fn finish(environment: &mut ProjectEnvironment, findings: Vec<ProjectFinding>) {
    if !findings.is_empty() {
        environment.status = ProjectEnvironmentStatus::Incomplete;
    }
    environment.findings = findings;
}
