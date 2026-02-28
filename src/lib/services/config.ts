import { readFileText, writeFileText } from '$lib/services/tauri';
import type { FavoriteItem } from '$lib/state/sidebar.svelte';
import type { Workspace } from '$lib/state/workspaces.svelte';
import type { S3Bookmark, SftpBookmark, ConnectionProfile, SortField, SortDirection } from '$lib/types';
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
  connections: ConnectionProfile[];
  s3Bookmarks: S3Bookmark[];
  sftpBookmarks: SftpBookmark[];
  bandwidthLimit: number;
  maxConcurrent: number;
  secureTempCleanup: boolean;
  sortField: SortField;
  sortDirection: SortDirection;
  syncExcludePatterns: string;
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
  connections: [],
  s3Bookmarks: [],
  sftpBookmarks: [],
  bandwidthLimit: 0,
  maxConcurrent: 2,
  secureTempCleanup: false,
  sortField: 'name',
  sortDirection: 'asc',
  syncExcludePatterns: '.DS_Store, Thumbs.db, .git/**',
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
    // Migrate old s3Profiles key → connections
    if (parsed.s3Profiles && !parsed.connections) {
      config.connections = parsed.s3Profiles;
    }
    // Migrate existing profiles that lack a provider or type field
    for (const p of config.connections) {
      if (!p.type) {
        p.type = 's3';
      }
      if (p.type === 's3' && !p.provider) {
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
