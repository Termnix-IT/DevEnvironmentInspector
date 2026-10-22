//! GUIと独立した読み取り中心のMCPサーバー。
pub mod backend;
pub mod handler;
pub mod service;

pub use handler::InspectorServer;
pub use service::{InspectionService, ServerPolicy};
