use super::{Budget, MAX_PROJECT_PACKAGES, ProjectDirectory, Reader, empty_environment, finish};
use inspector_core::projects::*;
use std::path::Path;

pub(super) fn inspect(
    dir: &dyn ProjectDirectory,
    budget: &mut Budget,
    prefix: &Path,
    absolute: &Path,
    id: &str,
    label: &str,
) -> Option<ProjectEnvironment> {
    let mut reader = Reader {
        dir,
        budget,
        findings: Vec::new(),
    };
    let config = reader.text(&prefix.join("pyvenv.cfg"), 32 * 1024);
    if config.is_none() && reader.findings.is_empty() {
        return None;
    }
    let mut environment = empty_environment(
        id,
        label,
        ProjectEnvironmentKind::PythonVenv,
        absolute.to_owned(),
    );
    if let Some(config) = config {
        let mut version_seen = false;
        let mut sharing_seen = false;
        for line in config.lines().take(512) {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "version" => {
                    if version_seen {
                        reader.note(ProjectFinding::MetadataInvalid);
                        environment.reported_runtime_version = None;
                    } else {
                        environment.reported_runtime_version = safe_version(value);
                        if environment.reported_runtime_version.is_none() {
                            reader.note(ProjectFinding::MetadataInvalid);
                        }
                    }
                    version_seen = true;
                }
                "include-system-site-packages" => {
                    if sharing_seen {
                        reader.note(ProjectFinding::MetadataInvalid);
                        environment.system_site_packages = None;
                    } else {
                        environment.system_site_packages =
                            match value.trim().to_ascii_lowercase().as_str() {
                                "true" => Some(true),
                                "false" => Some(false),
                                _ => {
                                    reader.note(ProjectFinding::MetadataInvalid);
                                    None
                                }
                            };
                    }
                    sharing_seen = true;
                }
                _ => {} // home、executable、command等のパスや任意フィールドを保持しない。
            }
        }
        if environment.system_site_packages == Some(true) {
            reader.note(ProjectFinding::SystemPackagesNotInspected);
        }
        environment.interpreter_present = reader.regular_file(&prefix.join("Scripts/python.exe"));
        if environment.interpreter_present == Some(false) {
            reader.note(ProjectFinding::InterpreterNotFound);
        }
        let site = prefix.join("Lib/site-packages");
        for name in reader.directories(&site) {
            if !name.ends_with(".dist-info") || name.len() > 240 {
                continue;
            }
            if environment.packages.len() >= MAX_PROJECT_PACKAGES {
                reader.note(ProjectFinding::ReadLimitReached);
                break;
            }
            let Some(metadata) = reader.text(&site.join(name).join("METADATA"), 128 * 1024) else {
                reader.note(ProjectFinding::MetadataInvalid);
                continue;
            };
            let Some((name, version)) = distribution(&metadata) else {
                reader.note(ProjectFinding::MetadataInvalid);
                continue;
            };
            if environment
                .packages
                .iter()
                .any(|package| normalize_name(&package.name) == normalize_name(&name))
            {
                reader.note(ProjectFinding::MetadataInvalid);
            }
            environment.packages.push(ProjectPackage {
                name,
                declarations: Vec::new(),
                locked_version: None,
                installed_version: Some(version),
                presence: PackagePresence::MetadataPresent,
                lock_matches: None,
                declaration_satisfied: None,
                sources: vec![ProjectSource::PythonDistribution],
            });
        }
        environment.packages.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.installed_version.cmp(&b.installed_version))
        });
    }
    finish(&mut environment, reader.findings);
    Some(environment)
}

pub(super) fn safe_version(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-!".contains(&b)))
    .then(|| value.to_string())
}
fn normalize_name(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}
fn distribution(metadata: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut version = None;
    for line in metadata.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Name:") {
            if name.is_some() {
                return None;
            }
            let value = value.trim();
            if value.is_empty()
                || value.len() > 214
                || !value.as_bytes()[0].is_ascii_alphanumeric()
                || !value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            {
                return None;
            }
            name = Some(value.to_string());
        }
        if let Some(value) = line.strip_prefix("Version:") {
            if version.is_some() {
                return None;
            }
            version = safe_version(value);
            version.as_ref()?;
        }
    }
    Some((name?, version?))
}
