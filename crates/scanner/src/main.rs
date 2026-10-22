use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
};

use inspector_core::{
    Catalog, PathExposure,
    storage::{JsonRepository, SnapshotRepository},
};
use inspector_scanner::{
    ScanOptions,
    platform::{path_engine, windows_engine},
};

const USAGE: &str = "Dev Environment Inspector (Phase 3)\n\n  dei scan [--path-only] [--probe-versions] [--paths omit|redact-home|full] [--output <file.json>]\n  dei catalog\n\n既定では外部CLIを実行せず、パスを省略したJSONを標準出力に返します。\n--path-only は探索をPATHだけに制限します。\n--probe-versions は許可リストのバージョン取得を有効にします。";

struct Arguments {
    options: ScanOptions,
    exposure: PathExposure,
    output: Option<PathBuf>,
    path_only: bool,
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Arguments, &'static str> {
    let mut result = Arguments {
        options: ScanOptions::default(),
        exposure: PathExposure::Omit,
        output: None,
        path_only: false,
    };
    let mut probe_seen = false;
    let mut paths_seen = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--path-only") if !result.path_only => {
                result.path_only = true;
            }
            Some("--probe-versions") if !probe_seen => {
                result.options.probe_versions = true;
                probe_seen = true;
            }
            Some("--paths") if !paths_seen => {
                result.exposure = match args.next().as_deref().and_then(|s| s.to_str()) {
                    Some("omit") => PathExposure::Omit,
                    Some("redact-home") => PathExposure::RedactHome,
                    Some("full") => PathExposure::Full,
                    _ => return Err("--pathsの値が不正です"),
                };
                paths_seen = true;
            }
            Some("--output") if result.output.is_none() => {
                let path = args.next().ok_or("保存先を指定してください")?;
                if path.is_empty() {
                    return Err("保存先を指定してください");
                }
                result.output = Some(path.into());
            }
            _ => return Err("未対応または重複した引数です"),
        }
    }
    Ok(result)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.is_none()
        || matches!(
            first.as_deref().and_then(|s| s.to_str()),
            Some("--help" | "-h")
        )
    {
        println!("{USAGE}");
        return Ok(());
    }
    let catalog = Catalog::bundled().map_err(|_| "同梱カタログの読み込みに失敗しました")?;
    if first.as_deref() == Some(std::ffi::OsStr::new("catalog")) {
        if args.next().is_some() {
            return Err("未対応の引数です");
        }
        return write_json(&catalog);
    }
    if first.as_deref() != Some(std::ffi::OsStr::new("scan")) {
        return Err("未対応の操作です。dei --helpを参照してください");
    }
    if !cfg!(windows) {
        return Err("実機スキャンはWindows専用です");
    }
    let arguments = parse(args)?;
    let engine = if arguments.path_only {
        path_engine(catalog)
    } else {
        windows_engine(catalog)
    };
    let snapshot = engine
        .scan(&arguments.options)
        .await
        .map_err(|_| "スキャン制限が不正です")?;
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    if let Some(path) = &arguments.output {
        JsonRepository { path }
            .save(&snapshot, arguments.exposure, home.as_deref())
            .map_err(|_| "JSON保存に失敗しました。保存先とアクセス権を確認してください")?;
    }
    write_json(&snapshot.project(arguments.exposure, home.as_deref()))
}

fn write_json(value: &impl serde::Serialize) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "JSON変換に失敗しました")?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "標準出力への書き込みに失敗しました")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_passive_and_private() {
        let parsed = parse(std::iter::empty()).unwrap();
        assert!(!parsed.options.probe_versions);
        assert_eq!(parsed.exposure, PathExposure::Omit);
    }

    #[test]
    fn rejects_arbitrary_execution_and_duplicate_flags() {
        for args in [
            vec!["--exec", "cmd"],
            vec!["--probe-versions", "--probe-versions"],
            vec!["--paths", "secret"],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
