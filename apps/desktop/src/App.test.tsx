import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
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

describe('仮GUIと通信境界', () => {
  it('起動時はカタログだけを取得し、スキャン・CLI実行・保存を始めない', async () => {
    const client = await loaded();
    expect(client.scan).not.toHaveBeenCalled(); expect(client.export).not.toHaveBeenCalled();
    expect((screen.getByRole('checkbox', { name: 'バージョン取得のためCLIを起動する' }) as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText('画面のパス表示') as HTMLSelectElement).value).toBe('redact_home');
    expect((screen.getByLabelText('JSONのパス公開範囲') as HTMLSelectElement).value).toBe('omit');
    expect((screen.getByRole('button', { name: 'JSONエクスポート' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('受動スキャン後に申告と実測を分け、登録だけのツールも表示する', async () => {
    const client = await scanned();
    expect(client.scan).toHaveBeenCalledWith({ probe_versions: false, path_only: false, paths: 'redact_home' });
    expect(screen.getByText('申告 24.1.0')).toBeTruthy();
    expect(screen.getByText(/未取得（CLI未実行）/)).toBeTruthy();
    expect(screen.getByText('<HOME>/開発 tools/node.exe')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Pythonの詳細' }));
    expect(screen.getByRole('heading', { name: '実行候補（0件）' })).toBeTruthy();
    expect(screen.getByText('<HOME>/Python314')).toBeTruthy();
    expect(screen.getByText(/登録はありますが、実行候補を確認できませんでした/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Gitの詳細' }));
    expect(screen.getAllByText(/範囲内で未検出/).length).toBeGreaterThan(0);
  });

  it('バージョン起動とPATH限定は明示的な操作だけで有効になる', async () => {
    const client = await loaded();
    fireEvent.click(screen.getByRole('checkbox', { name: 'バージョン取得のためCLIを起動する' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'PATHのみ探索' }));
    fireEvent.click(screen.getByRole('button', { name: 'スキャン実行' }));
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
    expect((screen.getByLabelText('画面のパス表示') as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'JSONエクスポート' }) as HTMLButtonElement).disabled).toBe(true);
    finish(view); await screen.findByText('スキャンが完了しました。');
  });

  it('再スキャン失敗時に前回の詳細と取得日時を保持し、生のエラーを表示しない', async () => {
    const client = await scanned();
    const previous = screen.getByText(/スキャン日時/).textContent;
    client.scan.mockRejectedValueOnce({ code: 'scan_failed', message: 'PRIVATE_TEST_VALUE' });
    fireEvent.click(screen.getByRole('button', { name: '再スキャン' }));
    await screen.findByRole('alert');
    expect(screen.getByText(/スキャン日時/).textContent).toBe(previous);
    expect(screen.getByText('<HOME>/開発 tools/node.exe')).toBeTruthy();
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull();
  });

  it('パスの変更は再スキャンせず、キャッシュの公開範囲を変更する', async () => {
    const client = await scanned();
    const omitted = structuredClone(view);
    omitted.snapshot.path_exposure = 'omit'; omitted.snapshot.tools[0]!.installations[0]!.executable_path = null;
    client.snapshot.mockResolvedValueOnce(omitted);
    fireEvent.change(screen.getByLabelText('画面のパス表示'), { target: { value: 'omit' } });
    await screen.findByText('パスを省略しています');
    expect(client.snapshot).toHaveBeenCalledWith('omit'); expect(client.scan).toHaveBeenCalledTimes(1);
  });

  it('JSON保存では画面とは独立した公開範囲と表示中のrevisionだけを送る', async () => {
    const client = await scanned();
    fireEvent.click(screen.getByRole('button', { name: 'JSONエクスポート' }));
    await screen.findByText('JSONを保存しました。');
    expect(client.export).toHaveBeenCalledWith({ revision: 1, paths: 'omit' });
    expect(client.export.mock.calls[0]![0]).not.toHaveProperty('path');
  });

  it('保存キャンセルと保存失敗を区別し、どちらでも結果を失わない', async () => {
    const client = api(); client.export.mockResolvedValueOnce('cancelled');
    await scanned(client);
    fireEvent.click(screen.getByRole('button', { name: 'JSONエクスポート' }));
    await screen.findByText('保存をキャンセルしました。');
    client.export.mockRejectedValueOnce({ code: 'export_failed' });
    fireEvent.click(screen.getByRole('button', { name: 'JSONエクスポート' }));
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
    expect(screen.getByText('ネイティブ実行')).toBeTruthy();
  });
});
