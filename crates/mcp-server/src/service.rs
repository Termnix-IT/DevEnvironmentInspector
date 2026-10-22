use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use inspector_core::{Catalog, PathExposure, ScanSnapshot};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::backend::{EnvironmentScanner, ScanFailure};

pub const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct ServerPolicy {
    pub paths: PathExposure,
    pub home: Option<PathBuf>,
    pub allow_version_probes: bool,
}

impl Default for ServerPolicy {
    fn default() -> Self {
        Self {
            paths: PathExposure::Omit,
            home: None,
            allow_version_probes: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("指定したツールIDはカタログにありません")]
    UnknownTool,
    #[error("指定したコマンドは許可リストにありません")]
    UnknownCommand,
    #[error("対象は重複のない1〜18件の既知ツールIDで指定してください")]
    InvalidScope,
    #[error("このサーバーではバージョン実行が許可されていません")]
    ProbesDisabled,
    #[error("別のスキャンを処理中です。完了後に再試行してください")]
    Busy,
    #[error("再スキャンの間隔が短すぎます。少し待って再試行してください")]
    RateLimited,
    #[error("このツールは直近のスキャン対象外です。scan_environmentのtool_idsで指定してください")]
    NotScanned,
    #[error("スキャンを完了できませんでした")]
    ScanFailed,
    #[error("実機スキャンはWindows専用です")]
    UnsupportedPlatform,
    #[error("サーバーの内部状態を取得できませんでした")]
    Unavailable,
}

impl ServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::UnknownCommand => "unknown_command",
            Self::InvalidScope => "invalid_scope",
            Self::ProbesDisabled => "version_probes_disabled",
            Self::Busy => "scan_in_progress",
            Self::RateLimited => "scan_rate_limited",
            Self::NotScanned => "tool_not_scanned",
            Self::ScanFailed => "scan_failed",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::Unavailable => "server_unavailable",
        }
    }
}

#[derive(Default)]
struct Cache {
    snapshot: Option<Arc<ScanSnapshot>>,
    completed: Option<Instant>,
}

struct Inner {
    catalog: Catalog,
    backend: Arc<dyn EnvironmentScanner>,
    policy: ServerPolicy,
    cache: Mutex<Cache>,
    scan_slot: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct InspectionService {
    inner: Arc<Inner>,
}

impl InspectionService {
    pub fn new(
        catalog: Catalog,
        backend: Arc<dyn EnvironmentScanner>,
        policy: ServerPolicy,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                catalog,
                backend,
                policy,
                cache: Mutex::new(Cache::default()),
                scan_slot: Arc::new(Semaphore::new(1)),
            }),
        }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }
    pub fn policy(&self) -> &ServerPolicy {
        &self.inner.policy
    }

    pub fn tool_id_for_command(&self, command: &str) -> Result<&str, ServiceError> {
        self.catalog()
            .tools
            .iter()
            .find(|t| t.commands.iter().any(|c| c.as_str() == command))
            .map(|t| t.id.as_str())
            .ok_or(ServiceError::UnknownCommand)
    }

    pub fn require_tool(&self, id: &str) -> Result<(), ServiceError> {
        if self.catalog().tools.iter().any(|t| t.id == id) {
            Ok(())
        } else {
            Err(ServiceError::UnknownTool)
        }
    }

    pub async fn snapshot(&self) -> Result<Arc<ScanSnapshot>, ServiceError> {
        let cached = self
            .inner
            .cache
            .lock()
            .map_err(|_| ServiceError::Unavailable)?
            .snapshot
            .clone();
        match cached {
            Some(snapshot) => Ok(snapshot),
            None => self.refresh(None, false).await,
        }
    }

    pub async fn refresh(
        &self,
        tool_ids: Option<Vec<String>>,
        probe_versions: bool,
    ) -> Result<Arc<ScanSnapshot>, ServiceError> {
        if probe_versions && !self.inner.policy.allow_version_probes {
            return Err(ServiceError::ProbesDisabled);
        }
        let mut catalog = self.inner.catalog.clone();
        if let Some(ids) = tool_ids {
            let unique: HashSet<_> = ids.iter().collect();
            if ids.is_empty()
                || ids.len() > self.catalog().tools.len()
                || unique.len() != ids.len()
                || ids
                    .iter()
                    .any(|id| !catalog.tools.iter().any(|t| t.id == *id))
            {
                return Err(ServiceError::InvalidScope);
            }
            catalog.tools.retain(|tool| ids.contains(&tool.id));
        }
        let permit = self
            .inner
            .scan_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| ServiceError::Busy)?;
        {
            let cache = self
                .inner
                .cache
                .lock()
                .map_err(|_| ServiceError::Unavailable)?;
            if cache
                .completed
                .is_some_and(|instant| instant.elapsed() < MIN_SCAN_INTERVAL)
            {
                return Err(ServiceError::RateLimited);
            }
        }
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            // リクエストがキャンセルされてもワーカーの終了まではスロットを保持する。
            let _permit = permit;
            let result = inner
                .backend
                .scan(catalog, probe_versions)
                .map_err(|failure| match failure {
                    ScanFailure::UnsupportedPlatform => ServiceError::UnsupportedPlatform,
                    ScanFailure::Failed => ServiceError::ScanFailed,
                })
                .and_then(|snapshot| {
                    if !snapshot.validate() {
                        return Err(ServiceError::ScanFailed);
                    }
                    Ok(Arc::new(
                        snapshot.project(inner.policy.paths, inner.policy.home.as_deref()),
                    ))
                });
            let mut cache = inner.cache.lock().map_err(|_| ServiceError::Unavailable)?;
            cache.completed = Some(Instant::now());
            if let Ok(snapshot) = &result {
                cache.snapshot = Some(snapshot.clone());
            }
            result
        })
        .await
        .map_err(|_| ServiceError::ScanFailed)?
    }
}
