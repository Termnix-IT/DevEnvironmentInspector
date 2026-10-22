use crate::{
    metadata::{MetadataFileSystem, add_candidate, warning},
    path_provider::{RealFileSystem, is_local_absolute, path_key},
    provider::{Discovery, DiscoveryProvider, ProviderFailure},
};
use inspector_core::{Catalog, DiagnosticCode, ProviderKind, SourceEvidence};
use std::{collections::HashSet, io, path::PathBuf};

#[derive(Clone)]
pub struct KnownLocation {
    pub tool_id: String,
    pub root: PathBuf,
    pub child_prefix: Option<String>,
    pub executable_directory: PathBuf,
}
pub struct KnownDirectoryProvider<F = RealFileSystem> {
    pub locations: Vec<KnownLocation>,
    pub file_system: F,
}
impl<F: MetadataFileSystem> DiscoveryProvider for KnownDirectoryProvider<F> {
    fn kind(&self) -> ProviderKind {
        ProviderKind::KnownDirectory
    }
    fn discover(&self, catalog: &Catalog) -> Result<Discovery, ProviderFailure> {
        let mut result = Discovery::default();
        let mut seen = HashSet::new();
        if self.locations.len() > 128 {
            warning(
                &mut result,
                self.kind(),
                DiagnosticCode::MetadataLimitReached,
                None,
            );
        }
        for location in self.locations.iter().take(128) {
            let Some(tool) = catalog.tools.iter().find(|t| t.id == location.tool_id) else {
                continue;
            };
            if !is_local_absolute(&location.root) {
                continue;
            }
            let roots = if let Some(prefix) = &location.child_prefix {
                match self.file_system.child_directories(&location.root, 128) {
                    Ok((children, truncated)) => {
                        if truncated {
                            warning(
                                &mut result,
                                self.kind(),
                                DiagnosticCode::MetadataLimitReached,
                                Some(&tool.id),
                            );
                        }
                        children
                            .into_iter()
                            .filter(|p| {
                                p.file_name().is_some_and(|name| {
                                    name.to_string_lossy()
                                        .to_ascii_lowercase()
                                        .starts_with(&prefix.to_ascii_lowercase())
                                })
                            })
                            .collect()
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        warning(
                            &mut result,
                            self.kind(),
                            if e.kind() == io::ErrorKind::PermissionDenied {
                                DiagnosticCode::AccessDenied
                            } else {
                                DiagnosticCode::ProviderFailed
                            },
                            Some(&tool.id),
                        );
                        continue;
                    }
                }
            } else {
                vec![location.root.clone()]
            };
            for root in roots {
                for command in &tool.commands {
                    for extension in ["exe", "cmd"] {
                        let path = root
                            .join(&location.executable_directory)
                            .join(format!("{}.{extension}", command.as_str()));
                        if seen.insert((tool.id.clone(), path_key(&path))) {
                            add_candidate(
                                &self.file_system,
                                catalog,
                                &mut result,
                                &tool.id,
                                path,
                                SourceEvidence {
                                    provider: self.kind(),
                                    reported_version: None,
                                    install_method: None,
                                },
                            );
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
