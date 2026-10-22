//! GUIだけが登録情報を書き、MCPは最新の許可を毎回読み取る。
use crate::projects::*;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use time::OffsetDateTime;

const MAX_REGISTRY_BYTES: u64 = 256 * 1024;

pub trait ProjectRepository: Send + Sync + 'static {
    fn load(&self) -> Result<ProjectRegistry, ProjectError>;
    fn register(&self, project: NewProject) -> Result<ProjectRegistration, ProjectError>;
    fn update(
        &self,
        id: &str,
        name: &str,
        allowed: bool,
    ) -> Result<ProjectRegistration, ProjectError>;
    fn remove(&self, id: &str) -> Result<(), ProjectError>;
}

pub struct JsonProjectRepository {
    pub path: PathBuf,
}
impl JsonProjectRepository {
    /// 起動時にはディレクトリもファイルも作らない。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| local_absolute(p))
            .map(|p| p.join("EnvKit").join("projects.json"))
    }
    fn transaction<T>(
        &self,
        change: impl FnOnce(&mut ProjectRegistry) -> Result<T, ProjectError>,
    ) -> Result<T, ProjectError> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(ProjectError::ProjectStorageUnavailable)?;
        std::fs::create_dir_all(parent).map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join("projects.lock"))
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => ProjectError::ProjectStorageBusy,
            _ => ProjectError::ProjectStorageUnavailable,
        })?;
        // ロック後に最新のデータを読む。破損データは空データとして上書きしない。
        let mut registry = self.load()?;
        let result = change(&mut registry)?;
        validate(&registry)?;
        let bytes = serde_json::to_vec_pretty(&registry)
            .map_err(|_| ProjectError::ProjectStorageInvalid)?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(ProjectError::ProjectLimitReached);
        }
        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.write_all(b"\n"))
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        temporary
            .persist(&self.path)
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        Ok(result)
    }
}
impl ProjectRepository for JsonProjectRepository {
    fn load(&self) -> Result<ProjectRegistry, ProjectError> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProjectRegistry::default());
            }
            Err(_) => return Err(ProjectError::ProjectStorageUnavailable),
        };
        let mut bytes = Vec::new();
        file.take(MAX_REGISTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ProjectError::ProjectStorageUnavailable)?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(ProjectError::ProjectStorageInvalid);
        }
        let registry: ProjectRegistry =
            serde_json::from_slice(&bytes).map_err(|_| ProjectError::ProjectStorageInvalid)?;
        validate(&registry)?;
        Ok(registry)
    }
    fn register(&self, project: NewProject) -> Result<ProjectRegistration, ProjectError> {
        if !valid_project_name(&project.name)
            || !local_absolute(&project.root)
            || project
                .extra_venv
                .as_deref()
                .is_some_and(|p| !local_absolute(p))
        {
            return Err(ProjectError::InvalidProjectInput);
        }
        self.transaction(|registry| {
            if registry.projects.len() >= MAX_PROJECTS {
                return Err(ProjectError::ProjectLimitReached);
            }
            if registry
                .projects
                .iter()
                .any(|p| same_path(&p.root, &project.root))
            {
                return Err(ProjectError::ProjectAlreadyRegistered);
            }
            let registration = ProjectRegistration {
                id: format!("project-{}", registry.next_id),
                name: project.name,
                root: project.root,
                extra_venv: project.extra_venv,
                mcp_allowed: project.mcp_allowed,
                revision: 1,
                created_at: OffsetDateTime::now_utc(),
            };
            registry.next_id = registry
                .next_id
                .checked_add(1)
                .ok_or(ProjectError::ProjectLimitReached)?;
            registry.projects.push(registration.clone());
            Ok(registration)
        })
    }
    fn update(
        &self,
        id: &str,
        name: &str,
        allowed: bool,
    ) -> Result<ProjectRegistration, ProjectError> {
        if !valid_project_id(id) || !valid_project_name(name) {
            return Err(ProjectError::InvalidProjectInput);
        }
        self.transaction(|registry| {
            let project = registry
                .projects
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or(ProjectError::ProjectNotFound)?;
            project.name = name.into();
            project.mcp_allowed = allowed;
            project.revision = project
                .revision
                .checked_add(1)
                .ok_or(ProjectError::ProjectLimitReached)?;
            Ok(project.clone())
        })
    }
    fn remove(&self, id: &str) -> Result<(), ProjectError> {
        if !valid_project_id(id) {
            return Err(ProjectError::InvalidProjectInput);
        }
        self.transaction(|registry| {
            let index = registry
                .projects
                .iter()
                .position(|p| p.id == id)
                .ok_or(ProjectError::ProjectNotFound)?;
            registry.projects.remove(index);
            Ok(())
        })
    }
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}
pub fn local_absolute(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    if path.as_os_str().len() > 32760
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return false;
    }
    if cfg!(windows) {
        let mut components = path.components();
        matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
            && matches!(components.next(), Some(Component::RootDir))
    } else {
        path.is_absolute()
    }
}
fn validate(registry: &ProjectRegistry) -> Result<(), ProjectError> {
    if registry.schema_version != PROJECT_SCHEMA_VERSION
        || registry.next_id == 0
        || registry.projects.len() > MAX_PROJECTS
    {
        return Err(ProjectError::ProjectStorageInvalid);
    }
    let mut ids = HashSet::new();
    let mut roots: Vec<&Path> = Vec::new();
    for project in &registry.projects {
        if !valid_project_id(&project.id)
            || !ids.insert(&project.id)
            || !valid_project_name(&project.name)
            || !local_absolute(&project.root)
            || project
                .extra_venv
                .as_deref()
                .is_some_and(|p| !local_absolute(p))
            || project.revision == 0
            || !project.id[8..]
                .parse::<u64>()
                .is_ok_and(|id| id > 0 && id < registry.next_id)
            || roots.iter().any(|p| same_path(p, &project.root))
        {
            return Err(ProjectError::ProjectStorageInvalid);
        }
        roots.push(project.root.as_path());
    }
    Ok(())
}
