<script lang="ts">
  import { onMount } from 'svelte';
  import { appState } from '$lib/state/app.svelte';
  import { getFileProperties, getDirectorySize } from '$lib/services/tauri';
  import { s3HeadObject, s3ChangeStorageClass, s3RestoreObject, s3ListObjectVersions, s3DownloadVersion, s3RestoreVersion, s3DeleteVersion } from '$lib/services/s3';
  import { invoke } from '@tauri-apps/api/core';
  import { formatSize, formatDate, formatPermissions } from '$lib/utils/format';
  import type { FileProperties, S3ObjectProperties, S3ObjectVersion, PanelBackend } from '$lib/types';

  interface Props {
    path: string;
    backend: PanelBackend;
    s3ConnectionId: string;
    onClose: () => void;
  }

  let { path, backend, s3ConnectionId, onClose }: Props = $props();

  let fileProps = $state<FileProperties | null>(null);
  let s3Props = $state<S3ObjectProperties | null>(null);
  let s3IsPrefix = $state(false);
  let loading = $state(true);
  let error = $state('');
  let dirSize = $state<number | null>(null);
  let dirSizeLoading = $state(false);

  // Editable permissions state
  let editMode = $state(0);
  let permsDirty = $state(false);
  let applyingPerms = $state(false);

  const permBits = [
    { label: 'r', bit: 0o400, row: 'Owner' },
    { label: 'w', bit: 0o200, row: 'Owner' },
    { label: 'x', bit: 0o100, row: 'Owner' },
    { label: 'r', bit: 0o040, row: 'Group' },
    { label: 'w', bit: 0o020, row: 'Group' },
    { label: 'x', bit: 0o010, row: 'Group' },
    { label: 'r', bit: 0o004, row: 'Other' },
    { label: 'w', bit: 0o002, row: 'Other' },
    { label: 'x', bit: 0o001, row: 'Other' },
  ];

  function toggleBit(bit: number) {
    editMode = editMode ^ bit;
    permsDirty = true;
  }

  function hasBit(bit: number): boolean {
    return (editMode & bit) !== 0;
  }

  function octalString(): string {
    return '0' + ((editMode >> 6) & 7).toString() + ((editMode >> 3) & 7).toString() + (editMode & 7).toString();
  }

  function handleOctalInput(e: Event) {
    const val = (e.target as HTMLInputElement).value;
    const parsed = parseInt(val, 8);
    if (!isNaN(parsed) && parsed >= 0 && parsed <= 0o777) {
      editMode = parsed;
      permsDirty = true;
    }
  }

  async function applyPermissions() {
    if (!fileProps) return;
    applyingPerms = true;
    try {
      await invoke('set_permissions', { path: fileProps.path, mode: editMode });
      fileProps.permissions = editMode;
      permsDirty = false;
    } catch (err: unknown) {
      error = String(err);
    } finally {
      applyingPerms = false;
    }
  }

  // Storage class management
  const storageClasses = [
    'STANDARD', 'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING',
    'GLACIER', 'DEEP_ARCHIVE', 'GLACIER_IR',
  ];
  let selectedStorageClass = $state('');
  let applyingClass = $state(false);
  let classMessage = $state('');

  const isGlacier = $derived(
    s3Props?.storage_class === 'GLACIER' ||
    s3Props?.storage_class === 'DEEP_ARCHIVE' ||
    s3Props?.storage_class === 'GLACIER_IR'
  );

  // Glacier restore
  let restoreDays = $state(7);
  let restoreTier = $state('Standard');
  let restoringGlacier = $state(false);
  let restoreMessage = $state('');

  // Versioning
  let versionsExpanded = $state(false);
  let versions = $state<S3ObjectVersion[]>([]);
  let versionsLoading = $state(false);
  let versionsError = $state('');
  let versionActionLoading = $state<string | null>(null);

  async function applyStorageClass() {
    if (!s3Props || !selectedStorageClass || selectedStorageClass === s3Props.storage_class) return;
    applyingClass = true;
    classMessage = '';
    try {
      await s3ChangeStorageClass(s3ConnectionId, path, selectedStorageClass);
      s3Props.storage_class = selectedStorageClass;
      classMessage = 'Storage class updated';
    } catch (err: unknown) {
      classMessage = 'Error: ' + String(err);
    } finally {
      applyingClass = false;
    }
  }

  async function restoreFromGlacier() {
    if (!s3Props) return;
    restoringGlacier = true;
    restoreMessage = '';
    try {
      await s3RestoreObject(s3ConnectionId, path, restoreDays, restoreTier);
      restoreMessage = 'Restore initiated';
      // Refresh to get updated restore status
      s3Props = await s3HeadObject(s3ConnectionId, path);
    } catch (err: unknown) {
      restoreMessage = 'Error: ' + String(err);
    } finally {
      restoringGlacier = false;
    }
  }

  async function loadVersions() {
    if (versionsLoading) return;
    versionsExpanded = !versionsExpanded;
    if (!versionsExpanded || versions.length > 0) return;
    versionsLoading = true;
    versionsError = '';
    try {
      versions = await s3ListObjectVersions(s3ConnectionId, path);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionsLoading = false;
    }
  }

  async function handleDownloadVersion(vid: string) {
    versionActionLoading = vid;
    try {
      const tempPath = await s3DownloadVersion(s3ConnectionId, path, vid);
      // Open in viewer
      const { appState: app } = await import('$lib/state/app.svelte');
      app.viewerMode = 'text';
      app.viewerPath = tempPath;
      app.modal = 'viewer';
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  async function handleRestoreVersion(vid: string) {
    if (!confirm(`Restore this version as current? This will overwrite the current object.`)) return;
    versionActionLoading = vid;
    try {
      await s3RestoreVersion(s3ConnectionId, path, vid);
      // Reload versions
      versions = await s3ListObjectVersions(s3ConnectionId, path);
      s3Props = await s3HeadObject(s3ConnectionId, path);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  async function handleDeleteVersion(vid: string) {
    if (!confirm(`Permanently delete this version? This cannot be undone.`)) return;
    versionActionLoading = vid;
    try {
      await s3DeleteVersion(s3ConnectionId, path, vid);
      versions = versions.filter(v => v.version_id !== vid);
    } catch (err: unknown) {
      versionsError = String(err);
    } finally {
      versionActionLoading = null;
    }
  }

  function truncateVid(vid: string): string {
    return vid.length > 16 ? vid.slice(0, 16) + '\u2026' : vid;
  }

  let overlayEl = $state<HTMLDivElement | null>(null);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  onMount(async () => {
    overlayEl?.focus();
    try {
      if (backend === 's3') {
        // S3 "directories" are just prefixes — no real object to head_object
        if (path.endsWith('/')) {
          s3IsPrefix = true;
        } else {
          s3Props = await s3HeadObject(s3ConnectionId, path);
          selectedStorageClass = s3Props.storage_class ?? 'STANDARD';
        }
      } else {
        fileProps = await getFileProperties(path);
        editMode = fileProps.permissions & 0o777;
        if (fileProps.is_dir) {
          dirSizeLoading = true;
          getDirectorySize(fileProps.path)
            .then((size) => {
              dirSize = size;
            })
            .catch(() => {})
            .finally(() => {
              dirSizeLoading = false;
            });
        }
      }
    } catch (err: unknown) {
      error = String(err);
    } finally {
      loading = false;
    }
  });
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Properties</div>
    <div class="dialog-body">
      {#if loading}
        <div class="loading">Loading...</div>
      {:else if error}
        <div class="error">{error}</div>
      {:else if fileProps}
        <!-- Local file/directory properties -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">Name</td><td class="prop-value">{fileProps.name}</td></tr>
            <tr><td class="prop-label">Path</td><td class="prop-value path">{fileProps.path}</td></tr>
            <tr><td class="prop-label">Kind</td><td class="prop-value">{fileProps.kind}</td></tr>
            <tr>
              <td class="prop-label">Size</td>
              <td class="prop-value">
                {#if fileProps.is_dir}
                  {#if dirSizeLoading}
                    Calculating...
                  {:else if dirSize !== null}
                    {formatSize(dirSize)} ({dirSize.toLocaleString()} bytes)
                  {:else}
                    --
                  {/if}
                {:else}
                  {formatSize(fileProps.size)} ({fileProps.size.toLocaleString()} bytes)
                {/if}
              </td>
            </tr>
            <tr><td class="prop-label">Created</td><td class="prop-value">{formatDate(fileProps.created)}</td></tr>
            <tr><td class="prop-label">Modified</td><td class="prop-value">{formatDate(fileProps.modified)}</td></tr>
            <tr><td class="prop-label">Accessed</td><td class="prop-value">{formatDate(fileProps.accessed)}</td></tr>
            <tr><td class="prop-label">Owner</td><td class="prop-value">{fileProps.owner}</td></tr>
            <tr><td class="prop-label">Group</td><td class="prop-value">{fileProps.group}</td></tr>
            {#if fileProps.is_symlink && fileProps.symlink_target}
              <tr><td class="prop-label">Target</td><td class="prop-value path">{fileProps.symlink_target}</td></tr>
            {/if}
          </tbody>
        </table>

        <!-- Permissions editor -->
        <div class="section-title">Permissions</div>
        <div class="perms-section">
          <div class="octal-row">
            <span class="perm-display">{formatPermissions(editMode)}</span>
            <input
              class="octal-input"
              type="text"
              value={octalString()}
              maxlength="4"
              oninput={handleOctalInput}
            />
          </div>
          <div class="rwx-grid">
            {#each ['Owner', 'Group', 'Other'] as rowLabel}
              <div class="rwx-row">
                <span class="rwx-label">{rowLabel}</span>
                {#each permBits.filter((b) => b.row === rowLabel) as pb}
                  <label class="rwx-checkbox" class:checked={hasBit(pb.bit)}>
                    <input
                      type="checkbox"
                      checked={hasBit(pb.bit)}
                      onchange={() => toggleBit(pb.bit)}
                    />
                    {pb.label}
                  </label>
                {/each}
              </div>
            {/each}
          </div>
          {#if permsDirty}
            <button class="dialog-btn apply-btn" onclick={applyPermissions} disabled={applyingPerms}>
              {applyingPerms ? 'Applying...' : 'Apply'}
            </button>
          {/if}
        </div>
      {:else if s3Props}
        <!-- S3 object properties -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">Key</td><td class="prop-value path">{s3Props.key}</td></tr>
            <tr><td class="prop-label">Size</td><td class="prop-value">{formatSize(s3Props.size)} ({s3Props.size.toLocaleString()} bytes)</td></tr>
            <tr><td class="prop-label">Last Modified</td><td class="prop-value">{formatDate(s3Props.modified)}</td></tr>
            <tr><td class="prop-label">Content Type</td><td class="prop-value">{s3Props.content_type ?? '--'}</td></tr>
            <tr><td class="prop-label">ETag</td><td class="prop-value mono">{s3Props.etag ?? '--'}</td></tr>
            {#if s3Props.version_id}
              <tr><td class="prop-label">Version ID</td><td class="prop-value mono">{s3Props.version_id}</td></tr>
            {/if}
          </tbody>
        </table>

        <!-- Storage Class editor -->
        <div class="section-title">Storage Class</div>
        <div class="storage-class-section">
          <div class="sc-row">
            <select class="sc-select" bind:value={selectedStorageClass}>
              {#each storageClasses as sc}
                <option value={sc}>{sc}</option>
              {/each}
            </select>
            <button
              class="dialog-btn apply-btn"
              onclick={applyStorageClass}
              disabled={applyingClass || selectedStorageClass === s3Props.storage_class}
            >
              {applyingClass ? 'Applying...' : 'Apply'}
            </button>
          </div>
          {#if classMessage}
            <div class="sc-message" class:sc-error={classMessage.startsWith('Error')}>{classMessage}</div>
          {/if}
        </div>

        <!-- Glacier restore (only for glacier classes) -->
        {#if isGlacier}
          <div class="section-title">Glacier Restore</div>
          <div class="glacier-section">
            {#if s3Props.restore_status}
              <div class="restore-status">Restore status: {s3Props.restore_status}</div>
            {/if}
            <div class="glacier-row">
              <label class="glacier-label">
                Days:
                <input class="glacier-input" type="number" min="1" max="365" bind:value={restoreDays} />
              </label>
              <label class="glacier-label">
                Tier:
                <select class="glacier-select" bind:value={restoreTier}>
                  <option value="Standard">Standard</option>
                  <option value="Bulk">Bulk</option>
                  <option value="Expedited">Expedited</option>
                </select>
              </label>
              <button class="dialog-btn apply-btn" onclick={restoreFromGlacier} disabled={restoringGlacier}>
                {restoringGlacier ? 'Restoring...' : 'Restore'}
              </button>
            </div>
            {#if restoreMessage}
              <div class="sc-message" class:sc-error={restoreMessage.startsWith('Error')}>{restoreMessage}</div>
            {/if}
          </div>
        {/if}

        <!-- Versions section -->
        <button class="section-title versions-toggle" onclick={loadVersions}>
          Versions {versionsExpanded ? '\u25B4' : '\u25BE'}
        </button>
        {#if versionsExpanded}
          <div class="versions-section">
            {#if versionsLoading}
              <div class="loading">Loading versions...</div>
            {:else if versionsError}
              <div class="error">{versionsError}</div>
            {:else if versions.length === 0}
              <div class="versions-empty">No version history (versioning may not be enabled on this bucket)</div>
            {:else}
              <div class="versions-list">
                {#each versions as ver}
                  <div class="version-row" class:version-latest={ver.is_latest} class:version-delete-marker={ver.is_delete_marker}>
                    <div class="version-info">
                      <span class="version-id mono" title={ver.version_id}>{truncateVid(ver.version_id)}</span>
                      <span class="version-date">{formatDate(ver.modified)}</span>
                      {#if !ver.is_delete_marker}
                        <span class="version-size">{formatSize(ver.size)}</span>
                      {/if}
                      {#if ver.is_latest}
                        <span class="version-badge latest">Latest</span>
                      {/if}
                      {#if ver.is_delete_marker}
                        <span class="version-badge delete-marker">Delete Marker</span>
                      {/if}
                    </div>
                    <div class="version-actions">
                      {#if !ver.is_delete_marker}
                        <button
                          class="version-action-btn"
                          onclick={() => handleDownloadVersion(ver.version_id)}
                          disabled={versionActionLoading === ver.version_id}
                          title="Download this version"
                        >DL</button>
                        {#if !ver.is_latest}
                          <button
                            class="version-action-btn"
                            onclick={() => handleRestoreVersion(ver.version_id)}
                            disabled={versionActionLoading === ver.version_id}
                            title="Restore as current"
                          >Restore</button>
                        {/if}
                      {/if}
                      <button
                        class="version-action-btn danger"
                        onclick={() => handleDeleteVersion(ver.version_id)}
                        disabled={versionActionLoading === ver.version_id}
                        title="Delete this version"
                      >Del</button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {:else if s3IsPrefix}
        <!-- S3 prefix (virtual directory) -->
        <table class="props-table">
          <tbody>
            <tr><td class="prop-label">Prefix</td><td class="prop-value path">{path}</td></tr>
            <tr><td class="prop-label">Kind</td><td class="prop-value">S3 Prefix (virtual directory)</td></tr>
          </tbody>
        </table>
      {/if}

      <div class="dialog-buttons">
        <button class="dialog-btn primary" onclick={onClose}>Close</button>
      </div>
    </div>
  </div>
</div>

<style>
  .dialog-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    z-index: 100;
  }

  .dialog-box {
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-lg);
    min-width: 50ch;
    max-width: 70ch;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .loading, .error {
    text-align: center;
    padding: 20px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .error {
    color: var(--text-error, #ff6b6b);
  }

  .props-table {
    width: 100%;
    border-collapse: collapse;
  }

  .props-table td {
    padding: 4px 0;
    font-size: 13px;
    vertical-align: top;
  }

  .prop-label {
    color: var(--text-secondary);
    width: 110px;
    white-space: nowrap;
    padding-right: 12px;
  }

  .prop-value {
    color: var(--text-primary);
    word-break: break-all;
  }

  .prop-value.path {
    font-size: 12px;
    opacity: 0.85;
  }

  .prop-value.mono {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-secondary);
    opacity: 0.7;
    padding-top: 4px;
  }

  .perms-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .octal-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .perm-display {
    font-family: var(--font-mono, monospace);
    font-size: 14px;
    color: var(--text-primary);
    letter-spacing: 1px;
  }

  .octal-input {
    width: 60px;
    padding: 4px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    text-align: center;
  }

  .octal-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .rwx-grid {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .rwx-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .rwx-label {
    font-size: 12px;
    color: var(--text-secondary);
    width: 50px;
  }

  .rwx-checkbox {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 3px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .rwx-checkbox:hover {
    background: var(--bg-hover);
  }

  .rwx-checkbox.checked {
    border-color: var(--border-active);
    background: rgba(110, 168, 254, 0.1);
    color: var(--text-accent);
  }

  .rwx-checkbox input[type='checkbox'] {
    display: none;
  }

  .apply-btn {
    align-self: flex-start;
    padding: 6px 18px;
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-sm);
    color: var(--text-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .apply-btn:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  .apply-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    margin-top: 8px;
  }

  .dialog-btn {
    padding: 8px 24px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    font-family: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .dialog-btn.primary {
    background: rgba(110, 168, 254, 0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  /* Storage class section */
  .storage-class-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .sc-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .sc-select {
    flex: 1;
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .sc-select:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .sc-message {
    font-size: 12px;
    color: var(--success-color, #4ec990);
  }

  .sc-message.sc-error {
    color: var(--text-error, #ff6b6b);
  }

  /* Glacier section */
  .glacier-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .glacier-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .glacier-label {
    font-size: 12px;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .glacier-input {
    width: 60px;
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    text-align: center;
  }

  .glacier-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .glacier-select {
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .restore-status {
    font-size: 12px;
    color: var(--text-secondary);
    font-style: italic;
  }

  /* Versions section */
  .versions-toggle {
    cursor: pointer;
    background: none;
    border: none;
    text-align: left;
    padding: 4px 0;
    width: 100%;
  }

  .versions-toggle:hover {
    opacity: 1;
  }

  .versions-section {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 4px;
  }

  .versions-empty {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
    padding: 8px;
  }

  .versions-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .version-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    transition: background var(--transition-fast);
  }

  .version-row:hover {
    background: var(--bg-hover);
  }

  .version-row.version-latest {
    background: rgba(110, 168, 254, 0.05);
  }

  .version-row.version-delete-marker {
    opacity: 0.6;
  }

  .version-info {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .version-id {
    font-size: 10px;
    opacity: 0.7;
  }

  .version-date {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .version-size {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .version-badge {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 1px 4px;
    border-radius: 2px;
    white-space: nowrap;
  }

  .version-badge.latest {
    background: rgba(110, 168, 254, 0.2);
    color: var(--text-accent);
  }

  .version-badge.delete-marker {
    background: rgba(255, 107, 107, 0.2);
    color: var(--text-error, #ff6b6b);
  }

  .version-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .version-action-btn {
    padding: 2px 6px;
    font-size: 10px;
    font-family: inherit;
    border: 1px solid var(--border-subtle);
    border-radius: 2px;
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .version-action-btn:hover {
    background: var(--bg-hover);
    border-color: var(--border-active);
  }

  .version-action-btn.danger:hover {
    border-color: var(--text-error, #ff6b6b);
    color: var(--text-error, #ff6b6b);
  }

  .version-action-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
