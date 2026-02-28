import type { SftpBookmark } from '$lib/types';
import { appState } from '$lib/state/app.svelte';

class SftpBookmarksState {
  bookmarks = $state<SftpBookmark[]>([]);

  load(bookmarks?: SftpBookmark[]) {
    if (bookmarks) {
      this.bookmarks = bookmarks;
    }
  }

  add(bookmark: SftpBookmark) {
    this.bookmarks = [...this.bookmarks, bookmark];
    this.persist();
  }

  remove(id: string) {
    this.bookmarks = this.bookmarks.filter((b) => b.id !== id);
    this.persist();
  }

  rename(id: string, name: string) {
    const idx = this.bookmarks.findIndex((b) => b.id === id);
    if (idx >= 0) {
      this.bookmarks[idx] = { ...this.bookmarks[idx], name };
      this.bookmarks = [...this.bookmarks];
    }
    this.persist();
  }

  private persist() {
    appState.persistConfig();
  }
}

export const sftpBookmarksState = new SftpBookmarksState();
