use inspector_core::{Catalog, ScanSnapshot};
use inspector_scanner::{
    NativeVersionRunner, PathEnvironment, PathProvider, RealFileSystem, ScanEngine, ScanOptions,
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

pub struct WindowsScanner;

impl EnvironmentScanner for WindowsScanner {
    fn scan(&self, catalog: Catalog, probe_versions: bool) -> Result<ScanSnapshot, ScanFailure> {
        if !cfg!(windows) {
            return Err(ScanFailure::UnsupportedPlatform);
        }
        let path = PathEnvironment::current();
        let runner = NativeVersionRunner::new(&path);
        let provider = PathProvider {
            environment: path,
            file_system: RealFileSystem,
        };
        let engine = ScanEngine::new(catalog, vec![Box::new(provider)], Box::new(runner));
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
