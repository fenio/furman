<script lang="ts">
  import { onMount } from 'svelte';
  import { panels } from '$lib/state/panels.svelte.ts';
  import { appState } from '$lib/state/app.svelte.ts';
  import { terminalState } from '$lib/state/terminal.svelte.ts';
  import { sidebarState } from '$lib/state/sidebar.svelte.ts';
  import { workspacesState } from '$lib/state/workspaces.svelte.ts';
  import { loadConfig } from '$lib/services/config.ts';
  import MenuBar from '$lib/components/MenuBar.svelte';
  import DualPanel from '$lib/components/DualPanel.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import FunctionBar from '$lib/components/FunctionBar.svelte';
  import StatusBar from '$lib/components/StatusBar.svelte';
  import TerminalPanel from '$lib/components/TerminalPanel.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import InputDialog from '$lib/components/InputDialog.svelte';
  import ProgressDialog from '$lib/components/ProgressDialog.svelte';
  import Viewer from '$lib/components/Viewer.svelte';
  import Editor from '$lib/components/Editor.svelte';
  import S3ConnectDialog from '$lib/components/S3ConnectDialog.svelte';
  import SearchDialog from '$lib/components/SearchDialog.svelte';
  import MenuDropdown from '$lib/components/MenuDropdown.svelte';
  import PreferencesDialog from '$lib/components/PreferencesDialog.svelte';

  let bottomResizing = $state(false);
  let quakeResizing = $state(false);

  onMount(async () => {
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
    await Promise.all([
      panels.left.loadDirectory(homePath),
      panels.right.loadDirectory(homePath)
    ]);
  });

  const archiveExtensions = new Set(['zip', 'rar', '7z']);

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

  <StatusBar />
  <FunctionBar />

  {#if appState.modal === 'confirm'}
    <ConfirmDialog
      message={appState.confirmMessage}
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
      onSubmit={(val) => {
        const cb = appState.inputCallback;
        if (cb) cb(val);
      }}
      onCancel={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'progress'}
    <ProgressDialog progress={appState.progressData} />
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
      onNavigate={handleSearchNavigate}
      onClose={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 's3-connect'}
    <S3ConnectDialog
      onConnect={(bucket, region, endpoint, profile, accessKey, secretKey) => {
        const cb = appState.s3ConnectCallback;
        appState.closeModal();
        if (cb) cb(bucket, region, endpoint, profile, accessKey, secretKey);
      }}
      onCancel={() => appState.closeModal()}
    />
  {/if}

  {#if appState.modal === 'preferences'}
    <PreferencesDialog onClose={() => appState.closeModal()} />
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
