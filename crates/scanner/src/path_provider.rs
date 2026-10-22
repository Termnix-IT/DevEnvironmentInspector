use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

use inspector_core::{
    AccessStatus, Catalog, Diagnostic, DiagnosticCode, EvidenceClass, ProviderKind, Severity,
    SourceEvidence,
};

use crate::provider::{Candidate, Discovery, DiscoveryProvider, ProviderFailure};

pub const MAX_PATH_ENTRIES: usize = 256;
const DEFAULT_EXTENSIONS: [&str; 4] = [".COM", ".EXE", ".BAT", ".CMD"];

#[derive(Debug, Clone)]
pub struct PathEnvironment {
    pub directories: Vec<PathBuf>,
    pub extensions: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

fn diagnostic(code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        code,
        class: EvidenceClass::Fact,
        severity: Severity::Warning,
        provider: Some(ProviderKind::Path),
        installation_id: None,
    }
}

impl PathEnvironment {
    pub fn current() -> Self {
        Self::from_parts(
            std::env::var_os("PATH").as_deref(),
            std::env::var_os("PATHEXT").as_deref(),
        )
    }

    pub fn from_parts(path: Option<&OsStr>, pathext: Option<&OsStr>) -> Self {
        let directories: Vec<_> = path
            .map(std::env::split_paths)
            .into_iter()
            .flatten()
            .collect();
        Self::new(directories, pathext.and_then(OsStr::to_str))
    }

    pub fn new(directories: Vec<PathBuf>, pathext: Option<&str>) -> Self {
        let mut diagnostics = Vec::new();
        let mut seen = HashSet::new();
        let mut approved = Vec::new();
        let mut skipped = false;
        for (index, path) in directories.into_iter().enumerate() {
            if index >= MAX_PATH_ENTRIES || !is_local_absolute(&path) {
                skipped = true;
                continue;
            }
            if seen.insert(path_key(&path)) {
                approved.push(path);
            }
        }
        if skipped {
            diagnostics.push(diagnostic(DiagnosticCode::PathEntriesSkipped));
        }
        let mut extensions = Vec::new();
        let mut unsupported = false;
        for extension in pathext.unwrap_or(".COM;.EXE;.BAT;.CMD").split(';') {
            let upper = extension.to_ascii_uppercase();
            if DEFAULT_EXTENSIONS.contains(&upper.as_str()) {
                if !extensions.contains(&upper) {
                    extensions.push(upper);
                }
            } else if !extension.is_empty() {
                unsupported = true;
            }
        }
        if unsupported {
            diagnostics.push(diagnostic(DiagnosticCode::UnsupportedPathExtension));
        }
        Self {
            directories: approved,
            extensions,
            diagnostics,
        }
    }
}

pub(crate) fn path_key(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn is_local_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    path.is_absolute()
        && !text.starts_with(r"\\")
        && !text.starts_with("//")
        && !path.components().any(|p| matches!(p, Component::ParentDir))
}

/// テスト時に権限エラーなどをOSの設定変更なしで再現する境界。
pub trait FileSystem: Send + Sync {
    /// 存在を確認できた通常ファイルだけSomeを返す。存在確認の失敗と読取拒否を区別する。
    fn inspect(&self, path: &Path) -> io::Result<Option<AccessStatus>>;
}

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn inspect(&self, path: &Path) -> io::Result<Option<AccessStatus>> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Ok(None);
        }
        let access = match File::open(path) {
            Ok(_) => AccessStatus::Readable,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => AccessStatus::Denied,
            Err(_) => AccessStatus::Unknown,
        };
        Ok(Some(access))
    }
}

pub struct PathProvider<F = RealFileSystem> {
    pub environment: PathEnvironment,
    pub file_system: F,
}

impl<F: FileSystem> DiscoveryProvider for PathProvider<F> {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Path
    }

    fn discover(&self, catalog: &Catalog) -> Result<Discovery, ProviderFailure> {
        let mut result = Discovery {
            diagnostics: self.environment.diagnostics.clone(),
            incomplete: !self.environment.diagnostics.is_empty(),
            ..Discovery::default()
        };
        for tool in &catalog.tools {
            for command in &tool.commands {
                for (directory_index, directory) in self.environment.directories.iter().enumerate()
                {
                    for (extension_index, extension) in
                        self.environment.extensions.iter().enumerate()
                    {
                        let path = directory.join(format!("{}{extension}", command.as_str()));
                        let access = match self.file_system.inspect(&path) {
                            Ok(Some(access)) => access,
                            Ok(None) => continue,
                            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                            Err(error) => {
                                if !result.incomplete_tools.contains(&tool.id) {
                                    result.incomplete_tools.push(tool.id.clone());
                                }
                                let code = if error.kind() == io::ErrorKind::PermissionDenied {
                                    DiagnosticCode::AccessDenied
                                } else {
                                    DiagnosticCode::ProviderFailed
                                };
                                if !result.diagnostics.iter().any(|d| d.code == code) {
                                    result.diagnostics.push(diagnostic(code));
                                }
                                continue;
                            }
                        };
                        result.candidates.push(Candidate {
                            tool_id: tool.id.clone(),
                            command: *command,
                            path,
                            on_path: true,
                            path_order: Some(
                                directory_index * DEFAULT_EXTENSIONS.len() + extension_index,
                            ),
                            access,
                            evidence: SourceEvidence {
                                provider: ProviderKind::Path,
                                reported_version: None,
                                install_method: None,
                            },
                        });
                    }
                }
            }
        }
        Ok(result)
    }
}
