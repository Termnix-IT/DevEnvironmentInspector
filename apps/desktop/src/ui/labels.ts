import type { Category, DetectionStatus, DiagnosticCode, InstallMethod, ProviderKind, ToolScan, VersionStatus } from '../api/contracts';
import { primaryPathCandidate } from './toolPresentation';

export const categories: Record<Category, string> = { runtime: 'ランタイム', ai_cli: 'AI CLI', development: '開発・インフラ', package_manager: 'パッケージ管理' };
export const providers: Record<ProviderKind, string> = { path: 'PATH', installed_applications: '製品登録', winget: 'WinGet', npm_global: 'npm global', registry: 'レジストリ', known_directory: '既知ディレクトリ' };
export const statuses: Record<DetectionStatus, string> = { not_found: '範囲内で未検出', detected: '検出あり', incomplete: '一部未確認' };
export const installMethods: Record<InstallMethod, string> = { winget: 'WinGet', npm: 'npm', installer: 'インストーラー', portable: 'ポータブル' };
export const versionStatuses: Record<VersionStatus, string> = {
  not_requested: 'CLI未実行', success: '取得成功', nonzero_exit: '終了コードが非ゼロ', timed_out: 'タイムアウト',
  access_denied: 'アクセス拒否', spawn_failed: '起動失敗', unrecognized_output: '未対応のバージョン出力',
  output_limit_exceeded: '出力量の上限超過', unsupported_wrapper: '未対応のラッパー形式', probe_limit_reached: '実行回数の上限到達',
  scan_deadline_reached: 'スキャンの時間予算に到達', unsupported_platform: '未対応OS', cleanup_failed: 'プロセス終了処理の失敗',
};
export const diagnosticLabels: Record<DiagnosticCode, string> = {
  path_candidate_found: 'PATH上に候補があります。', multiple_command_candidates: '同名コマンドの候補が複数あります。',
  access_denied: 'アクセスが拒否されました。', version_probe_failed: 'バージョン取得を完了できませんでした。',
  cli_available: 'PATH優先候補の固定バージョン引数が成功しました。', availability_unverified: 'CLIの利用可否は未確認です。',
  wrapper_not_executed: 'ラッパー自体は実行せず、既知の起動先を確認しました。',
  conflicting_version_evidence: '同じ実行候補の申告値・実測値が食い違います。', version_format_difference: 'バージョンに表記上の差があります。',
  path_entries_skipped: '許可範囲外または上限超過のPATH要素を除外しました。', unsupported_path_extension: '未対応のPATHEXT拡張子があります。',
  candidate_limit_reached: '保持する実行候補の上限に到達しました。', provider_failed: '情報源の一部を読み取れませんでした。',
  scan_deadline_reached: 'スキャンの時間予算に到達しました。', metadata_invalid: '不正または矛盾するメタデータがあります。',
  metadata_limit_reached: 'メタデータの探索・保持上限に到達しました。', registered_executable_unresolved: '登録はありますが、実行候補を確認できませんでした。',
  registered_tool_not_on_path: '登録はありますが、このツールのPATH候補を確認できませんでした。', conflicting_registration_evidence: '同じ登録の申告バージョンが食い違います。',
};
export function versionSummary(scan?: ToolScan): string {
  if (!scan) return '未スキャン';
  const primary = primaryPathCandidate(scan);
  return primary?.version.status === 'success' && primary.version.source && primary.version.version ? primary.version.version : '未確認';
}
export function dateTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '日時不明' : date.toLocaleString('ja-JP', { hour12: false });
}
export function scanDuration(startedAt: string, completedAt: string): string {
  const milliseconds = Date.parse(completedAt) - Date.parse(startedAt);
  if (!Number.isFinite(milliseconds) || milliseconds < 0) return '不明';
  return `${new Intl.NumberFormat('ja-JP', { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(milliseconds / 1000)}秒`;
}
