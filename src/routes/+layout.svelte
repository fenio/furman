<script lang="ts">
  import favicon from '$lib/assets/favicon.svg';
  import '../app.css';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import { workspacesState } from '$lib/state/workspaces.svelte';
  import { copyFiles, moveFiles, deleteFiles, renameFile, createDirectory, openFileDefault, openInEditor, checkConflicts, extractArchive, cancelFileOperation } from '$lib/services/tauri';
  import { statusState } from '$lib/state/status.svelte';
  import { s3Download, s3Upload, s3CopyObjects, s3DeleteObjects, s3RenameObject, s3CreateFolder, s3PresignUrl } from '$lib/services/s3';
  import { s3PathToPrefix } from '$lib/state/panels.svelte';
  import { error } from '$lib/services/log';
  import type { ProgressEvent, S3ConnectionInfo } from '$lib/types';

  let { children } = $props();

  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
  const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);
  const systemOpenExtensions = new Set([
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
    'odt', 'ods', 'odp', 'rtf',
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
    appState.modal = 'editor';
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
    execute: (finalSources: string[]) => Promise<void>,
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

  async function executeCopy(
    sources: string[],
    dest: string,
    srcBackend: string,
    destBackend: string,
  ) {
    const active = panels.active;
    const inactive = panels.inactive;
    const opId = 'file-op-' + Date.now();
    appState.showProgress(opId);
    const fileCount = sources.length;
    let cancelled = false;
    try {
      const onProgress = (e: ProgressEvent) => {
        appState.progressData = e;
        const pct = e.bytes_total > 0 ? (e.bytes_done / e.bytes_total) * 100 : 0;
        statusState.setProgress(e.current_file?.split('/').pop() ?? 'Copying...', pct);
      };
      if (srcBackend === 'archive' && destBackend === 'local') {
        const archivePath = active.archiveInfo!.archivePath;
        // Sources are archive://path#internal — extract internal paths
        const internalPaths = sources.map((s) => {
          const hashIdx = s.indexOf('#');
          return hashIdx >= 0 ? s.substring(hashIdx + 1) : s;
        });
        await extractArchive(opId, archivePath, internalPaths, dest, onProgress);
      } else if (srcBackend === 'local' && destBackend === 'local') {
        await copyFiles(opId, sources, dest, onProgress);
      } else if (srcBackend === 's3' && destBackend === 'local') {
        const conn = active.s3Connection!;
        await s3Download(conn.connectionId, opId, sources, dest, onProgress);
      } else if (srcBackend === 'local' && destBackend === 's3') {
        const conn = inactive.s3Connection!;
        const prefix = s3PathToPrefix(dest, conn.bucket);
        await s3Upload(conn.connectionId, opId, sources, prefix, onProgress);
      } else if (srcBackend === 's3' && destBackend === 's3') {
        const srcConn = active.s3Connection!;
        const destConn = inactive.s3Connection!;
        const destPrefix = s3PathToPrefix(dest, destConn.bucket);
        await s3CopyObjects(srcConn.connectionId, opId, sources, destConn.connectionId, destPrefix, onProgress);
      }
    } catch (err: unknown) {
      const msg = String(err);
      if (msg.includes('cancelled')) {
        cancelled = true;
      } else {
        error(String(err));
      }
    } finally {
      appState.closeModal();
      statusState.setMessage(cancelled ? 'Copy cancelled' : `Copied ${fileCount} file(s)`);
      const reloads: Promise<void>[] = [];
      if (active.backend !== 'archive') reloads.push(active.loadDirectory(active.path));
      if (inactive.backend !== 'archive') reloads.push(inactive.loadDirectory(inactive.path));
      await Promise.all(reloads);
    }
  }

  async function executeMove(
    sources: string[],
    dest: string,
    srcBackend: string,
    destBackend: string,
  ) {
    const active = panels.active;
    const inactive = panels.inactive;
    const opId = 'file-op-' + Date.now();
    appState.showProgress(opId);
    const fileCount = sources.length;
    let cancelled = false;
    try {
      const onProgress = (e: ProgressEvent) => {
        appState.progressData = e;
        const pct = e.bytes_total > 0 ? (e.bytes_done / e.bytes_total) * 100 : 0;
        statusState.setProgress(e.current_file?.split('/').pop() ?? 'Moving...', pct);
      };
      if (srcBackend === 'local' && destBackend === 'local') {
        await moveFiles(opId, sources, dest, onProgress);
      } else if (srcBackend === 's3' && destBackend === 'local') {
        const conn = active.s3Connection!;
        await s3Download(conn.connectionId, opId, sources, dest, onProgress);
        await s3DeleteObjects(conn.connectionId, sources);
      } else if (srcBackend === 'local' && destBackend === 's3') {
        const conn = inactive.s3Connection!;
        const prefix = s3PathToPrefix(dest, conn.bucket);
        await s3Upload(conn.connectionId, opId, sources, prefix, onProgress);
        await deleteFiles(sources, false);
      } else if (srcBackend === 's3' && destBackend === 's3') {
        const srcConn = active.s3Connection!;
        const destConn = inactive.s3Connection!;
        const destPrefix = s3PathToPrefix(dest, destConn.bucket);
        await s3CopyObjects(srcConn.connectionId, opId, sources, destConn.connectionId, destPrefix, onProgress);
        await s3DeleteObjects(srcConn.connectionId, sources);
      }
    } catch (err: unknown) {
      const msg = String(err);
      if (msg.includes('cancelled')) {
        cancelled = true;
      } else {
        error(String(err));
      }
    } finally {
      appState.closeModal();
      statusState.setMessage(cancelled ? 'Move cancelled' : `Moved ${fileCount} file(s)`);
      const reloads: Promise<void>[] = [];
      if (active.backend !== 'archive') reloads.push(active.loadDirectory(active.path));
      if (inactive.backend !== 'archive') reloads.push(inactive.loadDirectory(inactive.path));
      await Promise.all(reloads);
    }
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
    appState.showS3Connect(async (bucket, region, endpoint, profile, accessKey, secretKey) => {
      const connectionId = `s3-${Date.now()}`;
      const info: S3ConnectionInfo = { bucket, region, connectionId };
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
        error(String(err));
      } finally {
        await active.loadDirectory(active.path);
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
    if (!entry || entry.name === '..') return;
    appState.showProperties(
      entry.path,
      active.backend,
      active.s3Connection?.connectionId,
    );
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
              openViewer(entry.path, entry.extension);  // Cmd+3 = View (F3)
            }
          }
          return;
        case 'e':
          e.preventDefault();
          {
            const entry = active.currentEntry;
            if (entry && !entry.is_dir && entry.name !== '..') {
              openEditor(entry.path);            // Cmd+E = Edit (F4)
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
        case 'd':
          e.preventDefault();
          handleDelete();                        // Cmd+Delete or Cmd+D = Delete (F8)
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
        case 'i':
          e.preventDefault();
          handleProperties();                    // Cmd+I = Properties (F9)
          return;
        case 'q':
          e.preventDefault();
          handleQuit();                          // Cmd+Q = Quit (F10)
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
        {
          const entry = active.currentEntry;
          if (entry && !entry.is_dir && entry.name !== '..') {
            openViewer(entry.path, entry.extension);
          }
        }
        break;
      case 'F4':
        e.preventDefault();
        {
          const entry = active.currentEntry;
          if (entry && !entry.is_dir && entry.name !== '..') {
            openEditor(entry.path);
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
