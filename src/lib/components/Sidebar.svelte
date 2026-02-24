<script lang="ts">
  import { sidebarState } from '$lib/state/sidebar.svelte.ts';
  import { panels } from '$lib/state/panels.svelte.ts';
  import { appState } from '$lib/state/app.svelte.ts';

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }

  // Load volumes when sidebar becomes visible
  $effect(() => {
    if (sidebarState.visible) {
      sidebarState.loadVolumes();
    }
  });

  // Derive S3 connections from panels
  const s3Connections = $derived.by(() => {
    const conns: { panel: 'left' | 'right'; bucket: string; connectionId: string }[] = [];
    if (panels.left.s3Connection) {
      conns.push({
        panel: 'left',
        bucket: panels.left.s3Connection.bucket,
        connectionId: panels.left.s3Connection.connectionId,
      });
    }
    if (panels.right.s3Connection) {
      conns.push({
        panel: 'right',
        bucket: panels.right.s3Connection.bucket,
        connectionId: panels.right.s3Connection.connectionId,
      });
    }
    return conns;
  });

  function navigateFavorite(path: string) {
    panels.active.loadDirectory(path);
  }

  function navigateVolume(mountPoint: string) {
    panels.active.loadDirectory(mountPoint);
  }

  function navigateS3(panelSide: 'left' | 'right', bucket: string) {
    panels.activePanel = panelSide;
    panels.active.loadDirectory(`s3://${bucket}/`);
  }

  function addCurrentAsFavorite() {
    const path = panels.active.path;
    const name = path.replace(/\/+$/, '').split('/').pop() || path;
    sidebarState.addFavorite(name, path);
  }
</script>

{#if sidebarState.visible}
  <div class="sidebar no-select">
    <!-- Favorites -->
    <div class="section">
      <div class="section-header">FAVORITES</div>
      {#each sidebarState.favorites as fav (fav.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="sidebar-item" onclick={() => navigateFavorite(fav.path)} role="button" tabindex="-1">
          <span class="item-name">{fav.name}</span>
          <button
            class="remove-btn"
            onclick={(e) => { e.stopPropagation(); sidebarState.removeFavorite(fav.path); }}
            title="Remove favorite"
          >&times;</button>
        </div>
      {/each}
      <button class="sidebar-item add-btn" onclick={addCurrentAsFavorite}>
        + Add Current
      </button>
    </div>

    <!-- Devices -->
    <div class="section">
      <div class="section-header">DEVICES</div>
      {#if sidebarState.volumesLoading}
        <div class="sidebar-item loading">Loading...</div>
      {:else}
        {#each sidebarState.volumes as vol (vol.mount_point)}
          <button class="sidebar-item" onclick={() => navigateVolume(vol.mount_point)}>
            <span class="item-name">{vol.name || vol.mount_point}</span>
            <span class="item-detail">{formatSize(vol.free_space)} free</span>
          </button>
        {/each}
      {/if}
    </div>

    <!-- S3 Connections (conditional) -->
    {#if s3Connections.length > 0}
      <div class="section">
        <div class="section-header">S3</div>
        {#each s3Connections as conn (conn.connectionId)}
          <button class="sidebar-item" onclick={() => navigateS3(conn.panel, conn.bucket)}>
            <span class="item-name">{conn.bucket}</span>
            <span class="item-detail">{conn.panel} panel</span>
          </button>
        {/each}
      </div>
    {/if}

    <!-- Theme toggle -->
    <div class="section theme-section">
      <button class="sidebar-item theme-toggle" onclick={() => appState.toggleTheme()}>
        {#if appState.theme === 'dark'}
          <span class="theme-icon">☀</span>
          <span class="item-name">Light Mode</span>
        {:else}
          <span class="theme-icon">🌙</span>
          <span class="item-name">Dark Mode</span>
        {/if}
      </button>
    </div>
  </div>
{/if}

<style>
  .sidebar {
    width: 200px;
    min-width: 200px;
    background: var(--bg-primary);
    border-right: 1px solid var(--border-subtle);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .section {
    padding: 8px 0;
  }

  .section:not(:last-child) {
    border-bottom: 1px solid var(--border-subtle);
  }

  .section-header {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    padding: 4px 12px;
    letter-spacing: 0.8px;
    text-transform: uppercase;
    opacity: 0.4;
  }

  .sidebar-item {
    display: flex;
    align-items: center;
    width: calc(100% - 12px);
    padding: 6px 12px;
    margin: 1px 6px;
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
    text-align: left;
    font-size: 13px;
    color: var(--text-primary);
    gap: 4px;
  }

  .sidebar-item:hover {
    background: var(--bg-hover);
  }

  .sidebar-item.loading {
    color: var(--text-secondary);
    cursor: default;
    font-style: italic;
  }

  .item-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-detail {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .remove-btn {
    display: none;
    font-size: 16px;
    line-height: 1;
    color: var(--text-secondary);
    padding: 0 2px;
  }

  .remove-btn:hover {
    color: var(--text-primary);
  }

  .sidebar-item:hover .remove-btn {
    display: block;
  }

  .add-btn {
    color: var(--text-secondary);
    font-size: 12px;
    opacity: 0.5;
    transition: opacity var(--transition-fast);
  }

  .add-btn:hover {
    color: var(--text-primary);
    opacity: 0.8;
  }

  .theme-section {
    margin-top: auto;
  }

  .theme-toggle {
    justify-content: center;
    gap: 6px;
    opacity: 0.6;
    transition: opacity var(--transition-fast), background var(--transition-fast);
  }

  .theme-toggle:hover {
    opacity: 1;
  }

  .theme-icon {
    font-size: 14px;
  }
</style>
