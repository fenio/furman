import { panels, s3PathToPrefix } from '$lib/state/panels.svelte';
import { appState, canSyncBackends } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { transfersState } from '$lib/state/transfers.svelte';
import { copyFiles, deleteFiles } from '$lib/services/tauri';
import { s3Download, s3Upload, s3CopyObjects, s3DeleteObjects } from '$lib/services/s3';
import { error } from '$lib/services/log';
import type { ProgressEvent, SyncEntry, TransferCheckpoint } from '$lib/types';

export function executeSyncTransfer(detail: {
  entries: SyncEntry[];
  sourceBackend: string;
  sourcePath: string;
  sourceS3Id: string;
  destBackend: string;
  destPath: string;
  destS3Id: string;
}) {
  const { entries, sourceBackend, sourcePath, sourceS3Id, destBackend, destPath, destS3Id } = detail;
  if (!canSyncBackends(sourceBackend, destBackend)) {
    statusState.setMessage('Sync not supported for SFTP/archive panels');
    return;
  }

  const toCopy = entries.filter((e) => e.status === 'new' || e.status === 'modified');
  const toDelete = entries.filter((e) => e.status === 'deleted');

  if (toCopy.length > 0) {
    const opId = 'sync-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
    const copySourcePaths = toCopy.map((e) => {
      const base = sourcePath.endsWith('/') ? sourcePath : sourcePath + '/';
      return base + e.relative_path;
    });

    transfersState.add(opId, 'copy', copySourcePaths, destPath);

    (async () => {
      let copySucceeded = false;
      try {
        const onProgress = (e: ProgressEvent) => {
          transfersState.updateProgress(opId, e);
        };
        let result: TransferCheckpoint | null;

        if (sourceBackend === 'local' && destBackend === 'local') {
          result = await copyFiles(opId, copySourcePaths, destPath, onProgress);
        } else if (sourceBackend === 's3' && destBackend === 'local') {
          result = await s3Download(sourceS3Id, opId, copySourcePaths, destPath, onProgress);
        } else if (sourceBackend === 'local' && destBackend === 's3') {
          const prefix = s3PathToPrefix(destPath, '');
          result = await s3Upload(destS3Id, opId, copySourcePaths, prefix, onProgress);
        } else if (sourceBackend === 's3' && destBackend === 's3') {
          const destPrefix = s3PathToPrefix(destPath, '');
          result = await s3CopyObjects(sourceS3Id, opId, copySourcePaths, destS3Id, destPrefix, onProgress);
        } else {
          throw new Error(`Unsupported sync: ${sourceBackend} -> ${destBackend}`);
        }

        if (result !== null) {
          transfersState.markPaused(opId, result);
          return;
        }

        copySucceeded = true;
        transfersState.complete(opId);
        statusState.setMessage(`Synced ${toCopy.length} file(s)`);
      } catch (err: unknown) {
        const msg = String(err);
        if (msg.includes('cancelled')) {
          transfersState.markCancelled(opId);
          statusState.setMessage('Sync cancelled');
        } else {
          error(msg);
          transfersState.fail(opId, msg);
          appState.showAlert('Sync failed: ' + msg);
        }
      } finally {
        if (copySucceeded && toDelete.length > 0) {
          await executeSyncDeletes(toDelete, destBackend, destPath, destS3Id);
        }
        const reloads: Promise<void>[] = [];
        if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
        if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
        await Promise.all(reloads);
      }
    })();
  } else if (toDelete.length > 0) {
    (async () => {
      await executeSyncDeletes(toDelete, destBackend, destPath, destS3Id);
      const reloads: Promise<void>[] = [];
      if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
      if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
      await Promise.all(reloads);
    })();
  }
}

export async function executeSyncDeletes(
  toDelete: SyncEntry[],
  destBackend: string,
  destPath: string,
  destS3Id: string,
) {
  const deletePaths = toDelete.map((e) => {
    const base = destPath.endsWith('/') ? destPath : destPath + '/';
    return base + e.relative_path;
  });

  try {
    if (destBackend === 's3') {
      await s3DeleteObjects(destS3Id, 'sync-del-' + Date.now(), deletePaths);
    } else if (destBackend === 'local') {
      await deleteFiles(deletePaths, true);
    } else {
      throw new Error(`Unsupported sync destination: ${destBackend}`);
    }
    statusState.setMessage(`Deleted ${toDelete.length} file(s) from destination`);
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Sync delete failed: ' + String(err));
  }
}
