import type { ProgressEvent } from '$lib/types';
import { cancelFileOperation } from '$lib/services/tauri';
import { formatSize } from '$lib/utils/format';

type TransferStatus = 'running' | 'completed' | 'failed' | 'cancelled';
type TransferType = 'copy' | 'move' | 'extract';

interface Transfer {
  id: string;
  type: TransferType;
  status: TransferStatus;
  sources: string[];
  destination: string;
  progress: ProgressEvent | null;
  error?: string;
  startedAt: number;
  completedAt?: number;
}

class TransfersState {
  transfers = $state<Transfer[]>([]);
  panelVisible = $state(false);

  get active(): Transfer[] {
    return this.transfers.filter((t) => t.status === 'running');
  }

  get hasActive(): boolean {
    return this.active.length > 0;
  }

  get aggregatePercent(): number {
    const running = this.active;
    if (running.length === 0) return 0;
    let totalBytes = 0;
    let doneBytes = 0;
    for (const t of running) {
      if (t.progress) {
        totalBytes += t.progress.bytes_total;
        doneBytes += t.progress.bytes_done;
      }
    }
    if (totalBytes === 0) return 0;
    return Math.round((doneBytes / totalBytes) * 100);
  }

  get aggregateSummary(): string {
    const count = this.active.length;
    if (count === 0) return '';
    const pct = this.aggregatePercent;
    let totalBytes = 0;
    let doneBytes = 0;
    for (const t of this.active) {
      if (t.progress) {
        totalBytes += t.progress.bytes_total;
        doneBytes += t.progress.bytes_done;
      }
    }
    const suffix = totalBytes > 0 ? ` ${formatSize(doneBytes)}/${formatSize(totalBytes)}` : '';
    return count === 1
      ? `1 transfer — ${pct}%${suffix}`
      : `${count} transfers — ${pct}%${suffix}`;
  }

  add(id: string, type: TransferType, sources: string[], destination: string) {
    this.transfers.push({
      id,
      type,
      status: 'running',
      sources,
      destination,
      progress: null,
      startedAt: Date.now(),
    });
    this.panelVisible = true;
  }

  updateProgress(id: string, event: ProgressEvent) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) t.progress = event;
  }

  complete(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'completed';
      t.completedAt = Date.now();
    }
  }

  fail(id: string, error: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'failed';
      t.error = error;
      t.completedAt = Date.now();
    }
  }

  markCancelled(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'cancelled';
      t.completedAt = Date.now();
    }
  }

  async cancel(id: string) {
    try {
      await cancelFileOperation(id);
    } catch {
      // Already completed or unknown op
    }
  }

  dismiss(id: string) {
    this.transfers = this.transfers.filter((t) => t.id !== id);
  }

  dismissCompleted() {
    this.transfers = this.transfers.filter((t) => t.status === 'running');
  }

  toggle() {
    this.panelVisible = !this.panelVisible;
  }
}

export const transfersState = new TransfersState();
