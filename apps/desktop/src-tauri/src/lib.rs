mod commands;
pub mod service;

use inspector_core::Catalog;
use service::{DesktopService, NativeScanner};
use std::{path::PathBuf, sync::Arc};

pub fn run() -> tauri::Result<()> {
    let catalog = Catalog::bundled().expect("同梱カタログが不正です");
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DesktopService::new(catalog, Arc::new(NativeScanner), home))
        .invoke_handler(tauri::generate_handler![
            commands::get_catalog,
            commands::get_snapshot,
            commands::scan_environment,
            commands::export_snapshot
        ])
        .run(tauri::generate_context!())
}
