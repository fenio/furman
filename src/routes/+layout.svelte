<script lang="ts">
  import favicon from '$lib/assets/favicon.svg';
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { panels, s3PathToPrefix } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import { workspacesState } from '$lib/state/workspaces.svelte';
  import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
  import { sftpBookmarksState } from '$lib/state/sftpbookmarks.svelte';
  import { connectionsState } from '$lib/state/connections.svelte';
  import { renameFile, restoreFromTrash } from '$lib/services/tauri';
  import { operationsState } from '$lib/state/operations.svelte';
  import { statusState } from '$lib/state/status.svelte';
  import { transfersState } from '$lib/state/transfers.svelte';
  import { error } from '$lib/services/log';
  import { dragState } from '$lib/services/drag';
  import type { PanelData } from '$lib/state/panels.svelte';
  import type { S3ConnectionInfo, SyncEntry } from '$lib/types';
  import { commandRegistry, type Command } from '$lib/state/commands.svelte';
  import { platform } from '$lib/state/platform.svelte';
  import { comparisonState } from '$lib/state/comparison.svelte';
  import { clipboardState } from '$lib/state/clipboard.svelte';
  import { previewState } from '$lib/state/preview.svelte';

  // ── Extracted action modules ──────────────────────────────────────────────
  import {
    activateEntry, openViewer, openEditor, openS3Viewer, openArchiveViewer,
    openSftpViewer, openS3Editor, openSftpEditor, quickLook,
    systemOpenExtensions,
  } from '$lib/actions/viewers';
  import {
    handleCopy, handleMove, handleDelete, handleRename, handleMkDir,
    handleClipboardPaste,
  } from '$lib/actions/fileops';
  import { executeSyncTransfer } from '$lib/actions/sync';
  import {
    handlePresignUrl, handleCopyS3Uri, handleBulkStorageClassChange,
    handleBucketProperties, handleBookmarkS3, handleBookmarkSftp,
    handleProperties, handleQuit,
  } from '$lib/actions/s3sftp';
  import {
    buildSidebarItems, activateSidebarItem,
    restoreTabsForSide,
  } from '$lib/actions/navigation';

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
    // Disable autocomplete on all inputs globally
    document.addEventListener('focusin', (e) => {
      if (e.target instanceof HTMLInputElement) e.target.autocomplete = 'off';
    });

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
          } else if (target.backend === 'sftp' && target.sftpConnection) {
            const conn = target.sftpConnection;
            transfersState.enqueue({
              id: opId,
              type: 'copy',
              sources: paths,
              destination: target.path,
              srcBackend: 'local',
              destBackend: 'sftp',
              sftpDestConnectionId: conn.connectionId,
              sftpDestPath: target.path,
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

  // ── Command Palette Registry ──────────────────────────────────────────────
  function populateCommandRegistry() {
    const mod = platform.mod;
    const shift = platform.shift;
    const alt = platform.alt;
    const isS3 = () => panels.active.backend === 's3';
    const isSftp = () => panels.active.backend === 'sftp';
    const isLocal = () => panels.active.backend === 'local';

    const cmds: Command[] = [
      // File operations
      { id: 'rename', label: 'Rename', shortcut: `${mod}R`, category: 'File', execute: () => handleRename() },
      { id: 'copy', label: 'Copy to other panel', shortcut: `${mod}C`, category: 'File', execute: () => handleCopy() },
      { id: 'move', label: 'Move to other panel', shortcut: `${mod}M`, category: 'File', execute: () => handleMove() },
      { id: 'delete', label: 'Delete', shortcut: `${mod}⌫`, category: 'File', execute: () => handleDelete() },
      { id: 'mkdir', label: 'Create directory', shortcut: `${mod}N`, category: 'File', execute: () => handleMkDir() },
      { id: 'properties', label: 'Properties', shortcut: `${mod}I`, category: 'File', execute: () => handleProperties() },
      { id: 'view', label: 'View file', shortcut: `${mod}3`, category: 'File', execute: () => quickLook() },
      { id: 'edit', label: 'Edit file', shortcut: `${mod}E`, category: 'File', execute: () => {
        const entry = panels.active.currentEntry;
        if (entry && !entry.is_dir && entry.name !== '..') {
          if (panels.active.backend === 's3' && panels.active.s3Connection) {
            openS3Editor(entry.path, panels.active.s3Connection.connectionId);
          } else if (panels.active.backend === 'sftp' && panels.active.sftpConnection) {
            openSftpEditor(entry.path, panels.active.sftpConnection.connectionId);
          } else {
            openEditor(entry.path);
          }
        }
      }},
      { id: 'select-all', label: 'Select / Deselect all', shortcut: '*', category: 'File', execute: () => {
        const active = panels.active;
        const allCount = active.entries.filter(e => e.name !== '..').length;
        if (active.selectedPaths.size === allCount) active.deselectAll(); else active.selectAll();
      }},
      { id: 'select-by-pattern', label: 'Select by pattern', shortcut: '+', category: 'File', execute: () => {
        appState.showInput('Select by pattern:', '*', (pattern) => {
          appState.closeModal();
          if (pattern) panels.active.selectByPattern(pattern);
        });
      }},
      { id: 'deselect-by-pattern', label: 'Deselect by pattern', shortcut: '\u2212', category: 'File', execute: () => {
        appState.showInput('Deselect by pattern:', '*', (pattern) => {
          appState.closeModal();
          if (pattern) panels.active.deselectByPattern(pattern);
        });
      }},

      // Navigation
      { id: 'go-parent', label: 'Go to parent directory', shortcut: 'Backspace', category: 'Navigation', execute: () => {
        const parentEntry = panels.active.filteredSortedEntries.find((en) => en.name === '..');
        if (parentEntry) {
          const currentDirName = panels.active.path.replace(/\/+$/, '').split('/').pop() ?? '';
          panels.active.loadDirectory(parentEntry.path, currentDirName);
        }
      }},
      { id: 'switch-panel', label: 'Switch active panel', shortcut: 'Tab', category: 'Navigation', execute: () => panels.switchPanel() },
      { id: 'go-home', label: 'Jump to first entry', shortcut: 'Home', category: 'Navigation', execute: () => panels.active.moveCursorTo(0) },
      { id: 'go-end', label: 'Jump to last entry', shortcut: 'End', category: 'Navigation', execute: () => {
        panels.active.moveCursorTo(panels.active.filteredSortedEntries.length - 1);
      }},

      // Panel
      { id: 'toggle-layout', label: 'Toggle single / dual pane', shortcut: `${mod}P`, category: 'Panel', execute: () => appState.toggleLayout() },
      { id: 'toggle-sidebar', label: 'Toggle sidebar', shortcut: `${mod}B`, category: 'Panel', execute: () => {
        if (sidebarState.focused) sidebarState.toggle();
        else if (sidebarState.visible) sidebarState.focus();
        else sidebarState.toggle();
      }},
      { id: 'save-workspace', label: 'Save workspace', shortcut: `${mod}D`, category: 'Panel', execute: () => {
        appState.showInput('Workspace name:', '', (name) => {
          appState.closeModal();
          if (!name) return;
          workspacesState.save({
            name,
            leftPath: panels.left.path,
            rightPath: panels.right.path,
            activePanel: panels.activePanel,
            leftTabs: panels.leftTabs.map(t => t.path),
            rightTabs: panels.rightTabs.map(t => t.path),
            leftActiveTab: panels.leftActiveTab,
            rightActiveTab: panels.rightActiveTab,
          });
        });
      }, enabled: isLocal },
      { id: 'toggle-transfers', label: 'Toggle transfer panel', shortcut: `${mod}J`, category: 'Panel', execute: () => transfersState.toggle() },
      { id: 'sync', label: 'Sync directories', shortcut: `${mod}Y`, category: 'Panel', execute: () => {
        const src = panels.active;
        const dst = panels.inactive;
        if (src.backend !== 'archive' && dst.backend !== 'archive') {
          appState.showSync(
            { backend: src.backend, path: src.path, s3Id: src.s3Connection?.connectionId ?? '' },
            { backend: dst.backend, path: dst.path, s3Id: dst.s3Connection?.connectionId ?? '' },
          );
        }
      }},
      { id: 'new-tab', label: 'New tab', shortcut: `${mod}${alt}T`, category: 'Panel', execute: () => {
        const side = panels.activePanel;
        const path = panels.active.path;
        const tab = panels.addTab(side);
        tab.loadDirectory(path);
      }},
      { id: 'close-tab', label: 'Close tab', shortcut: `${mod}${alt}W`, category: 'Panel', execute: () => {
        const side = panels.activePanel;
        const tabs = side === 'left' ? panels.leftTabs : panels.rightTabs;
        const activeIdx = side === 'left' ? panels.leftActiveTab : panels.rightActiveTab;
        if (tabs.length > 1) panels.closeTab(side, activeIdx);
      }},

      // Terminal
      { id: 'terminal-bottom', label: 'Bottom terminal', shortcut: `${mod}T`, category: 'Terminal', execute: () => terminalState.toggle('bottom') },
      { id: 'terminal-inpane', label: 'In-pane terminal', shortcut: `${mod}${shift}T`, category: 'Terminal', execute: () => {
        terminalState.inPaneSlot = panels.activePanel === 'left' ? 'right' : 'left';
        terminalState.toggle('in-pane');
      }},
      { id: 'terminal-quake', label: 'Quake console', shortcut: `${mod}\``, category: 'Terminal', execute: () => terminalState.toggle('quake') },

      // Display
      { id: 'toggle-theme', label: 'Toggle dark / light theme', shortcut: `${mod}${shift}L`, category: 'Display', execute: () => appState.toggleTheme() },
      { id: 'preferences', label: 'Preferences', category: 'Display', execute: () => appState.showPreferences() },
      { id: 'shortcuts', label: 'Keyboard shortcuts', shortcut: `${mod}/`, category: 'Display', execute: () => { appState.modal = 'shortcuts'; } },

      // Search
      { id: 'search', label: 'Search files', shortcut: `${mod}F`, category: 'Search', execute: () => {
        const active = panels.active;
        if (active.backend === 'local' || active.backend === 's3') {
          appState.showSearch(active.path, active.backend, active.s3Connection?.connectionId ?? '');
        }
      }},

      // Connection
      { id: 'connect', label: 'Connect / Disconnect', shortcut: `${mod}S`, category: 'Connection', execute: () => {
        const active = panels.active;
        if (active.backend === 's3') active.disconnectS3();
        else if (active.backend === 'sftp') active.disconnectSftp();
        else appState.showConnectionManager();
      }},
      { id: 'connection-manager', label: 'Connection Manager', category: 'Connection', execute: () => appState.showConnectionManager() },

      // S3
      { id: 's3-presign', label: 'Generate presigned URL', shortcut: `${mod}U`, category: 'S3', execute: () => handlePresignUrl(), enabled: isS3 },
      { id: 's3-copy-uri', label: 'Copy S3 URI', shortcut: `${mod}K`, category: 'S3', execute: () => handleCopyS3Uri(), enabled: isS3 },
      { id: 's3-storage-class', label: 'Bulk change storage class', shortcut: `${mod}L`, category: 'S3', execute: () => handleBulkStorageClassChange(), enabled: isS3 },
      { id: 's3-bucket-props', label: 'Bucket properties', shortcut: `${mod}${shift}I`, category: 'S3', execute: () => handleBucketProperties(), enabled: isS3 },
      { id: 's3-bookmark', label: 'Bookmark S3 path', shortcut: `${mod}D`, category: 'S3', execute: () => handleBookmarkS3(), enabled: isS3 },
      { id: 'sftp-bookmark', label: 'Bookmark SFTP path', shortcut: `${mod}D`, category: 'S3', execute: () => handleBookmarkSftp(), enabled: isSftp },

      // Undo
      { id: 'undo', label: 'Undo last operation', shortcut: `${mod}Z`, category: 'File', execute: () => executeUndo() },

      // Compare
      { id: 'compare', label: 'Compare directories', shortcut: `${mod}${shift}D`, category: 'Panel', execute: () => {
        if (comparisonState.active) {
          comparisonState.stopComparison();
        } else {
          const left = panels.left;
          const right = panels.right;
          comparisonState.startComparison(
            left.path, left.backend, left.s3Connection?.connectionId ?? '',
            right.path, right.backend, right.s3Connection?.connectionId ?? '',
          );
        }
      }},

      // Quit
      { id: 'quit', label: 'Quit Furman', shortcut: `${mod}Q`, category: 'File', execute: () => handleQuit() },
    ];

    commandRegistry.length = 0;
    commandRegistry.push(...cmds);
  }

  populateCommandRegistry();

  // ── Event handlers ────────────────────────────────────────────────────────

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

  function handleContextAction(e: Event) {
    const key = (e as CustomEvent).detail as string;
    if (key === 'presign') handlePresignUrl();
    else if (key === 'copy-uri') handleCopyS3Uri();
    else if (key === 'bulk-storage') handleBulkStorageClassChange();
  }

  async function executeUndo() {
    const op = operationsState.undo();
    if (!op) return;

    try {
      if (op.type === 'delete' && op.trashItems && op.trashItems.length > 0) {
        await restoreFromTrash(op.trashItems);
        statusState.setMessage(`Restored ${op.trashItems.length} file(s)`);
      } else if (op.type === 'rename' && op.newPath && op.originalName) {
        await renameFile(op.newPath, op.originalName);
        statusState.setMessage(`Renamed back to ${op.originalName}`);
      } else {
        statusState.setMessage('Cannot undo this operation');
        return;
      }
      const reloads: Promise<void>[] = [];
      if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
      if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
      await Promise.all(reloads);
    } catch (err: unknown) {
      error(String(err));
      appState.showAlert('Undo failed: ' + String(err));
    }
  }

  function handleUndoEvent() {
    executeUndo();
  }

  window.addEventListener('undo-last-operation', handleUndoEvent);
  window.addEventListener('sync-execute', handleSyncExecuteEvent);
  window.addEventListener('context-action', handleContextAction);

  function handleTransferDone() {
    const reloads: Promise<void>[] = [];
    if (panels.active.backend !== 'archive') reloads.push(panels.active.loadDirectory(panels.active.path));
    if (panels.inactive.backend !== 'archive') reloads.push(panels.inactive.loadDirectory(panels.inactive.path));
    Promise.all(reloads);
  }

  window.addEventListener('transfer-done', handleTransferDone);

  // ── Native menu action handler ─────────────────────────────────────────
  const alwaysAllowed = new Set([
    'preferences', 'quit', 'toggle-theme', 'shortcuts', 'command-palette', 'github',
  ]);

  async function handleMenuAction(action: string) {
    // Modal guard: skip file-operation actions when a modal is open or xterm is focused
    if (!alwaysAllowed.has(action)) {
      if (isXtermFocused()) return;
      if (appState.modal !== 'none' && appState.modal !== 'volume-selector') return;
    }

    const active = panels.active;

    switch (action) {
      case 'preferences':
        appState.showPreferences();
        break;
      case 'quit':
        handleQuit();
        break;
      case 'mkdir':
        handleMkDir();
        break;
      case 'rename':
        handleRename();
        break;
      case 'delete':
        handleDelete();
        break;
      case 'view':
        quickLook();
        break;
      case 'edit': {
        const entry = active.currentEntry;
        if (entry && !entry.is_dir && entry.name !== '..') {
          if (active.backend === 's3' && active.s3Connection) {
            openS3Editor(entry.path, active.s3Connection.connectionId);
          } else if (active.backend === 'sftp' && active.sftpConnection) {
            openSftpEditor(entry.path, active.sftpConnection.connectionId);
          } else {
            openEditor(entry.path);
          }
        }
        break;
      }
      case 'search':
        if (active.backend === 'local' || active.backend === 's3') {
          appState.showSearch(active.path, active.backend, active.s3Connection?.connectionId ?? '');
        }
        break;
      case 'properties':
        handleProperties();
        break;
      case 'copy':
        handleCopy();
        break;
      case 'move':
        handleMove();
        break;
      case 'clipboard-copy': {
        const paths = active.getSelectedOrCurrent();
        if (paths.length > 0) {
          clipboardState.copy(paths, active.backend, {
            s3ConnectionId: active.s3Connection?.connectionId,
            sftpConnectionId: active.sftpConnection?.connectionId,
          });
          statusState.setMessage(`Copied ${paths.length} item(s) to clipboard`);
        }
        break;
      }
      case 'clipboard-cut': {
        const paths = active.getSelectedOrCurrent();
        if (paths.length > 0) {
          clipboardState.cut(paths, active.backend, {
            s3ConnectionId: active.s3Connection?.connectionId,
            sftpConnectionId: active.sftpConnection?.connectionId,
          });
          statusState.setMessage(`Cut ${paths.length} item(s) to clipboard`);
        }
        break;
      }
      case 'clipboard-paste':
        if (!clipboardState.isEmpty) handleClipboardPaste();
        break;
      case 'select-all': {
        const allCount = active.entries.filter(e => e.name !== '..').length;
        if (active.selectedPaths.size === allCount) active.deselectAll();
        else active.selectAll();
        break;
      }
      case 'undo':
        executeUndo();
        break;
      case 'toggle-sidebar':
        if (sidebarState.focused) sidebarState.toggle();
        else if (sidebarState.visible) sidebarState.focus();
        else sidebarState.toggle();
        break;
      case 'toggle-layout':
        appState.toggleLayout();
        break;
      case 'toggle-preview':
        previewState.toggle();
        break;
      case 'toggle-theme':
        appState.toggleTheme();
        break;
      case 'refresh':
        panels.left.loadDirectory(panels.left.path);
        panels.right.loadDirectory(panels.right.path);
        break;
      case 'swap-panels':
        panels.swapPanels();
        break;
      case 'equal-panels':
        panels.inactive.loadDirectory(active.path);
        break;
      case 'compare':
        if (comparisonState.active) {
          comparisonState.stopComparison();
        } else {
          const left = panels.left;
          const right = panels.right;
          comparisonState.startComparison(
            left.path, left.backend, left.s3Connection?.connectionId ?? '',
            right.path, right.backend, right.s3Connection?.connectionId ?? '',
          );
        }
        break;
      case 'connect':
        if (active.backend === 's3') active.disconnectS3();
        else if (active.backend === 'sftp') active.disconnectSftp();
        else appState.showConnectionManager();
        break;
      case 'go-home':
        try {
          const { homeDir } = await import('@tauri-apps/api/path');
          const home = await homeDir();
          active.loadDirectory(home);
        } catch { /* ignore */ }
        break;
      case 'go-parent': {
        const parentEntry = active.filteredSortedEntries.find((en) => en.name === '..');
        if (parentEntry) {
          const currentDirName = active.path.replace(/\/+$/, '').split('/').pop() ?? '';
          active.loadDirectory(parentEntry.path, currentDirName);
        }
        break;
      }
      case 'history-back':
        active.goBack();
        break;
      case 'history-forward':
        active.goForward();
        break;
      case 'terminal-bottom':
        terminalState.toggle('bottom');
        break;
      case 'terminal-inpane':
        terminalState.inPaneSlot = panels.activePanel === 'left' ? 'right' : 'left';
        terminalState.toggle('in-pane');
        break;
      case 'terminal-quake':
        terminalState.toggle('quake');
        break;
      case 'new-tab': {
        const side = panels.activePanel;
        const path = active.path;
        const tab = panels.addTab(side);
        tab.loadDirectory(path);
        break;
      }
      case 'close-tab': {
        const side = panels.activePanel;
        const tabs = side === 'left' ? panels.leftTabs : panels.rightTabs;
        const activeIdx = side === 'left' ? panels.leftActiveTab : panels.rightActiveTab;
        if (tabs.length > 1) panels.closeTab(side, activeIdx);
        break;
      }
      case 'toggle-transfers':
        transfersState.toggle();
        break;
      case 'sync': {
        const src = panels.active;
        const dst = panels.inactive;
        if (src.backend !== 'archive' && dst.backend !== 'archive') {
          appState.showSync(
            { backend: src.backend, path: src.path, s3Id: src.s3Connection?.connectionId ?? '' },
            { backend: dst.backend, path: dst.path, s3Id: dst.s3Connection?.connectionId ?? '' },
          );
        }
        break;
      }
      case 'shortcuts':
        appState.modal = 'shortcuts';
        break;
      case 'github':
        invoke('open_url', { url: 'https://github.com/fenio/furman' });
        break;
      case 'command-palette':
        appState.showCommandPalette();
        break;
    }
  }

  let menuUnlisten: (() => void) | null = null;
  listen<string>('menu-action', (event) => {
    handleMenuAction(event.payload);
  }).then((fn) => { menuUnlisten = fn; });

  onDestroy(() => {
    dragDropUnlisten?.();
    menuUnlisten?.();
    window.removeEventListener('undo-last-operation', handleUndoEvent);
    window.removeEventListener('sync-execute', handleSyncExecuteEvent);
    window.removeEventListener('transfer-done', handleTransferDone);
    window.removeEventListener('context-action', handleContextAction);
  });

  // ── Keyboard helpers ──────────────────────────────────────────────────────

  function isXtermFocused(): boolean {
    const el = document.activeElement;
    return !!el?.closest('.xterm');
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    const cmd = e.metaKey || e.ctrlKey;

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
    if (appState.modal !== 'none' && appState.modal !== 'volume-selector') {
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

    // Cmd/Ctrl shortcuts not in the native menu (S3-specific, context-dependent)
    if (cmd) {
      switch (e.key) {
        case 'd':
          e.preventDefault();
          if (active.backend === 's3') {
            handleBookmarkS3();                  // Cmd+D = Bookmark S3 path
          } else if (active.backend === 'sftp') {
            handleBookmarkSftp();                // Cmd+D = Bookmark SFTP path
          } else {
            appState.showInput('Workspace name:', '', (name) => {
              appState.closeModal();
              if (!name) return;
              workspacesState.save({
                name,
                leftPath: panels.left.path,
                rightPath: panels.right.path,
                activePanel: panels.activePanel,
                leftTabs: panels.leftTabs.map(t => t.path),
                rightTabs: panels.rightTabs.map(t => t.path),
                leftActiveTab: panels.leftActiveTab,
                rightActiveTab: panels.rightActiveTab,
              });
            });                                  // Cmd+D = Save workspace
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
        case 'I':
          e.preventDefault();
          handleBucketProperties();               // Cmd+Shift+I = Bucket Properties
          return;
      }
    }

    const isIconMode = active.viewMode === 'icon';
    const isColumnMode = active.viewMode === 'column';
    const cols = active.gridColumns;

    switch (e.key) {
      case 'Escape':
        if (comparisonState.active) {
          e.preventDefault();
          comparisonState.stopComparison();
        } else if (active.filterText) {
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
        if (isColumnMode || isIconMode) {
          e.preventDefault();
          // In column mode, cols = rowsPerCol so this jumps between columns
          // In icon mode, cols = 1 so this moves one entry
          const leftDelta = isColumnMode ? cols : 1;
          if (e.shiftKey) {
            active.moveCursor(-leftDelta);
            active.selectRange(active.selectionAnchor, active.cursorIndex);
          } else {
            active.moveCursor(-leftDelta);
            active.selectionAnchor = active.cursorIndex;
          }
        }
        break;
      case 'ArrowRight':
        if (isColumnMode || isIconMode) {
          e.preventDefault();
          const rightDelta = isColumnMode ? cols : 1;
          if (e.shiftKey) {
            active.moveCursor(rightDelta);
            active.selectRange(active.selectionAnchor, active.cursorIndex);
          } else {
            active.moveCursor(rightDelta);
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
      case '*': {
        e.preventDefault();
        const allCount = active.entries.filter(e => e.name !== '..').length;
        if (active.selectedPaths.size === allCount) {
          active.deselectAll();
        } else {
          active.selectAll();
        }
        break;
      }
      case '+':
        e.preventDefault();
        appState.showInput('Select by pattern:', '*', (pattern) => {
          appState.closeModal();
          if (pattern) active.selectByPattern(pattern);
        });
        break;
      case '-':
        e.preventDefault();
        appState.showInput('Deselect by pattern:', '*', (pattern) => {
          appState.closeModal();
          if (pattern) active.deselectByPattern(pattern);
        });
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
            } else if (active.backend === 'sftp' && active.sftpConnection) {
              openSftpEditor(entry.path, active.sftpConnection.connectionId);
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
