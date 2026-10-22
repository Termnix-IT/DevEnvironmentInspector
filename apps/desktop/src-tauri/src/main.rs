#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if inspector_desktop::run().is_err() {
        eprintln!("デスクトップアプリを起動できませんでした");
        std::process::exit(1);
    }
}
