import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import type { InspectorApi } from './api/contracts';
import type { ProjectApi, ProjectView } from './api/projects';
import fixture from './test/fixtures/projects.json';

const view = fixture as ProjectView;
function client(initial = true) {
  let projects = initial ? [structuredClone(view.project)] : [];
  let cached: ProjectView | null = null;
  return {
    list: vi.fn<ProjectApi['list']>().mockImplementation(async () => structuredClone(projects)),
    snapshot: vi.fn<ProjectApi['snapshot']>().mockImplementation(async (request) => cached ? structuredClone(cached) : { project: structuredClone(projects.find((p) => p.id === request.project_id)!), snapshot: null, scan_revision: null }),
    scan: vi.fn<ProjectApi['scan']>().mockImplementation(async () => { cached = structuredClone(view); return structuredClone(cached); }),
    chooseFolder: vi.fn<ProjectApi['chooseFolder']>().mockResolvedValue({ token: 'selected-1', path: '<HOME>/開発 tools/sample-api' }),
    register: vi.fn<ProjectApi['register']>().mockImplementation(async (request) => { const project = { ...structuredClone(view.project), name: request.name, mcp_allowed: request.mcp_allowed }; projects.push(project); return structuredClone(project); }),
    update: vi.fn<ProjectApi['update']>().mockImplementation(async (request) => { const project = { ...structuredClone(view.project), name: request.name, mcp_allowed: request.mcp_allowed, revision: 2 }; projects = [project]; cached = null; return structuredClone(project); }),
    remove: vi.fn<ProjectApi['remove']>().mockImplementation(async (request) => { projects = projects.filter((p) => p.id !== request.project_id); cached = null; }),
    export: vi.fn<ProjectApi['export']>().mockResolvedValue('saved'),
  };
}
const globalApi: InspectorApi = { catalog: async () => ({ schema_version: 1, tools: [] }), scan: vi.fn(), snapshot: async () => null, export: vi.fn() };
function navigate(name: string) { fireEvent.click(within(screen.getByRole('navigation')).getByRole('button', { name: new RegExp(`^${name}`) })); }
async function loaded(api = client()) {
  render(<App api={globalApi} projectApi={api} />); navigate('Projects');
  await waitFor(() => expect((screen.getByRole('button', { name: 'プロジェクトを登録' }) as HTMLButtonElement).disabled).toBe(false));
  return api;
}
async function scanned(api = client()) {
  await loaded(api);
  fireEvent.click(within(screen.getByRole('region', { name: '選択プロジェクトの詳細' })).getByRole('button', { name: 'プロジェクトをスキャン' }));
  await screen.findByText('プロジェクトのスキャンが完了しました。'); return api;
}
beforeEach(() => {
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: function (this: HTMLDialogElement) { this.setAttribute('open', ''); } });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: function (this: HTMLDialogElement) { this.removeAttribute('open'); } });
});

describe('登録プロジェクトの画面と通信境界', () => {
  it('一覧を開いても登録済み環境を自動スキャンしない', async () => {
    const api = await loaded();
    expect(api.list).toHaveBeenCalledWith('redact_home'); expect(api.scan).not.toHaveBeenCalled();
    expect(screen.getByText('このプロセスでは未スキャンです。')).toBeTruthy();
    expect(screen.queryByText('fastapi')).toBeNull();
  });
  it('登録はユーザー選択のトークンだけを送り、既定ではMCP参照もスキャンも有効にしない', async () => {
    const api = await loaded(client(false));
    fireEvent.click(screen.getByRole('button', { name: 'プロジェクトを登録' }));
    const dialog = screen.getByRole('dialog');
    expect((within(dialog).getByRole('checkbox', { name: /MCPから/ }) as HTMLInputElement).checked).toBe(false);
    fireEvent.change(within(dialog).getByRole('textbox', { name: '表示名' }), { target: { value: 'new-project' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'プロジェクトフォルダーを選択' }));
    await waitFor(() => expect(api.chooseFolder).toHaveBeenCalledWith({ kind: 'project', paths: 'redact_home' }));
    await waitFor(() => expect((within(dialog).getByRole('button', { name: '登録' }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(within(dialog).getByRole('button', { name: '登録' }));
    await screen.findByRole('heading', { name: 'new-project' });
    expect(api.register).toHaveBeenCalledWith({ name: 'new-project', root_selection: 'selected-1', extra_venv_selection: null, mcp_allowed: false, paths: 'redact_home' });
    expect(JSON.stringify(api.register.mock.calls)).not.toContain('開発 tools'); expect(api.scan).not.toHaveBeenCalled();
  });
  it('明示スキャン後、venvとNodeの環境を切り替えて情報を分離する', async () => {
    const api = await scanned();
    expect(api.scan).toHaveBeenCalledWith({ project_id: 'project-1', paths: 'redact_home' });
    expect(screen.getByText('fastapi')).toBeTruthy(); expect(screen.getByText('0.115.12')).toBeTruthy();
    expect(screen.getByText(/有効化状態・importの成功は未確認/)).toBeTruthy();
    const timestamps = screen.getByLabelText('プロジェクトのスキャン日時');
    expect(within(timestamps).getByText('1.3秒')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Node.js / npm' }));
    const table = screen.getByRole('table', { name: 'Node.js / npmのパッケージ情報' });
    for (const text of ['^6.3.0', '6.3.2', '6.2.6', 'ロックと差異', '配置未確認']) expect(within(table).getByText(text)).toBeTruthy();
    expect(screen.queryByText('fastapi')).toBeNull();
  });
  it('失敗した再スキャンで前回のパッケージと日時を失わず、生のエラーを出さない', async () => {
    const api = await scanned(); const previous = screen.getByLabelText('プロジェクトのスキャン日時').textContent;
    api.scan.mockRejectedValueOnce({ code: 'project_scan_failed', message: 'PRIVATE_TEST_VALUE' });
    fireEvent.click(screen.getByRole('button', { name: '再スキャン' })); await screen.findByRole('alert');
    expect(screen.getByText('fastapi')).toBeTruthy(); expect(screen.getByLabelText('プロジェクトのスキャン日時').textContent).toBe(previous);
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull();
  });
  it('MCP参照の許可を明示変更でき、登録解除は確認してから行う', async () => {
    const api = await loaded(); fireEvent.click(screen.getByRole('button', { name: '登録設定' }));
    let dialog = screen.getByRole('dialog'); fireEvent.click(within(dialog).getByRole('checkbox', { name: /MCPから/ }));
    fireEvent.click(within(dialog).getByRole('button', { name: '設定を保存' }));
    await screen.findByText('登録設定を更新しました。');
    expect(api.update).toHaveBeenCalledWith({ project_id: 'project-1', name: 'sample-api', mcp_allowed: true, paths: 'redact_home' });
    fireEvent.click(screen.getByRole('button', { name: '登録設定' })); dialog = screen.getByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: '登録解除' })); expect(api.remove).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: '登録を解除する' })); await screen.findByText('プロジェクトの登録を解除しました。');
    expect(api.remove).toHaveBeenCalledWith({ project_id: 'project-1' }); expect(screen.getByText('プロジェクトを登録してください')).toBeTruthy();
  });
  it('JSONは表示中のプロジェクト・スキャンrevisionと独立した公開設定だけを送る', async () => {
    const api = await scanned(); fireEvent.click(screen.getByRole('button', { name: 'プロジェクト環境をJSONエクスポート' }));
    await screen.findByText('プロジェクト環境のJSONを保存しました。');
    expect(api.export).toHaveBeenCalledWith({ project_id: 'project-1', scan_revision: 3, paths: 'omit' });
  });
  it('登録情報の破損を空の成功として扱わず、再読み込みできる', async () => {
    const api = client(); api.list.mockRejectedValueOnce({ code: 'project_storage_invalid', message: 'PRIVATE_TEST_VALUE' });
    await loaded(api); expect(screen.getByRole('alert').textContent).toContain('既存ファイルは保持');
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '登録情報を再読み込み' }));
    await screen.findByRole('heading', { name: 'sample-api' }); expect(api.scan).not.toHaveBeenCalled();
  });
});
