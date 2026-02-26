<script lang="ts">
  import { sidebarState } from '$lib/state/sidebar.svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { workspacesState } from '$lib/state/workspaces.svelte';
  import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
  import { s3ProfilesState } from '$lib/state/s3profiles.svelte';
  import { keychainGet } from '$lib/services/keychain';
  import { resolveCapabilities } from '$lib/data/s3-providers';
  import { statusState } from '$lib/state/status.svelte';
  import { error } from '$lib/services/log';
  import type { S3ConnectionInfo } from '$lib/types';

  // Compute base offsets for each section so we can derive flat indices in the template
  const favCount = $derived(sidebarState.favorites.length);
  // favorites: 0..favCount-1, then "add current" at favCount
  const wsBase = $derived(favCount + 1);
  const wsCount = $derived(workspacesState.workspaces.length);
  // workspaces: wsBase..wsBase+wsCount-1, then "save current" at wsBase+wsCount
  const bmBase = $derived(wsBase + wsCount + 1);
  const bmCount = $derived(s3BookmarksState.bookmarks.length);
  const volBase = $derived(bmBase + bmCount);
  const volCount = $derived(sidebarState.volumes.length);

  const s3Base = $derived(volBase + volCount);
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
  const themeIdx = $derived(s3Base + s3Connections.length);

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

  function navigateFavorite(path: string) {
    sidebarState.blur();
    panels.active.loadDirectory(path);
  }

  function navigateVolume(mountPoint: string) {
    sidebarState.blur();
    panels.active.loadDirectory(mountPoint);
  }

  function navigateS3(panelSide: 'left' | 'right', bucket: string) {
    sidebarState.blur();
    panels.activePanel = panelSide;
    panels.active.loadDirectory(`s3://${bucket}/`);
  }

  function addCurrentAsFavorite() {
    const path = panels.active.path;
    const name = path.replace(/\/+$/, '').split('/').pop() || path;
    sidebarState.addFavorite(name, path);
  }

  async function navigateWorkspace(ws: { name: string; leftPath: string; rightPath: string; activePanel: 'left' | 'right' }) {
    sidebarState.blur();
    panels.activePanel = ws.activePanel;
    await Promise.all([
      panels.left.loadDirectory(ws.leftPath),
      panels.right.loadDirectory(ws.rightPath),
    ]);
  }

  function saveCurrentWorkspace() {
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
  }

  async function navigateBookmark(bm: { id: string; name: string; profileId: string; path: string }) {
    sidebarState.blur();
    const profile = s3ProfilesState.profiles.find((p) => p.id === bm.profileId);
    if (!profile) {
      statusState.setMessage('S3 profile not found — save the connection as a profile first');
      return;
    }

    const panel = panels.active;

    // Extract bucket from bookmark path (s3://bucket/...)
    const bmBucket = bm.path.replace(/^s3:\/\//, '').split('/')[0];

    // If already connected to the same bucket, just navigate
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
      // connectS3 loads the bucket root; now navigate to the bookmarked path
      if (bm.path !== `s3://${profile.bucket}/`) {
        await panel.loadDirectory(bm.path);
      }
    } catch (err: unknown) {
      error(String(err));
      statusState.setMessage('Failed to connect: ' + String(err));
    }
  }

  function isFocused(idx: number): boolean {
    return sidebarState.focused && sidebarState.focusIndex === idx;
  }
</script>

{#if sidebarState.visible}
  <div class="sidebar no-select" class:kb-active={sidebarState.focused}>
    <!-- Favorites -->
    <div class="section">
      <div class="section-header">FAVORITES</div>
      {#each sidebarState.favorites as fav, i (fav.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="sidebar-item" class:focused={isFocused(i)} onclick={() => navigateFavorite(fav.path)} role="button" tabindex="-1">
          <span class="item-name">{fav.name}</span>
          <button
            class="remove-btn"
            onclick={(e) => { e.stopPropagation(); sidebarState.removeFavorite(fav.path); }}
            title="Remove favorite"
          >&times;</button>
        </div>
      {/each}
      <button class="sidebar-item add-btn" class:focused={isFocused(favCount)} onclick={addCurrentAsFavorite}>
        + Add Current
      </button>
    </div>

    <!-- Workspaces -->
    <div class="section">
      <div class="section-header">WORKSPACES</div>
      {#each workspacesState.workspaces as ws, i (ws.name)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="sidebar-item" class:focused={isFocused(wsBase + i)} onclick={() => navigateWorkspace(ws)} role="button" tabindex="-1">
          <span class="item-name">{ws.name}</span>
          <button
            class="remove-btn"
            onclick={(e) => { e.stopPropagation(); workspacesState.remove(ws.name); }}
            title="Remove workspace"
          >&times;</button>
        </div>
      {/each}
      <button class="sidebar-item add-btn" class:focused={isFocused(wsBase + wsCount)} onclick={saveCurrentWorkspace}>
        + Save Current
      </button>
    </div>

    <!-- S3 Bookmarks -->
    {#if s3BookmarksState.bookmarks.length > 0}
      <div class="section">
        <div class="section-header">S3 BOOKMARKS</div>
        {#each s3BookmarksState.bookmarks as bm, i (bm.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div class="sidebar-item" class:focused={isFocused(bmBase + i)} onclick={() => navigateBookmark(bm)} role="button" tabindex="-1">
            <span class="item-name">{bm.name}</span>
            <button
              class="remove-btn"
              onclick={(e) => { e.stopPropagation(); s3BookmarksState.remove(bm.id); }}
              title="Remove bookmark"
            >&times;</button>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Devices -->
    <div class="section">
      <div class="section-header">DEVICES</div>
      {#if sidebarState.volumesLoading}
        <div class="sidebar-item loading">Loading...</div>
      {:else}
        {#each sidebarState.volumes as vol, i (vol.mount_point)}
          <button class="sidebar-item" class:focused={isFocused(volBase + i)} onclick={() => navigateVolume(vol.mount_point)}>
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
        {#each s3Connections as conn, i (conn.connectionId)}
          <button class="sidebar-item" class:focused={isFocused(s3Base + i)} onclick={() => navigateS3(conn.panel, conn.bucket)}>
            <span class="item-name">{conn.bucket}</span>
            <span class="item-detail">{conn.panel} panel</span>
          </button>
        {/each}
      </div>
    {/if}

    <!-- Theme toggle -->
    <div class="section theme-section">
      <button class="sidebar-item theme-toggle" class:focused={isFocused(themeIdx)} onclick={() => appState.toggleTheme()}>
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

  .sidebar.kb-active {
    border-right: 1px solid var(--border-active);
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
    opacity: 0.9;
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

  .sidebar-item.focused {
    background: var(--cursor-bg);
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

  .sidebar-item:hover .remove-btn,
  .sidebar-item.focused .remove-btn {
    display: block;
  }

  .add-btn {
    color: var(--text-secondary);
    font-size: 12px;
    opacity: 0.9;
    transition: opacity var(--transition-fast);
  }

  .add-btn:hover {
    color: var(--text-primary);
    opacity: 1;
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

  .theme-toggle.focused {
    opacity: 1;
  }

  .theme-icon {
    font-size: 14px;
  }
</style>
