import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

export interface WindowApi {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
}

function currentWindow() {
  if (!isTauri()) throw { code: 'desktop_unavailable' };
  return getCurrentWindow();
}

export const tauriWindowApi: WindowApi = {
  minimize: async () => currentWindow().minimize(),
  toggleMaximize: async () => currentWindow().toggleMaximize(),
  close: async () => currentWindow().close(),
};
