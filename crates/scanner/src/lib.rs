//! 許可済みの検出プロバイダーとバージョン取得。GUI・MCPには依存しない。
pub mod engine;
pub mod path_provider;
pub mod process;
pub mod provider;
pub mod version;

pub use engine::{ScanEngine, ScanOptions, VersionRunner};
pub use path_provider::{PathEnvironment, PathProvider, RealFileSystem};
pub use process::NativeVersionRunner;
