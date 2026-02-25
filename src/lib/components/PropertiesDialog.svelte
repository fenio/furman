<script lang="ts">
  import { onMount } from 'svelte';
  import { appState } from '$lib/state/app.svelte';
  import { getFileProperties, getDirectorySize } from '$lib/services/tauri';
  import { s3HeadObject } from '$lib/services/s3';
  import { invoke } from '@tauri-apps/api/core';
  import { formatSize, formatDate, formatPermissions } from '$lib/utils/format';
  import type { FileProperties, S3ObjectProperties, PanelBackend } from '$lib/types';

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

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  onMount(async () => {
    try {
      if (backend === 's3') {
        // S3 "directories" are just prefixes — no real object to head_object
        if (path.endsWith('/')) {
          s3IsPrefix = true;
        } else {
          s3Props = await s3HeadObject(s3ConnectionId, path);
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
            <tr><td class="prop-label">Storage Class</td><td class="prop-value">{s3Props.storage_class ?? '--'}</td></tr>
          </tbody>
        </table>
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
</style>
