# MCP連携

## 起動と公開範囲

MCPサーバーはGUIに依存しない`dei-mcp.exe`として動作する。stdio専用で、HTTPポートやネットワーク待受は作らない。ビルドはリポジトリのルートから実行する。

```powershell
cargo build --release --bin dei-mcp
```

生成された`target/release/dei-mcp.exe`をMCPクライアントから起動する。通常の端末で起動するとクライアントの入力を待つ。`--help`の説明も標準エラー出力に書き、標準出力はJSON-RPC専用にする。

| 起動オプション | 意味 |
| --- | --- |
| 指定なし | 受動スキャン、パス省略、バージョン実行不可 |
| `--path-only` | 情報源をPATHだけに制限。既定ではWindows追加プロバイダーも使用 |
| `--paths redact-home` | ホーム以下を`<HOME>`に置換。ホーム不明ならパス省略 |
| `--paths full` | 完全な実行ファイルパスを公開 |
| `--allow-version-probes` | `scan_environment`からの明示的なバージョン取得を許可 |
| `--projects-file <path>` | Projects登録JSONの絶対パスを起動時に固定。省略時は`%LOCALAPPDATA%/EnvKit/projects.json` |

パス公開と実行許可はサーバー起動時のポリシーで固定する。MCPツール引数から変更できない。`redact-home`ではホーム外の個人情報を消せないため、公開範囲は利用環境に合わせて選ぶ。

## クライアント設定

### Codex

[examples/codex.config.toml](../examples/codex.config.toml)のエントリーを、利用するCodexの`config.toml`へ追加する。`command`はビルド済み実行ファイルの絶対パスに置き換える。Windowsでは`C:/...`形式またはTOMLの適切なエスケープを使用する。

設定後、`codex mcp list`で登録内容を確認する。これは接続・ツール実行の成功を保証するコマンドではないため、CodexのMCP状態表示でも8つのツールが認識されることを確認する。

### Claude Code

[examples/claude.mcp.json](../examples/claude.mcp.json)の`command`を実際の絶対パスに置き換え、利用するプロジェクトの`.mcp.json`へ統合する。既存のサーバー設定は上書きしない。

信頼できるプロジェクト内で`claude mcp list`を実行し、`Connected`を確認する。このコマンドはstdioサーバーを起動して接続を検査する。Claude Code内の`/mcp`でも接続状態を確認できる。プロジェクト設定の承認が求められた場合は、起動コマンドとパス公開設定を確認する。

両方の例は既定で受動スキャン・パス省略とし、グローバル設定を自動変更するインストール処理は提供しない。バージョン取得が必要な場合は`args`に`--allow-version-probes`を追加し、その後のスキャンでも`probe_versions: true`を指定する。バージョン取得の副作用と実行制限は[検出仕様](検出仕様.md#バージョン取得と実行制限)を参照する。

## 公開するツール

MCPプロトコルの`tools/list`は、以下の8つの操作を列挙する。公開ツールの`list_tools`は、検出された開発ツールの一覧を返す操作であり、プロトコルのメソッドとは別である。

| ツール | 引数 | 結果 |
| --- | --- | --- |
| `list_tools` | `{}` | 直近スキャンで実体候補または製品登録を検出したツール。両方ないツールは除外 |
| `get_tool_info` | `{"tool_id":"node"}` | 既知ツールの実体・バージョン・取得元・診断。対象内の未検出ツールも返す |
| `check_command` | `{"command":"node"}` | 既知コマンドのキャッシュ済み候補とPATH上の優先候補ID。コマンドは実行しない |
| `get_environment_summary` | `{}` | 対象件数、検出件数、カテゴリ別件数、警告・エラー・注意事項 |
| `scan_environment` | `{"tool_ids":["node","python"],"probe_versions":false}` | 指定範囲の新しいスナップショット。引数省略時は全カタログを受動スキャン |
| `list_projects` | `{}` | GUIで登録し、MCP参照を許可したプロジェクトだけ。フォルダーのスキャンは行わない |
| `get_project_environment` | `{"project_id":"project-1","refresh":false}` | 登録環境の概要・確認事項・パッケージ件数とスキャンrevision。初回は読み取りスキャン、`refresh: true`で再実行 |
| `list_project_packages` | `{"project_id":"project-1","environment_id":"node-npm","offset":0,"limit":100,"scan_revision":1}` | 同じ取得時点のパッケージ一覧をページ分割して返す |

ツール名、コマンド名、実行引数、ファイルパス、保存先を任意指定する機能はない。未知フィールド、シェル文、実行ファイルパス、未知のコマンドを拒否する。スキャン対象は重複のない1〜18件のカタログIDに限定する。`tool_ids`を省略または`null`にすると全体が対象になる。

応答は`structuredContent`と、同じJSONのテキスト表現を`content`に含める。PC全体の5ツールの共通メタデータには`schema_version`、`target_os`、`started_at`、`completed_at`、`providers`、`path_exposure`、`scanned_tool_ids`、全体の`diagnostics`を含める。バージョンを実行しなかった候補の状態は`not_requested`であり、取得済みとして推測しない。

Phase 3からスキーマは2となり、ツールに`registrations`を含める。製品の申告バージョンは`registrations[].sources[].reported_version`、実行取得したバージョンは`installations[].version`に分ける。`installation_count`と`registration_count`（カテゴリ別は`installations`と`registrations`）は別集計であり、合計をインストール数と解釈しない。登録だけの検出でも`list_tools`に現れるが、`check_command`に架空の候補や優先IDは追加しない。登録の配置パスにも起動時の公開ポリシーを適用する。

## 登録プロジェクトの参照

GUIのProjectsでフォルダーを登録し、プロジェクトごとにMCP参照を許可する。既定では許可しない。GUIとMCPは登録情報だけを共有し、GUIが終了していてもサーバー単独で読み取れる。登録・変更・解除を行うMCPツールは提供しない。登録JSONがなければ一覧は空になり、破損時は固定のエラーを返して既存ファイルを保持する。保存方式と完全パスのローカル保持については[アーキテクチャ](アーキテクチャ.md#登録プロジェクトの境界)を参照する。

`get_project_environment`は`{project, scan_revision, snapshot}`を返し、snapshotにはスキーマ1、UTCの開始・完了日時、パス公開設定、環境・確認事項を含める。環境ごとのpackages本文は省略し、`package_count`を返す。`list_projects`は登録情報だけを返すため、スキャン日時を作らない。

パッケージ一覧は`limit`が1〜200（既定100）、`offset`が0〜512で、`total`と`next_offset`を返す。`environment_id`は概要にある`python-dot-venv`、`python-venv`、`python-external`、`node-npm`だけを受け付ける。ページ間で同じ`scan_revision`を指定し、変更時は`project_snapshot_changed`を受けて概要から再取得する。取得開始・完了日時と公開設定も各ページに含む。

パスには起動時の`--paths`を適用し、呼出し側から範囲を拡大できない。任意のルート・venv・ファイル・コマンドは指定できない。毎回最新の登録許可を確認し、スキャン終了時にも再確認するため、許可撤回や解除後に旧キャッシュを返さない。ただし既にクライアントへ返した情報は回収できない。

Projectsのキャッシュと実行枠はPC全体のスキャンから独立し、プロセスごとに同時1件、完了後2秒の間隔を持つ。GUIとMCP間のキャッシュ同期は行わない。1プロジェクトの読み取り予算は8秒で、ファイル数・容量・列挙数も固定する。OSのブロッキングI/Oの厳密な時間隔離は保証しない。対象・未対応形式・上限は[検出仕様](検出仕様.md#登録プロジェクトの環境)を参照する。`--path-only`はPC全体の探索にだけ適用し、Projectsの読み取り範囲は変えない。

Pythonやnpmのプロセスを起動せず、既存メタデータだけを読む。したがって`--allow-version-probes`もProjectsには影響しない。配置・導入情報と実行成功を区別し、URL・パス等の非バージョン依存指定は省略する。クライアントとの接続状態の表示・監視はこの機能に含まない。

## キャッシュと処理負荷

サーバー起動直後はスキャンせず、PC全体の最初の参照ツール呼出しで全カタログを受動スキャンする。初期化とツール定義の取得だけでは開発ツールを起動しない。参照ツールは同じスナップショットを返し、自動更新は行わない。

`scan_environment`は新しいスナップショットでキャッシュを置き換える。部分スキャン後の参照範囲もその部分だけになる。未走査のツールへの問い合わせは`tool_not_scanned`を返し、未検出と混同しない。応答の`scanned_tool_ids`と取得日時で範囲・鮮度を確認できる。異なる日時の結果を暗黙に混在させない。

同時スキャンはサーバープロセスごとに1件、完了から次の開始までは2秒以上とする。最初の受動スキャンと失敗したスキャンもこの制限に含む。実行中は既存キャッシュの参照を続けられる。スキャン失敗時には以前のキャッシュと元の取得日時を保持する。

ファイル探索とプロセス実行は専用ワーカーで処理し、MCPの入力処理をブロックしない。呼出しをキャンセルしてもワーカーを即座には停止せず、終了まで実行枠を保持して結果をキャッシュする。したがってキャンセル後の連続要求でスキャンを増殖させない。クライアント側のタイムアウトは、既定の処理予算30秒と終了処理を上回る45秒を設定例に使用する。

探索・プローブの数、時間、出力量は共通スキャナーの上限を継承する。MCPから上限は変更できない。入力はSDKのstdioトランスポートで1行64 KiBに制限する。OSのブロッキングI/Oに厳密な時間保証がないこと、バージョン取得が完全な隔離ではないことも共通スキャナーと同じである。

## エラーと注釈

- 未知のMCPツール・メソッドはJSON-RPC `-32601`、引数の型違い・未知フィールド等は`-32602`として返す。
- 既知ツールの処理失敗や対象指定の誤りは`isError: true`とし、`error.code`と固定の説明を返す。入力値や生のI/Oエラーを説明文に埋め込まない。
- 主な処理エラーは`unknown_tool`、`unknown_command`、`invalid_scope`、`tool_not_scanned`、`version_probes_disabled`、`scan_in_progress`、`scan_rate_limited`、`scan_failed`、`unsupported_platform`である。
- Projectsでは`project_storage_unavailable`、`project_storage_invalid`、`project_not_found`、`invalid_project_input`、`project_scan_in_progress`、`project_scan_rate_limited`、`project_scan_failed`、`project_snapshot_changed`を使う。非許可の登録IDも`project_not_found`として扱う。
- 実体ごとのバージョン失敗はスキャン結果の`version.status`と診断に残す。スキャン自体が完了した場合に、全体を実行エラーとはしない。

通常の参照ツールと受動スキャンには`readOnlyHint: true`を付ける。起動時にバージョン実行を許可した場合、`scan_environment`はCLIの副作用を完全に排除できないため`readOnlyHint: false`、`openWorldHint: true`とする。インストール・更新・削除の操作はどの場合も公開しない。

## 互換性と検証

公式Rust SDK `rmcp 3.6.0`を使用し、プロトコルの初期化、世代差、通知、キャンセル、stdioのフレーミングをSDKに任せる。`2025-11-25`の`initialize`方式は実行ファイルとの結合テスト、`2026-07-28`の`server/discover`方式は公式SDKクライアントとの結合テストで検証する。

モックとフィクスチャで8ツールの応答、公開範囲、未走査と未検出の区別、キャッシュ、単一実行枠、キャンセル後の枠保持、実行許可、連続実行の制限を検証する。実プロセスではstdoutのJSON-RPC専用性、一覧・詳細・サマリー、未知入力、過大入力からの復帰、EOF終了と、Projectsの登録共有・許可撤回を確認する。Projectsの追加3ツールは公式SDKクライアントとstdio結合テストが検証範囲で、Codex・Claude Code実クライアントでの追加機能の接続確認は別途必要である。通常の自動テストには外部LLMへの接続や認証を必要としない。

参照：[MCP stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)、[MCPツールとエラー](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)、[公式Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)、[OpenAI DocsのMCP設定](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)、[Claude CodeのMCP設定](https://code.claude.com/docs/en/mcp)。
