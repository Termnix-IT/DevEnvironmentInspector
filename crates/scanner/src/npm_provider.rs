use crate::{
    metadata::{MetadataFileSystem, add_candidate, relative_path, reported_version, warning},
    path_provider::{RealFileSystem, is_local_absolute, path_key},
    provider::{Discovery, DiscoveryProvider, ProviderFailure, RegistrationObservation},
};
use inspector_core::{Catalog, DiagnosticCode, InstallMethod, ProviderKind, SourceEvidence};
use serde::Deserialize;
use std::{collections::HashSet, io, path::PathBuf};

const PACKAGES: [(&str, &str); 5] = [
    ("npm", "npm"),
    ("pnpm", "pnpm"),
    ("codex", "@openai/codex"),
    ("claude", "@anthropic-ai/claude-code"),
    ("gemini", "@google/gemini-cli"),
];

pub struct NpmGlobalProvider<F = RealFileSystem> {
    pub prefixes: Vec<PathBuf>,
    pub file_system: F,
}
#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    bin: serde_json::Value,
}
impl<F: MetadataFileSystem> DiscoveryProvider for NpmGlobalProvider<F> {
    fn kind(&self) -> ProviderKind {
        ProviderKind::NpmGlobal
    }
    fn discover(&self, catalog: &Catalog) -> Result<Discovery, ProviderFailure> {
        let mut result = Discovery::default();
        let mut seen = HashSet::new();
        let roots: Vec<_> = self
            .prefixes
            .iter()
            .filter(|p| is_local_absolute(p) && seen.insert(path_key(p)))
            .collect();
        if roots.len() > 256 {
            warning(
                &mut result,
                self.kind(),
                DiagnosticCode::MetadataLimitReached,
                None,
            );
        }
        for prefix in roots.into_iter().take(256) {
            for (id, package_name) in PACKAGES {
                let Some(tool) = catalog.tools.iter().find(|t| t.id == id) else {
                    continue;
                };
                let root = prefix.join("node_modules").join(package_name);
                let bytes = match self
                    .file_system
                    .read_bounded(&root.join("package.json"), 256 * 1024)
                {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        warning(
                            &mut result,
                            self.kind(),
                            if e.kind() == io::ErrorKind::PermissionDenied {
                                DiagnosticCode::AccessDenied
                            } else {
                                DiagnosticCode::MetadataInvalid
                            },
                            Some(id),
                        );
                        continue;
                    }
                };
                let package: Package = match serde_json::from_slice(&bytes) {
                    Ok(package) => package,
                    Err(_) => {
                        warning(
                            &mut result,
                            self.kind(),
                            DiagnosticCode::MetadataInvalid,
                            Some(id),
                        );
                        continue;
                    }
                };
                if package.name != package_name || reported_version(&package.version).is_none() {
                    warning(
                        &mut result,
                        self.kind(),
                        DiagnosticCode::MetadataInvalid,
                        Some(id),
                    );
                    continue;
                }
                let evidence = SourceEvidence {
                    provider: self.kind(),
                    reported_version: reported_version(&package.version),
                    // npm本体はNode.jsに同梱される場合があり、導入方式を確定しない。
                    install_method: (id != "npm").then_some(InstallMethod::Npm),
                };
                result.registrations.push(RegistrationObservation {
                    tool_id: id.into(),
                    identity: format!("npm:{}", path_key(&root)),
                    package_id: Some(package_name.into()),
                    install_location: Some(root.clone()),
                    evidence: evidence.clone(),
                });
                for command in &tool.commands {
                    let bin = package
                        .bin
                        .as_str()
                        .or_else(|| package.bin.get(command.as_str()).and_then(|v| v.as_str()));
                    let Some(bin) = bin.filter(|b| b.len() <= 256).and_then(relative_path) else {
                        warning(
                            &mut result,
                            self.kind(),
                            DiagnosticCode::MetadataInvalid,
                            Some(id),
                        );
                        continue;
                    };
                    add_candidate(
                        &self.file_system,
                        catalog,
                        &mut result,
                        id,
                        root.join(&bin),
                        evidence.clone(),
                    );
                    let shim = prefix.join(format!("{}.cmd", command.as_str()));
                    match self.file_system.read_bounded(&shim, 16 * 1024) {
                        Ok(bytes) => {
                            let target = format!(
                                "node_modules/{package_name}/{}",
                                bin.to_string_lossy().replace('\\', "/")
                            );
                            if std::str::from_utf8(&bytes)
                                .is_ok_and(|s| s.replace('\\', "/").contains(&target))
                            {
                                add_candidate(
                                    &self.file_system,
                                    catalog,
                                    &mut result,
                                    id,
                                    shim,
                                    evidence.clone(),
                                );
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(_) => warning(
                            &mut result,
                            self.kind(),
                            DiagnosticCode::MetadataInvalid,
                            Some(id),
                        ),
                    }
                }
            }
        }
        Ok(result)
    }
}
