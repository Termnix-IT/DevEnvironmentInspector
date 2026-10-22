use super::{Budget, MAX_PROJECT_PACKAGES, ProjectDirectory, Reader, empty_environment, finish};
use inspector_core::projects::*;
use node_semver::{Range, Version};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn inspect(
    dir: &dyn ProjectDirectory,
    budget: &mut Budget,
    root: &Path,
) -> Option<ProjectEnvironment> {
    let mut reader = Reader {
        dir,
        budget,
        findings: Vec::new(),
    };
    let manifest = read_json(&mut reader, Path::new("package.json"), 256 * 1024);
    let lock = read_json(&mut reader, Path::new("package-lock.json"), 4 * 1024 * 1024);
    let installed = installed_names(&mut reader);
    if manifest.is_none() && lock.is_none() && installed.is_empty() && reader.findings.is_empty() {
        return None;
    }
    let mut environment = empty_environment(
        "node-npm",
        "Node.js / npm",
        ProjectEnvironmentKind::NodeNpm,
        root.join("node_modules"),
    );
    let mut packages = BTreeMap::new();
    if let Some(manifest) = &manifest {
        if !manifest.is_object() {
            reader.note(ProjectFinding::MetadataInvalid);
        }
        environment.declared_runtime_requirement = manifest
            .get("engines")
            .and_then(|v| v.get("node"))
            .and_then(Value::as_str)
            .and_then(safe_range);
        for (field, kind) in [
            ("dependencies", DependencyKind::Dependency),
            ("devDependencies", DependencyKind::Development),
            ("optionalDependencies", DependencyKind::Optional),
            ("peerDependencies", DependencyKind::Peer),
        ] {
            if let Some(values) = manifest.get(field) {
                if let Some(values) = values.as_object() {
                    for (name, value) in values.iter().take(MAX_PROJECT_PACKAGES + 1) {
                        if !valid_name(name) {
                            reader.note(ProjectFinding::MetadataInvalid);
                            continue;
                        }
                        let Some(package) = package(&mut packages, name, &mut reader) else {
                            continue;
                        };
                        let requirement = value.as_str().and_then(safe_range);
                        if requirement.is_none() {
                            reader.note(ProjectFinding::NonVersionSpecifierRedacted);
                        }
                        package
                            .declarations
                            .push(DependencyDeclaration { kind, requirement });
                        if !package.sources.contains(&ProjectSource::PackageManifest) {
                            package.sources.push(ProjectSource::PackageManifest);
                        }
                    }
                    if values.len() > MAX_PROJECT_PACKAGES {
                        reader.note(ProjectFinding::ReadLimitReached);
                    }
                } else {
                    reader.note(ProjectFinding::MetadataInvalid);
                }
            }
        }
    }
    if let Some(lock) = lock {
        if !matches!(
            lock.get("lockfileVersion").and_then(Value::as_u64),
            Some(2 | 3)
        ) {
            reader.note(ProjectFinding::UnsupportedLayout);
        } else if let Some(entries) = lock.get("packages").and_then(Value::as_object) {
            // ロックファイルに記録された任意のパスは追跡しない。直下のnpm配置だけを照合する。
            for (path, entry) in entries.iter().take(8192) {
                let Some(name) = path
                    .strip_prefix("node_modules/")
                    .filter(|name| valid_name(name))
                else {
                    continue;
                };
                let Some(package) = package(&mut packages, name, &mut reader) else {
                    continue;
                };
                if entry.get("link").and_then(Value::as_bool) == Some(true) {
                    reader.note(ProjectFinding::UnsupportedLayout);
                    continue;
                }
                package.locked_version = entry
                    .get("version")
                    .and_then(Value::as_str)
                    .and_then(safe_node_version);
                if package.locked_version.is_none() {
                    reader.note(ProjectFinding::MetadataInvalid);
                }
                package.sources.push(ProjectSource::NpmLock);
            }
            if entries.len() > 8192 {
                reader.note(ProjectFinding::ReadLimitReached);
            }
        } else {
            reader.note(ProjectFinding::MetadataInvalid);
        }
    }
    for name in installed {
        let _ = package(&mut packages, &name, &mut reader);
    }
    for (name, mut package) in packages {
        let path = Path::new("node_modules").join(&name).join("package.json");
        match reader.bytes(&path, 256 * 1024) {
            Ok(Some(bytes)) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) if value.get("name").and_then(Value::as_str) == Some(name.as_str()) => {
                    package.installed_version = value
                        .get("version")
                        .and_then(Value::as_str)
                        .and_then(safe_node_version);
                    if package.installed_version.is_some() {
                        package.presence = PackagePresence::MetadataPresent;
                        package.sources.push(ProjectSource::NpmInstalled);
                    } else {
                        reader.note(ProjectFinding::MetadataInvalid);
                    }
                }
                Ok(_) if package.declarations.iter().any(|d| d.requirement.is_none()) => {
                    reader.note(ProjectFinding::UnsupportedLayout)
                }
                _ => reader.note(ProjectFinding::MetadataInvalid),
            },
            Ok(None) => package.presence = PackagePresence::NotFound,
            Err(error) => reader.note(error),
        }
        package.lock_matches = package
            .locked_version
            .as_ref()
            .zip(package.installed_version.as_ref())
            .map(|(a, b)| a == b);
        package.declaration_satisfied = package
            .installed_version
            .as_ref()
            .and_then(|version| version.parse::<Version>().ok())
            .and_then(|version| {
                // 複数の依存区分の優先関係は依存解決器を再実装せず、未判定にする。
                if package.declarations.len() != 1
                    || package.declarations.iter().any(|d| d.requirement.is_none())
                {
                    return None;
                }
                Some(package.declarations.iter().all(|d| {
                    d.requirement
                        .as_deref()
                        .and_then(|r| r.parse::<Range>().ok())
                        .is_some_and(|r| version.satisfies(&r))
                }))
            });
        if package.lock_matches == Some(false) || package.declaration_satisfied == Some(false) {
            reader.note(ProjectFinding::DependencyMismatch);
        }
        environment.packages.push(package);
    }
    finish(&mut environment, reader.findings);
    Some(environment)
}

fn package<'a>(
    packages: &'a mut BTreeMap<String, ProjectPackage>,
    name: &str,
    reader: &mut Reader<'_>,
) -> Option<&'a mut ProjectPackage> {
    if packages.len() >= MAX_PROJECT_PACKAGES && !packages.contains_key(name) {
        reader.note(ProjectFinding::ReadLimitReached);
        return None;
    }
    Some(
        packages
            .entry(name.to_string())
            .or_insert_with(|| ProjectPackage {
                name: name.into(),
                declarations: Vec::new(),
                locked_version: None,
                installed_version: None,
                presence: PackagePresence::Unverified,
                lock_matches: None,
                declaration_satisfied: None,
                sources: Vec::new(),
            }),
    )
}
fn read_json(reader: &mut Reader<'_>, path: &Path, limit: usize) -> Option<Value> {
    let bytes = reader.read(path, limit)?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            reader.note(ProjectFinding::MetadataInvalid);
            None
        }
    }
}
fn installed_names(reader: &mut Reader<'_>) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    for name in reader.directories(Path::new("node_modules")) {
        if name.starts_with('.') {
            continue;
        }
        if let Some(scope) = name.strip_prefix('@') {
            if !valid_part(scope) {
                continue;
            }
            for child in reader.directories(&Path::new("node_modules").join(&name)) {
                let full = format!("{name}/{child}");
                if valid_name(&full) {
                    result.insert(full);
                }
                if result.len() >= MAX_PROJECT_PACKAGES {
                    reader.note(ProjectFinding::ReadLimitReached);
                    return result;
                }
            }
        } else if valid_name(&name) {
            result.insert(name);
        }
        if result.len() >= MAX_PROJECT_PACKAGES {
            reader.note(ProjectFinding::ReadLimitReached);
            break;
        }
    }
    result
}
fn valid_part(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', '_'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn valid_name(name: &str) -> bool {
    if name.len() > 214 {
        return false;
    }
    if let Some(scoped) = name.strip_prefix('@') {
        scoped
            .split_once('/')
            .is_some_and(|(scope, part)| valid_part(scope) && valid_part(part))
    } else {
        valid_part(name)
    }
}
fn safe_range(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .-*+^~<>=|".contains(&b))
        && value.parse::<Range>().is_ok())
    .then(|| value.to_string())
}
fn safe_node_version(value: &str) -> Option<String> {
    if value.len() > 64 {
        return None;
    }
    value
        .parse::<Version>()
        .ok()
        .map(|version| version.to_string())
}
