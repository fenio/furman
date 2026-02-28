<script lang="ts">
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';

  function handleConnectClick() {
    appState.showConnectionManager();
  }

  function switchToLeft() {
    panels.activePanel = 'left';
  }

  function switchToRight() {
    panels.activePanel = 'right';
  }

  function refreshPanels() {
    panels.left.loadDirectory(panels.left.path);
    panels.right.loadDirectory(panels.right.path);
  }

  function swapPanels() {
    panels.swapPanels();
  }

  function equalPanels() {
    const activePath = panels.active.path;
    panels.inactive.loadDirectory(activePath);
  }

  const isSingle = $derived(appState.layoutMode === 'single');
</script>

<div class="menu-bar no-select">
  <button class="menu-item" onclick={() => sidebarState.toggle()}>
    {sidebarState.visible ? 'Hide Sidebar' : 'Sidebar'}
  </button>
  {#if !isSingle}
    <button class="menu-item" onclick={switchToLeft}> Left </button>
  {/if}
  <button class="menu-item" onclick={refreshPanels}> Refresh </button>
  {#if !isSingle}
    <button class="menu-item" onclick={swapPanels}> Swap </button>
    <button class="menu-item" onclick={equalPanels}> Equal </button>
  {/if}
  <button class="menu-item" onclick={() => appState.toggleLayout()}>
    {isSingle ? 'Dual' : 'Single'}
  </button>
  <button class="menu-item" onclick={handleConnectClick}> Connect </button>
  {#if !isSingle}
    <button class="menu-item" onclick={switchToRight}> Right </button>
  {/if}
  <div class="spacer"></div>
  <button class="menu-item" onclick={() => { appState.menuActive = !appState.menuActive; }}> Menu </button>
</div>

<style>
  .menu-bar {
    display: flex;
    flex-direction: row;
    background: var(--menu-bg);
    color: var(--menu-text);
    flex: 0 0 auto;
    height: 36px;
    line-height: 36px;
    padding: 0 8px;
    gap: 2px;
    border-bottom: none;
    box-shadow: 0 1px 0 var(--border-subtle);
    font-size: 13px;
    -webkit-app-region: drag;
  }

  .menu-item {
    cursor: pointer;
    padding: 2px 12px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast), opacity var(--transition-fast);
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    line-height: inherit;
    opacity: 0.8;
    -webkit-app-region: no-drag;
  }

  .menu-item:hover {
    background: var(--bg-hover);
    opacity: 1;
  }

  .spacer {
    flex: 1 1 0;
    -webkit-app-region: drag;
  }
</style>
