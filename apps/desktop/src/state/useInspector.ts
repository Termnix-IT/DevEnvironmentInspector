import { useEffect, useRef, useState } from 'react';
import type { Catalog, InspectorApi, PathExposure, SnapshotView } from '../api/contracts';

const errors: Record<string, string> = {
  desktop_unavailable: 'デスクトップアプリから起動してください。ブラウザー単独では検出できません。',
  scan_in_progress: '別のスキャンを実行中です。完了後に再試行してください。',
  scan_rate_limited: '前回のスキャン完了から2秒以上待って再試行してください。',
  scan_failed: 'スキャンを完了できませんでした。前回の結果を保持しています。',
  unsupported_platform: '実機スキャンはWindows専用です。',
  no_snapshot: '先にスキャンを実行してください。',
  stale_snapshot: '保存対象が更新されています。最新の結果を確認して再度保存してください。',
  export_in_progress: '保存ダイアログを処理中です。',
  export_failed: 'JSONを保存できませんでした。保存先とアクセス権を確認してください。',
  invalid_destination: 'JSON形式の保存先を選択してください。',
  unavailable: 'アプリの内部状態を取得できませんでした。',
};
export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'code' in error && typeof error.code === 'string') {
    return Object.hasOwn(errors, error.code) ? errors[error.code]! : '処理に失敗しました。再度お試しください。';
  }
  return '処理に失敗しました。再度お試しください。';
}

export function useInspector(api: InspectorApi) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [view, setView] = useState<SnapshotView | null>(null);
  const [paths, setPaths] = useState<PathExposure>('redact_home');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loadingCatalog, setLoadingCatalog] = useState(true);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const [operation, setOperation] = useState<'scan' | 'paths' | 'export' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);

  useEffect(() => {
    let active = true;
    setLoadingCatalog(true);
    setError(null);
    void api.catalog().then((result) => {
      if (!active) return;
      setCatalog(result);
      setSelectedId((id) => id ?? result.tools[0]?.id ?? null);
    }).catch((failure: unknown) => {
      if (active) setError(errorMessage(failure));
    }).finally(() => { if (active) setLoadingCatalog(false); });
    return () => { active = false; };
  }, [api, catalogAttempt]);

  function begin(next: NonNullable<typeof operation>): boolean {
    if (inFlight.current || loadingCatalog || !catalog) return false;
    inFlight.current = true;
    setOperation(next); setError(null); setNotice(null);
    return true;
  }
  function finish() { inFlight.current = false; setOperation(null); }

  async function scan(probeVersions: boolean, pathOnly: boolean) {
    if (!begin('scan')) return;
    try {
      const result = await api.scan({ probe_versions: probeVersions, path_only: pathOnly, paths });
      setView(result);
      setNotice('スキャンが完了しました。');
    } catch (failure) { setError(errorMessage(failure)); }
    finally { finish(); }
  }
  async function changePaths(next: PathExposure) {
    if (!view) { setPaths(next); return; }
    if (!begin('paths')) return;
    try {
      const result = await api.snapshot(next);
      if (!result) throw { code: 'no_snapshot' };
      setView(result); setPaths(next);
    } catch (failure) { setError(errorMessage(failure)); }
    finally { finish(); }
  }
  async function exportJson(exposure: PathExposure) {
    if (!view || !begin('export')) return;
    try {
      const outcome = await api.export({ revision: view.revision, paths: exposure });
      setNotice(outcome === 'saved' ? 'JSONを保存しました。' : '保存をキャンセルしました。');
    } catch (failure) { setError(errorMessage(failure)); }
    finally { finish(); }
  }
  return { catalog, view, paths, selectedId, setSelectedId, loadingCatalog, operation, error, notice,
    busy: loadingCatalog || operation !== null, scan, changePaths, exportJson,
    reloadCatalog: () => setCatalogAttempt((value) => value + 1) };
}
