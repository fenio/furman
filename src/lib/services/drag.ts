import { startDrag } from '@crabnebula/tauri-plugin-drag';
import { s3DownloadToTemp } from './s3';
import { sftpDownloadTemp } from './sftp';
import { cleanupTempPath } from './tauri';

export interface DragSource {
  side: 'left' | 'right';
  backend: string;
  paths: string[];
  s3ConnectionId?: string;
  sftpConnectionId?: string;
  isMove: boolean;
}

/** Shared drag state for coordinating between drag initiation and drop handling. */
export const dragState = {
  source: null as DragSource | null,
  /** Path of the directory row currently hovered during a drag (set by dragover tracking). */
  dragOverDir: null as string | null,
  /** DOM element currently highlighted as drag-over target. */
  _dragOverEl: null as Element | null,
};

/** Generate a simple drag icon as a base64 PNG data URI. */
function makeDragIcon(count: number): string {
  const canvas = document.createElement('canvas');
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext('2d')!;

  // File icon shape
  ctx.fillStyle = '#6b7280';
  ctx.beginPath();
  ctx.moveTo(12, 4);
  ctx.lineTo(40, 4);
  ctx.lineTo(52, 16);
  ctx.lineTo(52, 60);
  ctx.lineTo(12, 60);
  ctx.closePath();
  ctx.fill();

  // Folded corner
  ctx.fillStyle = '#9ca3af';
  ctx.beginPath();
  ctx.moveTo(40, 4);
  ctx.lineTo(52, 16);
  ctx.lineTo(40, 16);
  ctx.closePath();
  ctx.fill();

  // Badge with count
  if (count > 1) {
    ctx.fillStyle = '#3b82f6';
    ctx.beginPath();
    ctx.arc(50, 50, 13, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 14px sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(count > 99 ? '99+' : String(count), 50, 51);
  }

  return canvas.toDataURL('image/png');
}

/** Dispatch a custom event so +layout.svelte can handle internal panel-to-panel drops. */
function emitInternalDrop(x: number, y: number): void {
  window.dispatchEvent(new CustomEvent('native-drag-drop', {
    detail: { x, y },
  }));
}

export async function startLocalFileDrag(paths: string[], mode: 'copy' | 'move' = 'copy'): Promise<void> {
  await startDrag({ item: paths, icon: makeDragIcon(paths.length), mode }, (payload) => {
    if (payload.result === 'Dropped') {
      emitInternalDrop(Number(payload.cursorPos.x), Number(payload.cursorPos.y));
    }
  });
}

export async function startS3FileDrag(connectionId: string, keys: string[], mode: 'copy' | 'move' = 'copy'): Promise<void> {
  await startRemoteFileDrag(keys, (key) => s3DownloadToTemp(connectionId, key), mode);
}

export async function startSftpFileDrag(connectionId: string, paths: string[], mode: 'copy' | 'move' = 'copy'): Promise<void> {
  await startRemoteFileDrag(paths, (path) => sftpDownloadTemp(connectionId, path), mode);
}

async function startRemoteFileDrag(
  sources: string[],
  download: (source: string) => Promise<string>,
  mode: 'copy' | 'move',
) {
  const results = await Promise.allSettled(sources.map(download));
  const tempPaths = results
    .filter((result): result is PromiseFulfilledResult<string> => result.status === 'fulfilled')
    .map((result) => result.value);
  const failed = results.find((result): result is PromiseRejectedResult => result.status === 'rejected');

  if (failed) {
    await Promise.allSettled(tempPaths.map(cleanupTempPath));
    throw failed.reason;
  }

  let dropped = false;
  try {
    await new Promise<void>((resolve, reject) => {
      startDrag({ item: tempPaths, icon: makeDragIcon(tempPaths.length), mode }, (payload) => {
        if (payload.result === 'Dropped') {
          dropped = true;
          emitInternalDrop(Number(payload.cursorPos.x), Number(payload.cursorPos.y));
        }
        resolve();
      }).catch(reject);
    });
  } finally {
    if (dropped) {
      // External drop targets may continue reading after accepting the paths.
      setTimeout(() => void Promise.allSettled(tempPaths.map(cleanupTempPath)), 30 * 60 * 1000);
    } else {
      await Promise.allSettled(tempPaths.map(cleanupTempPath));
    }
  }
}
