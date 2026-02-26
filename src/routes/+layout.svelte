<script lang="ts">
  import favicon from '$lib/assets/favicon.svg';
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import { workspacesState } from '$lib/state/workspaces.svelte';
  import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
  import { s3ProfilesState } from '$lib/state/s3profiles.svelte';
  import { copyFiles, moveFiles, deleteFiles, renameFile, createDirectory, openFileDefault, openInEditor, checkConflicts } from '$lib/services/tauri';
  import { statusState } from '$lib/state/status.svelte';
  import { transfersState } from '$lib/state/transfers.svelte';
  import { s3Download, s3Upload, s3CopyObjects, s3DeleteObjects, s3RenameObject, s3CreateFolder, s3PresignUrl, s3DownloadToTemp, s3BulkChangeStorageClass, s3IsObjectEncrypted, type EncryptionConfig } from '$lib/services/s3';
  import { keychainGet } from '$lib/services/keychain';
  import { s3PathToPrefix } from '$lib/state/panels.svelte';
  import { error } from '$lib/services/log';
  import { resolveCapabilities } from '$lib/data/s3-providers';
  import type { S3Bookmark } from '$lib/types';
  import { dragState } from '$lib/services/drag';
  import type { PanelData } from '$lib/state/panels.svelte';
  import type { ProgressEvent, S3ConnectionInfo, S3ProviderCapabilities, SyncEntry } from '$lib/types';

  let { children } = $props();

  // ── Native drag-and-drop from OS ──────────────────────────────────────────

  function getTargetPanel(position: { x: number; y: number }): { panel: PanelData; side: 'left' | 'right' } | null {
    const panelEls = document.querySelectorAll('.file-panel');
    for (const [i, el] of Array.from(panelEls).entries()) {
      const rect = el.getBoundingClientRect();
      if (position.x >= rect.left && position.x <= rect.right &&
          position.y >= rect.top && position.y <= rect.bottom) {
        const side = i === 0 ? 'left' as const : 'right' as const;
        return { panel: i === 0 ? panels.left : panels.right, side };
      }
    }
    return null;
  }

  let dragDropUnlisten: (() => void) | null = null;

  onMount(async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      dragDropUnlisten = await getCurrentWindow().onDragDropEvent((event) => {
        if (event.payload.type === 'drop') {
          const paths = event.payload.paths;
          const position = event.payload.position;
          if (!paths || paths.length === 0) return;

          const result = getTargetPanel(position);
          if (!result) return;
          const { panel: target, side: targetSide } = result;

          // Internal drag from within the app (panel-to-panel via native drag)
          if (dragState.source && dragState.source.side !== targetSide) {
            const sourceSide = dragState.source.side;
            panels.activePanel = sourceSide;
            // Dispatch F5 (copy) or F6 (move if Shift held) — same as HTML5 panel-to-panel drop
            const key = dragState.shiftHeld ? 'F6' : 'F5';
            window.dispatchEvent(new KeyboardEvent('keydown', { key }));
            dragState.source = null;
            return;
          }
          // Internal drag dropped on the same panel — ignore
          if (dragState.source) {
            dragState.source = null;
            return;
          }

          // External drag from OS (Finder → app)
          const opId = 'drop-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);

          if (target.backend === 's3' && target.s3Connection) {
            const conn = target.s3Connection;
            const prefix = s3PathToPrefix(target.path, conn.bucket);
            transfersState.enqueue({
              id: opId,
              type: 'copy',
              sources: paths,
              destination: target.path,
              srcBackend: 'local',
              destBackend: 's3',
              s3DestConnectionId: conn.connectionId,
              s3DestPrefix: prefix,
            });
          } else if (target.backend === 'local') {
            transfersState.enqueue({
              id: opId,
              type: 'copy',
              sources: paths,
              destination: target.path,
              srcBackend: 'local',
              destBackend: 'local',
            });
          }
        }
      });
    } catch {
      // onDragDropEvent not available — will be handled by capability check
    }
  });

  function handleSyncExecuteEvent(e: Event) {
    const detail = (e as CustomEvent).detail as {
      entries: SyncEntry[];
      sourceBackend: string;
      sourcePath: string;
      sourceS3Id: string;
      destBackend: string;
      destPath: string;
      destS3Id: string;
    };
    executeSyncTransfer(detail);
  }

  window.addEventListener('sync-execute', handleSyncExecuteEvent);

  function handleTransferDone() {
    const reloads: Promise<void>[] = [];
    if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
    if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
    Promise.all(reloads);
  }

  window.addEventListener('transfer-done', handleTransferDone);

  onDestroy(() => {
    dragDropUnlisten?.();
    window.removeEventListener('sync-execute', handleSyncExecuteEvent);
    window.removeEventListener('transfer-done', handleTransferDone);
  });

  function executeSyncTransfer(detail: {
    entries: SyncEntry[];
    sourceBackend: string;
    sourcePath: string;
    sourceS3Id: string;
    destBackend: string;
    destPath: string;
    destS3Id: string;
  }) {
    const { entries, sourceBackend, sourcePath, sourceS3Id, destBackend, destPath, destS3Id } = detail;

    // Split entries by action needed
    const toCopy = entries.filter((e) => e.status === 'new' || e.status === 'modified');
    const toDelete = entries.filter((e) => e.status === 'deleted');

    // Build full source paths for copy operations
    if (toCopy.length > 0) {
      const opId = 'sync-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
      const copySourcePaths = toCopy.map((e) => {
        if (sourceBackend === 's3') {
          // For S3 source, build full s3://bucket/prefix/relative_path
          const base = sourcePath.endsWith('/') ? sourcePath : sourcePath + '/';
          return base + e.relative_path;
        } else {
          // For local source, join path with relative path
          const base = sourcePath.endsWith('/') ? sourcePath : sourcePath + '/';
          return base + e.relative_path;
        }
      });

      transfersState.add(opId, 'copy', copySourcePaths, destPath);

      (async () => {
        try {
          const onProgress = (e: ProgressEvent) => {
            transfersState.updateProgress(opId, e);
          };

          if (sourceBackend === 'local' && destBackend === 'local') {
            await copyFiles(opId, copySourcePaths, destPath, onProgress);
          } else if (sourceBackend === 's3' && destBackend === 'local') {
            await s3Download(sourceS3Id, opId, copySourcePaths, destPath, onProgress);
          } else if (sourceBackend === 'local' && destBackend === 's3') {
            const prefix = s3PathToPrefix(destPath, '');
            await s3Upload(destS3Id, opId, copySourcePaths, prefix, onProgress);
          } else if (sourceBackend === 's3' && destBackend === 's3') {
            const destPrefix = s3PathToPrefix(destPath, '');
            await s3CopyObjects(sourceS3Id, opId, copySourcePaths, destS3Id, destPrefix, onProgress);
          }

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
            statusState.setMessage('Sync failed');
          }
        } finally {
          // Handle deletions after copies complete
          if (toDelete.length > 0) {
            await executeSyncDeletes(toDelete, destBackend, destPath, destS3Id);
          }
          // Reload both panels
          const reloads: Promise<void>[] = [];
          if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
          if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
          await Promise.all(reloads);
        }
      })();
    } else if (toDelete.length > 0) {
      // Only deletions, no copies
      (async () => {
        await executeSyncDeletes(toDelete, destBackend, destPath, destS3Id);
        const reloads: Promise<void>[] = [];
        if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
        if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
        await Promise.all(reloads);
      })();
    }
  }

  async function executeSyncDeletes(
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
        await s3DeleteObjects(destS3Id, deletePaths);
      } else {
        await deleteFiles(deletePaths, true);
      }
      statusState.setMessage(`Deleted ${toDelete.length} file(s) from destination`);
    } catch (err: unknown) {
      error(String(err));
      statusState.setMessage('Sync delete failed');
    }
  }

  // ── Constants ──────────────────────────────────────────────────────────────

  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
  const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);
  const systemOpenExtensions = new Set([
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
    'odt', 'ods', 'odp', 'rtf',
    'heic', 'heif', 'tiff', 'tif', 'raw', 'cr2', 'nef', 'arw', 'dng', 'psd', 'ai',
    'mp3', 'mp4', 'avi', 'mkv', 'mov', 'wav', 'flac', 'aac', 'ogg', 'wma', 'wmv',
    'dmg', 'app', 'pkg', 'deb', 'rpm',
    'pages', 'numbers', 'keynote',
  ]);

  async function activateEntry() {
    const panel = panels.active;
    const entry = panel.currentEntry;
    if (!entry) return;

    if (entry.is_dir) {
      if (entry.name === '..') {
        // Navigate to parent — focus on the directory we just left
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
      } else {
        // Open file in viewer
        openViewer(entry.path, entry.extension);
      }
    }
  }

  function openViewer(filePath: string, ext: string | null) {
    const lower = (ext ?? '').toLowerCase();
    if (imageExtensions.has(lower)) {
      appState.viewerMode = 'image';
    } else {
      appState.viewerMode = 'text';
    }
    appState.viewerPath = filePath;
    appState.modal = 'viewer';
  }

  function openEditor(filePath: string) {
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

  async function openS3Viewer(s3Path: string, ext: string | null, connectionId: string, password?: string) {
    // Check encryption if no password provided
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
      statusState.setMessage('Preview failed: ' + String(err));
    }
  }

  function quickLook() {
    const panel = panels.active;
    const entry = panel.currentEntry;
    if (!entry || entry.is_dir || entry.name === '..') return;
    if (panel.backend === 's3' && panel.s3Connection) {
      openS3Viewer(entry.path, entry.extension, panel.s3Connection.connectionId);
    } else {
      const lower = (entry.extension ?? '').toLowerCase();
      if (systemOpenExtensions.has(lower)) {
        openFileDefault(entry.path).catch((err: unknown) => error(String(err)));
      } else {
        openViewer(entry.path, entry.extension);
      }
    }
  }

  async function openS3Editor(s3Path: string, connectionId: string, password?: string) {
    // Check encryption if no password provided
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
      statusState.setMessage('Edit failed: ' + String(err));
    }
  }

  async function getConflicts(sources: string[], destBackend: string, dest: string): Promise<string[]> {
    if (destBackend === 'local') {
      return await checkConflicts(sources, dest);
    }
    // For S3 destination, check against loaded panel entries
    const destNames = new Set(panels.inactive.entries.map((e) => e.name));
    return sources.filter((s) => destNames.has(s.split('/').pop() ?? ''));
  }

  function withConflictCheck(
    sources: string[],
    dest: string,
    destBackend: string,
    execute: (finalSources: string[]) => void,
  ) {
    getConflicts(sources, destBackend, dest).then((conflicts) => {
      if (conflicts.length === 0) {
        execute(sources);
        return;
      }
      const conflictNames = conflicts.map((s) => s.split('/').pop() ?? s);
      appState.showOverwrite(conflictNames, (action) => {
        const finalSources = action === 'skip'
          ? sources.filter((s) => !conflicts.includes(s))
          : sources;
        if (finalSources.length === 0) {
          statusState.setMessage('All files skipped');
          return;
        }
        execute(finalSources);
      });
    });
  }

  function executeCopy(
    sources: string[],
    dest: string,
    srcBackend: string,
    destBackend: string,
    encryptionPassword?: string,
    encryptionConfig?: EncryptionConfig,
  ) {
    const active = panels.active;
    const inactive = panels.inactive;
    const opId = 'file-op-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
    const transferType = srcBackend === 'archive' ? 'extract' as const : 'copy' as const;

    if (srcBackend === 'archive' && destBackend === 'local') {
      const archivePath = active.archiveInfo!.archivePath;
      const internalPaths = sources.map((s) => {
        const hashIdx = s.indexOf('#');
        return hashIdx >= 0 ? s.substring(hashIdx + 1) : s;
      });
      transfersState.enqueue({
        id: opId,
        type: transferType,
        sources,
        destination: dest,
        srcBackend,
        destBackend,
        archivePath,
        archiveInternalPaths: internalPaths,
      });
    } else {
      transfersState.enqueue({
        id: opId,
        type: transferType,
        sources,
        destination: dest,
        srcBackend,
        destBackend,
        s3SrcConnectionId: active.s3Connection?.connectionId,
        s3DestConnectionId: inactive.s3Connection?.connectionId,
        s3DestPrefix: destBackend === 's3' && inactive.s3Connection
          ? s3PathToPrefix(dest, inactive.s3Connection.bucket)
          : undefined,
        encryptionPassword,
        encryptionConfig,
      });
    }
  }

  function executeMove(
    sources: string[],
    dest: string,
    srcBackend: string,
    destBackend: string,
    encryptionPassword?: string,
    encryptionConfig?: EncryptionConfig,
  ) {
    const active = panels.active;
    const inactive = panels.inactive;
    const opId = 'file-op-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);

    transfersState.enqueue({
      id: opId,
      type: 'move',
      sources,
      destination: dest,
      srcBackend,
      destBackend,
      s3SrcConnectionId: active.s3Connection?.connectionId,
      s3DestConnectionId: inactive.s3Connection?.connectionId,
      s3DestPrefix: destBackend === 's3' && inactive.s3Connection
        ? s3PathToPrefix(dest, inactive.s3Connection.bucket)
        : undefined,
      encryptionPassword,
      encryptionConfig,
    });
  }

  function findProfileForConnection(connectionId: string): import('$lib/types').S3Profile | undefined {
    const panel = [panels.left, panels.right].find(
      (p) => p.s3Connection?.connectionId === connectionId,
    );
    if (!panel?.s3Connection) return undefined;
    return s3ProfilesState.profiles.find(
      (p) => p.bucket === panel.s3Connection!.bucket,
    );
  }

  function buildEncryptionConfig(profile: import('$lib/types').S3Profile): EncryptionConfig {
    return {
      algorithm: profile.encryptionCipher ?? 'aes-256-gcm',
      kdf_memory_cost: profile.kdfMemoryCost ?? 19456,
      kdf_time_cost: profile.kdfTimeCost ?? 2,
      kdf_parallelism: profile.kdfParallelism ?? 1,
      secure_temp_cleanup: appState.secureTempCleanup,
    };
  }

  /** Check whether auto-encrypt should be skipped based on profile thresholds. */
  function shouldAutoEncrypt(sources: string[], profile: import('$lib/types').S3Profile): boolean {
    // Extension filter: if set, at least one file must match
    const exts = profile.autoEncryptExtensions;
    if (exts && exts.length > 0) {
      const extSet = new Set(exts.map((e) => e.toLowerCase().replace(/^\./, '')));
      const hasMatch = sources.some((s) => {
        const name = s.split('/').pop() ?? '';
        const dot = name.lastIndexOf('.');
        if (dot < 0) return false;
        return extSet.has(name.substring(dot + 1).toLowerCase());
      });
      if (!hasMatch) return false;
    }
    // Min-size threshold: if ALL files are below the threshold, skip
    const minSize = profile.autoEncryptMinSize;
    if (minSize && minSize > 0) {
      const panel = panels.active;
      const allSmall = sources.every((s) => {
        const entry = panel.entries.find((e) => e.path === s);
        return entry && !entry.is_dir && entry.size < minSize;
      });
      if (allSmall) return false;
    }
    return true;
  }

  function promptEncryptionPassword(
    callback: (password: string) => void,
    promptText = 'Encryption password:',
  ) {
    appState.showInput(promptText, '', (pw) => {
      appState.closeModal();
      if (pw) callback(pw);
    }, 'password');
  }

  async function handleCopy() {
    const active = panels.active;
    const inactive = panels.inactive;
    const sources = active.getSelectedOrCurrent();
    if (sources.length === 0) return;

    const dest = inactive.path;
    const names = sources.map((s) => s.split('/').pop()).join(', ');
    const srcBackend = active.backend;
    const destBackend = inactive.backend;

    appState.showConfirm(`Copy ${sources.length} item(s) to ${dest}?\n${names}`, () => {
      appState.closeModal();

      // Check if we need encryption for upload (local→s3)
      if (srcBackend === 'local' && destBackend === 's3' && inactive.s3Connection) {
        const profile = findProfileForConnection(inactive.s3Connection.connectionId);
        if (profile?.defaultClientEncryption && shouldAutoEncrypt(sources, profile)) {
          const config = buildEncryptionConfig(profile);
          promptEncryptionPassword((pw) => {
            withConflictCheck(sources, dest, destBackend, (finalSources) =>
              executeCopy(finalSources, dest, srcBackend, destBackend, pw, config),
            );
          });
          return;
        }
      }

      // Check if we need decryption for download (s3→local)
      if (srcBackend === 's3' && destBackend === 'local' && active.s3Connection) {
        const firstFile = sources.find((s) => !s.endsWith('/'));
        if (firstFile) {
          s3IsObjectEncrypted(active.s3Connection.connectionId, firstFile).then((encrypted) => {
            if (encrypted) {
              promptEncryptionPassword((pw) => {
                withConflictCheck(sources, dest, destBackend, (finalSources) =>
                  executeCopy(finalSources, dest, srcBackend, destBackend, pw),
                );
              }, 'Decryption password:');
            } else {
              withConflictCheck(sources, dest, destBackend, (finalSources) =>
                executeCopy(finalSources, dest, srcBackend, destBackend),
              );
            }
          }).catch(() => {
            withConflictCheck(sources, dest, destBackend, (finalSources) =>
              executeCopy(finalSources, dest, srcBackend, destBackend),
            );
          });
          return;
        }
      }

      withConflictCheck(sources, dest, destBackend, (finalSources) =>
        executeCopy(finalSources, dest, srcBackend, destBackend),
      );
    });
  }

  async function handleMove() {
    const active = panels.active;
    const inactive = panels.inactive;
    const sources = active.getSelectedOrCurrent();
    if (sources.length === 0) return;

    const dest = inactive.path;
    const names = sources.map((s) => s.split('/').pop()).join(', ');
    const srcBackend = active.backend;
    const destBackend = inactive.backend;

    appState.showConfirm(`Move ${sources.length} item(s) to ${dest}?\n${names}`, () => {
      appState.closeModal();

      // Check if we need encryption for upload (local→s3)
      if (srcBackend === 'local' && destBackend === 's3' && inactive.s3Connection) {
        const profile = findProfileForConnection(inactive.s3Connection.connectionId);
        if (profile?.defaultClientEncryption && shouldAutoEncrypt(sources, profile)) {
          const config = buildEncryptionConfig(profile);
          promptEncryptionPassword((pw) => {
            withConflictCheck(sources, dest, destBackend, (finalSources) =>
              executeMove(finalSources, dest, srcBackend, destBackend, pw, config),
            );
          });
          return;
        }
      }

      // Check if we need decryption for download (s3→local)
      if (srcBackend === 's3' && destBackend === 'local' && active.s3Connection) {
        const firstFile = sources.find((s) => !s.endsWith('/'));
        if (firstFile) {
          s3IsObjectEncrypted(active.s3Connection.connectionId, firstFile).then((encrypted) => {
            if (encrypted) {
              promptEncryptionPassword((pw) => {
                withConflictCheck(sources, dest, destBackend, (finalSources) =>
                  executeMove(finalSources, dest, srcBackend, destBackend, pw),
                );
              }, 'Decryption password:');
            } else {
              withConflictCheck(sources, dest, destBackend, (finalSources) =>
                executeMove(finalSources, dest, srcBackend, destBackend),
              );
            }
          }).catch(() => {
            withConflictCheck(sources, dest, destBackend, (finalSources) =>
              executeMove(finalSources, dest, srcBackend, destBackend),
            );
          });
          return;
        }
      }

      withConflictCheck(sources, dest, destBackend, (finalSources) =>
        executeMove(finalSources, dest, srcBackend, destBackend),
      );
    });
  }

  async function handleDelete() {
    const active = panels.active;
    const sources = active.getSelectedOrCurrent();
    if (sources.length === 0) return;

    const names = sources.map((s) => s.split('/').pop()).join(', ');

    appState.showConfirm(`Delete ${sources.length} item(s)?\n${names}`, async () => {
      appState.closeModal();
      const fileCount = sources.length;
      try {
        if (active.backend === 's3' && active.s3Connection) {
          await s3DeleteObjects(active.s3Connection.connectionId, sources);
        } else {
          await deleteFiles(sources, true);
        }
      } catch (err: unknown) {
        error(String(err));
        statusState.setMessage('Delete failed');
      } finally {
        statusState.setMessage(`Deleted ${fileCount} file(s)`);
        await active.loadDirectory(active.path);
      }
    });
  }

  function handleS3Connect() {
    const panel = panels.active;
    appState.showS3Connect(async (bucket, region, endpoint, profile, accessKey, secretKey, provider, customCapabilities) => {
      const connectionId = `s3-${Date.now()}`;
      const caps = resolveCapabilities({ provider, customCapabilities });
      const info: S3ConnectionInfo = { bucket, region, connectionId, provider, capabilities: caps };
      if (endpoint) info.endpoint = endpoint;
      if (profile) info.profile = profile;
      try {
        await panel.connectS3(info, endpoint, profile, accessKey, secretKey);
      } catch (err: unknown) {
        error(String(err));
      }
    });
  }

  function handleRename() {
    const active = panels.active;
    const entry = active.currentEntry;
    if (!entry || entry.name === '..') return;

    appState.showInput('Rename to:', entry.name, async (newName: string) => {
      appState.closeModal();
      if (!newName || newName === entry.name) return;
      try {
        if (active.backend === 's3' && active.s3Connection) {
          await s3RenameObject(active.s3Connection.connectionId, entry.path, newName);
        } else {
          await renameFile(entry.path, newName);
        }
      } catch (err: unknown) {
        error(String(err));
      } finally {
        await active.loadDirectory(active.path);
      }
    });
  }

  function handleMkDir() {
    const active = panels.active;

    appState.showInput('Create directory:', '', async (name: string) => {
      appState.closeModal();
      if (!name) return;
      let mkdirError = '';
      try {
        if (active.backend === 's3' && active.s3Connection) {
          const prefix = s3PathToPrefix(active.path, active.s3Connection.bucket);
          const folderKey = prefix + name + '/';
          await s3CreateFolder(active.s3Connection.connectionId, folderKey);
        } else {
          const newPath = active.path.replace(/\/+$/, '') + '/' + name;
          await createDirectory(newPath);
        }
      } catch (err: unknown) {
        const raw = err instanceof Error ? err.message : String(err);
        mkdirError = raw.includes('Already exists') ? 'Directory already exists' : raw;
        error(String(err));
      }
      await active.loadDirectory(active.path);
      if (mkdirError) {
        appState.showAlert(mkdirError);
      }
    });
  }

  function handlePresignUrl() {
    const active = panels.active;
    const entry = active.currentEntry;
    if (!entry || entry.name === '..' || entry.is_dir) return;
    if (active.backend !== 's3' || !active.s3Connection) return;

    const connectionId = active.s3Connection.connectionId;
    appState.showInput('Link expires in (minutes):', '60', async (val: string) => {
      appState.closeModal();
      const minutes = parseInt(val, 10);
      if (!minutes || minutes <= 0) return;
      try {
        const url = await s3PresignUrl(connectionId, entry.path, minutes * 60);
        await navigator.clipboard.writeText(url);
        statusState.setMessage('Presigned URL copied to clipboard');
      } catch (err: unknown) {
        error(String(err));
      }
    });
  }

  function handleProperties() {
    const active = panels.active;
    const entry = active.currentEntry;
    if (!entry) return;
    // If cursor is on '..' in an S3 bucket, show bucket-level properties
    if (entry.name === '..') {
      if (active.backend === 's3' && active.s3Connection) {
        appState.showProperties(
          `s3://${active.s3Connection.bucket}/`,
          active.backend,
          active.s3Connection.connectionId,
          active.s3Connection.capabilities,
        );
      }
      return;
    }
    // Multi-selection: batch edit (S3 only, skip folders)
    if (active.backend === 's3' && active.s3Connection && active.selectedPaths.size > 1) {
      const keys = [...active.selectedPaths].filter(p => !p.endsWith('/'));
      if (keys.length > 0) {
        appState.showBatchEdit(
          keys,
          active.s3Connection.connectionId,
          active.s3Connection.capabilities,
        );
        return;
      }
    }
    appState.showProperties(
      entry.path,
      active.backend,
      active.s3Connection?.connectionId,
      active.s3Connection?.capabilities,
    );
  }

  function handleBucketProperties() {
    const active = panels.active;
    if (active.backend !== 's3' || !active.s3Connection) return;
    appState.showProperties(
      `s3://${active.s3Connection.bucket}/`,
      active.backend,
      active.s3Connection.connectionId,
      active.s3Connection.capabilities,
    );
  }

  async function handleCopyS3Uri() {
    const active = panels.active;
    if (active.backend !== 's3') return;
    const entry = active.currentEntry;
    if (!entry || entry.name === '..') return;
    // entry.path is already s3://bucket/key
    try {
      await navigator.clipboard.writeText(entry.path);
      statusState.setMessage(`Copied: ${entry.path}`);
    } catch (err: unknown) {
      error(String(err));
    }
  }

  function handleBulkStorageClassChange() {
    const active = panels.active;
    if (active.backend !== 's3' || !active.s3Connection) return;
    const caps = active.s3Connection.capabilities;
    if (caps && caps.storageClasses.length <= 1) return;
    const selected = active.getSelectedOrCurrent();
    if (selected.length === 0) return;

    const connectionId = active.s3Connection.connectionId;
    const defaultClass = caps && caps.storageClasses.length > 1 ? caps.storageClasses[1] : 'STANDARD_IA';
    appState.showInput('Target storage class (e.g. STANDARD_IA, GLACIER):', defaultClass, async (targetClass: string) => {
      appState.closeModal();
      if (!targetClass) return;
      try {
        const failed = await s3BulkChangeStorageClass(connectionId, selected, targetClass);
        if (failed.length === 0) {
          statusState.setMessage(`Storage class changed to ${targetClass} for ${selected.length} object(s)`);
        } else {
          statusState.setMessage(`${selected.length - failed.length} succeeded, ${failed.length} failed`);
        }
        await active.loadDirectory(active.path);
      } catch (err: unknown) {
        error(String(err));
      }
    });
  }

  function handleBookmarkS3() {
    const active = panels.active;
    if (active.backend !== 's3' || !active.s3Connection) return;

    const conn = active.s3Connection;
    // Find a saved profile matching this connection
    const profile = s3ProfilesState.profiles.find((p) =>
      p.bucket === conn.bucket &&
      p.region === conn.region &&
      (p.endpoint ?? '') === (conn.endpoint ?? ''),
    );

    if (!profile) {
      statusState.setMessage('Save this connection as a profile first');
      return;
    }

    // Default name: last path segment or bucket name
    const pathSegments = active.path.replace(/\/+$/, '').split('/');
    const defaultName = pathSegments[pathSegments.length - 1] || conn.bucket;

    appState.showInput('Bookmark name:', defaultName, (name) => {
      appState.closeModal();
      if (!name) return;
      s3BookmarksState.add({
        id: Date.now().toString(36),
        name,
        profileId: profile.id,
        path: active.path,
      });
      statusState.setMessage(`Bookmarked: ${name}`);
    });
  }

  function handleQuit() {
    appState.showConfirm('Quit Furman?', async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        await getCurrentWindow().close();
      } catch {
        // Fallback
        window.close();
      }
    });
  }

  type SidebarAction =
    | { type: 'favorite'; path: string }
    | { type: 'add-favorite' }
    | { type: 'workspace'; name: string; leftPath: string; rightPath: string; activePanel: 'left' | 'right' }
    | { type: 'save-workspace' }
    | { type: 's3-bookmark'; id: string; name: string; profileId: string; path: string }
    | { type: 'volume'; mountPoint: string }
    | { type: 's3'; panel: 'left' | 'right'; bucket: string }
    | { type: 'theme' };

  function buildSidebarItems(): SidebarAction[] {
    const list: SidebarAction[] = [];
    for (const fav of sidebarState.favorites) {
      list.push({ type: 'favorite', path: fav.path });
    }
    list.push({ type: 'add-favorite' });
    for (const ws of workspacesState.workspaces) {
      list.push({ type: 'workspace', name: ws.name, leftPath: ws.leftPath, rightPath: ws.rightPath, activePanel: ws.activePanel });
    }
    list.push({ type: 'save-workspace' });
    for (const bm of s3BookmarksState.bookmarks) {
      list.push({ type: 's3-bookmark', id: bm.id, name: bm.name, profileId: bm.profileId, path: bm.path });
    }
    for (const vol of sidebarState.volumes) {
      list.push({ type: 'volume', mountPoint: vol.mount_point });
    }
    if (panels.left.s3Connection) {
      list.push({ type: 's3', panel: 'left', bucket: panels.left.s3Connection.bucket });
    }
    if (panels.right.s3Connection) {
      list.push({ type: 's3', panel: 'right', bucket: panels.right.s3Connection.bucket });
    }
    list.push({ type: 'theme' });
    return list;
  }

  async function navigateBookmark(bm: S3Bookmark) {
    sidebarState.blur();
    const profile = s3ProfilesState.profiles.find((p) => p.id === bm.profileId);
    if (!profile) {
      statusState.setMessage('S3 profile not found — save the connection as a profile first');
      return;
    }

    const panel = panels.active;
    const bmBucket = bm.path.replace(/^s3:\/\//, '').split('/')[0];

    // Already connected to this bucket — just navigate
    if (panel.backend === 's3' && panel.s3Connection && panel.s3Connection.bucket === bmBucket) {
      await panel.loadDirectory(bm.path);
      return;
    }

    // Connect using the profile
    let secretKey: string | undefined;
    let accessKey: string | undefined = profile.accessKeyId;
    if (profile.credentialType === 'keychain' && profile.accessKeyId) {
      try {
        const secret = await keychainGet(profile.id);
        if (secret) secretKey = secret;
      } catch (err: unknown) {
        error(String(err));
        statusState.setMessage('Failed to retrieve credentials from keychain');
        return;
      }
    }

    const connectionId = `s3-${Date.now()}`;
    const caps = resolveCapabilities({ provider: profile.provider, customCapabilities: profile.customCapabilities });
    const info: S3ConnectionInfo = { bucket: profile.bucket, region: profile.region, connectionId, provider: profile.provider, capabilities: caps };
    if (profile.endpoint) info.endpoint = profile.endpoint;
    if (profile.profile) info.profile = profile.profile;

    try {
      await panel.connectS3(info, profile.endpoint, profile.profile, accessKey, secretKey, profile.roleArn, profile.externalId, profile.sessionName, profile.sessionDurationSecs, profile.useTransferAcceleration);
      if (bm.path !== `s3://${profile.bucket}/`) {
        await panel.loadDirectory(bm.path);
      }
    } catch (err: unknown) {
      error(String(err));
      statusState.setMessage('Failed to connect: ' + String(err));
    }
  }

  function activateSidebarItem(action: SidebarAction) {
    if (!action) return;
    switch (action.type) {
      case 'favorite':
        sidebarState.blur();
        panels.active.loadDirectory(action.path);
        break;
      case 'add-favorite': {
        const path = panels.active.path;
        const name = path.replace(/\/+$/, '').split('/').pop() || path;
        sidebarState.addFavorite(name, path);
        break;
      }
      case 'workspace':
        sidebarState.blur();
        panels.activePanel = action.activePanel;
        Promise.all([
          panels.left.loadDirectory(action.leftPath),
          panels.right.loadDirectory(action.rightPath),
        ]);
        break;
      case 'save-workspace':
        sidebarState.blur();
        appState.showInput('Workspace name:', '', (name) => {
          appState.closeModal();
          if (!name) return;
          workspacesState.save({
            name,
            leftPath: panels.left.path,
            rightPath: panels.right.path,
            activePanel: panels.activePanel,
          });
        });
        break;
      case 's3-bookmark':
        navigateBookmark({ id: action.id, name: action.name, profileId: action.profileId, path: action.path });
        break;
      case 'volume':
        sidebarState.blur();
        panels.active.loadDirectory(action.mountPoint);
        break;
      case 's3':
        sidebarState.blur();
        panels.activePanel = action.panel;
        panels.active.loadDirectory(`s3://${action.bucket}/`);
        break;
      case 'theme':
        appState.toggleTheme();
        break;
    }
  }

  function isXtermFocused(): boolean {
    const el = document.activeElement;
    return !!el?.closest('.xterm');
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    const cmd = e.metaKey || e.ctrlKey;

    // Theme toggle — always active regardless of focus
    if (cmd && e.shiftKey && e.key === 'L') {
      e.preventDefault();
      appState.toggleTheme();
      return;
    }

    // Terminal toggle shortcuts — always active regardless of focus
    if (cmd) {
      if (e.key === '`') {
        e.preventDefault();
        terminalState.toggle('quake');
        return;
      }
      if (e.key === 't' && e.shiftKey) {
        e.preventDefault();
        // In-pane: replace the inactive panel
        terminalState.inPaneSlot = panels.activePanel === 'left' ? 'right' : 'left';
        terminalState.toggle('in-pane');
        return;
      }
      if (e.key === 't' && !e.shiftKey) {
        e.preventDefault();
        terminalState.toggle('bottom');
        return;
      }
    }

    // ESC hides quake console
    if (e.key === 'Escape' && terminalState.displayMode === 'quake') {
      e.preventDefault();
      terminalState.displayMode = 'none';
      return;
    }

    // If xterm is focused, let all other keys pass through to the terminal
    if (isXtermFocused()) {
      return;
    }

    // If a modal is open, let the modal handle its own keys
    if (appState.modal !== 'none' && appState.modal !== 'menu' && appState.modal !== 'volume-selector') {
      return;
    }

    // Sidebar keyboard navigation
    if (sidebarState.focused && sidebarState.visible) {
      const sidebarItems = buildSidebarItems();
      const count = sidebarItems.length;
      switch (e.key) {
        case 'ArrowUp':
          e.preventDefault();
          sidebarState.focusIndex = sidebarState.focusIndex > 0 ? sidebarState.focusIndex - 1 : count - 1;
          return;
        case 'ArrowDown':
          e.preventDefault();
          sidebarState.focusIndex = sidebarState.focusIndex < count - 1 ? sidebarState.focusIndex + 1 : 0;
          return;
        case 'Enter':
          e.preventDefault();
          activateSidebarItem(sidebarItems[sidebarState.focusIndex]);
          return;
        case 'Escape':
          e.preventDefault();
          sidebarState.blur();
          return;
        case 'Delete':
        case 'Backspace': {
          const item = sidebarItems[sidebarState.focusIndex];
          if (item && item.type === 'favorite') {
            e.preventDefault();
            sidebarState.removeFavorite(item.path);
          } else if (item && item.type === 'workspace') {
            e.preventDefault();
            workspacesState.remove(item.name);
          } else if (item && item.type === 's3-bookmark') {
            e.preventDefault();
            s3BookmarksState.remove(item.id);
          }
          return;
        }
      }
      // Don't let other keys fall through to panel navigation while sidebar is focused
      if (!cmd) return;
    }

    const active = panels.active;

    // Cmd/Ctrl shortcuts (macOS F-key alternatives)
    if (cmd) {
      switch (e.key) {
        case 'r':
          e.preventDefault();
          handleRename();                       // Cmd+R = Rename (F2)
          return;
        case '3':
          e.preventDefault();
          {
            const entry = active.currentEntry;
            if (entry && !entry.is_dir && entry.name !== '..') {
              if (active.backend === 's3' && active.s3Connection) {
                openS3Viewer(entry.path, entry.extension, active.s3Connection.connectionId);
              } else {
                openViewer(entry.path, entry.extension);  // Cmd+3 = View (F3)
              }
            }
          }
          return;
        case 'e':
          e.preventDefault();
          {
            const entry = active.currentEntry;
            if (entry && !entry.is_dir && entry.name !== '..') {
              if (active.backend === 's3' && active.s3Connection) {
                openS3Editor(entry.path, active.s3Connection.connectionId);
              } else {
                openEditor(entry.path);            // Cmd+E = Edit (F4)
              }
            }
          }
          return;
        case 'c':
          e.preventDefault();
          handleCopy();                          // Cmd+C = Copy (F5)
          return;
        case 'm':
          e.preventDefault();
          handleMove();                          // Cmd+M = Move (F6)
          return;
        case 'n':
          e.preventDefault();
          handleMkDir();                         // Cmd+N = MkDir (F7)
          return;
        case 'Backspace':
          e.preventDefault();
          handleDelete();                        // Cmd+Delete = Delete (F8)
          return;
        case 'd':
          e.preventDefault();
          if (active.backend === 's3') {
            handleBookmarkS3();                  // Cmd+D = Bookmark S3 path
          } else {
            appState.showInput('Workspace name:', '', (name) => {
              appState.closeModal();
              if (!name) return;
              workspacesState.save({
                name,
                leftPath: panels.left.path,
                rightPath: panels.right.path,
                activePanel: panels.activePanel,
              });
            });                                  // Cmd+D = Save workspace
          }
          return;
        case 's':
          e.preventDefault();
          if (active.backend === 's3') {
            active.disconnectS3();               // Cmd+S = Disconnect if S3
          } else {
            handleS3Connect();                   // Cmd+S = S3 Connect if local
          }
          return;
        case 'f':
          e.preventDefault();
          if (active.backend === 'local' || active.backend === 's3') {
            appState.showSearch(
              active.path,
              active.backend,
              active.s3Connection?.connectionId ?? '',
            );
          }
          return;
        case 'b':
          e.preventDefault();
          if (sidebarState.focused) {
            sidebarState.toggle();               // Focused → close sidebar
          } else if (sidebarState.visible) {
            sidebarState.focus();                // Visible → focus it
          } else {
            sidebarState.toggle();               // Hidden → open sidebar
          }
          return;
        case 'u':
          e.preventDefault();
          handlePresignUrl();                    // Cmd+U = Presigned URL
          return;
        case 'k':
          e.preventDefault();
          handleCopyS3Uri();                     // Cmd+K = Copy S3 URI
          return;
        case 'l':
          e.preventDefault();
          handleBulkStorageClassChange();         // Cmd+L = Bulk Storage Class
          return;
        case 'p':
          e.preventDefault();
          appState.toggleLayout();               // Cmd+P = Toggle single/dual pane
          return;
        case '/':
          e.preventDefault();
          appState.modal = 'shortcuts';          // Cmd+/ = Shortcuts cheatsheet
          return;
        case 'j':
          e.preventDefault();
          transfersState.toggle();               // Cmd+J = Transfer panel
          return;
        case 'I':
          e.preventDefault();
          handleBucketProperties();               // Cmd+Shift+I = Bucket Properties
          return;
        case 'i':
          e.preventDefault();
          handleProperties();                    // Cmd+I = Properties (F9)
          return;
        case 'q':
          e.preventDefault();
          handleQuit();                          // Cmd+Q = Quit (F10)
          return;
        case 'y':
          e.preventDefault();
          {
            const src = panels.active;
            const dst = panels.inactive;
            if (src.backend !== 'archive' && dst.backend !== 'archive') {
              appState.showSync(
                { backend: src.backend, path: src.path, s3Id: src.s3Connection?.connectionId ?? '' },
                { backend: dst.backend, path: dst.path, s3Id: dst.s3Connection?.connectionId ?? '' },
              );
            }
          }
          return;
      }
    }

    const isIconMode = active.viewMode === 'icon';
    const cols = active.gridColumns;

    switch (e.key) {
      case 'Escape':
        if (active.filterText) {
          e.preventDefault();
          active.clearFilter();
        }
        break;
      case 'ArrowUp':
        e.preventDefault();
        if (e.shiftKey) {
          active.moveCursor(isIconMode ? -cols : -1);
          active.selectRange(active.selectionAnchor, active.cursorIndex);
        } else {
          active.moveCursor(isIconMode ? -cols : -1);
          active.selectionAnchor = active.cursorIndex;
        }
        break;
      case 'ArrowDown':
        e.preventDefault();
        if (e.shiftKey) {
          active.moveCursor(isIconMode ? cols : 1);
          active.selectRange(active.selectionAnchor, active.cursorIndex);
        } else {
          active.moveCursor(isIconMode ? cols : 1);
          active.selectionAnchor = active.cursorIndex;
        }
        break;
      case 'ArrowLeft':
        if (isIconMode) {
          e.preventDefault();
          if (e.shiftKey) {
            active.moveCursor(-1);
            active.selectRange(active.selectionAnchor, active.cursorIndex);
          } else {
            active.moveCursor(-1);
            active.selectionAnchor = active.cursorIndex;
          }
        }
        break;
      case 'ArrowRight':
        if (isIconMode) {
          e.preventDefault();
          if (e.shiftKey) {
            active.moveCursor(1);
            active.selectRange(active.selectionAnchor, active.cursorIndex);
          } else {
            active.moveCursor(1);
            active.selectionAnchor = active.cursorIndex;
          }
        }
        break;
      case 'Home':
        e.preventDefault();
        if (e.shiftKey) {
          active.cursorIndex = 0;
          active.selectRange(active.selectionAnchor, 0);
        } else {
          active.moveCursorTo(0);
        }
        break;
      case 'End':
        e.preventDefault();
        {
          const lastIdx = active.filteredSortedEntries.length - 1;
          if (e.shiftKey) {
            active.cursorIndex = lastIdx;
            active.selectRange(active.selectionAnchor, lastIdx);
          } else {
            active.moveCursorTo(lastIdx);
          }
        }
        break;
      case 'PageUp':
        e.preventDefault();
        if (e.shiftKey) {
          active.moveCursor(isIconMode ? -cols * 4 : -20);
          active.selectRange(active.selectionAnchor, active.cursorIndex);
        } else {
          active.moveCursor(isIconMode ? -cols * 4 : -20);
          active.selectionAnchor = active.cursorIndex;
        }
        break;
      case 'PageDown':
        e.preventDefault();
        if (e.shiftKey) {
          active.moveCursor(isIconMode ? cols * 4 : 20);
          active.selectRange(active.selectionAnchor, active.cursorIndex);
        } else {
          active.moveCursor(isIconMode ? cols * 4 : 20);
          active.selectionAnchor = active.cursorIndex;
        }
        break;
      case 'Enter':
        e.preventDefault();
        activateEntry();
        break;
      case 'Backspace':
        e.preventDefault();
        if (active.filterText) {
          // Delete last character from filter
          active.filterText = active.filterText.slice(0, -1);
          if (active.filterText) {
            active.cursorIndex = Math.min(1, active.filteredSortedEntries.length - 1);
          }
        } else {
          // Go to parent directory — focus on the directory we just left
          const parentEntry = active.filteredSortedEntries.find((en) => en.name === '..');
          if (parentEntry) {
            const currentDirName = active.path.replace(/\/+$/, '').split('/').pop() ?? '';
            active.loadDirectory(parentEntry.path, currentDirName);
          }
        }
        break;
      case 'Tab':
        e.preventDefault();
        panels.switchPanel();
        break;
      case 'Insert':
        e.preventDefault();
        {
          const entry = active.currentEntry;
          if (entry && entry.name !== '..') {
            active.toggleSelection(entry.path);
          }
          active.moveCursor(1);
        }
        break;
      case ' ':
        e.preventDefault();
        {
          const entry = active.currentEntry;
          if (entry && entry.name !== '..') {
            active.toggleSelection(entry.path);
          }
          active.moveCursor(1);
        }
        break;
      case 'F2':
        e.preventDefault();
        handleRename();
        break;
      case 'F3':
        e.preventDefault();
        quickLook();
        break;
      case 'F4':
        e.preventDefault();
        {
          const entry = active.currentEntry;
          if (entry && !entry.is_dir && entry.name !== '..') {
            if (active.backend === 's3' && active.s3Connection) {
              openS3Editor(entry.path, active.s3Connection.connectionId);
            } else {
              openEditor(entry.path);
            }
          }
        }
        break;
      case 'F5':
        e.preventDefault();
        handleCopy();
        break;
      case 'F6':
        if (e.shiftKey) {
          e.preventDefault();
          handleRename();
        } else {
          e.preventDefault();
          handleMove();
        }
        break;
      case 'F7':
        e.preventDefault();
        handleMkDir();
        break;
      case 'F8':
        e.preventDefault();
        handleDelete();
        break;
      case 'F9':
        e.preventDefault();
        handleProperties();
        break;
      case 'F10':
        e.preventDefault();
        handleQuit();
        break;
      default:
        // Quick filter: typing a character appends to active panel filter
        if (e.key.length === 1 && !e.metaKey && !e.altKey && !e.ctrlKey && e.key !== ' ') {
          e.preventDefault();
          active.filterText += e.key;
          active.cursorIndex = Math.min(1, active.filteredSortedEntries.length - 1);
        }
        break;
    }
  }
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
</svelte:head>

<svelte:window onkeydown={handleGlobalKeydown} />

{@render children()}
