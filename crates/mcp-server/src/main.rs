use std::{ffi::OsString, path::PathBuf, sync::Arc};

use inspector_core::project_store::{JsonProjectRepository, local_absolute};
use inspector_core::{Catalog, PathExposure};
use inspector_mcp_server::{
    InspectionService, InspectorServer, ServerPolicy, backend::WindowsScanner,
};
use inspector_scanner::projects::{MetadataProjectScanner, service::ProjectService};
use rmcp::{ServiceExt, transport::async_rw::AsyncRwTransport};

const HELP: &str = "Dev Environment Inspector MCP server\n\n  dei-mcp [--path-only] [--paths omit|redact-home|full] [--allow-version-probes] [--projects-file <absolute-local-json-path>]\n\n既定は受動スキャン・パス省略です。ProjectsはGUIで登録・MCP参照を許可したものだけを取得します。標準出力はMCP stdio通信専用です。";

fn parse(args: impl Iterator<Item = OsString>) -> Result<Option<ServerPolicy>, &'static str> {
    let mut args = args.peekable();
    if args.peek().is_some_and(|s| s == "--help" || s == "-h") {
        args.next();
        if args.next().is_some() {
            return Err("未対応の引数です");
        }
        return Ok(None);
    }
    let mut policy = ServerPolicy::default();
    let mut paths_seen = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--path-only") if !policy.path_only => {
                policy.path_only = true;
            }
            Some("--allow-version-probes") if !policy.allow_version_probes => {
                policy.allow_version_probes = true
            }
            Some("--paths") if !paths_seen => {
                policy.paths = match args.next().as_deref().and_then(|s| s.to_str()) {
                    Some("omit") => PathExposure::Omit,
                    Some("redact-home") => PathExposure::RedactHome,
                    Some("full") => PathExposure::Full,
                    _ => return Err("--pathsの指定が不正です"),
                };
                paths_seen = true;
            }
            Some("--projects-file") if policy.projects_file.is_none() => {
                let path = PathBuf::from(args.next().ok_or("--projects-fileの指定が不正です")?);
                if !local_absolute(&path) {
                    return Err("--projects-fileにはローカルの絶対パスを指定してください");
                }
                policy.projects_file = Some(path);
            }
            _ => return Err("未対応または重複した引数です"),
        }
    }
    policy.home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    Ok(Some(policy))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let Some(policy) = parse(std::env::args_os().skip(1))? else {
        eprintln!("{HELP}");
        return Ok(());
    };
    let catalog = Catalog::bundled().map_err(|_| "同梱カタログを読み込めませんでした")?;
    let projects = policy
        .projects_file
        .clone()
        .or_else(JsonProjectRepository::default_path)
        .map(|path| {
            ProjectService::new(
                Arc::new(JsonProjectRepository { path }),
                Arc::new(MetadataProjectScanner::default()),
                policy.home.clone(),
            )
        });
    let service = InspectionService::new_with_projects(
        catalog,
        Arc::new(WindowsScanner {
            path_only: policy.path_only,
        }),
        policy,
        projects,
    );
    // SDK標準のstdioフレーミングに、用途に合わせた入力1行64KiBの上限を設定する。
    let transport = AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout())
        .with_max_line_length(64 * 1024);
    let server = InspectorServer::new(service)
        .serve(transport)
        .await
        .map_err(|_| "MCP接続の初期化に失敗しました")?;
    server
        .waiting()
        .await
        .map_err(|_| "MCP接続を終了できませんでした")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_private_and_passive() {
        let policy = parse(std::iter::empty()).unwrap().unwrap();
        assert_eq!(policy.paths, PathExposure::Omit);
        assert!(!policy.allow_version_probes);
    }
    #[test]
    fn rejects_unknown_paths_and_execution_options() {
        for args in [
            vec!["--exec", "cmd"],
            vec!["--paths", "arbitrary"],
            vec!["--allow-version-probes", "--allow-version-probes"],
        ] {
            assert!(parse(args.into_iter().map(Into::into)).is_err());
        }
    }
}
