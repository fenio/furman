import type { S3Bookmark } from '$lib/types';
import { appState } from '$lib/state/app.svelte';

class S3BookmarksState {
  bookmarks = $state<S3Bookmark[]>([]);

  load(bookmarks?: S3Bookmark[]) {
    if (bookmarks) {
      this.bookmarks = bookmarks;
    }
  }

  add(bookmark: S3Bookmark) {
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

export const s3BookmarksState = new S3BookmarksState();
