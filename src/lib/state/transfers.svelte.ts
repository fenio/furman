import type { ProgressEvent, TransferCheckpoint } from '$lib/types';
import { cancelFileOperation, pauseFileOperation, copyFiles, moveFiles, extractArchive } from '$lib/services/tauri';
import { s3Download, s3Upload, s3CopyObjects, s3UploadEncrypted, type EncryptionConfig } from '$lib/services/s3';
import { formatSize } from '$lib/utils/format';

export type TransferStatus = 'queued' | 'running' | 'paused' | 'completed' | 'failed' | 'cancelled';
type TransferType = 'copy' | 'move' | 'extract';

export interface Transfer {
  id: string;
  type: TransferType;
  status: TransferStatus;
  sources: string[];
  destination: string;
  progress: ProgressEvent | null;
  error?: string;
  startedAt: number;
  completedAt?: number;
  priority: number;
  srcBackend: string;
  destBackend: string;
  s3SrcConnectionId?: string;
  s3DestConnectionId?: string;
  s3DestPrefix?: string;
  archivePath?: string;
  archiveInternalPaths?: string[];
  encryptionPassword?: string;
  encryptionConfig?: EncryptionConfig;
  checkpoint?: TransferCheckpoint | null;
  speedBytesPerSec: number;
  /** @internal */ _lastProgressAt: number;
  /** @internal */ _lastBytesDone: number;
}

class TransfersState {
  transfers = $state<Transfer[]>([]);
  panelVisible = $state(false);
  maxConcurrent = $state(2);
  bandwidthLimit = $state(0);

  get active(): Transfer[] {
    return this.transfers.filter((t) => t.status === 'running');
  }

  get queued(): Transfer[] {
    return this.transfers
      .filter((t) => t.status === 'queued')
      .sort((a, b) => a.priority - b.priority);
  }

  get paused(): Transfer[] {
    return this.transfers.filter((t) => t.status === 'paused');
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

  enqueue(transfer: Omit<Transfer, 'status' | 'progress' | 'startedAt' | 'priority' | 'speedBytesPerSec' | '_lastProgressAt' | '_lastBytesDone'>) {
    this.transfers.push({
      ...transfer,
      status: 'queued',
      progress: null,
      startedAt: Date.now(),
      priority: Date.now(),
      speedBytesPerSec: 0,
      _lastProgressAt: 0,
      _lastBytesDone: 0,
    });
    this.panelVisible = true;
    this.processQueue();
  }

  /** Start queued transfers up to maxConcurrent slots. */
  processQueue() {
    const runningCount = this.active.length;
    const available = this.maxConcurrent - runningCount;
    if (available <= 0) return;

    const queued = this.queued;
    const toStart = queued.slice(0, available);
    for (const t of toStart) {
      t.status = 'running';
      t.startedAt = Date.now();
      this.dispatchTransfer(t);
    }
  }

  /** Legacy add for compatibility — immediately runs (used by OS drag-drop & sync). */
  add(id: string, type: TransferType, sources: string[], destination: string) {
    this.transfers.push({
      id,
      type,
      status: 'running',
      sources,
      destination,
      progress: null,
      startedAt: Date.now(),
      priority: Date.now(),
      srcBackend: 'local',
      destBackend: 'local',
      speedBytesPerSec: 0,
      _lastProgressAt: 0,
      _lastBytesDone: 0,
    });
    this.panelVisible = true;
  }

  updateProgress(id: string, event: ProgressEvent) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t) return;

    const now = Date.now();
    if (t._lastProgressAt > 0) {
      const dt = (now - t._lastProgressAt) / 1000; // seconds
      if (dt > 0) {
        const bytesDelta = event.bytes_done - t._lastBytesDone;
        const instantSpeed = bytesDelta / dt;
        const alpha = 0.3;
        t.speedBytesPerSec = t.speedBytesPerSec > 0
          ? alpha * instantSpeed + (1 - alpha) * t.speedBytesPerSec
          : instantSpeed;
      }
    }
    t._lastProgressAt = now;
    t._lastBytesDone = event.bytes_done;
    t.progress = event;
  }

  complete(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'completed';
      t.completedAt = Date.now();
    }
    this.processQueue();
    window.dispatchEvent(new CustomEvent('transfer-done'));
  }

  fail(id: string, error: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'failed';
      t.error = error;
      t.completedAt = Date.now();
    }
    this.processQueue();
  }

  markCancelled(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'cancelled';
      t.completedAt = Date.now();
    }
    this.processQueue();
  }

  markPaused(id: string, checkpoint?: TransferCheckpoint | null) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'paused';
      if (checkpoint) t.checkpoint = checkpoint;
    }
    this.processQueue();
  }

  async cancel(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t) return;

    if (t.status === 'queued') {
      // Never started — just remove
      t.status = 'cancelled';
      t.completedAt = Date.now();
      return;
    }

    try {
      await cancelFileOperation(id);
    } catch {
      // Already completed or unknown op
    }
  }

  async pause(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t || t.status !== 'running') return;

    try {
      await pauseFileOperation(id);
      // The backend will return a checkpoint via the dispatch promise,
      // which calls markPaused()
    } catch {
      // Already completed or unknown op
    }
  }

  resume(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t || t.status !== 'paused') return;
    t.status = 'queued';
    t.priority = Date.now();
    this.processQueue();
  }

  moveUp(id: string) {
    const queued = this.queued;
    const idx = queued.findIndex((t) => t.id === id);
    if (idx <= 0) return;
    const temp = queued[idx].priority;
    queued[idx].priority = queued[idx - 1].priority;
    queued[idx - 1].priority = temp;
  }

  moveDown(id: string) {
    const queued = this.queued;
    const idx = queued.findIndex((t) => t.id === id);
    if (idx < 0 || idx >= queued.length - 1) return;
    const temp = queued[idx].priority;
    queued[idx].priority = queued[idx + 1].priority;
    queued[idx + 1].priority = temp;
  }

  dismiss(id: string) {
    this.transfers = this.transfers.filter((t) => t.id !== id);
  }

  dismissCompleted() {
    this.transfers = this.transfers.filter(
      (t) => t.status === 'running' || t.status === 'queued' || t.status === 'paused',
    );
  }

  toggle() {
    this.panelVisible = !this.panelVisible;
  }

  /** Dispatch a transfer to the appropriate backend. */
  private async dispatchTransfer(t: Transfer) {
    const onProgress = (e: ProgressEvent) => {
      this.updateProgress(t.id, e);
    };

    try {
      let result: TransferCheckpoint | null | undefined;

      if (t.type === 'extract' && t.archivePath && t.archiveInternalPaths) {
        await extractArchive(t.id, t.archivePath, t.archiveInternalPaths, t.destination, onProgress);
      } else if (t.type === 'copy' || t.type === 'move') {
        result = await this.dispatchCopyMove(t, onProgress);
      }

      // Check if paused (backend returned checkpoint)
      if (result) {
        this.markPaused(t.id, result);
        return;
      }

      this.complete(t.id);
    } catch (err: unknown) {
      const msg = String(err);
      if (msg.includes('cancelled')) {
        this.markCancelled(t.id);
      } else {
        this.fail(t.id, msg);
      }
    }
  }

  private async dispatchCopyMove(
    t: Transfer,
    onProgress: (e: ProgressEvent) => void,
  ): Promise<TransferCheckpoint | null | undefined> {
    const { srcBackend, destBackend } = t;

    if (t.type === 'copy') {
      if (srcBackend === 'local' && destBackend === 'local') {
        return await copyFiles(t.id, t.sources, t.destination, onProgress);
      }
      if (srcBackend === 's3' && destBackend === 'local') {
        return await s3Download(t.s3SrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.encryptionPassword);
      }
      if (srcBackend === 'local' && destBackend === 's3') {
        if (t.encryptionPassword) {
          return await s3UploadEncrypted(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, t.encryptionPassword, onProgress, t.encryptionConfig);
        }
        return await s3Upload(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, onProgress);
      }
      if (srcBackend === 's3' && destBackend === 's3') {
        return await s3CopyObjects(
          t.s3SrcConnectionId!, t.id, t.sources,
          t.s3DestConnectionId!, t.s3DestPrefix!, onProgress,
        );
      }
    }

    if (t.type === 'move') {
      if (srcBackend === 'local' && destBackend === 'local') {
        return await moveFiles(t.id, t.sources, t.destination, onProgress);
      }
      // S3 move = copy + delete (handled by caller in +layout.svelte)
      if (srcBackend === 's3' && destBackend === 'local') {
        return await s3Download(t.s3SrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.encryptionPassword);
      }
      if (srcBackend === 'local' && destBackend === 's3') {
        if (t.encryptionPassword) {
          return await s3UploadEncrypted(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, t.encryptionPassword, onProgress, t.encryptionConfig);
        }
        return await s3Upload(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, onProgress);
      }
      if (srcBackend === 's3' && destBackend === 's3') {
        return await s3CopyObjects(
          t.s3SrcConnectionId!, t.id, t.sources,
          t.s3DestConnectionId!, t.s3DestPrefix!, onProgress,
        );
      }
    }

    return null;
  }
}

export const transfersState = new TransfersState();
