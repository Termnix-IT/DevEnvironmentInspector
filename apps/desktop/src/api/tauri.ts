import { invoke, isTauri } from '@tauri-apps/api/core';
import type { InspectorApi } from './contracts';
import type { ProjectApi } from './projects';

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
export const tauriProjectApi: ProjectApi = {
  list: (paths) => call('list_projects', { paths }),
  snapshot: (request) => call('get_project_snapshot', { request }),
  scan: (request) => call('scan_project', { request }),
  chooseFolder: (request) => call('choose_project_folder', { request }),
  register: (request) => call('register_project', { request }),
  update: (request) => call('update_project', { request }),
  remove: (request) => call('remove_project', { request }),
  export: (request) => call('export_project', { request }),
};
