//! テスト専用。--features test-fixturesでのみビルドし、製品機能から呼び出さない。
use std::{
    fs,
    io::{self, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn main() {
    let executable = std::env::current_exe().unwrap();
    let mode_file = executable.with_extension("mode");
    if std::env::args().any(|arg| arg == "--fixture-descendant") {
        let heartbeat = executable.with_extension("heartbeat");
        for index in 0..3000 {
            fs::write(&heartbeat, index.to_string()).unwrap();
            thread::sleep(Duration::from_millis(20));
        }
        return;
    }
    let mode = fs::read_to_string(mode_file).unwrap_or_default();
    match mode.trim() {
        "nonzero" => {
            eprintln!("PRIVATE_OUTPUT_MUST_NOT_BE_RETAINED");
            std::process::exit(7);
        }
        "unrecognized" => println!("PRIVATE_OUTPUT_MUST_NOT_BE_RETAINED"),
        "timeout" => thread::sleep(Duration::from_secs(60)),
        "child" => {
            let mut child = Command::new(&executable)
                .arg("--fixture-descendant")
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            child.wait().unwrap();
        }
        "overflow" => {
            let mut out = io::stdout().lock();
            loop {
                if out.write_all(&[b'x'; 4096]).is_err() {
                    break;
                }
            }
        }
        "stderr" => eprintln!("java version \"21.0.4\""),
        "environment" => {
            assert!(std::env::var_os("DEI_TEST_SECRET").is_none());
            assert!(std::env::var_os("NODE_OPTIONS").is_none());
            println!("v1.2.3");
        }
        "wrapper" => {
            let args: Vec<_> = std::env::args_os().skip(1).collect();
            assert_eq!(args.len(), 2);
            assert!(std::path::Path::new(&args[0]).ends_with("node_modules/npm/bin/npm-cli.js"));
            assert_eq!(args[1], "--version");
            println!("1.2.3");
        }
        _ => println!("v1.2.3"),
    }
}
