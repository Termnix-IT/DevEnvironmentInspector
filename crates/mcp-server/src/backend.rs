use inspector_core::{Catalog, ScanSnapshot};
use inspector_scanner::{
    ScanOptions,
    platform::{path_engine, windows_engine},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScanFailure {
    #[error("実機スキャンはWindows専用です")]
    UnsupportedPlatform,
    #[error("スキャンを完了できませんでした")]
    Failed,
}

/// ブロッキングワーカー上で呼ぶ。テストではOS・CLIに触れない実装に差し替える。
pub trait EnvironmentScanner: Send + Sync + 'static {
    fn scan(&self, catalog: Catalog, probe_versions: bool) -> Result<ScanSnapshot, ScanFailure>;
}

pub struct WindowsScanner {
    pub path_only: bool,
}

impl EnvironmentScanner for WindowsScanner {
    fn scan(&self, catalog: Catalog, probe_versions: bool) -> Result<ScanSnapshot, ScanFailure> {
        if !cfg!(windows) {
            return Err(ScanFailure::UnsupportedPlatform);
        }
        let engine = if self.path_only {
            path_engine(catalog)
        } else {
            windows_engine(catalog)
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ScanFailure::Failed)?;
        runtime
            .block_on(engine.scan(&ScanOptions {
                probe_versions,
                ..ScanOptions::default()
            }))
            .map_err(|_| ScanFailure::Failed)
    }
}
