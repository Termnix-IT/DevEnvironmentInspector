//! CLI・MCP・将来のGUIで同じWindows探索方針を共有する。
use crate::{
    NativeVersionRunner, PathEnvironment, PathProvider, RealFileSystem, ScanEngine,
    known_provider::{KnownDirectoryProvider, KnownLocation},
    npm_provider::NpmGlobalProvider,
    provider::DiscoveryProvider,
    registry_provider::{RegistryProvider, SystemRegistry},
};
use inspector_core::Catalog;
use std::{path::PathBuf, sync::Arc};

pub fn path_engine(catalog: Catalog) -> ScanEngine {
    let path = PathEnvironment::current();
    let runner = NativeVersionRunner::new(&path);
    ScanEngine::new(
        catalog,
        vec![Box::new(PathProvider {
            environment: path,
            file_system: RealFileSystem,
        })],
        Box::new(runner),
    )
}

pub fn windows_engine(catalog: Catalog) -> ScanEngine {
    let path = PathEnvironment::current();
    let runner = NativeVersionRunner::new(&path);
    let mut prefixes = Vec::new();
    let mut locations = Vec::new();
    let mut add = |tool: &str, root: PathBuf, child: Option<&str>, bin: &str| {
        locations.push(KnownLocation {
            tool_id: tool.into(),
            root,
            child_prefix: child.map(str::to_string),
            executable_directory: bin.into(),
        })
    };
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            let node = root.join("nodejs");
            prefixes.push(node.clone());
            add("node", node, None, "");
            add("git", root.join("Git"), None, "cmd");
            add("go", root.join("Go"), None, "bin");
            add("docker", root.join("Docker/Docker"), None, "resources/bin");
            add("aws", root.join("Amazon/AWSCLIV2"), None, "");
            add("java", root.join("Java"), Some("jdk"), "bin");
            add("java", root.join("Eclipse Adoptium"), Some("jdk"), "bin");
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        prefixes.push(appdata.join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        add("python", local.join("Programs/Python"), Some("Python3"), "");
        add("winget", local.join("Microsoft/WindowsApps"), None, "");
    }
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        add("rust", home.join(".cargo"), None, "bin");
        for tool in ["uv", "claude", "codex"] {
            add(tool, home.join(".local/bin"), None, "");
        }
    }
    prefixes.extend(path.directories.iter().cloned());
    let mut providers: Vec<Box<dyn DiscoveryProvider>> = vec![Box::new(PathProvider {
        environment: path,
        file_system: RealFileSystem,
    })];
    providers.extend(RegistryProvider::providers(
        Arc::new(SystemRegistry),
        Arc::new(RealFileSystem),
    ));
    providers.push(Box::new(NpmGlobalProvider {
        prefixes,
        file_system: RealFileSystem,
    }));
    providers.push(Box::new(KnownDirectoryProvider {
        locations,
        file_system: RealFileSystem,
    }));
    ScanEngine::new(catalog, providers, Box::new(runner))
}
