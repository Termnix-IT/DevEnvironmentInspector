use cap_std::{ambient_authority, fs::Dir};
use inspector_core::{
    project_store::local_absolute,
    projects::{ProjectError, ProjectFinding},
};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub struct DirectoryEntries {
    pub names: Vec<String>,
    pub visited: usize,
    pub truncated: bool,
    pub links_skipped: bool,
}
/// 既知メタデータだけを読むための境界。テストではアクセス失敗等を差し替える。
pub trait ProjectDirectory: Send {
    fn read(&self, relative: &Path, limit: usize) -> Result<Option<Vec<u8>>, ProjectFinding>;
    fn directories(
        &self,
        relative: &Path,
        limit: usize,
    ) -> Result<DirectoryEntries, ProjectFinding>;
    fn regular_file(&self, relative: &Path) -> Result<bool, ProjectFinding>;
}
pub trait ProjectFileSystem: Send + Sync + 'static {
    fn open(&self, root: &Path) -> Result<Box<dyn ProjectDirectory>, ProjectFinding>;
}
pub struct NativeProjectFileSystem;
struct NativeProjectDirectory(Dir);

fn io_finding(error: io::Error) -> ProjectFinding {
    if error.kind() == io::ErrorKind::PermissionDenied {
        ProjectFinding::AccessDenied
    } else {
        ProjectFinding::ReadFailed
    }
}

pub fn canonical_project_folder(path: &Path) -> Result<PathBuf, ProjectError> {
    if !local_absolute(path) {
        return Err(ProjectError::InvalidProjectFolder);
    }
    // 登録ルート自身・その祖先がリンクなら、明示した範囲として固定できないため拒否。
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let metadata =
            fs::symlink_metadata(ancestor).map_err(|_| ProjectError::InvalidProjectFolder)?;
        let mut linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            linked |= metadata.file_attributes() & 0x400 != 0;
        }
        if linked {
            return Err(ProjectError::InvalidProjectFolder);
        }
    }
    let canonical = fs::canonicalize(path).map_err(|_| ProjectError::InvalidProjectFolder)?;
    #[cfg(windows)]
    let canonical = PathBuf::from(
        canonical
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .ok_or(ProjectError::InvalidProjectFolder)?,
    );
    if !local_absolute(&canonical) || !fs::metadata(&canonical).is_ok_and(|m| m.is_dir()) {
        return Err(ProjectError::InvalidProjectFolder);
    }
    Ok(canonical)
}
impl ProjectFileSystem for NativeProjectFileSystem {
    fn open(&self, root: &Path) -> Result<Box<dyn ProjectDirectory>, ProjectFinding> {
        let root = canonical_project_folder(root).map_err(|_| ProjectFinding::AccessDenied)?;
        Dir::open_ambient_dir(root, ambient_authority())
            .map(|dir| Box::new(NativeProjectDirectory(dir)) as Box<dyn ProjectDirectory>)
            .map_err(io_finding)
    }
}
impl NativeProjectDirectory {
    fn checked_metadata(
        &self,
        path: &Path,
    ) -> Result<Option<cap_std::fs::Metadata>, ProjectFinding> {
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(ProjectFinding::LinkedPathSkipped);
        }
        let mut prefix = PathBuf::new();
        let mut last = None;
        for component in path.components() {
            prefix.push(component);
            let metadata = match self.0.symlink_metadata(&prefix) {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(io_finding(error)),
            };
            let mut linked = metadata.file_type().is_symlink();
            #[cfg(windows)]
            {
                use cap_std::fs::MetadataExt;
                linked |= metadata.file_attributes() & 0x400 != 0;
            }
            if linked {
                return Err(ProjectFinding::LinkedPathSkipped);
            }
            last = Some(metadata);
        }
        if path == Path::new(".") {
            return self.0.dir_metadata().map(Some).map_err(io_finding);
        }
        Ok(last)
    }
}
impl ProjectDirectory for NativeProjectDirectory {
    fn read(&self, relative: &Path, limit: usize) -> Result<Option<Vec<u8>>, ProjectFinding> {
        let Some(metadata) = self.checked_metadata(relative)? else {
            return Ok(None);
        };
        if !metadata.is_file() {
            return Err(ProjectFinding::MetadataInvalid);
        }
        if metadata.len() > limit as u64 {
            return Err(ProjectFinding::ReadLimitReached);
        }
        let file = self.0.open(relative).map_err(io_finding)?;
        if !file.metadata().map_err(io_finding)?.is_file() {
            return Err(ProjectFinding::MetadataInvalid);
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_finding)?;
        if bytes.len() > limit {
            return Err(ProjectFinding::ReadLimitReached);
        }
        Ok(Some(bytes))
    }
    fn directories(
        &self,
        relative: &Path,
        limit: usize,
    ) -> Result<DirectoryEntries, ProjectFinding> {
        if self.checked_metadata(relative)?.is_none() {
            return Ok(DirectoryEntries {
                names: Vec::new(),
                visited: 0,
                truncated: false,
                links_skipped: false,
            });
        }
        let mut result = DirectoryEntries {
            names: Vec::new(),
            visited: 0,
            truncated: false,
            links_skipped: false,
        };
        for (index, entry) in self.0.read_dir(relative).map_err(io_finding)?.enumerate() {
            if index >= limit {
                result.truncated = true;
                break;
            }
            result.visited += 1;
            let entry = entry.map_err(io_finding)?;
            let kind = entry.file_type().map_err(io_finding)?;
            if kind.is_symlink() {
                result.links_skipped = true;
                continue;
            }
            if kind.is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                result.names.push(name.to_string());
            }
        }
        result.names.sort();
        Ok(result)
    }
    fn regular_file(&self, relative: &Path) -> Result<bool, ProjectFinding> {
        Ok(self
            .checked_metadata(relative)?
            .is_some_and(|m| m.is_file()))
    }
}
