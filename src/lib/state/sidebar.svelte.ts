import type { VolumeInfo } from '$lib/types';
import { listVolumes } from '$lib/services/tauri.ts';

export interface FavoriteItem {
  name: string;
  path: string;
}

const STORAGE_KEY = 'furman-sidebar-favorites';

class SidebarState {
  visible = $state(false);
  favorites = $state<FavoriteItem[]>([]);
  volumes = $state<VolumeInfo[]>([]);
  volumesLoading = $state(false);

  toggle() {
    this.visible = !this.visible;
  }

  async loadVolumes() {
    this.volumesLoading = true;
    try {
      this.volumes = await listVolumes();
    } catch (err: unknown) {
      console.error('Failed to load volumes:', err);
    } finally {
      this.volumesLoading = false;
    }
  }

  loadFavorites(homePath: string) {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      try {
        this.favorites = JSON.parse(stored);
        return;
      } catch {
        // Fall through to defaults
      }
    }
    // Seed defaults
    const home = homePath.replace(/\/+$/, '');
    this.favorites = [
      { name: 'Home', path: home },
      { name: 'Desktop', path: `${home}/Desktop` },
      { name: 'Documents', path: `${home}/Documents` },
      { name: 'Downloads', path: `${home}/Downloads` },
    ];
    this.persistFavorites();
  }

  addFavorite(name: string, path: string) {
    if (this.favorites.some((f) => f.path === path)) return;
    this.favorites = [...this.favorites, { name, path }];
    this.persistFavorites();
  }

  removeFavorite(path: string) {
    this.favorites = this.favorites.filter((f) => f.path !== path);
    this.persistFavorites();
  }

  persistFavorites() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(this.favorites));
  }
}

export const sidebarState = new SidebarState();
