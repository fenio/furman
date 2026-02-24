<script lang="ts">
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import type { S3ConnectionInfo } from '$lib/types';

  function handleS3Click() {
    const panel = panels.active;
    appState.showS3Connect(async (bucket, region, endpoint, profile, accessKey, secretKey) => {
      const connectionId = `s3-${Date.now()}`;
      const info: S3ConnectionInfo = { bucket, region, connectionId };
      if (endpoint) info.endpoint = endpoint;
      if (profile) info.profile = profile;
      try {
        await panel.connectS3(info, endpoint, profile, accessKey, secretKey);
      } catch (err: unknown) {
        console.error('S3 connect failed:', err);
      }
    });
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
    const leftPath = panels.left.path;
    const rightPath = panels.right.path;
    panels.left.loadDirectory(rightPath);
    panels.right.loadDirectory(leftPath);
  }

  function equalPanels() {
    const activePath = panels.active.path;
    panels.inactive.loadDirectory(activePath);
  }
</script>

<div class="menu-bar no-select">
  <button class="menu-item" onclick={() => sidebarState.toggle()}>
    {sidebarState.visible ? 'Hide Sidebar' : 'Sidebar'}
  </button>
  <button class="menu-item" onclick={switchToLeft}> Left </button>
  <button class="menu-item" onclick={refreshPanels}> Refresh </button>
  <button class="menu-item" onclick={swapPanels}> Swap </button>
  <button class="menu-item" onclick={equalPanels}> Equal </button>
  <button class="menu-item" onclick={handleS3Click}> S3 </button>
  <button class="menu-item" onclick={switchToRight}> Right </button>
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
