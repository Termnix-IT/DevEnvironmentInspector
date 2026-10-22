# Dev Environment Inspector

Windowsの開発ツールを検出し、人間とAIが同じ情報を利用するためのローカルアプリケーションです。
現在は **検出基盤、独立MCPサーバー、Windows追加検出、TauriのデスクトップGUI** を実装しています。GUIは参照デザインに基づくダークテーマで、画面上の名称はEnvKitです。

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

## Windowsデスクトップアプリ

GUIの開発・ビルドには上記のRust環境に加え、Node.js 22.22.2以上の22系、24.15.0以上の24系、または26以上と、Microsoft Edge WebView2 Runtimeが必要です。依存関係はアプリ内に導入し、グローバルインストールは不要です。CLIとMCPだけを使う場合、Node.jsは不要です。

```powershell
# リポジトリのルートから
npm ci --prefix apps/desktop
npm --prefix apps/desktop run desktop:dev

# フロントエンドを埋め込んだリリース実行ファイルを作成
npm --prefix apps/desktop run desktop:build -- -- --locked

# リリース版を起動（開発サーバーやNode.jsの常駐は不要）
./target/release/dei-desktop.exe
```

初期ウィンドウは1280×720の16:9で、起動後は自由にリサイズできます。OS標準のタイトルバーを表示せず、上部44pxのツールバーの空白部分と左上のEnvKit部分をドラッグして移動します。同じ領域のダブルクリックで最大化・復元、右上のボタンで最小化・最大化・終了を行えます。

起動時はカタログだけを読み込み、右上の枠なしリロードアイコンまたは「スキャンを開始」で検出を始めます。Inventoryではツール名を選択すると、一覧内に実行候補・製品登録・バージョン・パス・情報源・診断を展開します。検索（Ctrl+K）とカテゴリを組み合わせて絞り込めます。「再スキャン」は現在の結果を置き換え、失敗時には前回結果と取得日時を保持します。

一覧のVersionは、確認できたPATH優先候補の実測値だけを表示します。候補が未実測なら「未確認」とし、別候補の実測値や登録情報の申告値で補いません。コマンド別名があるツールは、カタログ順で最初にPATH候補が見つかったコマンドを使い、詳細にコマンド名を示します。PATHはこのアプリが参照する探索範囲のもので、別のターミナルやプロジェクトの仮想環境で有効なバージョンを保証しません。

状態ドットと文字はPATH優先候補の検査結果を示します。緑の「実測済み」はネイティブ実行ファイルが固定バージョン引数で成功した状態、灰色は未検証・PATH未確認など、黄色はタイムアウトやバージョン取得失敗、赤色はアクセス拒否・起動失敗・終了処理失敗です。npmラッパーの既知の起動先だけを実測した場合は、灰色の「起動先確認」とし、ラッパーの利用可否を確定しません。取得元の不一致・探索範囲の制限は別の「補足」に残し、実測成功を黄色へ置き換えません。ほかの候補の結果や申告値は詳細で確認できます。

Settingsの「バージョン取得のためCLIを起動する」は既定で未選択です。「PATHのみ探索」は情報源をPATHに限定します。画面のパス表示は既定でホームを`<HOME>`へ置換し、変更しても再スキャンは行いません。詳細のコピー操作は表示中のパスだけをクリップボードに書き込みます。「JSONエクスポート」は右上のその他メニューまたはSettingsから利用でき、画面とは別の公開設定を使います。既定で全パスを省略し、保存先はWindowsのダイアログで選択します。キャンセル時には書き込みません。

本文12px・補足11px・見出し14pxを基本に、Diagnosticsは分類・重要度・対象・取得元を持つ診断表、MCPは公開ツールと設定例の左右分割、Settingsは項目と入力欄をそろえた設定行で表示します。Diagnosticsには表示中のスキャンの開始・完了日時（ローカル時刻）と、その時刻差による所要時間を表示します。診断ごとの発生時刻は記録していないため、個別の時刻は推定しません。診断には「実行確認」「情報照合」「探索範囲」「PATH・登録」の区分を添え、ツール名から該当ツールの詳細へ移動できます。MCPはクライアント接続状態を監視しないため、画面・ステータスバーには「接続未確認」と明記します。下部ステータスバーは高さ28pxで、検出されたツール数、警告・エラーの診断件数、前回スキャン時刻を表示します。未検証などの情報レベルの診断は警告・エラー件数に含めません。

結果はプロセス内のメモリーに保持し、自動保存や次回起動時の復元は行いません。現段階の配布成果物はWindows用の実行ファイルで、インストーラー・更新機能は含みません。参照画像にあるVS Codeや拡張機能の検出は、現在のカタログの対象外です。

## 実装状況と完了条件

| フェーズ | 完了条件 | 状態 |
| --- | --- | --- |
| 1：基盤 | 共通モデル、18ツールのカタログ、PATH検出、制限付きバージョン取得、JSON保存、Windows上の自動テスト | 実装済み |
| 2：MCP | 独立したstdioサーバー、5ツール、プロトコルテスト、Codex / Claude Codeからの接続確認と設定例 | 実装済み |
| 3：Windows拡張 | WinGet明示マーカー、npm global、レジストリ・製品登録、限定した既知ディレクトリの検出と統合テスト | 実装済み（下記の範囲） |
| 4：GUI | Tauri v2 + React / TypeScript + Vite、一覧・詳細・再スキャン・JSON出力、Windows起動確認 | 実装済み、参照デザイン適用 |

候補が見つからない場合も、PCに未インストールとは断定しません。WinGetはレジストリに既知のパッケージIDを明示する登録を対象とし、WinGetで導入された全製品の網羅ではありません。npm globalは既知のprefixとPATH配下にある5パッケージを確認し、独自prefixを設定ファイルから探索する処理はありません。製品登録だけの検出と実行ファイルの存在を区別します。詳細・上限は[検出仕様](docs/検出仕様.md)を参照してください。
同名コマンドの複数候補は情報として記録し、複数バージョンがあるという理由だけで警告しません。

## 構成と仕様

```text
catalog/tools.json         ツール定義（ビルド時に埋め込み）
catalog/windows.json       Windows製品名・WinGet ID・相対配置の許可リスト
apps/desktop/              ReactのGUI、TauriコマンドとGUI専用の状態管理
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
# 全workspaceの検証前にGUIアセットを生成
npm ci --prefix apps/desktop
npm --prefix apps/desktop run build
npm --prefix apps/desktop test

cargo fmt --all --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

GUIに依存せず検出エンジンとMCPだけをテストする場合は、`cargo test --all-features`を使えます。workspaceの既定メンバーは従来の3クレートで、Tauriを含む全体検証は`--workspace`を明示します。

`--all-features`は`test-fixtures`を有効にし、テスト用の小さな実行ファイルでWindowsのプロセス制御を検証します。既存ツールのインストール・変更は行いません。このフィクスチャは通常の`cargo build --release --bin dei`には含まれません。

テストは一時ディレクトリを使用します。サンドボックスの一時領域でファイル置換が拒否される場合は、書き込み可能な作業領域へ`TEMP`と`TMP`を向けて実行できます。

```powershell
New-Item -ItemType Directory -Path ./.cache/tmp -Force | Out-Null
$env:TEMP = (Resolve-Path ./.cache/tmp).Path
$env:TMP = $env:TEMP
cargo test --workspace --all-features
```

Windowsでの実機スキャンとプロセス結合テストを対象にしています。Linux・WSLの検出と実行制御は未対応です。
