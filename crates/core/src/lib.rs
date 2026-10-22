//! GUI・MCPに依存しない共通モデル、カタログ、保存境界。
pub mod catalog;
pub mod model;
pub mod storage;

pub use catalog::{Catalog, Category, KnownCommand, ToolDefinition};
pub use model::*;
