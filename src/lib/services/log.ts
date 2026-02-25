import { info, warn, error, attachConsole } from '@tauri-apps/plugin-log';
export { info, warn, error };

export async function initLogging() {
  if (import.meta.env.DEV) {
    await attachConsole();
  }
}
