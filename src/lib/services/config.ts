import { readFileText, writeFileText } from '$lib/services/tauri.ts';
import type { FavoriteItem } from '$lib/state/sidebar.svelte.ts';
import type { Workspace } from '$lib/state/workspaces.svelte.ts';

export interface Config {
  theme: 'dark' | 'light';
  iconSize: number;
  startupSound: boolean;
  showHidden: boolean;
  externalEditor: string;
  favorites: FavoriteItem[];
  workspaces: Workspace[];
}

export const DEFAULT_CONFIG: Config = {
  theme: 'dark',
  iconSize: 48,
  startupSound: true,
  showHidden: false,
  externalEditor: '',
  favorites: [],
  workspaces: [],
};

let configPath = '';

export async function getConfigPath(): Promise<string> {
  if (configPath) return configPath;
  const { homeDir } = await import('@tauri-apps/api/path');
  const home = await homeDir();
  configPath = `${home.replace(/\/+$/, '')}/.config/furman/config.json`;
  return configPath;
}

export async function loadConfig(): Promise<Config> {
  try {
    const path = await getConfigPath();
    const text = await readFileText(path);
    const parsed = JSON.parse(text);
    return { ...DEFAULT_CONFIG, ...parsed };
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}

export async function saveConfig(config: Config): Promise<void> {
  const path = await getConfigPath();
  await writeFileText(path, JSON.stringify(config, null, 2));
}
