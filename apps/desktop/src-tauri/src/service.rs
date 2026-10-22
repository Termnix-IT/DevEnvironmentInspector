use inspector_core::{
    Catalog, PathExposure, ScanSnapshot,
    storage::{JsonRepository, SnapshotRepository},
};
use inspector_scanner::{
    ScanOptions,
    platform::{path_engine, windows_engine},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// 外部のエラー文・パスをIPCへ返さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DesktopError {
    ScanInProgress,
    ScanRateLimited,
    ScanFailed,
    UnsupportedPlatform,
    NoSnapshot,
    StaleSnapshot,
    ExportInProgress,
    ExportFailed,
    InvalidDestination,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanRequest {
    pub probe_versions: bool,
    pub path_only: bool,
    pub paths: PathExposure,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub revision: u64,
    pub paths: PathExposure,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotView {
    pub revision: u64,
    pub snapshot: ScanSnapshot,
}

pub trait DesktopScanner: Send + Sync + 'static {
    fn scan(&self, catalog: Catalog, request: ScanRequest) -> Result<ScanSnapshot, DesktopError>;
}
pub struct NativeScanner;
impl DesktopScanner for NativeScanner {
    fn scan(&self, catalog: Catalog, request: ScanRequest) -> Result<ScanSnapshot, DesktopError> {
        if !cfg!(windows) {
            return Err(DesktopError::UnsupportedPlatform);
        }
        let engine = if request.path_only {
            path_engine(catalog)
        } else {
            windows_engine(catalog)
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| DesktopError::ScanFailed)?
            .block_on(engine.scan(&ScanOptions {
                probe_versions: request.probe_versions,
                ..ScanOptions::default()
            }))
            .map_err(|_| DesktopError::ScanFailed)
    }
}

#[derive(Default)]
struct Cache {
    revision: u64,
    snapshot: Option<Arc<ScanSnapshot>>,
    completed: Option<Instant>,
}
struct Inner {
    catalog: Catalog,
    scanner: Arc<dyn DesktopScanner>,
    home: Option<PathBuf>,
    cache: Mutex<Cache>,
    scan_slot: Arc<Semaphore>,
    export_slot: Arc<Semaphore>,
}
#[derive(Clone)]
pub struct DesktopService {
    inner: Arc<Inner>,
}
impl DesktopService {
    pub fn new(catalog: Catalog, scanner: Arc<dyn DesktopScanner>, home: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                catalog,
                scanner,
                home,
                cache: Mutex::new(Cache::default()),
                scan_slot: Arc::new(Semaphore::new(1)),
                export_slot: Arc::new(Semaphore::new(1)),
            }),
        }
    }
    pub fn catalog(&self) -> Catalog {
        self.inner.catalog.clone()
    }
    pub fn snapshot(&self, paths: PathExposure) -> Result<Option<SnapshotView>, DesktopError> {
        let cache = self
            .inner
            .cache
            .lock()
            .map_err(|_| DesktopError::Unavailable)?;
        Ok(cache
            .snapshot
            .as_ref()
            .map(|snapshot| self.view(cache.revision, snapshot, paths)))
    }
    fn view(&self, revision: u64, snapshot: &ScanSnapshot, paths: PathExposure) -> SnapshotView {
        SnapshotView {
            revision,
            snapshot: snapshot.project(paths, self.inner.home.as_deref()),
        }
    }
    pub async fn scan(&self, request: ScanRequest) -> Result<SnapshotView, DesktopError> {
        let permit = self
            .inner
            .scan_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| DesktopError::ScanInProgress)?;
        if self
            .inner
            .cache
            .lock()
            .map_err(|_| DesktopError::Unavailable)?
            .completed
            .is_some_and(|time| time.elapsed() < MIN_SCAN_INTERVAL)
        {
            return Err(DesktopError::ScanRateLimited);
        }
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            // IPC呼出し側のキャンセルでは、実行枠を早期に解放しない。
            let _permit = permit;
            let result = service
                .inner
                .scanner
                .scan(service.catalog(), request)
                .and_then(|snapshot| {
                    if !snapshot.validate() || snapshot.path_exposure != PathExposure::Full {
                        return Err(DesktopError::ScanFailed);
                    }
                    Ok(Arc::new(snapshot))
                });
            let mut cache = service
                .inner
                .cache
                .lock()
                .map_err(|_| DesktopError::Unavailable)?;
            cache.completed = Some(Instant::now());
            let snapshot = result?;
            cache.revision = cache
                .revision
                .checked_add(1)
                .ok_or(DesktopError::Unavailable)?;
            cache.snapshot = Some(snapshot.clone());
            Ok(service.view(cache.revision, &snapshot, request.paths))
        })
        .await
        .map_err(|_| DesktopError::ScanFailed)?
    }
    /// ダイアログ開始時に保存対象を固定する。画面の古い結果を別スキャンとして保存しない。
    pub fn begin_export(
        &self,
        revision: u64,
    ) -> Result<(Arc<ScanSnapshot>, OwnedSemaphorePermit), DesktopError> {
        let permit = self
            .inner
            .export_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| DesktopError::ExportInProgress)?;
        let cache = self
            .inner
            .cache
            .lock()
            .map_err(|_| DesktopError::Unavailable)?;
        let snapshot = cache.snapshot.clone().ok_or(DesktopError::NoSnapshot)?;
        if revision != cache.revision {
            return Err(DesktopError::StaleSnapshot);
        }
        Ok((snapshot, permit))
    }
    pub fn write_export(
        &self,
        snapshot: &ScanSnapshot,
        path: &Path,
        exposure: PathExposure,
    ) -> Result<(), DesktopError> {
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("json"))
        {
            return Err(DesktopError::InvalidDestination);
        }
        JsonRepository { path }
            .save(snapshot, exposure, self.inner.home.as_deref())
            .map_err(|_| DesktopError::ExportFailed)
    }
}
