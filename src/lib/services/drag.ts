import { startDrag } from '@crabnebula/tauri-plugin-drag';
import { s3DownloadToTemp } from './s3';

export interface DragSource {
  side: 'left' | 'right';
  backend: string;
  paths: string[];
  s3ConnectionId?: string;
}

/** Shared drag state for coordinating between drag initiation and drop handling. */
export const dragState = {
  source: null as DragSource | null,
  shiftHeld: false,
};

// Track Shift key state globally for detecting move vs copy during native drag
if (typeof window !== 'undefined') {
  window.addEventListener('keydown', (e) => { if (e.key === 'Shift') dragState.shiftHeld = true; }, true);
  window.addEventListener('keyup', (e) => { if (e.key === 'Shift') dragState.shiftHeld = false; }, true);
}

export async function startLocalFileDrag(paths: string[]): Promise<void> {
  // Use the first file as the drag icon preview
  await startDrag({ item: paths, icon: paths[0] });
}

export async function startS3FileDrag(connectionId: string, keys: string[]): Promise<void> {
  const tempPaths = await Promise.all(
    keys.map((key) => s3DownloadToTemp(connectionId, key)),
  );
  await startDrag({ item: tempPaths, icon: tempPaths[0] });
}
