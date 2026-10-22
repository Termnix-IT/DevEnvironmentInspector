use super::{ProjectScanner, filesystem::canonical_project_folder};
use inspector_core::{PathExposure, project_store::ProjectRepository, projects::*};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub const MIN_PROJECT_SCAN_INTERVAL: Duration = Duration::from_secs(2);
struct Cached {
    registration: ProjectRegistration,
    snapshot: ProjectSnapshot,
    revision: u64,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Cached>,
    next_revision: u64,
    completed: Option<Instant>,
}
struct Inner {
    repository: Arc<dyn ProjectRepository>,
    scanner: Arc<dyn ProjectScanner>,
    home: Option<PathBuf>,
    cache: Mutex<Cache>,
    scan_slot: Arc<Semaphore>,
}
#[derive(Clone)]
pub struct ProjectService {
    inner: Arc<Inner>,
}
impl ProjectService {
    pub fn new(
        repository: Arc<dyn ProjectRepository>,
        scanner: Arc<dyn ProjectScanner>,
        home: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                repository,
                scanner,
                home,
                cache: Mutex::new(Cache::default()),
                scan_slot: Arc::new(Semaphore::new(1)),
            }),
        }
    }
    fn registration(&self, id: &str, mcp_only: bool) -> Result<ProjectRegistration, ProjectError> {
        if !valid_project_id(id) {
            return Err(ProjectError::InvalidProjectInput);
        }
        self.inner
            .repository
            .load()?
            .projects
            .into_iter()
            .find(|p| p.id == id && (!mcp_only || p.mcp_allowed))
            .ok_or(ProjectError::ProjectNotFound)
    }
    pub fn list(
        &self,
        exposure: PathExposure,
        mcp_only: bool,
    ) -> Result<Vec<ProjectSummary>, ProjectError> {
        Ok(self
            .inner
            .repository
            .load()?
            .projects
            .into_iter()
            .filter(|p| !mcp_only || p.mcp_allowed)
            .map(|p| p.summary(exposure, self.inner.home.as_deref()))
            .collect())
    }
    pub fn register(
        &self,
        name: String,
        root: &Path,
        extra_venv: Option<&Path>,
        mcp_allowed: bool,
        exposure: PathExposure,
    ) -> Result<ProjectSummary, ProjectError> {
        let root = canonical_project_folder(root)?;
        let extra_venv = extra_venv.map(canonical_project_folder).transpose()?;
        self.inner
            .repository
            .register(NewProject {
                name: name.trim().into(),
                root,
                extra_venv,
                mcp_allowed,
            })
            .map(|p| p.summary(exposure, self.inner.home.as_deref()))
    }
    pub fn update(
        &self,
        id: &str,
        name: &str,
        allowed: bool,
        exposure: PathExposure,
    ) -> Result<ProjectSummary, ProjectError> {
        let project = self.inner.repository.update(id, name.trim(), allowed)?;
        self.inner
            .cache
            .lock()
            .map_err(|_| ProjectError::ProjectScanFailed)?
            .entries
            .remove(id);
        Ok(project.summary(exposure, self.inner.home.as_deref()))
    }
    pub fn remove(&self, id: &str) -> Result<(), ProjectError> {
        self.inner.repository.remove(id)?;
        self.inner
            .cache
            .lock()
            .map_err(|_| ProjectError::ProjectScanFailed)?
            .entries
            .remove(id);
        Ok(())
    }
    pub fn cached(
        &self,
        id: &str,
        exposure: PathExposure,
        mcp_only: bool,
    ) -> Result<ProjectView, ProjectError> {
        let registration = self.registration(id, mcp_only)?;
        let cache = self
            .inner
            .cache
            .lock()
            .map_err(|_| ProjectError::ProjectScanFailed)?;
        let cached = cache
            .entries
            .get(id)
            .filter(|c| c.registration == registration);
        let result = ProjectView {
            project: registration.summary(exposure, self.inner.home.as_deref()),
            scan_revision: cached.map(|c| c.revision),
            snapshot: cached.map(|c| c.snapshot.project(exposure, self.inner.home.as_deref())),
        };
        drop(cache);
        if self.registration(id, mcp_only)? != registration {
            return Err(ProjectError::ProjectSnapshotChanged);
        }
        Ok(result)
    }
    pub async fn inspect(
        &self,
        id: &str,
        exposure: PathExposure,
        mcp_only: bool,
        refresh: bool,
    ) -> Result<ProjectView, ProjectError> {
        let current = self.cached(id, exposure, mcp_only)?;
        if !refresh && current.snapshot.is_some() {
            return Ok(current);
        }
        let registration = self.registration(id, mcp_only)?;
        let permit = self
            .inner
            .scan_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProjectError::ProjectScanInProgress)?;
        if self
            .inner
            .cache
            .lock()
            .map_err(|_| ProjectError::ProjectScanFailed)?
            .completed
            .is_some_and(|time| time.elapsed() < MIN_PROJECT_SCAN_INTERVAL)
        {
            return Err(ProjectError::ProjectScanRateLimited);
        }
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            // リクエストのキャンセル後も、実ワーカー終了まで枠を保持する。
            let _permit = permit;
            let result = inner.scanner.scan(&registration);
            let mut cache = inner
                .cache
                .lock()
                .map_err(|_| ProjectError::ProjectScanFailed)?;
            cache.completed = Some(Instant::now());
            let snapshot = result?;
            if !snapshot.validate() || snapshot.path_exposure != PathExposure::Full {
                return Err(ProjectError::ProjectScanFailed);
            }
            let latest = inner
                .repository
                .load()?
                .projects
                .into_iter()
                .find(|p| p.id == registration.id && (!mcp_only || p.mcp_allowed))
                .ok_or(ProjectError::ProjectNotFound)?;
            if latest != registration {
                return Err(ProjectError::ProjectSnapshotChanged);
            }
            cache.next_revision = cache
                .next_revision
                .checked_add(1)
                .ok_or(ProjectError::ProjectScanFailed)?;
            let revision = cache.next_revision;
            let view = ProjectView {
                project: registration.summary(exposure, inner.home.as_deref()),
                scan_revision: Some(revision),
                snapshot: Some(snapshot.project(exposure, inner.home.as_deref())),
            };
            cache.entries.insert(
                registration.id.clone(),
                Cached {
                    registration,
                    snapshot,
                    revision,
                },
            );
            Ok(view)
        })
        .await
        .map_err(|_| ProjectError::ProjectScanFailed)?
    }
}
