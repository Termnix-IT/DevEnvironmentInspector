import { describe, expect, it } from 'vitest';
import type { Installation, SnapshotView, ToolScan, VersionStatus } from '../api/contracts';
import fixture from '../test/fixtures/snapshot.json';
import { scanDuration, versionSummary } from './labels';
import { primaryPathCandidate, toolStatus } from './toolPresentation';

const view = fixture as SnapshotView;
function node(): ToolScan { return structuredClone(view.snapshot.tools[0]!); }
function measured(candidate: Installation, version: string): Installation {
  return { ...candidate, version: { status: 'success', version, exit_code: 0, source: 'native_executable' } };
}

describe('PATH優先候補の表示', () => {
  it('保存順・バージョンの新旧ではなく、同じコマンドのPATH順で選ぶ', () => {
    const scan = node();
    const first = measured(scan.installations[0]!, '24.19.0');
    const second = { ...measured(first, '24.22.1'), id: 'node-2', path_order: 8 };
    scan.installations = [second, first];
    expect(primaryPathCandidate(scan)?.id).toBe('node-1');
    expect(versionSummary(scan)).toBe('24.19.0');
    expect(scan.installations[0]?.id).toBe('node-2');
  });

  it('優先候補が未実測なら、別候補の実測や製品登録の申告で埋めない', () => {
    const scan = node();
    scan.installations.push({ ...measured(scan.installations[0]!, '24.22.1'), id: 'node-2', path_order: 1 });
    expect(versionSummary(scan)).toBe('未確認');
    expect(toolStatus(scan).tone).toBe('neutral');
  });

  it('PATH外の候補を優先候補にせず、コマンド別名のPATH候補は文脈を保つ', () => {
    const scan = node();
    scan.tool.commands = ['python', 'python3'];
    scan.installations = [
      { ...measured(scan.installations[0]!, '3.13.0'), command: 'python', on_path: false, path_order: null },
      { ...measured(scan.installations[0]!, '3.14.0'), id: 'python3-1', command: 'python3', path_order: 4 },
    ];
    expect(primaryPathCandidate(scan)?.command).toBe('python3');
    expect(versionSummary(scan)).toBe('3.14.0');
    expect(toolStatus(scan).detail).toContain('python3:');
  });

  it('異なるコマンド間のPATH順を混ぜず、カタログで先に定義されたコマンドを使う', () => {
    const scan = node();
    scan.tool.commands = ['python', 'python3'];
    scan.installations = [
      { ...measured(scan.installations[0]!, '3.13.0'), command: 'python', path_order: 8 },
      { ...measured(scan.installations[0]!, '3.14.0'), id: 'python3-1', command: 'python3', path_order: 0 },
    ];
    expect(versionSummary(scan)).toBe('3.13.0');
  });

  it.each([null, -1, 0.5, Number.NaN])('PATH順が不明・不正なら優先候補を断定しない: %s', (path_order) => {
    const scan = node();
    scan.installations[0]!.path_order = path_order;
    expect(primaryPathCandidate(scan)).toBeUndefined();
    expect(versionSummary(scan)).toBe('未確認');
    expect(toolStatus(scan).label).toBe('解決順不明');
  });

  it('同順位の候補がある場合も断定しない', () => {
    const scan = node();
    scan.installations.push({ ...scan.installations[0]!, id: 'node-2' });
    expect(primaryPathCandidate(scan)).toBeUndefined();
  });

  it('npm起動先の成功をラッパー自体の実行確認にしない', () => {
    const scan = node();
    scan.installations[0] = measured(scan.installations[0]!, '1.2.3');
    scan.installations[0]!.version.source = 'npm_entrypoint';
    expect(versionSummary(scan)).toBe('1.2.3');
    expect(toolStatus(scan)).toMatchObject({ tone: 'neutral', label: '起動先確認' });
  });

  it.each([
    ['spawn_failed', 'negative', '起動失敗'], ['access_denied', 'negative', 'アクセス拒否'],
    ['cleanup_failed', 'negative', '終了処理失敗'], ['timed_out', 'caution', '時間超過'],
    ['nonzero_exit', 'caution', '取得失敗'], ['unrecognized_output', 'caution', '取得失敗'],
    ['not_requested', 'neutral', '未検証'], ['probe_limit_reached', 'neutral', '未検証'],
  ] as const)('バージョン検査の状態を区別する: %s', (status: VersionStatus, tone, label) => {
    const scan = node(); scan.installations[0]!.version.status = status;
    expect(toolStatus(scan)).toMatchObject({ tone, label });
  });

  it('優先候補以外の失敗や複数候補の存在で成功を上書きしない', () => {
    const scan = node();
    scan.installations[0] = measured(scan.installations[0]!, '24.19.0');
    scan.installations.push({ ...scan.installations[0]!, id: 'node-2', path_order: 2, access: 'denied' });
    scan.diagnostics.push({ code: 'multiple_command_candidates', class: 'fact', severity: 'info', provider: null, installation_id: null });
    expect(toolStatus(scan)).toMatchObject({ tone: 'positive', label: '実測済み' });
  });

  it('未検出・探索未完了・登録のみを区別する', () => {
    const scan = node(); scan.installations = []; scan.status = 'not_found';
    expect(toolStatus(scan).label).toBe('未検出');
    scan.status = 'incomplete'; expect(toolStatus(scan).label).toBe('探索未完了');
    scan.registrations = [structuredClone(view.snapshot.tools[1]!.registrations[0]!)];
    expect(toolStatus(scan).label).toBe('PATH未確認');
  });
});

describe('スキャンの所要時間', () => {
  it('異なるUTCオフセットと日付またぎでも経過時間を計算する', () => {
    expect(scanDuration('2026-10-22T23:59:59+09:00', '2026-10-22T15:00:01.350Z')).toBe('2.4秒');
    expect(scanDuration('2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z')).toBe('0.0秒');
  });
  it('不正な日時や時計の逆行を実際の所要時間として表示しない', () => {
    expect(scanDuration('invalid', '2026-10-22T00:00:00Z')).toBe('不明');
    expect(scanDuration('2026-10-22T00:00:01Z', '2026-10-22T00:00:00Z')).toBe('不明');
  });
});
