fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_catalog",
            "get_snapshot",
            "scan_environment",
            "export_snapshot",
        ]),
    ))
    .expect("Tauriのビルド設定を読み込めませんでした");
}
