mod commands;
pub mod projects;
pub mod service;

use inspector_core::Catalog;
use inspector_core::project_store::JsonProjectRepository;
use inspector_scanner::projects::{MetadataProjectScanner, service::ProjectService};
use projects::DesktopProjects;
use service::{DesktopService, NativeScanner};
use std::{path::PathBuf, sync::Arc};

pub fn run() -> tauri::Result<()> {
    let catalog = Catalog::bundled().expect("同梱カタログが不正です");
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let projects = JsonProjectRepository::default_path().map(|path| {
        ProjectService::new(
            Arc::new(JsonProjectRepository { path }),
            Arc::new(MetadataProjectScanner::default()),
            home.clone(),
        )
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DesktopService::new(catalog, Arc::new(NativeScanner), home))
        .manage(DesktopProjects::new(
            projects,
            std::env::var_os("USERPROFILE").map(PathBuf::from),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_catalog,
            commands::get_snapshot,
            commands::scan_environment,
            commands::export_snapshot,
            projects::choose_project_folder,
            projects::list_projects,
            projects::get_project_snapshot,
            projects::scan_project,
            projects::register_project,
            projects::update_project,
            projects::remove_project,
            projects::export_project
        ])
        .run(tauri::generate_context!())
}
