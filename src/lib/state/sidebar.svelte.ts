import type { VolumeInfo } from '$lib/types';
import { listVolumes } from '$lib/services/tauri';
import { error } from '$lib/services/log';
import { appState } from '$lib/state/app.svelte';

export interface FavoriteItem {
  name: string;
  path: string;
}

class SidebarState {
  visible = $state(false);
  focused = $state(false);
  focusIndex = $state(0);
  favorites = $state<FavoriteItem[]>([]);
  volumes = $state<VolumeInfo[]>([]);
  volumesLoading = $state(false);

  toggle() {
    if (this.visible) {
      this.visible = false;
      this.focused = false;
    } else {
      this.visible = true;
    }
  }

  focus() {
    if (!this.visible) {
      this.visible = true;
    }
    this.focused = true;
    this.focusIndex = 0;
  }

  blur() {
    this.focused = false;
  }

  async loadVolumes() {
    this.volumesLoading = true;
    try {
      this.volumes = await listVolumes();
    } catch (err: unknown) {
      error(String(err));
    } finally {
      this.volumesLoading = false;
    }
  }

  loadFavorites(homePath: string, favorites?: FavoriteItem[]) {
    if (favorites && favorites.length > 0) {
      this.favorites = favorites;
      return;
    }
    // Seed defaults
    const home = homePath.replace(/\/+$/, '');
    this.favorites = [
      { name: 'Home', path: home },
      { name: 'Desktop', path: `${home}/Desktop` },
      { name: 'Documents', path: `${home}/Documents` },
      { name: 'Downloads', path: `${home}/Downloads` },
    ];
  }

  addFavorite(name: string, path: string) {
    if (this.hasFavorite(path)) return;
    this.favorites = [...this.favorites, { name, path }];
    this.persistFavorites();
  }

  hasFavorite(path: string): boolean {
    const normalized = path.replace(/\/+$/, '') || '/';
    return this.favorites.some((favorite) => (favorite.path.replace(/\/+$/, '') || '/') === normalized);
  }

  removeFavorite(path: string) {
    this.favorites = this.favorites.filter((f) => f.path !== path);
    this.persistFavorites();
  }

  renameFavorite(path: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    const normalized = path.replace(/\/+$/, '') || '/';
    this.favorites = this.favorites.map((favorite) =>
      (favorite.path.replace(/\/+$/, '') || '/') === normalized
        ? { ...favorite, name: trimmed }
        : favorite
    );
    this.persistFavorites();
  }

  persistFavorites() {
    appState.persistConfig();
  }
}

export const sidebarState = new SidebarState();
