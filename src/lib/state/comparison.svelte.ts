import { syncDiff, cancelSync } from '$lib/services/tauri';
import type { SyncEvent, SyncEntry, PanelBackend, FileEntry } from '$lib/types';
import { statusState } from '$lib/state/status.svelte';
import { appState } from '$lib/state/app.svelte';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';

export type ComparisonStatus = 'new' | 'modified' | 'deleted' | 'same';
export type ComparisonFilter = 'all' | 'new' | 'modified' | 'deleted';
export type ComparisonSide = 'left' | 'right';

class ComparisonState {
  active = $state(false);
  scanning = $state(false);
  filter = $state<ComparisonFilter>('all');
  filterSide = $state<ComparisonSide>('left');
  leftStatuses: Map<string, ComparisonStatus> = $state(new SvelteMap());
  rightStatuses: Map<string, ComparisonStatus> = $state(new SvelteMap());
  counts = $state({ new: 0, modified: 0, deleted: 0 });
  private leftRoot = '';
  private rightRoot = '';
  private leftDirectoryStatuses = new SvelteMap<string, Set<ComparisonStatus>>();
  private rightDirectoryStatuses = new SvelteMap<string, Set<ComparisonStatus>>();
  private syncId = '';
  private flushTimer: ReturnType<typeof setTimeout> | null = null;

  /** Debounce map reassignment to batch multiple entries into one reactive update. */
  private scheduleFlush() {
    if (this.flushTimer) return;
    this.flushTimer = setTimeout(() => this.flushNow(), 50);
  }

  private flushNow() {
    if (this.flushTimer) { clearTimeout(this.flushTimer); this.flushTimer = null; }
    this.leftStatuses = new SvelteMap(this.leftStatuses);
    this.rightStatuses = new SvelteMap(this.rightStatuses);
    this.leftDirectoryStatuses = this.buildDirectoryStatuses(this.leftStatuses);
    this.rightDirectoryStatuses = this.buildDirectoryStatuses(this.rightStatuses);
  }

  async startComparison(
    leftPath: string,
    leftBackend: PanelBackend,
    leftS3Id: string,
    rightPath: string,
    rightBackend: PanelBackend,
    rightS3Id: string,
  ) {
    // Check supported backends
    if (leftBackend === 'sftp' || leftBackend === 'archive' ||
        rightBackend === 'sftp' || rightBackend === 'archive') {
      statusState.setMessage('Comparison not supported for SFTP/archive panels');
      return;
    }

    this.stopComparison();
    this.active = true;
    this.scanning = true;
    this.filter = 'all';
    this.filterSide = 'left';
    this.leftStatuses = new SvelteMap();
    this.rightStatuses = new SvelteMap();
    this.counts = { new: 0, modified: 0, deleted: 0 };
    this.leftRoot = this.normalizePath(leftPath);
    this.rightRoot = this.normalizePath(rightPath);

    const syncId = 'cmp-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
    this.syncId = syncId;

    try {
      await syncDiff(
        syncId,
        leftBackend,
        leftPath,
        leftS3Id,
        rightBackend,
        rightPath,
        rightS3Id,
        ['.DS_Store', 'Thumbs.db'],
        'size-and-date',
        (event: SyncEvent) => {
          if (this.syncId !== syncId) return;
          if (event.type === 'Entry') {
            const entry = event as SyncEvent & { type: 'Entry' } & SyncEntry;
            const relPath = entry.relative_path;
            const status = entry.status as ComparisonStatus;

            if (status === 'same') {
              this.leftStatuses.set(relPath, 'same');
              this.rightStatuses.set(relPath, 'same');
            } else if (status === 'new') {
              // "new" in syncDiff = only on source (left)
              this.leftStatuses.set(relPath, 'new');
              this.rightStatuses.set(relPath, 'deleted');
            } else if (status === 'deleted') {
              // "deleted" in syncDiff = only on dest (right)
              this.leftStatuses.set(relPath, 'deleted');
              this.rightStatuses.set(relPath, 'new');
            } else if (status === 'modified') {
              this.leftStatuses.set(relPath, 'modified');
              this.rightStatuses.set(relPath, 'modified');
            }
            // Batch reactivity: debounce map reassignment instead of per-entry
            this.scheduleFlush();
          } else if (event.type === 'Done') {
            this.flushNow();
            this.counts = {
              new: event.new_count ?? 0,
              modified: event.modified ?? 0,
              deleted: event.deleted ?? 0,
            };
            this.scanning = false;
            this.syncId = '';
          }
        },
      );
    } catch (err: unknown) {
      if (this.syncId !== syncId) return;
      const msg = String(err);
      if (!msg.includes('cancelled')) {
        appState.showAlert('Comparison failed: ' + msg);
      }
      this.scanning = false;
      this.syncId = '';
    }
  }

  stopComparison() {
    if (this.syncId) {
      cancelSync(this.syncId).catch(() => {});
      this.syncId = '';
    }
    this.active = false;
    this.scanning = false;
    this.leftStatuses = new SvelteMap();
    this.rightStatuses = new SvelteMap();
    this.leftDirectoryStatuses = new SvelteMap();
    this.rightDirectoryStatuses = new SvelteMap();
    this.leftRoot = '';
    this.rightRoot = '';
    this.counts = { new: 0, modified: 0, deleted: 0 };
    this.filter = 'all';
  }

  setFilter(f: ComparisonFilter, side: ComparisonSide) {
    this.filter = f;
    this.filterSide = side;
  }

  filterFor(side: ComparisonSide): ComparisonFilter {
    if (this.filter === 'all' || side === this.filterSide || this.filter === 'modified') {
      return this.filter;
    }
    return this.filter === 'new' ? 'deleted' : 'new';
  }

  containsPanelPath(side: ComparisonSide, panelPath: string): boolean {
    return this.entryRelativePath(side, panelPath, '') !== null;
  }

  statusForEntry(side: ComparisonSide, panelPath: string, entry: FileEntry): ComparisonStatus | undefined {
    if (!this.active || entry.name === '..') return undefined;
    const relativePath = this.entryRelativePath(side, panelPath, entry.name);
    if (relativePath === null) return undefined;
    const statuses = side === 'left' ? this.leftStatuses : this.rightStatuses;
    const direct = statuses.get(relativePath);
    if (direct) return direct;
    if (entry.is_dir) {
      const directoryStatuses = side === 'left'
        ? this.leftDirectoryStatuses
        : this.rightDirectoryStatuses;
      if (directoryStatuses.has(relativePath)) return 'modified';
    }
    return undefined;
  }

  matchesFilter(side: ComparisonSide, panelPath: string, entry: FileEntry): boolean {
    const filter = this.filterFor(side);
    if (!this.active || filter === 'all' || entry.name === '..') return true;
    const relativePath = this.entryRelativePath(side, panelPath, entry.name);
    if (relativePath === null) return false;
    const statuses = side === 'left' ? this.leftStatuses : this.rightStatuses;
    if (statuses.get(relativePath) === filter) return true;
    if (!entry.is_dir) return false;
    const directoryStatuses = side === 'left'
      ? this.leftDirectoryStatuses
      : this.rightDirectoryStatuses;
    return directoryStatuses.get(relativePath)?.has(filter) ?? false;
  }

  countsFor(side: ComparisonSide) {
    if (side === 'left') return this.counts;
    return {
      new: this.counts.deleted,
      modified: this.counts.modified,
      deleted: this.counts.new,
    };
  }

  private buildDirectoryStatuses(statuses: Map<string, ComparisonStatus>) {
    const result = new SvelteMap<string, Set<ComparisonStatus>>();
    for (const [path, status] of statuses) {
      if (status === 'same') continue;
      const parts = path.split('/').filter(Boolean);
      for (let i = 1; i < parts.length; i++) {
        const directory = parts.slice(0, i).join('/');
        const values = result.get(directory) ?? new SvelteSet<ComparisonStatus>();
        values.add(status);
        result.set(directory, values);
      }
    }
    return result;
  }

  private entryRelativePath(side: ComparisonSide, panelPath: string, entryName: string): string | null {
    const root = side === 'left' ? this.leftRoot : this.rightRoot;
    const current = this.normalizePath(panelPath);
    if (!root) return null;
    let prefix: string;
    if (root === '/') {
      if (!current.startsWith('/')) return null;
      prefix = current === '/' ? '' : current.slice(1);
    } else {
      if (current !== root && !current.startsWith(root + '/')) return null;
      prefix = current === root ? '' : current.slice(root.length + 1);
    }
    return prefix ? `${prefix}/${entryName}` : entryName;
  }

  private normalizePath(path: string) {
    return path.length > 1 ? path.replace(/\/+$/, '') : path;
  }
}

export const comparisonState = new ComparisonState();
