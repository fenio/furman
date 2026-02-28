<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import { workspacesState } from '$lib/state/workspaces.svelte';
  import { loadConfig } from '$lib/services/config';
  import MenuBar from '$lib/components/MenuBar.svelte';
  import DualPanel from '$lib/components/DualPanel.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import FunctionBar from '$lib/components/FunctionBar.svelte';
  import StatusBar from '$lib/components/StatusBar.svelte';
  import TerminalPanel from '$lib/components/TerminalPanel.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import InputDialog from '$lib/components/InputDialog.svelte';
  import TransferPanel from '$lib/components/TransferPanel.svelte';
  import { initLogging } from '$lib/services/log';
  import Viewer from '$lib/components/Viewer.svelte';
  import Editor from '$lib/components/Editor.svelte';
  import ConnectionManager from '$lib/components/ConnectionManager.svelte';
  import SearchDialog from '$lib/components/SearchDialog.svelte';
  import MenuDropdown from '$lib/components/MenuDropdown.svelte';
  import PreferencesDialog from '$lib/components/PreferencesDialog.svelte';
  import OverwriteDialog from '$lib/components/OverwriteDialog.svelte';
  import PropertiesDialog from '$lib/components/PropertiesDialog.svelte';
  import SyncDialog from '$lib/components/SyncDialog.svelte';
  import ShortcutsDialog from '$lib/components/ShortcutsDialog.svelte';
  import S3BatchEditDialog from '$lib/components/S3BatchEditDialog.svelte';
  import { connectionsState } from '$lib/state/connections.svelte';
  import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
  import { sftpBookmarksState } from '$lib/state/sftpbookmarks.svelte';
  import type { SyncEntry } from '$lib/types';

  let bottomResizing = $state(false);
  let quakeResizing = $state(false);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let reloadLeftTimer: ReturnType<typeof setTimeout> | null = null;
    let reloadRightTimer: ReturnType<typeof setTimeout> | null = null;

    (async () => {
      await initLogging();
      const config = await loadConfig();
      appState.initSettings(config);
      if (appState.startupSound) {
        new Audio('/whip.mp3').play().catch(() => {});
      }

      let homePath = '';
      try {
        const { homeDir } = await import('@tauri-apps/api/path');
        homePath = await homeDir();
      } catch {
        homePath = '';
      }

      sidebarState.loadFavorites(homePath, config.favorites);
      workspacesState.load(config.workspaces);
      connectionsState.load(config.connections);
      s3BookmarksState.load(config.s3Bookmarks);
      sftpBookmarksState.load(config.sftpBookmarks);
      await Promise.all([
        panels.left.loadDirectory(homePath),
        panels.right.loadDirectory(homePath)
      ]);

      // Listen for filesystem changes and debounce-reload affected panels
      unlisten = await listen<{ kind: string; paths: string[] }>('fs-change', (event) => {
        const { paths } = event.payload;
        let needLeft = false;
        let needRight = false;

        for (const p of paths) {
          const lastSlash = p.lastIndexOf('/');
          const parent = lastSlash <= 0 ? '/' : p.substring(0, lastSlash);
          if (parent === panels.left.path) needLeft = true;
          if (parent === panels.right.path) needRight = true;
        }

        if (needLeft && !reloadLeftTimer) {
          reloadLeftTimer = setTimeout(() => {
            reloadLeftTimer = null;
            panels.left.refresh();
          }, 300);
        }
        if (needRight && !reloadRightTimer) {
          reloadRightTimer = setTimeout(() => {
            reloadRightTimer = null;
            panels.right.refresh();
          }, 300);
        }
      });
    })();

    return () => {
      unlisten?.();
      panels.left.stopWatching();
      panels.right.stopWatching();
      if (reloadLeftTimer) clearTimeout(reloadLeftTimer);
      if (reloadRightTimer) clearTimeout(reloadRightTimer);
    };
  });

  const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);

  // Handle entry activation (Enter / double-click) from DualPanel
  async function handleEntryActivate(index: number) {
    const panel = panels.active;
    const entry = panel.filteredSortedEntries[index];
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
      }
    }
    // Non-directory files are handled by Enter key in layout
  }

  // Bottom panel resize
  function startBottomResize(e: MouseEvent) {
    e.preventDefault();
    bottomResizing = true;
    const startY = e.clientY;
    const startHeight = terminalState.bottomPanelHeight;

    function onMove(ev: MouseEvent) {
      const delta = startY - ev.clientY;
      terminalState.bottomPanelHeight = Math.max(100, Math.min(600, startHeight + delta));
    }
    function onUp() {
      bottomResizing = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  // Quake console resize
  function startQuakeResize(e: MouseEvent) {
    e.preventDefault();
    quakeResizing = true;
    const startY = e.clientY;
    const startHeight = terminalState.quakeHeight;

    function onMove(ev: MouseEvent) {
      const delta = ev.clientY - startY;
      const vh = (delta / window.innerHeight) * 100;
      terminalState.quakeHeight = Math.max(15, Math.min(80, startHeight + vh));
    }
    function onUp() {
      quakeResizing = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function handleDrop(sourceSide: 'left' | 'right', shiftKey: boolean) {
    // Set active panel to the drag source so active = source, inactive = drop target
    panels.activePanel = sourceSide;
    // Dispatch synthetic keydown to reuse the existing copy/move flow
    window.dispatchEvent(new KeyboardEvent('keydown', { key: shiftKey ? 'F6' : 'F5' }));
  }

  function handleSearchNavigate(dirPath: string, fileName: string) {
    appState.closeModal();
    panels.active.loadDirectory(dirPath, fileName);
  }

  function handleSyncExecute(entries: SyncEntry[]) {
    // Capture sync state before closeModal() resets it
    const sourceBackend = appState.syncSourceBackend;
    const sourcePath = appState.syncSourcePath;
    const sourceS3Id = appState.syncSourceS3Id;
    const destBackend = appState.syncDestBackend;
    const destPath = appState.syncDestPath;
    const destS3Id = appState.syncDestS3Id;

    appState.closeModal();
    if (entries.length === 0) return;
    // Dispatch custom event for +layout.svelte to handle the actual transfers
    window.dispatchEvent(
      new CustomEvent('sync-execute', {
        detail: {
          entries,
          sourceBackend,
          sourcePath,
          sourceS3Id,
          destBackend,
          destPath,
          destS3Id,
        },
      }),
    );
  }

  function hideQuake(e: KeyboardEvent) {
    if (e.key === 'Escape' && terminalState.displayMode === 'quake') {
      terminalState.displayMode = 'none';
    }
  }
</script>

<div class="app-container">
  <MenuBar />

  {#if appState.menuActive}
    <MenuDropdown />
  {/if}

  <!-- Quake console overlay -->
  {#if terminalState.displayMode === 'quake'}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="quake-console"
      style="height: {terminalState.quakeHeight}vh"
      role="region"
      onkeydown={hideQuake}
    >
      <TerminalPanel />
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="quake-resize-handle" role="separator" onmousedown={startQuakeResize}></div>
    </div>
  {/if}

  <div class="main-content">
    <div class="panels-row">
      <Sidebar />
      <DualPanel onEntryActivate={handleEntryActivate} onDrop={handleDrop} />
    </div>

    {#if terminalState.displayMode === 'bottom'}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div class="bottom-resize-handle" role="separator" onmousedown={startBottomResize}></div>
      <div class="bottom-terminal" style="height: {terminalState.bottomPanelHeight}px">
        <TerminalPanel />
      </div>
    {/if}
  </div><!-- /main-content -->

  <TransferPanel />
  <StatusBar />
  <FunctionBar />

  {#if appState.modal === 'confirm'}
    <ConfirmDialog
      message={appState.confirmMessage}
      alertOnly={appState.confirmAlertOnly}
      onConfirm={() => {
        const cb = appState.confirmCallback;
        if (cb) cb();
      }}
      onCancel={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'input'}
    <InputDialog
      prompt={appState.inputPrompt}
      value={appState.inputValue}
      inputType={appState.inputType}
      onSubmit={(val) => {
        const cb = appState.inputCallback;
        if (cb) cb(val);
      }}
      onCancel={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'viewer'}
    <Viewer
      path={appState.viewerPath}
      mode={appState.viewerMode}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'editor'}
    <Editor
      path={appState.editorPath}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'search'}
    <SearchDialog
      root={appState.searchRoot}
      backend={appState.searchBackend}
      s3ConnectionId={appState.searchS3ConnectionId}
      onNavigate={handleSearchNavigate}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'connection-manager'}
    <ConnectionManager
      initialTab={appState.connectionManagerTab}
      initialData={appState.connectionManagerInitialData}
      onConnect={(bucket, region, endpoint, profile, accessKey, secretKey, provider, customCapabilities, roleArn, externalId, sessionName, sessionDurationSecs, useTransferAcceleration) => {
        const cb = appState.connectCallback;
        appState.closeModal();
        if (cb) cb(bucket, region, endpoint, profile, accessKey, secretKey, provider, customCapabilities);
      }}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'overwrite'}
    <OverwriteDialog
      files={appState.overwriteFiles}
      onOverwrite={() => {
        const cb = appState.overwriteCallback;
        appState.closeModal();
        if (cb) cb('overwrite');
      }}
      onSkip={() => {
        const cb = appState.overwriteCallback;
        appState.closeModal();
        if (cb) cb('skip');
      }}
      onCancel={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'preferences'}
    <PreferencesDialog onClose={() => appState.closeModal()} />
  {/if}

  {#if appState.modal === 'properties'}
    <PropertiesDialog
      path={appState.propertiesPath}
      backend={appState.propertiesBackend}
      s3ConnectionId={appState.propertiesS3ConnectionId}
      sftpConnectionId={appState.propertiesSftpConnectionId}
      sftpConnection={appState.propertiesSftpConnection}
      archiveInfo={appState.propertiesArchiveInfo}
      capabilities={appState.propertiesCapabilities}
      s3Connection={appState.propertiesS3Connection}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'batch-edit'}
    <S3BatchEditDialog
      keys={appState.batchEditKeys}
      s3ConnectionId={appState.batchEditS3ConnectionId}
      capabilities={appState.batchEditCapabilities}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'shortcuts'}
    <ShortcutsDialog onClose={() => appState.closeModal()} />
  {/if}

  {#if appState.modal === 'sync'}
    <SyncDialog
      sourceBackend={appState.syncSourceBackend}
      sourcePath={appState.syncSourcePath}
      sourceS3Id={appState.syncSourceS3Id}
      destBackend={appState.syncDestBackend}
      destPath={appState.syncDestPath}
      destS3Id={appState.syncDestS3Id}
      onSync={handleSyncExecute}
      onClose={() => appState.closeModal()}
    />
  {/if}
</div>

<style>
  .app-container {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-primary);
  }

  .main-content {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    min-height: 0;
  }

  .panels-row {
    display: flex;
    flex-direction: row;
    flex: 1 1 0;
    min-height: 0;
  }

  /* Bottom terminal panel */
  .bottom-terminal {
    flex-shrink: 0;
    border-top: 1px solid var(--border-subtle);
  }

  .bottom-resize-handle {
    height: 3px;
    background: var(--border-subtle);
    cursor: ns-resize;
    flex-shrink: 0;
    border-radius: 2px;
    padding: 1px 0;
  }

  .bottom-resize-handle:hover {
    background: var(--border-active);
  }

  /* Quake console */
  .quake-console {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 150;
    background: var(--bg-primary);
    border-bottom: 2px solid var(--border-active);
    border-radius: 0 0 var(--radius-md) var(--radius-md);
    animation: quake-slide-down 0.25s ease-out;
  }

  @keyframes quake-slide-down {
    from {
      transform: translateY(-100%);
    }
    to {
      transform: translateY(0);
    }
  }

  .quake-resize-handle {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 3px;
    cursor: ns-resize;
    border-radius: 2px;
    padding: 1px 0;
  }

  .quake-resize-handle:hover {
    background: var(--border-active);
  }
</style>
