//! 許可済みの検出プロバイダーとバージョン取得。GUI・MCPには依存しない。
pub mod engine;
pub mod known_provider;
pub mod metadata;
pub mod npm_provider;
pub mod path_provider;
pub mod platform;
pub mod process;
pub mod provider;
pub mod registry_provider;
pub mod version;

pub use engine::{ScanEngine, ScanOptions, VersionRunner};
pub use path_provider::{PathEnvironment, PathProvider, RealFileSystem};
pub use process::NativeVersionRunner;
