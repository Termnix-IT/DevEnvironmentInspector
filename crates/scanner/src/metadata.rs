//! 外部メタデータの共通制限。文字列は命令や実行引数として使用しない。
use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use inspector_core::{
    Catalog, Diagnostic, DiagnosticCode, EvidenceClass, ProviderKind, Severity, SourceEvidence,
};
use serde::Deserialize;

use crate::{
    path_provider::{FileSystem, RealFileSystem, is_local_absolute},
    provider::{Candidate, Discovery},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WindowsHints {
    pub tools: Vec<WindowsHint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WindowsHint {
    pub id: String,
    pub application_names: Vec<String>,
    pub winget_ids: Vec<String>,
    pub executable_dirs: Vec<String>,
}
impl WindowsHints {
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../../../catalog/windows.json"))
            .expect("同梱Windowsカタログが不正です")
    }
}

pub(crate) fn reported_version(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    (!value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)))
    .then(|| value.to_string())
}

pub(crate) fn name_matches(name: &str, known: &str) -> bool {
    if name.eq_ignore_ascii_case(known) {
        return true;
    }
    let Some(prefix) = name.get(..known.len()) else {
        return false;
    };
    let suffix = name[known.len()..].trim_start();
    let suffix = suffix.strip_prefix("go").unwrap_or(suffix);
    prefix.eq_ignore_ascii_case(known)
        && name[known.len()..].starts_with(' ')
        && suffix.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && suffix.len() <= 80
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ._+-()".contains(&b))
}

pub(crate) fn warning(
    result: &mut Discovery,
    provider: ProviderKind,
    code: DiagnosticCode,
    tool: Option<&str>,
) {
    if let Some(id) = tool {
        if !result.incomplete_tools.iter().any(|t| t == id) {
            result.incomplete_tools.push(id.into());
        }
    } else {
        result.incomplete = true;
    }
    let diagnostic = Diagnostic {
        code,
        class: EvidenceClass::Fact,
        severity: Severity::Warning,
        provider: Some(provider),
        installation_id: None,
    };
    if !result.diagnostics.contains(&diagnostic) {
        result.diagnostics.push(diagnostic);
    }
}

pub(crate) fn add_candidate(
    fs: &dyn FileSystem,
    catalog: &Catalog,
    result: &mut Discovery,
    tool_id: &str,
    path: PathBuf,
    evidence: SourceEvidence,
) {
    if !is_local_absolute(&path) {
        return;
    }
    let Some(tool) = catalog.tools.iter().find(|t| t.id == tool_id) else {
        return;
    };
    let Some(command) = tool.commands.iter().find(|c| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case(c.as_str()))
    }) else {
        return;
    };
    if !path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["exe", "com", "cmd", "bat"]
            .iter()
            .any(|e| s.eq_ignore_ascii_case(e))
    }) {
        return;
    }
    match fs.inspect(&path) {
        Ok(Some(access)) => result.candidates.push(Candidate {
            tool_id: tool_id.into(),
            command: *command,
            path,
            on_path: false,
            path_order: None,
            access,
            evidence,
        }),
        Ok(None) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => warning(
            result,
            evidence.provider,
            if e.kind() == io::ErrorKind::PermissionDenied {
                DiagnosticCode::AccessDenied
            } else {
                DiagnosticCode::ProviderFailed
            },
            Some(tool_id),
        ),
    }
}

pub(crate) fn relative_path(value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if !path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let normal: PathBuf = path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();
    (!normal.as_os_str().is_empty()).then_some(normal)
}

pub trait MetadataFileSystem: FileSystem {
    fn read_bounded(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>>;
    /// 子ディレクトリ1階層のみ。上限到達時はboolを返す。
    fn child_directories(&self, path: &Path, limit: usize) -> io::Result<(Vec<PathBuf>, bool)>;
}

impl MetadataFileSystem for RealFileSystem {
    fn read_bounded(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        ensure_local_target(path)?;
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(bytes)
    }
    fn child_directories(&self, path: &Path, limit: usize) -> io::Result<(Vec<PathBuf>, bool)> {
        ensure_local_target(path)?;
        let mut result = Vec::new();
        for (index, entry) in std::fs::read_dir(path)?.enumerate() {
            if index >= limit {
                return Ok((result, true));
            }
            let entry = entry?;
            // junctionやsymlinkを階層探索しない。
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if entry.metadata()?.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink() {
                result.push(entry.path());
            }
        }
        result.sort();
        Ok((result, false))
    }
}

fn ensure_local_target(path: &Path) -> io::Result<()> {
    let resolved = std::fs::canonicalize(path)?;
    let text = resolved.to_string_lossy();
    let normal = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if !is_local_absolute(Path::new(normal)) || normal.starts_with("UNC\\") {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_complete_and_paths_are_bounded() {
        let catalog = Catalog::bundled().unwrap();
        let hints = WindowsHints::bundled();
        assert_eq!(hints.tools.len(), catalog.tools.len());
        for hint in hints.tools {
            assert!(catalog.tools.iter().any(|t| t.id == hint.id));
            for directory in hint.executable_dirs {
                assert!(directory.is_empty() || relative_path(&directory).is_some());
            }
        }
    }
    #[test]
    fn accepts_only_versions_and_precise_product_names() {
        assert_eq!(reported_version("v1.2.3"), Some("1.2.3".into()));
        assert!(reported_version("token=secret").is_none());
        assert!(reported_version("1.2\nsecret").is_none());
        assert!(name_matches("Python 3.14.1 (64-bit)", "Python"));
        assert!(name_matches("Git version 2.51.0", "Git version"));
        assert!(name_matches(
            "Go Programming Language amd64 go1.25.0",
            "Go Programming Language amd64"
        ));
        assert!(!name_matches("Python Launcher", "Python"));
        assert!(!name_matches("GitHub Desktop", "Git"));
        assert!(relative_path("../outside.exe").is_none());
        assert!(relative_path("C:/outside.exe").is_none());
    }
}
