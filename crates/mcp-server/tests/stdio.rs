#![cfg(windows)]

use std::{fs, process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

struct WireClient {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    directory: tempfile::TempDir,
}

impl WireClient {
    fn new(args: &[&str]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        // 検出専用のダミーファイル。既定の受動スキャンでは実行されない。
        fs::write(directory.path().join("node.exe"), b"not an executable").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_dei-mcp"))
            .arg("--path-only")
            .args(args)
            .env("PATH", directory.path())
            .env("PATHEXT", ".EXE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            directory,
        }
    }

    async fn send(&mut self, value: Value) {
        let mut data = serde_json::to_vec(&value).unwrap();
        data.push(b'\n');
        timeout(
            Duration::from_secs(3),
            self.input.as_mut().unwrap().write_all(&data),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn response(&mut self, id: u64) -> Value {
        timeout(Duration::from_secs(5), async {
            loop {
                let mut line = String::new();
                assert!(
                    self.output.read_line(&mut line).await.unwrap() > 0,
                    "MCPサーバーが応答前に終了"
                );
                let value: Value =
                    serde_json::from_str(&line).expect("stdoutにはJSON-RPC以外の文字列を出さない");
                assert_eq!(value["jsonrpc"], "2.0");
                if value["id"] == id {
                    return value;
                }
            }
        })
        .await
        .unwrap()
    }

    async fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"dei-wire-test","version":"1"}}})).await;
        let response = self.response(1).await;
        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "dev-environment-inspector"
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
    }

    async fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await;
        self.response(id).await
    }

    async fn close(mut self) {
        drop(self.input.take());
        assert!(
            timeout(Duration::from_secs(3), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        let mut errors = Vec::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_end(&mut errors)
            .await
            .unwrap();
        assert!(
            errors.is_empty(),
            "通常終了時にログや検出内容をstderrへ流さない"
        );
    }
}

#[tokio::test]
async fn real_stdio_server_lists_catalog_tools_and_returns_private_timestamped_details() {
    let mut client = WireClient::new(&[]);
    client.initialize().await;
    client
        .send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await;
    assert_eq!(
        client.response(2).await["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let list = client.call(3, "list_tools", json!({})).await;
    let data = &list["result"]["structuredContent"];
    assert_eq!(data["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        data["tools"][0]["installations"][0]["version"]["status"],
        "not_requested"
    );
    assert_eq!(
        data["tools"][0]["installations"][0]["executable_path"],
        Value::Null
    );
    assert!(data["completed_at"].is_string());
    let detail = client
        .call(4, "get_tool_info", json!({"tool_id":"node"}))
        .await;
    let command = client
        .call(5, "check_command", json!({"command":"node"}))
        .await;
    let summary = client.call(6, "get_environment_summary", json!({})).await;
    assert_eq!(
        detail["result"]["structuredContent"]["completed_at"],
        data["completed_at"]
    );
    assert_eq!(
        command["result"]["structuredContent"]["primary_candidate_id"],
        "node-1"
    );
    assert_eq!(
        summary["result"]["structuredContent"]["detected_tool_count"],
        1
    );
    for response in [list, detail, command, summary] {
        assert!(
            !response
                .to_string()
                .contains(client.directory.path().to_str().unwrap())
        );
    }
    client.close().await;
}

#[tokio::test]
async fn real_stdio_server_distinguishes_protocol_and_execution_errors() {
    let mut client = WireClient::new(&[]);
    client.initialize().await;
    let malformed = client.call(2, "check_command", json!({"command":42})).await;
    assert_eq!(malformed["error"]["code"], -32602);
    let unknown = client.call(3, "PRIVATE_TEST_VALUE", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
    assert!(!unknown.to_string().contains("PRIVATE_TEST_VALUE"));
    let injection = client
        .call(
            4,
            "check_command",
            json!({"command":"node & PRIVATE_TEST_VALUE"}),
        )
        .await;
    assert_eq!(injection["result"]["isError"], true);
    assert_eq!(
        injection["result"]["structuredContent"]["error"]["code"],
        "unknown_command"
    );
    assert!(!injection.to_string().contains("PRIVATE_TEST_VALUE"));
    let probes = client
        .call(5, "scan_environment", json!({"probe_versions":true}))
        .await;
    assert_eq!(
        probes["result"]["structuredContent"]["error"]["code"],
        "version_probes_disabled"
    );
    let scan = client
        .call(6, "scan_environment", json!({"tool_ids":["node"]}))
        .await;
    assert_eq!(
        scan["result"]["structuredContent"]["scanned_tool_ids"],
        json!(["node"])
    );
    let cooldown = client.call(7, "scan_environment", json!({})).await;
    assert_eq!(
        cooldown["result"]["structuredContent"]["error"]["code"],
        "scan_rate_limited"
    );
    client.close().await;
}

#[tokio::test]
async fn explicit_probe_permission_still_uses_the_shared_bounded_runner() {
    let mut client = WireClient::new(&["--allow-version-probes"]);
    client.initialize().await;
    let result = client
        .call(
            2,
            "scan_environment",
            json!({"tool_ids":["node"],"probe_versions":true}),
        )
        .await;
    assert_eq!(
        result["result"]["structuredContent"]["tools"][0]["installations"][0]["version"]["status"],
        "spawn_failed"
    );
    assert_eq!(
        result["result"]["isError"], false,
        "実体ごとの失敗はスキャン結果として返す"
    );
    client.close().await;
}

#[tokio::test]
async fn bounded_transport_recovers_after_invalid_or_oversized_lines() {
    let mut client = WireClient::new(&[]);
    client.initialize().await;
    client
        .input
        .as_mut()
        .unwrap()
        .write_all(b"not-json\n")
        .await
        .unwrap();
    let mut large = vec![b'x'; 70 * 1024];
    large.push(b'\n');
    timeout(
        Duration::from_secs(3),
        client.input.as_mut().unwrap().write_all(&large),
    )
    .await
    .unwrap()
    .unwrap();
    client
        .send(json!({"jsonrpc":"2.0","id":2,"method":"ping"}))
        .await;
    assert!(client.response(2).await["result"].is_object());
    client.close().await;
}

#[tokio::test]
async fn help_and_invalid_startup_arguments_never_pollute_protocol_stdout() {
    for args in [vec!["--help"], vec!["--exec", "PRIVATE_TEST_VALUE"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_dei-mcp"))
            .args(&args)
            .output()
            .await
            .unwrap();
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_TEST_VALUE"));
        assert_eq!(output.status.success(), args[0] == "--help");
    }
}
