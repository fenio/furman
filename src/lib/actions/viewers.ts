import { panels } from '$lib/state/panels.svelte';
import { appState } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { openFileDefault, openInEditor, extractArchiveToTemp, cleanupTempPath } from '$lib/services/tauri';
import { s3DownloadToTemp, s3IsObjectEncrypted } from '$lib/services/s3';
import { sftpDownloadTemp } from '$lib/services/sftp';
import { error } from '$lib/services/log';
import { promptEncryptionPassword } from './fileops';

let editorOpenGeneration = 0;
let viewerOpenGeneration = 0;

function showEditor(
  filePath: string,
  target?:
    | { backend: 's3'; connectionId: string; path: string }
    | { backend: 'sftp'; connectionId: string; path: string },
  temporary = false,
) {
  const previousTempPath = appState.editorTempPath;
  if (previousTempPath && previousTempPath !== filePath) {
    cleanupTempPath(previousTempPath).catch(() => {});
  }
  appState.editorPath = filePath;
  appState.editorDirty = false;
  appState.editorS3ConnectionId = '';
  appState.editorS3Key = '';
  appState.editorSftpConnectionId = '';
  appState.editorSftpPath = '';
  appState.editorTempPath = temporary ? filePath : '';

  if (target?.backend === 's3') {
    appState.editorS3ConnectionId = target.connectionId;
    appState.editorS3Key = target.path;
  } else if (target?.backend === 'sftp') {
    appState.editorSftpConnectionId = target.connectionId;
    appState.editorSftpPath = target.path;
  }

  appState.modal = 'editor';
}

function showViewer(filePath: string, ext: string | null, temporary = false) {
  const previousTempPath = appState.viewerTempPath;
  if (previousTempPath && previousTempPath !== filePath) {
    cleanupTempPath(previousTempPath).catch(() => {});
  }
  const lower = (ext ?? '').toLowerCase();
  appState.viewerMode = imageExtensions.has(lower) ? 'image' : 'text';
  appState.viewerPath = filePath;
  appState.viewerTempPath = temporary ? filePath : '';
  appState.modal = 'viewer';
}

export function closeViewer() {
  viewerOpenGeneration++;
  const tempPath = appState.viewerTempPath;
  appState.viewerTempPath = '';
  appState.closeModal();
  if (tempPath) cleanupTempPath(tempPath).catch(() => {});
}

export function closeEditor() {
  editorOpenGeneration++;
  const tempPath = appState.editorTempPath;
  appState.editorTempPath = '';
  appState.closeModal();
  if (tempPath) cleanupTempPath(tempPath).catch(() => {});
}

// ── Extension constants ─────────────────────────────────────────────────────

export const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
export const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst']);
export const systemOpenExtensions = new Set([
  'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
  'odt', 'ods', 'odp', 'rtf',
  'heic', 'heif', 'tiff', 'tif', 'raw', 'cr2', 'nef', 'arw', 'dng', 'psd', 'ai',
  'mp3', 'mp4', 'avi', 'mkv', 'mov', 'wav', 'flac', 'aac', 'ogg', 'wma', 'wmv',
  'dmg', 'app', 'pkg', 'deb', 'rpm',
  'pages', 'numbers', 'keynote',
]);

import { formatSize } from '$lib/utils/format';

/** Returns true if the user approves downloading a remote file (or if no prompt needed). */
function confirmRemoteDownload(size: number, name: string): Promise<boolean> {
  const limit = appState.remoteDownloadLimit;
  if (limit === -1) return Promise.resolve(true);   // never ask
  if (limit > 0 && size <= limit) return Promise.resolve(true);  // under limit
  return new Promise((resolve) => {
    let resolved = false;
    appState.showConfirm(
      `"${name}" is ${formatSize(size).trim()}. Download it to open?`,
      () => { resolved = true; resolve(true); },
    );
    // Detect cancellation: modal closes without calling the callback
    const check = setInterval(() => {
      if (appState.modal !== 'confirm') {
        clearInterval(check);
        if (!resolved) resolve(false);
      }
    }, 100);
  });
}

// ── Viewer / Editor helpers ─────────────────────────────────────────────────

export async function activateEntry() {
  const panel = panels.active;
  const entry = panel.currentEntry;
  if (!entry) return;

  if (entry.is_dir) {
    if (entry.name === '..') {
      const currentDirName = panel.path.replace(/\/+$/, '').split('/').pop() ?? '';
      await panel.loadDirectory(entry.path, currentDirName);
    } else {
      await panel.loadDirectory(entry.path);
    }
  } else {
    const lower = (entry.extension ?? '').toLowerCase();
    if (archiveExtensions.has(lower) && panel.backend === 'local') {
      await panel.enterArchive(entry.path);
    } else if (archiveExtensions.has(lower) && panel.backend === 'sftp' && panel.sftpConnection) {
      if (!await confirmRemoteDownload(entry.size, entry.name)) return;
      const conn = panel.sftpConnection;
      const parentPath = entry.path.replace(/\/[^/]+$/, '') || '/';
      statusState.setMessage('Downloading archive...');
      try {
        const localPath = await sftpDownloadTemp(conn.connectionId, entry.path);
        await panel.enterArchive(localPath, {
          backend: 'sftp',
          path: parentPath,
          remoteName: entry.name,
          sftpConnection: conn,
        });
      } catch (err: unknown) {
        error(String(err));
        statusState.setMessage(`Error: ${err}`);
        return;
      }
      statusState.setMessage('');
    } else if (archiveExtensions.has(lower) && panel.backend === 's3' && panel.s3Connection) {
      if (!await confirmRemoteDownload(entry.size, entry.name)) return;
      const conn = panel.s3Connection;
      const parentPath = entry.path.replace(/\/[^/]+$/, '') || `s3://${conn.bucket}/`;
      statusState.setMessage('Downloading archive...');
      try {
        const localPath = await s3DownloadToTemp(conn.connectionId, entry.path);
        await panel.enterArchive(localPath, {
          backend: 's3',
          path: parentPath,
          remoteName: entry.name,
          s3Connection: conn,
        });
      } catch (err: unknown) {
        error(String(err));
        statusState.setMessage(`Error: ${err}`);
        return;
      }
      statusState.setMessage('');
    } else if (systemOpenExtensions.has(lower) && panel.backend === 'local') {
      try {
        await openFileDefault(entry.path);
      } catch (err: unknown) {
        error(String(err));
      }
    } else if (panel.backend === 's3' && panel.s3Connection) {
      if (!await confirmRemoteDownload(entry.size, entry.name)) return;
      await openS3Viewer(entry.path, entry.extension, panel.s3Connection.connectionId);
    } else if (panel.backend === 'sftp' && panel.sftpConnection) {
      if (!await confirmRemoteDownload(entry.size, entry.name)) return;
      await openSftpViewer(entry.path, entry.extension, panel.sftpConnection.connectionId);
    } else if (panel.backend === 'archive' && panel.archiveInfo) {
      await openArchiveViewer(entry.path, entry.extension, panel.archiveInfo.archivePath);
    } else {
      openViewer(entry.path, entry.extension);
    }
  }
}

export function openViewer(filePath: string, ext: string | null) {
  viewerOpenGeneration++;
  showViewer(filePath, ext);
}

export function openTemporaryViewer(filePath: string, ext: string | null = null) {
  viewerOpenGeneration++;
  showViewer(filePath, ext, true);
}

export function openEditor(filePath: string) {
  editorOpenGeneration++;
  statusState.setMessage('');
  if (appState.externalEditor.trim()) {
    openInEditor(filePath, appState.externalEditor.trim()).catch((err) => {
      error(String(err));
    });
    return;
  }
  showEditor(filePath);
}

export async function openS3Viewer(s3Path: string, ext: string | null, connectionId: string, password?: string) {
  const generation = ++viewerOpenGeneration;
  if (!password) {
    try {
      const encrypted = await s3IsObjectEncrypted(connectionId, s3Path);
      if (generation !== viewerOpenGeneration) return;
      if (encrypted) {
        promptEncryptionPassword((pw) => {
          openS3Viewer(s3Path, ext, connectionId, pw);
        }, 'Decryption password:');
        return;
      }
    } catch {
      if (generation !== viewerOpenGeneration) return;
      // Continue without encryption when metadata detection is unavailable.
    }
  }

  statusState.setMessage('Downloading for preview...');
  try {
    const localPath = await s3DownloadToTemp(connectionId, s3Path, password);
    if (generation !== viewerOpenGeneration) {
      cleanupTempPath(localPath).catch(() => {});
      return;
    }
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      try {
        await openFileDefault(localPath);
      } catch (err) {
        cleanupTempPath(localPath).catch(() => {});
        throw err;
      }
      statusState.setMessage('');
    } else {
      showViewer(localPath, ext, true);
    }
  } catch (err: unknown) {
    if (generation !== viewerOpenGeneration) return;
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openArchiveViewer(entryPath: string, ext: string | null, archivePath: string) {
  const generation = ++viewerOpenGeneration;
  const hashIdx = entryPath.indexOf('#');
  if (hashIdx < 0) return;
  const internalPath = entryPath.substring(hashIdx + 1);

  statusState.setMessage('Extracting for preview...');
  try {
    const localPath = await extractArchiveToTemp(archivePath, internalPath);
    if (generation !== viewerOpenGeneration) {
      cleanupTempPath(localPath).catch(() => {});
      return;
    }
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      try {
        await openFileDefault(localPath);
      } catch (err) {
        cleanupTempPath(localPath).catch(() => {});
        throw err;
      }
      statusState.setMessage('');
    } else {
      showViewer(localPath, ext, true);
    }
  } catch (err: unknown) {
    if (generation !== viewerOpenGeneration) return;
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openSftpViewer(sftpPath: string, ext: string | null, connectionId: string) {
  const generation = ++viewerOpenGeneration;
  statusState.setMessage('Downloading for preview...');
  try {
    const localPath = await sftpDownloadTemp(connectionId, sftpPath);
    if (generation !== viewerOpenGeneration) {
      cleanupTempPath(localPath).catch(() => {});
      return;
    }
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      try {
        await openFileDefault(localPath);
      } catch (err) {
        cleanupTempPath(localPath).catch(() => {});
        throw err;
      }
      statusState.setMessage('');
    } else {
      showViewer(localPath, ext, true);
    }
  } catch (err: unknown) {
    if (generation !== viewerOpenGeneration) return;
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openS3Editor(s3Path: string, connectionId: string, password?: string) {
  const generation = ++editorOpenGeneration;
  if (!password) {
    try {
      const encrypted = await s3IsObjectEncrypted(connectionId, s3Path);
      if (generation !== editorOpenGeneration) return;
      if (encrypted) {
        promptEncryptionPassword((pw) => {
          openS3Editor(s3Path, connectionId, pw);
        }, 'Decryption password:');
        return;
      }
    } catch {
      if (generation !== editorOpenGeneration) return;
      // Continue without encryption when metadata detection is unavailable.
    }
  }

  statusState.setMessage('Downloading for editing...');
  try {
    const localPath = await s3DownloadToTemp(connectionId, s3Path, password);
    if (generation !== editorOpenGeneration) {
      cleanupTempPath(localPath).catch(() => {});
      return;
    }
    showEditor(localPath, { backend: 's3', connectionId, path: s3Path }, true);
  } catch (err: unknown) {
    if (generation !== editorOpenGeneration) return;
    error(String(err));
    appState.showAlert('Edit failed: ' + String(err));
  }
}

export async function openSftpEditor(sftpPath: string, connectionId: string) {
  const generation = ++editorOpenGeneration;
  statusState.setMessage('Downloading for editing...');
  try {
    const localPath = await sftpDownloadTemp(connectionId, sftpPath);
    if (generation !== editorOpenGeneration) {
      cleanupTempPath(localPath).catch(() => {});
      return;
    }
    showEditor(localPath, { backend: 'sftp', connectionId, path: sftpPath }, true);
  } catch (err: unknown) {
    if (generation !== editorOpenGeneration) return;
    error(String(err));
    appState.showAlert('Edit failed: ' + String(err));
  }
}

export function quickLook() {
  const panel = panels.active;
  const entry = panel.currentEntry;
  if (!entry || entry.is_dir || entry.name === '..') return;
  if (panel.backend === 's3' && panel.s3Connection) {
    openS3Viewer(entry.path, entry.extension, panel.s3Connection.connectionId);
  } else if (panel.backend === 'sftp' && panel.sftpConnection) {
    openSftpViewer(entry.path, entry.extension, panel.sftpConnection.connectionId);
  } else if (panel.backend === 'archive' && panel.archiveInfo) {
    openArchiveViewer(entry.path, entry.extension, panel.archiveInfo.archivePath);
  } else {
    const lower = (entry.extension ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      openFileDefault(entry.path).catch((err: unknown) => error(String(err)));
    } else {
      openViewer(entry.path, entry.extension);
    }
  }
}
