use inspector_core::{PathExposure, projects::*, storage::save_project_view};
use inspector_scanner::projects::{filesystem::canonical_project_folder, service::ProjectService};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Default)]
struct Selections {
    next: u64,
    paths: HashMap<String, PathBuf>,
}
#[derive(Clone)]
pub struct DesktopProjects {
    service: Option<ProjectService>,
    selections: Arc<Mutex<Selections>>,
    dialogs: Arc<Semaphore>,
    home: Option<PathBuf>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FolderSelection {
    pub token: String,
    pub path: Option<PathBuf>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFolderKind {
    Project,
    Venv,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickFolderRequest {
    pub kind: ProjectFolderKind,
    pub paths: PathExposure,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterProjectRequest {
    pub name: String,
    pub root_selection: String,
    pub extra_venv_selection: Option<String>,
    pub mcp_allowed: bool,
    pub paths: PathExposure,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProjectRequest {
    pub project_id: String,
    pub name: String,
    pub mcp_allowed: bool,
    pub paths: PathExposure,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRequest {
    pub project_id: String,
    pub paths: PathExposure,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveProjectRequest {
    pub project_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportProjectRequest {
    pub project_id: String,
    pub scan_revision: u64,
    pub paths: PathExposure,
}

impl DesktopProjects {
    pub fn new(service: Option<ProjectService>, home: Option<PathBuf>) -> Self {
        Self {
            service,
            selections: Arc::new(Mutex::new(Selections::default())),
            dialogs: Arc::new(Semaphore::new(1)),
            home,
        }
    }
    fn service(&self) -> Result<&ProjectService, ProjectError> {
        self.service
            .as_ref()
            .ok_or(ProjectError::ProjectStorageUnavailable)
    }
    fn dialog(&self) -> Result<OwnedSemaphorePermit, ProjectError> {
        self.dialogs
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProjectError::ProjectStorageBusy)
    }
    pub fn remember_selection(
        &self,
        path: &Path,
        exposure: PathExposure,
    ) -> Result<FolderSelection, ProjectError> {
        let path = canonical_project_folder(path)?;
        let mut selections = self
            .selections
            .lock()
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        if selections.paths.len() >= 8 {
            selections.paths.clear();
        }
        selections.next = selections
            .next
            .checked_add(1)
            .ok_or(ProjectError::ProjectStorageUnavailable)?;
        let token = format!("selection-{}", selections.next);
        selections.paths.insert(token.clone(), path.clone());
        Ok(FolderSelection {
            token,
            path: expose_project_path(&path, exposure, self.home.as_deref()),
        })
    }
    pub fn register(
        &self,
        request: RegisterProjectRequest,
    ) -> Result<ProjectSummary, ProjectError> {
        let (root, extra) = {
            let selections = self
                .selections
                .lock()
                .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
            (
                selections
                    .paths
                    .get(&request.root_selection)
                    .cloned()
                    .ok_or(ProjectError::InvalidProjectInput)?,
                request
                    .extra_venv_selection
                    .as_deref()
                    .map(|token| {
                        selections
                            .paths
                            .get(token)
                            .cloned()
                            .ok_or(ProjectError::InvalidProjectInput)
                    })
                    .transpose()?,
            )
        };
        let result = self.service()?.register(
            request.name,
            &root,
            extra.as_deref(),
            request.mcp_allowed,
            request.paths,
        )?;
        let mut selections = self
            .selections
            .lock()
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        selections.paths.remove(&request.root_selection);
        if let Some(token) = request.extra_venv_selection {
            selections.paths.remove(&token);
        }
        Ok(result)
    }
    pub fn export_view(&self, id: &str, revision: u64) -> Result<ProjectView, ProjectError> {
        let view = self.service()?.cached(id, PathExposure::Full, false)?;
        if view.snapshot.is_none() {
            return Err(ProjectError::ProjectScanFailed);
        }
        if view.scan_revision != Some(revision) {
            return Err(ProjectError::ProjectSnapshotChanged);
        }
        Ok(view)
    }
    pub fn write_export(
        &self,
        view: &ProjectView,
        path: &Path,
        exposure: PathExposure,
    ) -> Result<(), ProjectError> {
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("json"))
        {
            return Err(ProjectError::InvalidProjectInput);
        }
        save_project_view(path, view, exposure, self.home.as_deref())
            .map_err(|_| ProjectError::ProjectStorageUnavailable)
    }
}

#[tauri::command]
pub async fn choose_project_folder(
    window: tauri::WebviewWindow,
    service: tauri::State<'_, DesktopProjects>,
    request: PickFolderRequest,
) -> Result<Option<FolderSelection>, ProjectError> {
    let permit = service.dialog()?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let title = match request.kind {
            ProjectFolderKind::Project => "登録するプロジェクトを選択",
            ProjectFolderKind::Venv => "追加するvenvフォルダーを選択",
        };
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(title)
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|_| ProjectError::InvalidProjectFolder)?;
        service.remember_selection(&path, request.paths).map(Some)
    })
    .await
    .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn list_projects(
    service: tauri::State<'_, DesktopProjects>,
    paths: PathExposure,
) -> Result<Vec<ProjectSummary>, ProjectError> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.service()?.list(paths, false))
        .await
        .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn get_project_snapshot(
    service: tauri::State<'_, DesktopProjects>,
    request: ProjectRequest,
) -> Result<ProjectView, ProjectError> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service
            .service()?
            .cached(&request.project_id, request.paths, false)
    })
    .await
    .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn scan_project(
    service: tauri::State<'_, DesktopProjects>,
    request: ProjectRequest,
) -> Result<ProjectView, ProjectError> {
    service
        .service()?
        .inspect(&request.project_id, request.paths, false, true)
        .await
}
#[tauri::command]
pub async fn register_project(
    service: tauri::State<'_, DesktopProjects>,
    request: RegisterProjectRequest,
) -> Result<ProjectSummary, ProjectError> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.register(request))
        .await
        .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn update_project(
    service: tauri::State<'_, DesktopProjects>,
    request: UpdateProjectRequest,
) -> Result<ProjectSummary, ProjectError> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.service()?.update(
            &request.project_id,
            &request.name,
            request.mcp_allowed,
            request.paths,
        )
    })
    .await
    .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn remove_project(
    service: tauri::State<'_, DesktopProjects>,
    request: RemoveProjectRequest,
) -> Result<(), ProjectError> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.service()?.remove(&request.project_id))
        .await
        .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
#[tauri::command]
pub async fn export_project(
    window: tauri::WebviewWindow,
    service: tauri::State<'_, DesktopProjects>,
    request: ExportProjectRequest,
) -> Result<crate::commands::ExportOutcome, ProjectError> {
    let permit = service.dialog()?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let view = service.export_view(&request.project_id, request.scan_revision)?;
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("プロジェクト環境をJSONで保存")
            .add_filter("JSON", &["json"])
            .set_file_name("project-environment.json")
            .blocking_save_file()
        else {
            return Ok(crate::commands::ExportOutcome::Cancelled);
        };
        let path = path
            .into_path()
            .map_err(|_| ProjectError::InvalidProjectInput)?;
        service.write_export(&view, &path, request.paths)?;
        Ok(crate::commands::ExportOutcome::Saved)
    })
    .await
    .map_err(|_| ProjectError::ProjectStorageUnavailable)?
}
