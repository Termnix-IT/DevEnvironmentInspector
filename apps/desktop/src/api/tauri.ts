import { invoke, isTauri } from '@tauri-apps/api/core';
import type { InspectorApi } from './contracts';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw { code: 'desktop_unavailable' };
  return invoke<T>(command, args);
}
export const tauriApi: InspectorApi = {
  catalog: () => call('get_catalog'),
  snapshot: (paths) => call('get_snapshot', { paths }),
  scan: (request) => call('scan_environment', { request }),
  export: (request) => call('export_snapshot', { request }),
};
