use crate::service::{DesktopError, DesktopService, ExportRequest, ScanRequest, SnapshotView};
use inspector_core::{Catalog, PathExposure};
use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub fn get_catalog(service: tauri::State<'_, DesktopService>) -> Catalog {
    service.catalog()
}

#[tauri::command]
pub fn get_snapshot(
    service: tauri::State<'_, DesktopService>,
    paths: PathExposure,
) -> Result<Option<SnapshotView>, DesktopError> {
    service.snapshot(paths)
}

#[tauri::command]
pub async fn scan_environment(
    service: tauri::State<'_, DesktopService>,
    request: ScanRequest,
) -> Result<SnapshotView, DesktopError> {
    service.scan(request).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportOutcome {
    Saved,
    Cancelled,
}

#[tauri::command]
pub async fn export_snapshot(
    window: tauri::WebviewWindow,
    service: tauri::State<'_, DesktopService>,
    request: ExportRequest,
) -> Result<ExportOutcome, DesktopError> {
    let (snapshot, permit) = service.begin_export(request.revision)?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let path = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("検出結果をJSONで保存")
            .add_filter("JSON", &["json"])
            .set_file_name("dev-environment.json")
            .blocking_save_file();
        let Some(path) = path else {
            return Ok(ExportOutcome::Cancelled);
        };
        let path = path
            .into_path()
            .map_err(|_| DesktopError::InvalidDestination)?;
        service.write_export(&snapshot, &path, request.paths)?;
        Ok(ExportOutcome::Saved)
    })
    .await
    .map_err(|_| DesktopError::ExportFailed)?
}
