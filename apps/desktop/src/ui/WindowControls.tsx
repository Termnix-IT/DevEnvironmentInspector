import { useRef, useState } from 'react';
import { tauriWindowApi } from '../api/window';
import type { WindowApi } from '../api/window';
import { Icon } from './Icon';

export function WindowControls({ api = tauriWindowApi }: { api?: WindowApi }) {
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  async function run(action: keyof WindowApi) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(false);
    try { await api[action](); }
    catch { setError(true); }
    finally { inFlight.current = false; setBusy(false); }
  }
  return <div className="window-actions" aria-label="ウィンドウ操作" data-tauri-drag-region="false">
    <button className="window-button" disabled={busy} aria-label="最小化" title="最小化" onClick={() => void run('minimize')}><Icon name="minimize" /></button>
    <button className="window-button" disabled={busy} aria-label="最大化・復元" title="最大化・復元" onClick={() => void run('toggleMaximize')}><Icon name="maximize" /></button>
    <button className="window-button window-close" disabled={busy} aria-label="閉じる" title="閉じる" onClick={() => void run('close')}><Icon name="close" /></button>
    {error && <div role="alert" className="window-error">ウィンドウを操作できませんでした。Alt+F4で終了できます。</div>}
  </div>;
}
