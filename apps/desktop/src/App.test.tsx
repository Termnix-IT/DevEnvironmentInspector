import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import type { Catalog, InspectorApi, SnapshotView } from './api/contracts';
import fixture from './test/fixtures/snapshot.json';
import { errorMessage } from './state/useInspector';

const view = fixture as SnapshotView;
const catalog: Catalog = { schema_version: 1, tools: view.snapshot.tools.map((scan) => scan.tool) };
function api() {
  return { catalog: vi.fn<InspectorApi['catalog']>().mockResolvedValue(catalog), scan: vi.fn<InspectorApi['scan']>().mockImplementation(async () => structuredClone(view)),
    snapshot: vi.fn<InspectorApi['snapshot']>().mockImplementation(async () => structuredClone(view)), export: vi.fn<InspectorApi['export']>().mockResolvedValue('saved') } satisfies InspectorApi;
}
function navigate(name: string) { fireEvent.click(within(screen.getByRole('navigation')).getByRole('button', { name: new RegExp(name) })); }
function exportJson() {
  fireEvent.click(screen.getByLabelText('その他の操作'));
  fireEvent.click(screen.getByRole('button', { name: 'JSONエクスポート' }));
}
async function loaded(client = api()) {
  render(<App api={client} />);
  await waitFor(() => expect((screen.getByRole('button', { name: 'スキャン実行' }) as HTMLButtonElement).disabled).toBe(false));
  return client;
}
async function scanned(client = api()) {
  await loaded(client);
  fireEvent.click(screen.getByRole('button', { name: 'スキャン実行' }));
  await screen.findByText('スキャンが完了しました。');
  return client;
}
afterEach(() => vi.unstubAllGlobals());

describe('環境インスペクターの画面と通信境界', () => {
  it('起動時はカタログだけを取得し、スキャン・CLI実行・保存を始めない', async () => {
    const client = await loaded();
    expect(client.scan).not.toHaveBeenCalled(); expect(client.export).not.toHaveBeenCalled();
    navigate('Settings');
    expect((screen.getByRole('checkbox', { name: 'バージョン取得のためCLIを起動する' }) as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText('画面のパス表示') as HTMLSelectElement).value).toBe('redact_home');
    expect((screen.getByLabelText('JSONのパス公開範囲') as HTMLSelectElement).value).toBe('omit');
    expect((within(screen.getByRole('region', { name: '設定' })).getByRole('button', { name: 'JSONエクスポート' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('受動スキャン後に申告と実測を分け、登録だけのツールも詳細展開できる', async () => {
    const client = await scanned();
    expect(client.scan).toHaveBeenCalledWith({ probe_versions: false, path_only: false, paths: 'redact_home' });
    expect(screen.getByText('申告 24.1.0')).toBeTruthy();
    expect(screen.getByText(/未取得（CLI未実行）/)).toBeTruthy();
    expect(screen.getByText('<HOME>/開発 tools/node.exe')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Pythonの詳細' }));
    expect(screen.getByText('実行候補（0件）')).toBeTruthy();
    expect(screen.getByText('<HOME>/Python314')).toBeTruthy();
    fireEvent.click(screen.getByText('診断（1件）'));
    expect(screen.getByText(/登録はありますが、実行候補を確認できませんでした/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Gitの詳細' }));
    expect(screen.getAllByText(/範囲内で未検出/).length).toBeGreaterThan(0);
  });

  it('バージョン起動とPATH限定は設定での明示的な操作だけで有効になる', async () => {
    const client = await loaded();
    navigate('Settings');
    fireEvent.click(screen.getByRole('checkbox', { name: 'バージョン取得のためCLIを起動する' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'PATHのみ探索' }));
    fireEvent.click(screen.getByRole('button', { name: 'この設定でスキャン' }));
    await screen.findByText('スキャンが完了しました。');
    expect(client.scan).toHaveBeenCalledWith({ probe_versions: true, path_only: true, paths: 'redact_home' });
  });

  it('実行中は二重スキャンと保存・公開範囲変更を抑止する', async () => {
    const client = api();
    let finish!: (value: SnapshotView) => void;
    client.scan.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    await loaded(client);
    fireEvent.click(screen.getByRole('button', { name: 'スキャン実行' }));
    const running = screen.getByRole('button', { name: 'スキャン中…' }) as HTMLButtonElement;
    expect(running.disabled).toBe(true); fireEvent.click(running);
    expect(client.scan).toHaveBeenCalledTimes(1);
    navigate('Settings');
    expect((screen.getByLabelText('画面のパス表示') as HTMLSelectElement).disabled).toBe(true);
    expect((within(screen.getByRole('region', { name: '設定' })).getByRole('button', { name: 'JSONエクスポート' }) as HTMLButtonElement).disabled).toBe(true);
    finish(view); await screen.findByText('スキャンが完了しました。');
  });

  it('再スキャン失敗時に前回の詳細と取得日時を保持し、生のエラーを表示しない', async () => {
    const client = await scanned();
    const previous = document.querySelector('.statusbar-time time')?.textContent;
    client.scan.mockRejectedValueOnce({ code: 'scan_failed', message: 'PRIVATE_TEST_VALUE' });
    fireEvent.click(screen.getByRole('button', { name: '再スキャン' }));
    await screen.findByRole('alert');
    expect(document.querySelector('.statusbar-time time')?.textContent).toBe(previous);
    expect(screen.getByText('<HOME>/開発 tools/node.exe')).toBeTruthy();
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull();
  });

  it('パスの変更は再スキャンせず、キャッシュの公開範囲を変更する', async () => {
    const client = await scanned();
    const omitted = structuredClone(view);
    omitted.snapshot.path_exposure = 'omit'; omitted.snapshot.tools[0]!.installations[0]!.executable_path = null;
    client.snapshot.mockResolvedValueOnce(omitted);
    navigate('Settings');
    fireEvent.change(screen.getByLabelText('画面のパス表示'), { target: { value: 'omit' } });
    await waitFor(() => expect(client.snapshot).toHaveBeenCalledWith('omit'));
    navigate('Inventory');
    await screen.findByText('パスを省略しています');
    expect(client.scan).toHaveBeenCalledTimes(1);
    expect((screen.getByRole('button', { name: 'パスをコピー' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('JSON保存では画面とは独立した公開範囲と表示中のrevisionだけを送る', async () => {
    const client = await scanned();
    exportJson();
    await screen.findByText('JSONを保存しました。');
    expect(client.export).toHaveBeenCalledWith({ revision: 1, paths: 'omit' });
    expect(client.export.mock.calls[0]![0]).not.toHaveProperty('path');
  });

  it('保存キャンセルと保存失敗を区別し、どちらでも結果を失わない', async () => {
    const client = api(); client.export.mockResolvedValueOnce('cancelled');
    await scanned(client);
    exportJson();
    await screen.findByText('保存をキャンセルしました。');
    client.export.mockRejectedValueOnce({ code: 'export_failed' });
    exportJson();
    expect(within(await screen.findByRole('alert')).getByText(/保存できませんでした/)).toBeTruthy();
    expect(screen.getByText('<HOME>/開発 tools/node.exe')).toBeTruthy();
  });

  it('カタログの読込失敗から再試行できる', async () => {
    const client = api(); client.catalog.mockRejectedValueOnce({ code: 'unavailable' });
    render(<App api={client} />);
    fireEvent.click(await screen.findByRole('button', { name: 'カタログを再読込' }));
    await screen.findByRole('button', { name: 'Node.jsの詳細' });
    expect(client.scan).not.toHaveBeenCalled();
  });
  it('未知の例外や認証情報を含む文字列を画面へ反射しない', () => {
    expect(errorMessage('PRIVATE_TEST_VALUE')).not.toContain('PRIVATE_TEST_VALUE');
    expect(errorMessage({ code: 'PRIVATE_TEST_VALUE' })).not.toContain('PRIVATE_TEST_VALUE');
    expect(errorMessage({ code: 'toString' })).toBe('処理に失敗しました。再度お試しください。');
  });

  it('CLIから取得した値は実測バージョンと取得方法として表示する', async () => {
    const measured = structuredClone(view);
    measured.snapshot.tools[0]!.installations[0]!.version = { status: 'success', version: '24.22.1', exit_code: 0, source: 'native_executable' };
    const client = api(); client.scan.mockResolvedValueOnce(measured);
    await scanned(client);
    expect(screen.getByText('実測 24.22.1 / 申告 24.1.0')).toBeTruthy();
    expect(screen.getByText('24.22.1（取得成功）')).toBeTruthy();
    fireEvent.click(screen.getByText('アクセス・取得方法・申告値'));
    expect(screen.getByText('ネイティブ実行')).toBeTruthy();
  });

  it('検索とカテゴリを組み合わせ、コマンド別名でも絞り込み、空結果から戻れる', async () => {
    const client = await loaded();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'PYTHON3' } });
    expect(screen.getByRole('button', { name: 'Pythonの詳細' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Node.jsの詳細' })).toBeNull();
    fireEvent.change(screen.getByLabelText('カテゴリで絞り込み'), { target: { value: 'development' } });
    expect(screen.getByText('一致するツールがありません')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '絞り込みを解除' }));
    expect(screen.getByRole('button', { name: 'Gitの詳細' })).toBeTruthy();
    expect(client.scan).not.toHaveBeenCalled();
  });

  it('詳細行を開閉でき、状態ドットに実際の検出状態の説明がある', async () => {
    await scanned();
    const node = screen.getByRole('button', { name: 'Node.jsの詳細' });
    expect(node.getAttribute('aria-expanded')).toBe('true');
    fireEvent.click(node);
    expect(node.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('region', { name: 'Node.jsの詳細情報' })).toBeNull();
    fireEvent.click(node);
    expect(screen.getByRole('region', { name: 'Node.jsの詳細情報' })).toBeTruthy();
    expect(screen.getByRole('img', { name: '検出あり・警告あり' })).toBeTruthy();
  });

  it('診断画面から該当ツールの詳細へ戻り、検索条件も解除する', async () => {
    await scanned();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'node' } });
    navigate('Diagnostics');
    const pythonRow = screen.getByRole('button', { name: 'Pythonの詳細を表示' }).closest('tr')!;
    expect(within(pythonRow).getByText(/登録はありますが/)).toBeTruthy();
    fireEvent.click(within(pythonRow).getByRole('button', { name: 'Pythonの詳細を表示' }));
    expect(screen.getByRole('region', { name: 'Pythonの詳細情報' })).toBeTruthy();
    expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Pythonの詳細' }));
  });

  it('診断表は確認済みの事実と注意、重要度、取得元を保持する', async () => {
    const withGlobal = structuredClone(view);
    withGlobal.snapshot.diagnostics.push({ code: 'provider_failed', class: 'fact', severity: 'error', provider: 'registry', installation_id: null });
    const client = api(); client.scan.mockResolvedValueOnce(withGlobal);
    await scanned(client); navigate('Diagnostics');
    const table = screen.getByRole('table', { name: '環境とツールの診断結果' });
    expect(within(table).getByText('確認済み・エラー')).toBeTruthy();
    expect(within(table).getByText('注意・警告')).toBeTruthy();
    expect(within(table).getByText('Environment')).toBeTruthy();
    expect(within(table).getAllByText('レジストリ').length).toBeGreaterThan(0);
    expect(within(table).getByText('node-1')).toBeTruthy();
  });

  it('MCP画面は5ツールを案内し、未確認の接続状態を接続済みにしない', async () => {
    const client = await scanned();
    navigate('MCP');
    expect(screen.getByText('get_tool_info')).toBeTruthy();
    expect(screen.getByText('scan_environment')).toBeTruthy();
    expect(screen.getByText(/接続状態は監視していません/)).toBeTruthy();
    expect(screen.queryByText(/MCP connected|MCP接続済み/)).toBeNull();
    expect(client.scan).toHaveBeenCalledTimes(1);
    expect(document.querySelector('.statusbar-count')?.textContent).toBe('2 tools · 2 notices');
  });

  it('Ctrl+Kで別ページから検索欄へ移動し、操作メニューはEscapeで閉じる', async () => {
    await loaded();
    navigate('Settings');
    fireEvent.keyDown(document, { key: 'k', ctrlKey: true });
    expect(document.activeElement).toBe(screen.getByRole('searchbox'));
    const summary = screen.getByLabelText('その他の操作');
    fireEvent.click(summary);
    expect((summary.parentElement as HTMLDetailsElement).open).toBe(true);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect((summary.parentElement as HTMLDetailsElement).open).toBe(false);
    expect(document.activeElement).toBe(summary);
  });

  it('コピーは表示中のパスだけを書き込み、クリップボードを読み取らない', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const readText = vi.fn();
    vi.stubGlobal('navigator', { clipboard: { writeText, readText } });
    const client = await scanned();
    fireEvent.click(screen.getByRole('button', { name: 'パスをコピー' }));
    await screen.findByRole('button', { name: 'コピーしました' });
    expect(writeText).toHaveBeenCalledWith('<HOME>/開発 tools/node.exe');
    expect(readText).not.toHaveBeenCalled(); expect(client.scan).toHaveBeenCalledTimes(1);
  });

  it('コピーが拒否された場合は失敗を表示し、生の例外を反射しない', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue('PRIVATE_TEST_VALUE') } });
    await scanned();
    fireEvent.click(screen.getByRole('button', { name: 'パスをコピー' }));
    await screen.findByText(/コピーできませんでした/);
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull();
  });
});
