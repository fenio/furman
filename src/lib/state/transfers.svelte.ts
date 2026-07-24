import type { ProgressEvent, TransferCheckpoint } from '$lib/types';
import { cancelFileOperation, pauseFileOperation, copyFiles, moveFiles, deleteFiles, extractArchive, createTempDir, cleanupTempPath } from '$lib/services/tauri';
import { s3Download, s3Upload, s3CopyObjects, s3DeleteObjects, s3UploadEncrypted, type EncryptionConfig } from '$lib/services/s3';
import { sftpDownload, sftpUpload, sftpDelete } from '$lib/services/sftp';
import { formatSize } from '$lib/utils/format';

export type TransferStatus = 'queued' | 'running' | 'paused' | 'completed' | 'failed' | 'cancelled';
type TransferType = 'copy' | 'move' | 'extract' | 'delete';
type TransferPhase = 'source-ready' | 'source-to-staging' | 'staging-ready' | 'staging-to-destination' | 'delete-source-ready' | 'delete-source';

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
  sftpSrcConnectionId?: string;
  sftpDestConnectionId?: string;
  sftpDestPath?: string;
  archivePath?: string;
  archiveInternalPaths?: string[];
  encryptionPassword?: string;
  encryptionConfig?: EncryptionConfig;
  checkpoint?: TransferCheckpoint | null;
  phase?: TransferPhase;
  pauseRequested?: boolean;
  cancelRequested?: boolean;
  stagingPath?: string;
  /** Name to focus in the source panel after a move completes. */
  srcFocusName?: string;
  speedBytesPerSec: number;
  /** @internal */ _lastProgressAt: number;
  /** @internal */ _lastBytesDone: number;
}

class TransfersState {
  transfers = $state<Transfer[]>([]);
  panelVisible = $state(false);
  dialogVisible = $state(false);
  maxConcurrent = $state(2);
  bandwidthLimit = $state(0);

  showDialog() { this.dialogVisible = true; }
  hideDialog() { this.dialogVisible = false; }

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
    this.dialogVisible = true;
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
    this.dialogVisible = true;
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
    if (!this.hasActive && this.queued.length === 0) {
      this.dialogVisible = false;
    }
    window.dispatchEvent(new CustomEvent('transfer-done', { detail: { type: t?.type, destination: t?.destination, srcFocusName: t?.srcFocusName, count: t?.sources.length } }));
  }

  fail(id: string, error: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'failed';
      t.error = error;
      t.completedAt = Date.now();
    }
    this.processQueue();
    if (!this.hasActive && this.queued.length === 0) {
      this.dialogVisible = false;
    }
  }

  markCancelled(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'cancelled';
      t.completedAt = Date.now();
      t.checkpoint = null;
      t.phase = undefined;
    }
    this.processQueue();
    if (!this.hasActive && this.queued.length === 0) {
      this.dialogVisible = false;
    }
    window.dispatchEvent(new CustomEvent('transfer-done'));
  }

  markPaused(id: string, checkpoint?: TransferCheckpoint | null) {
    const t = this.transfers.find((t) => t.id === id);
    if (t) {
      t.status = 'paused';
      t.checkpoint = checkpoint ?? null;
    }
    this.processQueue();
  }

  async cancel(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t) return;
    if (t.phase === 'delete-source') return;

    if (t.status === 'queued' || t.status === 'paused') {
      this.markCancelled(id);
      await this.releaseStaging(t);
      return;
    }

    t.cancelRequested = true;
    try {
      await cancelFileOperation(id);
    } catch {
      // Already completed or unknown op
    }
  }

  async pause(id: string) {
    const t = this.transfers.find((t) => t.id === id);
    if (!t || t.status !== 'running') return;

    t.pauseRequested = true;
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
    t.pauseRequested = false;
    t.cancelRequested = false;
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

  canPause(t: Transfer): boolean {
    return (t.type === 'copy' || t.type === 'move') && !t.id.startsWith('sync-') && t.phase !== 'delete-source';
  }

  canCancel(t: Transfer): boolean {
    return t.phase !== 'delete-source';
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
      } else if (t.type === 'delete') {
        if (t.srcBackend === 's3') {
          await s3DeleteObjects(t.s3SrcConnectionId!, t.id, t.sources, onProgress);
        } else if (t.srcBackend === 'sftp') {
          await sftpDelete(t.sftpSrcConnectionId!, t.sources);
        }
      }

      // Check if paused (backend returned checkpoint)
      if (result !== null && result !== undefined) {
        this.throwIfCancelled(t);
        this.markPaused(t.id, result);
        return;
      }

      this.throwIfCancelled(t);
      await this.releaseStaging(t);
      t.checkpoint = null;
      t.phase = undefined;
      this.complete(t.id);
    } catch (err: unknown) {
      await this.releaseStaging(t);
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
  ): Promise<TransferCheckpoint | null> {
    const { srcBackend, destBackend } = t;

    if (t.type === 'move' && t.phase === 'delete-source-ready') {
      t.phase = 'delete-source';
      await this.deleteMoveSources(t);
      return null;
    }

    if (t.type === 'copy') {
      if (srcBackend === 'local' && destBackend === 'local') {
        return await copyFiles(t.id, t.sources, t.destination, onProgress, t.checkpoint);
      }
      if (srcBackend === 's3' && destBackend === 'local') {
        return await s3Download(t.s3SrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.encryptionPassword, t.checkpoint);
      }
      if (srcBackend === 'local' && destBackend === 's3') {
        if (t.encryptionPassword) {
          return await s3UploadEncrypted(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, t.encryptionPassword, onProgress, t.encryptionConfig, t.checkpoint);
        }
        return await s3Upload(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, onProgress, t.checkpoint);
      }
      if (srcBackend === 's3' && destBackend === 's3') {
        return await s3CopyObjects(
          t.s3SrcConnectionId!, t.id, t.sources,
          t.s3DestConnectionId!, t.s3DestPrefix!, onProgress, t.checkpoint,
        );
      }
      // SFTP transfers
      if (srcBackend === 'sftp' && destBackend === 'local') {
        return await sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.checkpoint);
      }
      if (srcBackend === 'local' && destBackend === 'sftp') {
        return await sftpUpload(t.sftpDestConnectionId!, t.id, t.sources, t.sftpDestPath!, onProgress, t.checkpoint);
      }
      if (srcBackend === 'sftp' && destBackend === 'sftp') {
        return await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, tempDir, onProgress, checkpoint),
          (downloaded, checkpoint) => sftpUpload(t.sftpDestConnectionId!, t.id, downloaded, t.sftpDestPath!, onProgress, checkpoint),
        );
      }
      // Cross-protocol: S3 ↔ SFTP (via temp dir)
      if (srcBackend === 's3' && destBackend === 'sftp') {
        return await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => s3Download(t.s3SrcConnectionId!, t.id, t.sources, tempDir, onProgress, t.encryptionPassword, checkpoint),
          (downloaded, checkpoint) => sftpUpload(t.sftpDestConnectionId!, t.id, downloaded, t.sftpDestPath!, onProgress, checkpoint),
        );
      }
      if (srcBackend === 'sftp' && destBackend === 's3') {
        return await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, tempDir, onProgress, checkpoint),
          (downloaded, checkpoint) => s3Upload(t.s3DestConnectionId!, t.id, downloaded, t.s3DestPrefix!, onProgress, checkpoint),
        );
      }
    }

    if (t.type === 'move') {
      if (srcBackend === 'local' && destBackend === 'local') {
        return await moveFiles(t.id, t.sources, t.destination, onProgress, t.checkpoint);
      }
      // S3 move = copy/download + delete source
      if (srcBackend === 's3' && destBackend === 'local') {
        const result = await s3Download(t.s3SrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.encryptionPassword, t.checkpoint);
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      if (srcBackend === 'local' && destBackend === 's3') {
        let result;
        if (t.encryptionPassword) {
          result = await s3UploadEncrypted(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, t.encryptionPassword, onProgress, t.encryptionConfig, t.checkpoint);
        } else {
          result = await s3Upload(t.s3DestConnectionId!, t.id, t.sources, t.s3DestPrefix!, onProgress, t.checkpoint);
        }
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      if (srcBackend === 's3' && destBackend === 's3') {
        const result = await s3CopyObjects(
          t.s3SrcConnectionId!, t.id, t.sources,
          t.s3DestConnectionId!, t.s3DestPrefix!, onProgress, t.checkpoint,
        );
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      // SFTP move = download/upload + delete source
      if (srcBackend === 'sftp' && destBackend === 'local') {
        const result = await sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, t.destination, onProgress, t.checkpoint);
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      if (srcBackend === 'local' && destBackend === 'sftp') {
        const result = await sftpUpload(t.sftpDestConnectionId!, t.id, t.sources, t.sftpDestPath!, onProgress, t.checkpoint);
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      if (srcBackend === 'sftp' && destBackend === 'sftp') {
        const result = await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, tempDir, onProgress, checkpoint),
          (downloaded, checkpoint) => sftpUpload(t.sftpDestConnectionId!, t.id, downloaded, t.sftpDestPath!, onProgress, checkpoint),
        );
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      // Cross-protocol: S3 ↔ SFTP (via temp dir)
      if (srcBackend === 's3' && destBackend === 'sftp') {
        const result = await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => s3Download(t.s3SrcConnectionId!, t.id, t.sources, tempDir, onProgress, t.encryptionPassword, checkpoint),
          (downloaded, checkpoint) => sftpUpload(t.sftpDestConnectionId!, t.id, downloaded, t.sftpDestPath!, onProgress, checkpoint),
        );
        if (result !== null) return result;
        return await this.finishMove(t);
      }
      if (srcBackend === 'sftp' && destBackend === 's3') {
        const result = await this.dispatchStaged(
          t,
          (tempDir, checkpoint) => sftpDownload(t.sftpSrcConnectionId!, t.id, t.sources, tempDir, onProgress, checkpoint),
          (downloaded, checkpoint) => s3Upload(t.s3DestConnectionId!, t.id, downloaded, t.s3DestPrefix!, onProgress, checkpoint),
        );
        if (result !== null) return result;
        return await this.finishMove(t);
      }
    }

    throw new Error(`Unsupported transfer: ${t.type} ${srcBackend} -> ${destBackend}`);
  }

  private async dispatchStaged(
    t: Transfer,
    download: (tempDir: string, checkpoint: TransferCheckpoint | null) => Promise<TransferCheckpoint | null>,
    upload: (sources: string[], checkpoint: TransferCheckpoint | null) => Promise<TransferCheckpoint | null>,
  ): Promise<TransferCheckpoint | null> {
    if (!t.stagingPath) t.stagingPath = await createTempDir('transfer');
    const tempDir = t.stagingPath;
    this.throwIfCancelled(t);
    if (t.pauseRequested && t.phase !== 'source-to-staging') {
      t.phase = 'source-ready';
      return this.boundaryCheckpoint(t);
    }

    if (t.phase !== 'staging-ready' && t.phase !== 'staging-to-destination') {
      const checkpoint = t.phase === 'source-to-staging' ? t.checkpoint ?? null : null;
      const result = await download(tempDir, checkpoint);
      if (result !== null) {
        t.phase = 'source-to-staging';
        return result;
      }
      this.throwIfCancelled(t);
      t.checkpoint = null;
      t.phase = 'staging-ready';
      if (t.pauseRequested) return this.boundaryCheckpoint(t);
    }

    const downloaded = t.sources.map((source) => {
      const name = source.replace(/\/+$/, '').split('/').pop()!;
      return `${tempDir}/${name}`;
    });
    const checkpoint = t.phase === 'staging-to-destination' ? t.checkpoint ?? null : null;
    t.phase = 'staging-to-destination';
    const result = await upload(downloaded, checkpoint);
    if (result !== null) return result;
    this.throwIfCancelled(t);
    t.checkpoint = null;
    await this.releaseStaging(t);
    return null;
  }

  private async finishMove(t: Transfer): Promise<TransferCheckpoint | null> {
    this.throwIfCancelled(t);
    if (t.pauseRequested) {
      t.phase = 'delete-source-ready';
      return this.boundaryCheckpoint(t);
    }
    t.phase = 'delete-source';
    await this.deleteMoveSources(t);
    return null;
  }

  private async deleteMoveSources(t: Transfer) {
    this.throwIfCancelled(t);
    if (t.srcBackend === 's3') {
      await s3DeleteObjects(t.s3SrcConnectionId!, t.id + '-del', t.sources);
    } else if (t.srcBackend === 'sftp') {
      await sftpDelete(t.sftpSrcConnectionId!, t.sources);
    } else if (t.srcBackend === 'local') {
      await deleteFiles(t.sources, true);
    } else {
      throw new Error(`Unsupported move source: ${t.srcBackend}`);
    }
  }

  private throwIfCancelled(t: Transfer) {
    if (t.cancelRequested) throw new Error('cancelled');
  }

  private boundaryCheckpoint(t: Transfer): TransferCheckpoint {
    const progress = t.progress;
    return {
      files_completed: [],
      bytes_done: progress?.bytes_done ?? 0,
      bytes_total: progress?.bytes_total ?? 0,
      files_done: progress?.files_done ?? 0,
      files_total: progress?.files_total ?? 0,
    };
  }

  private async releaseStaging(t: Transfer) {
    const path = t.stagingPath;
    if (!path) return;
    try {
      await cleanupTempPath(path);
    } catch {
      // Cleanup is best-effort and must not change a completed transfer result.
    } finally {
      t.stagingPath = undefined;
    }
  }
}

export const transfersState = new TransfersState();
