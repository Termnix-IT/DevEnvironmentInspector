import type { DiagnosticCode, Installation, ToolScan } from '../api/contracts';

// カタログのコマンド順を使い、最初に見つかったコマンドのPATH順だけを比較する。
// 別の候補が実測済みでも、優先候補の代わりにはしない。
export function primaryPathCandidate(scan?: ToolScan): Installation | undefined {
  if (!scan) return undefined;
  for (const command of scan.tool.commands) {
    const candidates = scan.installations.filter((item) => item.command === command && item.on_path);
    if (!candidates.length) continue;
    if (candidates.some((item) => item.path_order === null || !Number.isSafeInteger(item.path_order) || item.path_order < 0)) return undefined;
    const ordered = [...candidates].sort((a, b) => a.path_order! - b.path_order!);
    if (ordered.length > 1 && ordered[0]!.path_order === ordered[1]!.path_order) return undefined;
    return ordered[0];
  }
  return undefined;
}

interface ToolStatus { tone: 'positive' | 'neutral' | 'caution' | 'negative'; label: string; detail: string }
export function toolStatus(scan?: ToolScan): ToolStatus {
  if (!scan) return { tone: 'neutral', label: '未スキャン', detail: 'スキャンを実行していません。' };
  const primary = primaryPathCandidate(scan);
  if (!primary) {
    if (scan.installations.some((item) => item.on_path)) return { tone: 'neutral', label: '解決順不明', detail: 'PATH候補はありますが、優先候補を確定できません。' };
    if (scan.installations.length || scan.registrations.length) return { tone: 'neutral', label: 'PATH未確認', detail: '検出情報はありますが、PATH上の実行候補は確認できません。' };
    return { tone: 'neutral', label: scan.status === 'incomplete' ? '探索未完了' : '未検出', detail: '探索範囲で候補を確認できません。未インストールの断定ではありません。' };
  }
  const prefix = `${primary.command}: PATH優先候補。`;
  if (primary.access === 'denied' || primary.version.status === 'access_denied') return { tone: 'negative', label: 'アクセス拒否', detail: `${prefix}アクセスが拒否されました。` };
  switch (primary.version.status) {
    case 'spawn_failed': return { tone: 'negative', label: '起動失敗', detail: `${prefix}バージョン取得プロセスを起動できませんでした。` };
    case 'cleanup_failed': return { tone: 'negative', label: '終了処理失敗', detail: `${prefix}バージョン取得プロセスの終了処理に失敗しました。` };
    case 'timed_out': return { tone: 'caution', label: '時間超過', detail: `${prefix}バージョン取得がタイムアウトしました。` };
    case 'nonzero_exit':
    case 'unrecognized_output':
    case 'output_limit_exceeded': return { tone: 'caution', label: '取得失敗', detail: `${prefix}バージョン取得を完了できませんでした。通常の操作が実行不能かは未確認です。` };
    case 'success':
      if (primary.version.version && primary.version.source === 'native_executable') return { tone: 'positive', label: '実測済み', detail: `${prefix}固定のバージョン取得引数での実行に成功しました。` };
      if (primary.version.version && primary.version.source === 'npm_entrypoint') return { tone: 'neutral', label: '起動先確認', detail: `${prefix}既知のnpm起動先を実測しました。ラッパー自体の実行は未確認です。` };
  }
  return { tone: 'neutral', label: '未検証', detail: `${prefix}CLI自体の実行可否は未確認です。` };
}

export function diagnosticArea(code: DiagnosticCode): string {
  switch (code) {
    case 'conflicting_version_evidence':
    case 'conflicting_registration_evidence':
    case 'version_format_difference':
    case 'metadata_invalid': return '情報照合';
    case 'path_entries_skipped':
    case 'unsupported_path_extension':
    case 'candidate_limit_reached':
    case 'provider_failed':
    case 'scan_deadline_reached':
    case 'metadata_limit_reached': return '探索範囲';
    case 'path_candidate_found':
    case 'multiple_command_candidates':
    case 'registered_executable_unresolved':
    case 'registered_tool_not_on_path': return 'PATH・登録';
    default: return '実行確認';
  }
}
