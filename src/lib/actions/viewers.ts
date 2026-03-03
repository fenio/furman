import { panels } from '$lib/state/panels.svelte';
import { appState } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { openFileDefault, openInEditor, extractArchiveToTemp } from '$lib/services/tauri';
import { s3DownloadToTemp, s3IsObjectEncrypted } from '$lib/services/s3';
import { sftpDownloadTemp } from '$lib/services/sftp';
import { error } from '$lib/services/log';
import { promptEncryptionPassword } from './fileops';

// ── Extension constants ─────────────────────────────────────────────────────

export const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
export const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);
export const systemOpenExtensions = new Set([
  'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
  'odt', 'ods', 'odp', 'rtf',
  'heic', 'heif', 'tiff', 'tif', 'raw', 'cr2', 'nef', 'arw', 'dng', 'psd', 'ai',
  'mp3', 'mp4', 'avi', 'mkv', 'mov', 'wav', 'flac', 'aac', 'ogg', 'wma', 'wmv',
  'dmg', 'app', 'pkg', 'deb', 'rpm',
  'pages', 'numbers', 'keynote',
]);

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
    } else if (systemOpenExtensions.has(lower) && panel.backend === 'local') {
      try {
        await openFileDefault(entry.path);
      } catch (err: unknown) {
        error(String(err));
      }
    } else if (panel.backend === 's3' && panel.s3Connection) {
      await openS3Viewer(entry.path, entry.extension, panel.s3Connection.connectionId);
    } else if (panel.backend === 'sftp' && panel.sftpConnection) {
      await openSftpViewer(entry.path, entry.extension, panel.sftpConnection.connectionId);
    } else if (panel.backend === 'archive' && panel.archiveInfo) {
      await openArchiveViewer(entry.path, entry.extension, panel.archiveInfo.archivePath);
    } else {
      openViewer(entry.path, entry.extension);
    }
  }
}

export function openViewer(filePath: string, ext: string | null) {
  const lower = (ext ?? '').toLowerCase();
  if (imageExtensions.has(lower)) {
    appState.viewerMode = 'image';
  } else {
    appState.viewerMode = 'text';
  }
  appState.viewerPath = filePath;
  appState.modal = 'viewer';
}

export function openEditor(filePath: string) {
  if (appState.externalEditor.trim()) {
    openInEditor(filePath, appState.externalEditor.trim()).catch((err) => {
      error(String(err));
    });
    return;
  }
  appState.editorPath = filePath;
  appState.editorDirty = false;
  appState.editorS3ConnectionId = '';
  appState.editorS3Key = '';
  appState.modal = 'editor';
}

export async function openS3Viewer(s3Path: string, ext: string | null, connectionId: string, password?: string) {
  if (!password) {
    try {
      const encrypted = await s3IsObjectEncrypted(connectionId, s3Path);
      if (encrypted) {
        promptEncryptionPassword((pw) => {
          openS3Viewer(s3Path, ext, connectionId, pw);
        }, 'Decryption password:');
        return;
      }
    } catch { /* continue without encryption */ }
  }

  statusState.setMessage('Downloading for preview...');
  try {
    const localPath = await s3DownloadToTemp(connectionId, s3Path, password);
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      await openFileDefault(localPath);
      statusState.setMessage('');
    } else if (imageExtensions.has(lower)) {
      appState.viewerMode = 'image';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    } else {
      appState.viewerMode = 'text';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    }
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openArchiveViewer(entryPath: string, ext: string | null, archivePath: string) {
  const hashIdx = entryPath.indexOf('#');
  if (hashIdx < 0) return;
  const internalPath = entryPath.substring(hashIdx + 1);

  statusState.setMessage('Extracting for preview...');
  try {
    const localPath = await extractArchiveToTemp(archivePath, internalPath);
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      await openFileDefault(localPath);
      statusState.setMessage('');
    } else if (imageExtensions.has(lower)) {
      appState.viewerMode = 'image';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    } else {
      appState.viewerMode = 'text';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    }
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openSftpViewer(sftpPath: string, ext: string | null, connectionId: string) {
  statusState.setMessage('Downloading for preview...');
  try {
    const localPath = await sftpDownloadTemp(connectionId, sftpPath);
    const lower = (ext ?? '').toLowerCase();
    if (systemOpenExtensions.has(lower)) {
      await openFileDefault(localPath);
      statusState.setMessage('');
    } else if (imageExtensions.has(lower)) {
      appState.viewerMode = 'image';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    } else {
      appState.viewerMode = 'text';
      appState.viewerPath = localPath;
      appState.modal = 'viewer';
    }
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Preview failed: ' + String(err));
  }
}

export async function openS3Editor(s3Path: string, connectionId: string, password?: string) {
  if (!password) {
    try {
      const encrypted = await s3IsObjectEncrypted(connectionId, s3Path);
      if (encrypted) {
        promptEncryptionPassword((pw) => {
          openS3Editor(s3Path, connectionId, pw);
        }, 'Decryption password:');
        return;
      }
    } catch { /* continue without encryption */ }
  }

  statusState.setMessage('Downloading for editing...');
  try {
    const localPath = await s3DownloadToTemp(connectionId, s3Path, password);
    appState.editorPath = localPath;
    appState.editorDirty = false;
    appState.editorS3ConnectionId = connectionId;
    appState.editorS3Key = s3Path;
    appState.modal = 'editor';
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Edit failed: ' + String(err));
  }
}

export async function openSftpEditor(sftpPath: string, connectionId: string) {
  statusState.setMessage('Downloading for editing...');
  try {
    const localPath = await sftpDownloadTemp(connectionId, sftpPath);
    appState.editorPath = localPath;
    appState.editorDirty = false;
    appState.editorS3ConnectionId = '';
    appState.editorS3Key = '';
    appState.editorSftpConnectionId = connectionId;
    appState.editorSftpPath = sftpPath;
    appState.modal = 'editor';
  } catch (err: unknown) {
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
