# Dev Environment Inspector

Windowsの開発ツールを検出し、人間とAIが同じ情報を利用するためのローカルアプリケーションです。
現在は **Phase 1の検出基盤、Phase 2の独立MCPサーバー、Phase 3のWindows追加検出** を実装しています。Tauri GUIは未実装です。

## 利用方法

開発にはRust 1.95以上、MSVC C++ Build Tools、Windows SDKが必要です。依存関係の初回取得にはネットワークを使用します。実行時にネットワーク接続は必須ではありません。

PowerShellでリポジトリのルートから実行します。

```powershell
# PATH・製品登録・既知のnpm global配置等を確認。外部CLIの起動は行いません。
cargo run -p inspector-scanner --bin dei -- scan

# 探索をPATHだけに制限
cargo run -p inspector-scanner --bin dei -- scan --path-only

# バージョン取得を明示的に有効化
cargo run -p inspector-scanner --bin dei -- scan --probe-versions

# JSONを保存。保存先の親ディレクトリはあらかじめ用意してください。
cargo run -p inspector-scanner --bin dei -- scan --output ./environment.json

# 自分のホーム以下を<HOME>に置換してパスを表示
cargo run -p inspector-scanner --bin dei -- scan --paths redact-home

# 完全な実行ファイルパスを含める場合
cargo run -p inspector-scanner --bin dei -- scan --paths full

# 検出対象のカタログを表示
cargo run -p inspector-scanner --bin dei -- catalog

# 配布候補となるCLI実行ファイルを生成
cargo build --release --bin dei

# GUIなしで使えるMCPサーバーを生成（設定方法は下記MCP連携を参照）
cargo build --release --bin dei-mcp
```

CLIの標準出力はJSON、エラーは標準エラー出力です。`--output`指定時も同じJSONを標準出力に返します。
既定では出力と保存から実行ファイルパスを省略します。`redact-home`は自分のホームだけを置換するため、共有前に出力内容を確認してください。実機のスキャン結果はGit管理に追加しないでください。

バージョン取得では検出したローカルプログラムを起動します。固定引数、時間・出力量制限、最小限の環境変数を使用しますが、CLI自体によるファイルアクセスや副作用を完全に隔離する仕組みではありません。信頼できる開発環境で明示的に有効化してください。

## 実装状況と完了条件

| フェーズ | 完了条件 | 状態 |
| --- | --- | --- |
| 1：基盤 | 共通モデル、18ツールのカタログ、PATH検出、制限付きバージョン取得、JSON保存、Windows上の自動テスト | 実装済み |
| 2：MCP | 独立したstdioサーバー、5ツール、プロトコルテスト、Codex / Claude Codeからの接続確認と設定例 | 実装済み |
| 3：Windows拡張 | WinGet明示マーカー、npm global、レジストリ・製品登録、限定した既知ディレクトリの検出と統合テスト | 実装済み（下記の範囲） |
| 4：仮GUI | Tauri v2 + React / TypeScript + Vite、一覧・詳細・再スキャン・JSON出力、Windows起動確認 | 未実装 |

候補が見つからない場合も、PCに未インストールとは断定しません。WinGetはレジストリに既知のパッケージIDを明示する登録を対象とし、WinGetで導入された全製品の網羅ではありません。npm globalは既知のprefixとPATH配下にある5パッケージを確認し、独自prefixを設定ファイルから探索する処理はありません。製品登録だけの検出と実行ファイルの存在を区別します。詳細・上限は[検出仕様](docs/検出仕様.md)を参照してください。
同名コマンドの複数候補は情報として記録し、複数バージョンがあるという理由だけで警告しません。

## 構成と仕様

```text
catalog/tools.json         ツール定義（ビルド時に埋め込み）
catalog/windows.json       Windows製品名・WinGet ID・相対配置の許可リスト
crates/core/               共通モデル、公開範囲の変換、JSON保存境界
crates/scanner/            検出プロバイダー、情報統合、バージョン取得、dei CLI
crates/mcp-server/         独立stdioサーバー、5ツール、スキャン制限とキャッシュ
examples/                 Codex・Claude Code用MCP設定例
docs/                     設計と検出仕様
```

- [アーキテクチャ](docs/アーキテクチャ.md)：責務、データモデル、依存関係と設計判断。
- [検出仕様](docs/検出仕様.md)：探索範囲、実行制限、診断、保存形式、未対応事項。
- [MCP連携](docs/MCP連携.md)：公開ツール、処理制限、Codex・Claude Codeの設定方法。

## 検証

```powershell
cargo fmt --all --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`--all-features`は`test-fixtures`を有効にし、テスト用の小さな実行ファイルでWindowsのプロセス制御を検証します。既存ツールのインストール・変更は行いません。このフィクスチャは通常の`cargo build --release --bin dei`には含まれません。

テストは一時ディレクトリを使用します。サンドボックスの一時領域でファイル置換が拒否される場合は、書き込み可能な作業領域へ`TEMP`と`TMP`を向けて実行できます。

```powershell
New-Item -ItemType Directory -Path ./.cache/tmp -Force | Out-Null
$env:TEMP = (Resolve-Path ./.cache/tmp).Path
$env:TMP = $env:TEMP
cargo test --workspace --all-features
```

Windowsでの実機スキャンとプロセス結合テストを対象にしています。Linux・WSLの検出と実行制御は未対応です。
