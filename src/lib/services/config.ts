import { readFileText, writeFileText } from '$lib/services/tauri';
import type { FavoriteItem } from '$lib/state/sidebar.svelte';
import type { Workspace } from '$lib/state/workspaces.svelte';
import type { S3Profile, SortField, SortDirection } from '$lib/types';
import { inferProviderFromEndpoint } from '$lib/data/s3-providers';

export interface Config {
  theme: 'dark' | 'light';
  layoutMode: 'dual' | 'single';
  iconSize: number;
  startupSound: boolean;
  showHidden: boolean;
  calculateDirSizes: boolean;
  externalEditor: string;
  favorites: FavoriteItem[];
  workspaces: Workspace[];
  s3Profiles: S3Profile[];
  bandwidthLimit: number;
  sortField: SortField;
  sortDirection: SortDirection;
}

export const DEFAULT_CONFIG: Config = {
  theme: 'dark',
  layoutMode: 'dual',
  iconSize: 48,
  startupSound: true,
  showHidden: false,
  calculateDirSizes: true,
  externalEditor: '',
  favorites: [],
  workspaces: [],
  s3Profiles: [],
  bandwidthLimit: 0,
  sortField: 'name',
  sortDirection: 'asc',
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
    const config = { ...DEFAULT_CONFIG, ...parsed };
    // Migrate existing profiles that lack a provider field
    for (const p of config.s3Profiles) {
      if (!p.provider) {
        p.provider = inferProviderFromEndpoint(p.endpoint);
      }
    }
    return config;
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}

export async function saveConfig(config: Config): Promise<void> {
  const path = await getConfigPath();
  await writeFileText(path, JSON.stringify(config, null, 2));
}
