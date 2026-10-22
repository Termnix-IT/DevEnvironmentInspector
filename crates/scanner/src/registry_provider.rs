//! ARP・WinGetの明示マーカー・既知ランタイムキーを読み取り専用で取得する。
use crate::{
    metadata::{WindowsHints, add_candidate, name_matches, reported_version, warning},
    path_provider::{FileSystem, is_local_absolute},
    provider::{Discovery, DiscoveryProvider, ProviderFailure, RegistrationObservation},
};
use inspector_core::{Catalog, DiagnosticCode, InstallMethod, ProviderKind, SourceEvidence};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Clone, Default)]
pub struct RegisteredProduct {
    pub identity: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub winget_id: Option<String>,
    pub portable_target: Option<PathBuf>,
}
#[derive(Debug, Clone)]
pub struct RegisteredRuntime {
    pub identity: String,
    pub tool_id: String,
    pub version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub executable: Option<PathBuf>,
}
#[derive(Debug, Clone, Default)]
pub struct RegistryInventory {
    pub applications: Vec<RegisteredProduct>,
    pub runtimes: Vec<RegisteredRuntime>,
    pub incomplete: bool,
}
pub trait RegistrySource: Send + Sync {
    fn read(&self) -> RegistryInventory;
}
pub struct SystemRegistry;

pub struct RegistryProvider {
    kind: ProviderKind,
    source: Arc<dyn RegistrySource>,
    inventory: Arc<OnceLock<RegistryInventory>>,
    file_system: Arc<dyn FileSystem>,
}
impl RegistryProvider {
    /// 同一スキャン中は同じ読み取り結果を使い、情報源間の時点差を避ける。
    pub fn providers(
        source: Arc<dyn RegistrySource>,
        file_system: Arc<dyn FileSystem>,
    ) -> Vec<Box<dyn DiscoveryProvider>> {
        let inventory = Arc::new(OnceLock::new());
        [
            ProviderKind::InstalledApplications,
            ProviderKind::Winget,
            ProviderKind::Registry,
        ]
        .into_iter()
        .map(|kind| {
            Box::new(Self {
                kind,
                source: source.clone(),
                inventory: inventory.clone(),
                file_system: file_system.clone(),
            }) as Box<dyn DiscoveryProvider>
        })
        .collect()
    }
}
impl DiscoveryProvider for RegistryProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }
    fn discover(&self, catalog: &Catalog) -> Result<Discovery, ProviderFailure> {
        let inventory = self.inventory.get_or_init(|| self.source.read());
        let hints = WindowsHints::bundled();
        let mut result = Discovery::default();
        if inventory.incomplete {
            warning(&mut result, self.kind, DiagnosticCode::ProviderFailed, None);
        }
        if self.kind == ProviderKind::Registry {
            for runtime in inventory.runtimes.iter().take(512) {
                if !catalog.tools.iter().any(|t| t.id == runtime.tool_id) {
                    continue;
                }
                let evidence = evidence(self.kind, runtime.version.as_deref());
                let location = runtime
                    .install_location
                    .clone()
                    .filter(|p| is_local_absolute(p));
                result.registrations.push(RegistrationObservation {
                    tool_id: runtime.tool_id.clone(),
                    identity: runtime.identity.clone(),
                    package_id: None,
                    install_location: location.clone(),
                    evidence: evidence.clone(),
                });
                if let Some(path) = &runtime.executable {
                    add_candidate(
                        &*self.file_system,
                        catalog,
                        &mut result,
                        &runtime.tool_id,
                        path.clone(),
                        evidence.clone(),
                    );
                }
                if let Some(location) = location {
                    probe_directory(
                        &*self.file_system,
                        catalog,
                        &hints,
                        &mut result,
                        &runtime.tool_id,
                        &location,
                        &evidence,
                    );
                }
            }
        } else {
            for app in inventory.applications.iter().take(4096) {
                for hint in &hints.tools {
                    if !catalog.tools.iter().any(|t| t.id == hint.id) {
                        continue;
                    }
                    let package_id = app.winget_id.as_ref().filter(|id| {
                        hint.winget_ids
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(id))
                    });
                    let matched = if self.kind == ProviderKind::Winget {
                        package_id.is_some()
                    } else {
                        app.display_name.as_deref().is_some_and(|name| {
                            hint.application_names
                                .iter()
                                .any(|known| name_matches(name, known))
                        })
                    };
                    if !matched {
                        continue;
                    }
                    if self.kind == ProviderKind::InstalledApplications
                        && app.winget_id.as_ref().is_some_and(|id| {
                            hints.tools.iter().any(|other| {
                                other.id != hint.id
                                    && other
                                        .winget_ids
                                        .iter()
                                        .any(|known| known.eq_ignore_ascii_case(id))
                            })
                        })
                    {
                        warning(
                            &mut result,
                            self.kind,
                            DiagnosticCode::MetadataInvalid,
                            Some(&hint.id),
                        );
                        continue;
                    }
                    if app
                        .version
                        .as_deref()
                        .is_some_and(|v| reported_version(v).is_none())
                        || app
                            .install_location
                            .as_deref()
                            .is_some_and(|p| !is_local_absolute(p))
                    {
                        warning(
                            &mut result,
                            self.kind,
                            DiagnosticCode::MetadataInvalid,
                            Some(&hint.id),
                        );
                    }
                    let evidence = evidence(self.kind, app.version.as_deref());
                    let location = app
                        .install_location
                        .clone()
                        .filter(|p| is_local_absolute(p));
                    result.registrations.push(RegistrationObservation {
                        tool_id: hint.id.clone(),
                        identity: app.identity.clone(),
                        package_id: package_id.cloned(),
                        install_location: location.clone(),
                        evidence: evidence.clone(),
                    });
                    if let Some(location) = location {
                        probe_directory(
                            &*self.file_system,
                            catalog,
                            &hints,
                            &mut result,
                            &hint.id,
                            &location,
                            &evidence,
                        );
                    }
                    // WinGetの明示的なportable実行対象のみ。UninstallString等は使わない。
                    if self.kind == ProviderKind::Winget
                        && let Some(path) = &app.portable_target
                    {
                        add_candidate(
                            &*self.file_system,
                            catalog,
                            &mut result,
                            &hint.id,
                            path.clone(),
                            evidence,
                        );
                    }
                }
            }
        }
        if inventory.applications.len() > 4096 || inventory.runtimes.len() > 512 {
            warning(
                &mut result,
                self.kind,
                DiagnosticCode::MetadataLimitReached,
                None,
            );
        }
        Ok(result)
    }
}
fn evidence(provider: ProviderKind, version: Option<&str>) -> SourceEvidence {
    SourceEvidence {
        provider,
        reported_version: version.and_then(reported_version),
        install_method: (provider == ProviderKind::Winget).then_some(InstallMethod::Winget),
    }
}
fn probe_directory(
    fs: &dyn FileSystem,
    catalog: &Catalog,
    hints: &WindowsHints,
    result: &mut Discovery,
    tool_id: &str,
    location: &std::path::Path,
    evidence: &SourceEvidence,
) {
    let Some(hint) = hints.tools.iter().find(|h| h.id == tool_id) else {
        return;
    };
    let Some(tool) = catalog.tools.iter().find(|t| t.id == tool_id) else {
        return;
    };
    for directory in &hint.executable_dirs {
        for command in &tool.commands {
            for extension in ["exe", "cmd"] {
                add_candidate(
                    fs,
                    catalog,
                    result,
                    tool_id,
                    location
                        .join(directory)
                        .join(format!("{}.{extension}", command.as_str())),
                    evidence.clone(),
                );
            }
        }
    }
}

#[cfg(not(windows))]
impl RegistrySource for SystemRegistry {
    fn read(&self) -> RegistryInventory {
        RegistryInventory {
            incomplete: true,
            ..RegistryInventory::default()
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::io;
    use winreg::{RegKey, enums::*};
    const ARP: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    const MAX_KEYS: usize = 1024;

    fn open(parent: &RegKey, name: &str, flags: u32, incomplete: &mut bool) -> Option<RegKey> {
        match parent.open_subkey_with_flags(name, KEY_READ | flags) {
            Ok(key) => match key.query_info() {
                Ok(info) if info.max_value_len <= 16 * 1024 => Some(key),
                _ => {
                    *incomplete = true;
                    None
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(_) => {
                *incomplete = true;
                None
            }
        }
    }
    fn text(key: &RegKey, name: &str, incomplete: &mut bool) -> Option<String> {
        match key.get_value::<String, _>(name) {
            Ok(value) if value.trim().is_empty() => None,
            Ok(value) if value.len() <= 8192 && !value.chars().any(char::is_control) => Some(value),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            _ => {
                *incomplete = true;
                None
            }
        }
    }
    fn keys(key: &RegKey, limit: usize, incomplete: &mut bool) -> Vec<String> {
        let mut result = Vec::new();
        for (index, name) in key.enum_keys().enumerate() {
            if index >= limit {
                *incomplete = true;
                break;
            }
            match name {
                Ok(name) => result.push(name),
                Err(_) => *incomplete = true,
            }
        }
        result.sort();
        result
    }
    impl RegistrySource for SystemRegistry {
        fn read(&self) -> RegistryInventory {
            let mut result = RegistryInventory::default();
            let hints = WindowsHints::bundled();
            for (hive, label) in [(HKEY_CURRENT_USER, "user"), (HKEY_LOCAL_MACHINE, "machine")] {
                let root = RegKey::predef(hive);
                for (flags, view) in [(KEY_WOW64_64KEY, "64"), (KEY_WOW64_32KEY, "32")] {
                    if let Some(arp) = open(&root, ARP, flags, &mut result.incomplete) {
                        for name in keys(&arp, MAX_KEYS, &mut result.incomplete) {
                            let Some(key) = open(&arp, &name, flags, &mut result.incomplete) else {
                                continue;
                            };
                            let display_name = text(&key, "DisplayName", &mut result.incomplete);
                            let winget_id =
                                text(&key, "WinGetPackageIdentifier", &mut result.incomplete);
                            if !hints.tools.iter().any(|hint| {
                                display_name.as_deref().is_some_and(|name| {
                                    hint.application_names
                                        .iter()
                                        .any(|known| name_matches(name, known))
                                }) || winget_id.as_deref().is_some_and(|id| {
                                    hint.winget_ids
                                        .iter()
                                        .any(|known| known.eq_ignore_ascii_case(id))
                                })
                            }) {
                                continue;
                            }
                            result.applications.push(RegisteredProduct {
                                identity: format!("arp:{label}:{view}:{name}"),
                                display_name,
                                version: text(&key, "DisplayVersion", &mut result.incomplete),
                                install_location: text(
                                    &key,
                                    "InstallLocation",
                                    &mut result.incomplete,
                                )
                                .map(PathBuf::from),
                                winget_id,
                                portable_target: text(
                                    &key,
                                    "PortableTargetFullPath",
                                    &mut result.incomplete,
                                )
                                .map(PathBuf::from),
                            });
                        }
                    }
                    // PEP 514: 既知のPythonCoreだけを対象とし、ExecutableArgumentsは読まない。
                    if let Some(python) = open(
                        &root,
                        r"Software\Python\PythonCore",
                        flags,
                        &mut result.incomplete,
                    ) {
                        for tag in keys(&python, 64, &mut result.incomplete) {
                            let Some(key) = open(
                                &python,
                                &format!(r"{tag}\InstallPath"),
                                flags,
                                &mut result.incomplete,
                            ) else {
                                continue;
                            };
                            let location =
                                text(&key, "", &mut result.incomplete).map(PathBuf::from);
                            let executable = text(&key, "ExecutablePath", &mut result.incomplete)
                                .map(PathBuf::from)
                                .or_else(|| location.as_ref().map(|p| p.join("python.exe")));
                            result.runtimes.push(RegisteredRuntime {
                                identity: format!("python:{label}:{view}:{tag}"),
                                tool_id: "python".into(),
                                version: None,
                                install_location: location,
                                executable,
                            });
                        }
                    }
                    for base in [
                        r"Software\JavaSoft\JDK",
                        r"Software\JavaSoft\Java Development Kit",
                        r"Software\JavaSoft\Java Runtime Environment",
                    ] {
                        if let Some(java) = open(&root, base, flags, &mut result.incomplete) {
                            for tag in keys(&java, 64, &mut result.incomplete) {
                                let Some(key) = open(&java, &tag, flags, &mut result.incomplete)
                                else {
                                    continue;
                                };
                                result.runtimes.push(RegisteredRuntime {
                                    identity: format!("java:{label}:{view}:{base}:{tag}"),
                                    tool_id: "java".into(),
                                    version: reported_version(&tag),
                                    install_location: text(
                                        &key,
                                        "JavaHome",
                                        &mut result.incomplete,
                                    )
                                    .map(PathBuf::from),
                                    executable: None,
                                });
                            }
                        }
                    }
                    for (base, tool_id, value) in [
                        (r"Software\GitForWindows", "git", "InstallPath"),
                        (r"Software\GoProgrammingLanguage", "go", "installLocation"),
                    ] {
                        if let Some(key) = open(&root, base, flags, &mut result.incomplete)
                            && let Some(location) = text(&key, value, &mut result.incomplete)
                        {
                            result.runtimes.push(RegisteredRuntime {
                                identity: format!("runtime:{label}:{view}:{base}"),
                                tool_id: tool_id.into(),
                                version: None,
                                install_location: Some(location.into()),
                                executable: None,
                            });
                        }
                    }
                }
            }
            result
        }
    }
}
