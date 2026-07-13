import { panels, s3PathToPrefix } from '$lib/state/panels.svelte';
import { appState } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { operationsState } from '$lib/state/operations.svelte';
import { transfersState } from '$lib/state/transfers.svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import { clipboardState } from '$lib/state/clipboard.svelte';
import { checkConflicts, deleteFilesUndoable, renameFile, createDirectory } from '$lib/services/tauri';
import { s3RenameObject, s3CreateFolder, s3IsObjectEncrypted, type EncryptionConfig } from '$lib/services/s3';
import { sftpRename, sftpCreateFolder } from '$lib/services/sftp';
import { error } from '$lib/services/log';

// ── Encryption helpers ──────────────────────────────────────────────────────

export function findProfileForConnection(connectionId: string): import('$lib/types').S3Profile | undefined {
  const panel = [panels.left, panels.right].find(
    (p) => p.s3Connection?.connectionId === connectionId,
  );
  if (!panel?.s3Connection) return undefined;
  return connectionsState.s3Profiles.find(
    (p) => p.bucket === panel.s3Connection!.bucket,
  );
}

export function buildEncryptionConfig(profile: import('$lib/types').S3Profile): EncryptionConfig {
  return {
    algorithm: profile.encryptionCipher ?? 'aes-256-gcm',
    kdf_memory_cost: profile.kdfMemoryCost ?? 19456,
    kdf_time_cost: profile.kdfTimeCost ?? 2,
    kdf_parallelism: profile.kdfParallelism ?? 1,
    secure_temp_cleanup: appState.secureTempCleanup,
  };
}

export function shouldAutoEncrypt(sources: string[], profile: import('$lib/types').S3Profile): boolean {
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

export function promptEncryptionPassword(
  callback: (password: string) => void,
  promptText = 'Encryption password:',
) {
  appState.showInput(promptText, '', (pw) => {
    appState.closeModal();
    if (pw) callback(pw);
  }, 'password');
}

// ── Conflict checking ───────────────────────────────────────────────────────

export async function getConflicts(sources: string[], destBackend: string, dest: string): Promise<string[]> {
  if (destBackend === 'local') {
    return await checkConflicts(sources, dest);
  }
  const destNames = new Set(panels.inactive.entries.map((e) => e.name));
  return sources.filter((s) => destNames.has(s.split('/').pop() ?? ''));
}

export function withConflictCheck(
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
    if (appState.confirmOverwrite === 'always') {
      execute(sources);
      return;
    }
    if (appState.confirmOverwrite === 'never') {
      const finalSources = sources.filter((s) => !conflicts.includes(s));
      if (finalSources.length === 0) {
        statusState.setMessage('All files skipped');
        return;
      }
      execute(finalSources);
      return;
    }
    // 'ask' — show dialog (current behavior)
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

// ── Copy / Move execution ───────────────────────────────────────────────────

export function executeCopy(
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
      sftpSrcConnectionId: srcBackend === 'sftp' ? active.sftpConnection?.connectionId : undefined,
      sftpDestConnectionId: destBackend === 'sftp' ? inactive.sftpConnection?.connectionId : undefined,
      sftpDestPath: destBackend === 'sftp' ? dest : undefined,
      encryptionPassword,
      encryptionConfig,
    });
  }
}

export function executeMove(
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
    sftpSrcConnectionId: srcBackend === 'sftp' ? active.sftpConnection?.connectionId : undefined,
    sftpDestConnectionId: destBackend === 'sftp' ? inactive.sftpConnection?.connectionId : undefined,
    sftpDestPath: destBackend === 'sftp' ? dest : undefined,
    encryptionPassword,
    encryptionConfig,
  });
}

// ── User-facing file operations ─────────────────────────────────────────────

export async function handleCopy() {
  const active = panels.active;
  const inactive = panels.inactive;
  const sources = active.getSelectedOrCurrent();
  if (sources.length === 0) return;

  const dest = inactive.path;
  const allNames = sources.map((s) => s.replace(/\/+$/, '').split('/').pop() ?? s);
  const names = allNames.length > 5
    ? allNames.slice(0, 5).join(', ') + ` … and ${allNames.length - 5} more`
    : allNames.join(', ');
  const srcBackend = active.backend;
  const destBackend = inactive.backend;

  appState.showConfirm(`Copy ${sources.length} item(s) to ${dest}?\n${names}`, () => {
    appState.closeModal();

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

export async function handleMove() {
  const active = panels.active;
  const inactive = panels.inactive;
  const sources = active.getSelectedOrCurrent();
  if (sources.length === 0) return;

  const dest = inactive.path;
  const allNames = sources.map((s) => s.replace(/\/+$/, '').split('/').pop() ?? s);
  const names = allNames.length > 5
    ? allNames.slice(0, 5).join(', ') + ` … and ${allNames.length - 5} more`
    : allNames.join(', ');
  const srcBackend = active.backend;
  const destBackend = inactive.backend;

  appState.showConfirm(`Move ${sources.length} item(s) to ${dest}?\n${names}`, () => {
    appState.closeModal();

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

export function handleClipboardPaste() {
  const dest = panels.active.path;
  const destBackend = panels.active.backend;
  const sources = clipboardState.paths;
  const srcBackend = clipboardState.sourceBackend;
  const mode = clipboardState.mode;

  const allNames = sources.map((s) => s.replace(/\/+$/, '').split('/').pop() ?? s);
  const names = allNames.length > 5
    ? allNames.slice(0, 5).join(', ') + ` … and ${allNames.length - 5} more`
    : allNames.join(', ');

  const action = mode === 'cut' ? 'Move' : 'Paste';
  appState.showConfirm(`${action} ${sources.length} item(s) to ${dest}?\n${names}`, () => {
    appState.closeModal();

    if (mode === 'copy') {
      withConflictCheck(sources, dest, destBackend, (finalSources) => {
        const opId = 'clipboard-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
        transfersState.enqueue({
          id: opId,
          type: 'copy',
          sources: finalSources,
          destination: dest,
          srcBackend,
          destBackend,
          s3SrcConnectionId: srcBackend === 's3' ? clipboardState.sourceS3ConnectionId : undefined,
          s3DestConnectionId: destBackend === 's3' ? panels.active.s3Connection?.connectionId : undefined,
          s3DestPrefix: destBackend === 's3' && panels.active.s3Connection
            ? s3PathToPrefix(dest, panels.active.s3Connection.bucket)
            : undefined,
          sftpSrcConnectionId: srcBackend === 'sftp' ? clipboardState.sourceSftpConnectionId : undefined,
          sftpDestConnectionId: destBackend === 'sftp' ? panels.active.sftpConnection?.connectionId : undefined,
          sftpDestPath: destBackend === 'sftp' ? dest : undefined,
        });
      });
    } else {
      withConflictCheck(sources, dest, destBackend, (finalSources) => {
        const opId = 'clipboard-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
        transfersState.enqueue({
          id: opId,
          type: 'move',
          sources: finalSources,
          destination: dest,
          srcBackend,
          destBackend,
          s3SrcConnectionId: srcBackend === 's3' ? clipboardState.sourceS3ConnectionId : undefined,
          s3DestConnectionId: destBackend === 's3' ? panels.active.s3Connection?.connectionId : undefined,
          s3DestPrefix: destBackend === 's3' && panels.active.s3Connection
            ? s3PathToPrefix(dest, panels.active.s3Connection.bucket)
            : undefined,
          sftpSrcConnectionId: srcBackend === 'sftp' ? clipboardState.sourceSftpConnectionId : undefined,
          sftpDestConnectionId: destBackend === 'sftp' ? panels.active.sftpConnection?.connectionId : undefined,
          sftpDestPath: destBackend === 'sftp' ? dest : undefined,
        });
        clipboardState.clear();
      });
    }
  });
}

export async function handleDelete() {
  const active = panels.active;
  const sources = active.getSelectedOrCurrent();
  if (sources.length === 0) return;

  const names = sources.map((s) => s.split('/').pop()).join(', ');

  appState.showConfirm(`Delete ${sources.length} item(s)?\n${names}`, async () => {
    appState.closeModal();
    const fileCount = sources.length;
    const backend = active.backend;
    if ((backend === 's3' && active.s3Connection) || (backend === 'sftp' && active.sftpConnection)) {
      transfersState.enqueue({
        id: 'delete-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6),
        type: 'delete',
        sources,
        destination: active.path,
        srcBackend: backend,
        destBackend: backend,
        s3SrcConnectionId: active.s3Connection?.connectionId,
        sftpSrcConnectionId: active.sftpConnection?.connectionId,
      });
      return;
    }
    try {
      const trashItems = await deleteFilesUndoable(sources);
      operationsState.push({
        id: Date.now().toString(36),
        type: 'delete',
        timestamp: Date.now(),
        backend: 'local',
        trashItems,
        sourcePaths: sources,
        undone: false,
      });
      statusState.setMessage(`Deleted ${fileCount} file(s)`);
      await active.loadDirectory(active.path);
    } catch (err: unknown) {
      error(String(err));
      appState.showAlert('Delete failed: ' + String(err));
      await active.loadDirectory(active.path);
    }
  });
}

export function handleRename() {
  const active = panels.active;
  const entry = active.currentEntry;
  if (!entry || entry.name === '..') return;

  if (active.selectedPaths.size > 1 && active.backend !== 'archive') {
    const entries = active.filteredSortedEntries.filter(
      (e) => e.name !== '..' && active.selectedPaths.has(e.path),
    );
    if (entries.length > 1) {
      appState.showMultiRename(entries, active.backend, {
        s3ConnectionId: active.s3Connection?.connectionId,
        sftpConnectionId: active.sftpConnection?.connectionId,
      });
      return;
    }
  }

  appState.showInput('Rename to:', entry.name, async (newName: string) => {
    appState.closeModal();
    if (!newName || newName === entry.name) return;
    const backend = active.backend;
    const originalName = entry.name;
    const originalPath = entry.path;
    try {
      if (backend === 's3' && active.s3Connection) {
        await s3RenameObject(active.s3Connection.connectionId, entry.path, newName);
      } else if (backend === 'sftp' && active.sftpConnection) {
        await sftpRename(active.sftpConnection.connectionId, entry.path, newName);
      } else {
        await renameFile(entry.path, newName);
      }
      const parent = originalPath.substring(0, originalPath.lastIndexOf('/'));
      const newPath = parent + '/' + newName;
      operationsState.push({
        id: Date.now().toString(36),
        type: 'rename',
        timestamp: Date.now(),
        backend,
        originalPath,
        newPath,
        originalName,
        newName,
        undone: false,
      });
    } catch (err: unknown) {
      error(String(err));
    } finally {
      await active.loadDirectory(active.path);
    }
  });
}

export function handleMkDir() {
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
      } else if (active.backend === 'sftp' && active.sftpConnection) {
        const folderPath = active.path.replace(/\/+$/, '') + '/' + name;
        await sftpCreateFolder(active.sftpConnection.connectionId, folderPath);
      } else {
        const newPath = active.path.replace(/\/+$/, '') + '/' + name;
        await createDirectory(newPath);
      }
    } catch (err: unknown) {
      const raw = err instanceof Error ? err.message : String(err);
      mkdirError = raw.includes('Already exists') ? 'Directory already exists' : raw;
      error(String(err));
    }
    await active.loadDirectory(active.path, mkdirError ? undefined : name);
    if (mkdirError) {
      appState.showAlert(mkdirError);
    }
  });
}
