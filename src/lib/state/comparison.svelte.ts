import { syncDiff, cancelSync } from '$lib/services/tauri';
import type { SyncEvent, SyncEntry, PanelBackend } from '$lib/types';
import { statusState } from '$lib/state/status.svelte';
import { appState } from '$lib/state/app.svelte';

export type ComparisonStatus = 'new' | 'modified' | 'deleted' | 'same';
export type ComparisonFilter = 'all' | 'new' | 'modified' | 'deleted';

class ComparisonState {
  active = $state(false);
  scanning = $state(false);
  filter = $state<ComparisonFilter>('all');
  leftStatuses: Map<string, ComparisonStatus> = $state(new Map());
  rightStatuses: Map<string, ComparisonStatus> = $state(new Map());
  counts = $state({ new: 0, modified: 0, deleted: 0 });
  private syncId = '';
  private flushTimer: ReturnType<typeof setTimeout> | null = null;

  /** Debounce map reassignment to batch multiple entries into one reactive update. */
  private scheduleFlush() {
    if (this.flushTimer) return;
    this.flushTimer = setTimeout(() => this.flushNow(), 50);
  }

  private flushNow() {
    if (this.flushTimer) { clearTimeout(this.flushTimer); this.flushTimer = null; }
    this.leftStatuses = new Map(this.leftStatuses);
    this.rightStatuses = new Map(this.rightStatuses);
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
    this.leftStatuses = new Map();
    this.rightStatuses = new Map();
    this.counts = { new: 0, modified: 0, deleted: 0 };

    this.syncId = 'cmp-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);

    try {
      await syncDiff(
        this.syncId,
        leftBackend,
        leftPath,
        leftS3Id,
        rightBackend,
        rightPath,
        rightS3Id,
        ['.DS_Store', 'Thumbs.db'],
        'size-and-date',
        (event: SyncEvent) => {
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
            const done = event as SyncEvent & { type: 'Done' };
            this.counts = {
              new: (done as any).new_count ?? 0,
              modified: (done as any).modified ?? 0,
              deleted: (done as any).deleted ?? 0,
            };
            this.scanning = false;
          }
        },
      );
    } catch (err: unknown) {
      const msg = String(err);
      if (!msg.includes('cancelled')) {
        appState.showAlert('Comparison failed: ' + msg);
      }
      this.scanning = false;
    }
  }

  stopComparison() {
    if (this.syncId) {
      cancelSync(this.syncId).catch(() => {});
      this.syncId = '';
    }
    this.active = false;
    this.scanning = false;
    this.leftStatuses = new Map();
    this.rightStatuses = new Map();
    this.counts = { new: 0, modified: 0, deleted: 0 };
    this.filter = 'all';
  }

  setFilter(f: ComparisonFilter) {
    this.filter = f;
  }
}

export const comparisonState = new ComparisonState();
