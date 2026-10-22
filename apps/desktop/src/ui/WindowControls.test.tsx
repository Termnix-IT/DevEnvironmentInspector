import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { WindowApi } from '../api/window';
import { WindowControls } from './WindowControls';

function api() {
  return { minimize: vi.fn<WindowApi['minimize']>().mockResolvedValue(undefined), toggleMaximize: vi.fn<WindowApi['toggleMaximize']>().mockResolvedValue(undefined), close: vi.fn<WindowApi['close']>().mockResolvedValue(undefined) } satisfies WindowApi;
}

describe('枠なしウィンドウの操作境界', () => {
  it('表示だけでは操作せず、ボタンに対応した現在のウィンドウ操作だけを呼ぶ', async () => {
    const client = api(); render(<WindowControls api={client} />);
    expect(client.minimize).not.toHaveBeenCalled(); expect(client.close).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '最小化' }));
    await waitFor(() => expect((screen.getByRole('button', { name: '最小化' }) as HTMLButtonElement).disabled).toBe(false));
    expect(client.minimize).toHaveBeenCalledTimes(1); expect(client.toggleMaximize).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '最大化・復元' }));
    await waitFor(() => expect((screen.getByRole('button', { name: '最大化・復元' }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole('button', { name: '最大化・復元' }));
    await waitFor(() => expect(client.toggleMaximize).toHaveBeenCalledTimes(2));
    await waitFor(() => expect((screen.getByRole('button', { name: '閉じる' }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole('button', { name: '閉じる' }));
    await waitFor(() => expect(client.close).toHaveBeenCalledTimes(1));
  });

  it('操作の完了前に二重起動せず、失敗後は再試行でき、生の例外を表示しない', async () => {
    const client = api();
    let reject!: (error: unknown) => void;
    client.toggleMaximize.mockImplementationOnce(() => new Promise((_, failure) => { reject = failure; }));
    render(<WindowControls api={client} />);
    const maximize = screen.getByRole('button', { name: '最大化・復元' }) as HTMLButtonElement;
    fireEvent.click(maximize); fireEvent.click(maximize);
    expect(client.toggleMaximize).toHaveBeenCalledTimes(1); expect(maximize.disabled).toBe(true);
    reject('PRIVATE_TEST_VALUE'); await screen.findByRole('alert');
    expect(screen.queryByText(/PRIVATE_TEST_VALUE/)).toBeNull(); expect(maximize.disabled).toBe(false);
    fireEvent.click(maximize);
    await waitFor(() => expect(client.toggleMaximize).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
