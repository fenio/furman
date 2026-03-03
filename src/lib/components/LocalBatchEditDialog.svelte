<script lang="ts">
  import { onMount } from 'svelte';
  import { batchChmod, batchTouch } from '$lib/services/tauri';
  import { sftpBatchChmod } from '$lib/services/sftp';
  import { cancelFileOperation } from '$lib/services/tauri';
  import { formatPermissions } from '$lib/utils/format';
  import type { ProgressEvent } from '$lib/types';

  interface Props {
    paths: string[];
    backend: 'local' | 'sftp';
    sftpConnectionId?: string;
    onClose: () => void;
  }

  let { paths, backend, sftpConnectionId, onClose }: Props = $props();

  type TabType = 'permissions' | 'dates';
  let activeTab = $state<TabType>('permissions');

  // Permissions state
  let editMode = $state(0o644);

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
    }
  }

  // Dates state (local only)
  let modifiedDate = $state('');
  let accessedDate = $state('');

  // Operation state
  type Phase = 'edit' | 'progress' | 'done';
  let phase = $state<Phase>('edit');
  let opId = $state('');
  let filesDone = $state(0);
  let filesTotal = $state(0);
  let currentFile = $state('');
  let failedPaths = $state<string[]>([]);
  let showFailedList = $state(false);

  let overlayEl: HTMLDivElement;

  onMount(() => {
    overlayEl?.focus();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (phase === 'progress') {
        handleCancel();
      } else {
        onClose();
      }
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    if (e.target === overlayEl && phase !== 'progress') {
      onClose();
    }
  }

  async function handleCancel() {
    if (opId) {
      try {
        await cancelFileOperation(opId);
      } catch { /* ignore */ }
    }
  }

  function onProgress(e: ProgressEvent) {
    filesDone = e.files_done;
    filesTotal = e.files_total;
    currentFile = e.current_file;
  }

  async function applyChanges() {
    phase = 'progress';
    opId = 'batch-edit-' + Date.now();
    filesDone = 0;
    filesTotal = paths.length;
    failedPaths = [];

    try {
      if (activeTab === 'permissions') {
        let failed: string[];
        if (backend === 'sftp' && sftpConnectionId) {
          failed = await sftpBatchChmod(sftpConnectionId, paths, editMode, onProgress);
        } else {
          failed = await batchChmod(paths, editMode, onProgress);
        }
        failedPaths = failed;
      } else {
        // Dates tab (local only)
        const mod = modifiedDate ? new Date(modifiedDate).getTime() : null;
        const acc = accessedDate ? new Date(accessedDate).getTime() : null;
        if (mod !== null || acc !== null) {
          const failed = await batchTouch(paths, mod, acc, onProgress);
          failedPaths = failed;
        }
      }
    } catch (err: unknown) {
      failedPaths = [String(err)];
    }

    phase = 'done';
  }

  const progressPct = $derived(filesTotal > 0 ? Math.round((filesDone / filesTotal) * 100) : 0);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="dialog-overlay"
  role="dialog"
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
  onclick={handleOverlayClick}
>
  <div class="dialog">
    <div class="dialog-header">
      <h3>Batch Edit — {paths.length} file(s)</h3>
      {#if phase !== 'progress'}
        <button class="close-btn" onclick={onClose}>&times;</button>
      {/if}
    </div>

    {#if phase === 'edit'}
      <!-- Tab bar -->
      <div class="tab-bar">
        <button
          class="tab-btn"
          class:active={activeTab === 'permissions'}
          onclick={() => activeTab = 'permissions'}
        >Permissions</button>
        {#if backend === 'local'}
          <button
            class="tab-btn"
            class:active={activeTab === 'dates'}
            onclick={() => activeTab = 'dates'}
          >Dates</button>
        {/if}
      </div>

      <div class="tab-content">
        {#if activeTab === 'permissions'}
          <div class="perms-section">
            <div class="octal-row">
              <span class="perm-display">{formatPermissions(editMode)}</span>
              <input
                class="octal-input"
                type="text"
                value={octalString()}
                oninput={handleOctalInput}
                maxlength={4}
              />
            </div>
            <div class="rwx-grid">
              {#each ['Owner', 'Group', 'Other'] as rowLabel (rowLabel)}
                <div class="rwx-row">
                  <span class="rwx-label">{rowLabel}</span>
                  {#each permBits.filter((b) => b.row === rowLabel) as pb (pb.bit)}
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
          </div>
        {:else}
          <div class="dates-section">
            <label class="date-field">
              <span>Modified:</span>
              <input type="datetime-local" bind:value={modifiedDate} />
            </label>
            <label class="date-field">
              <span>Accessed:</span>
              <input type="datetime-local" bind:value={accessedDate} />
            </label>
            <p class="hint">Leave blank to keep unchanged.</p>
          </div>
        {/if}
      </div>

      <div class="dialog-footer">
        <button class="btn btn-secondary" onclick={onClose}>Cancel</button>
        <button
          class="btn btn-primary"
          onclick={applyChanges}
          disabled={activeTab === 'dates' && !modifiedDate && !accessedDate}
        >Apply</button>
      </div>

    {:else if phase === 'progress'}
      <div class="progress-section">
        <div class="progress-bar-track">
          <div class="progress-bar-fill" style="width: {progressPct}%"></div>
        </div>
        <div class="progress-text">{filesDone} / {filesTotal}</div>
        <div class="progress-file">{currentFile.split('/').pop()}</div>
        <button class="btn btn-secondary" onclick={handleCancel}>Cancel</button>
      </div>

    {:else}
      <div class="done-section">
        {#if failedPaths.length === 0}
          <p class="success-msg">All {paths.length} file(s) updated successfully.</p>
        {:else}
          <p class="partial-msg">
            {paths.length - failedPaths.length} succeeded, {failedPaths.length} failed.
          </p>
          <button class="btn btn-link" onclick={() => showFailedList = !showFailedList}>
            {showFailedList ? 'Hide' : 'Show'} failed files
          </button>
          {#if showFailedList}
            <ul class="failed-list">
              {#each failedPaths as fp}
                <li>{fp}</li>
              {/each}
            </ul>
          {/if}
        {/if}
        <div class="dialog-footer">
          <button class="btn btn-primary" onclick={onClose}>Close</button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .dialog {
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    width: 420px;
    max-height: 80vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .dialog-header h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .tab-bar {
    display: flex;
    border-bottom: 1px solid var(--border-subtle);
    padding: 0 16px;
  }

  .tab-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 12px;
    padding: 8px 12px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }

  .tab-btn.active {
    color: var(--text-primary);
    border-bottom-color: var(--border-active);
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-content {
    padding: 16px;
  }

  /* Permissions */
  .perms-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .octal-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .perm-display {
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    color: var(--text-secondary);
  }

  .octal-input {
    width: 60px;
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    padding: 4px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .rwx-grid {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .rwx-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .rwx-label {
    font-size: 12px;
    color: var(--text-secondary);
    width: 48px;
  }

  .rwx-checkbox {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-primary);
    padding: 3px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    user-select: none;
  }

  .rwx-checkbox:hover {
    background: var(--bg-hover);
  }

  .rwx-checkbox.checked {
    border-color: var(--border-active);
    background: rgba(110, 168, 254, 0.1);
    color: var(--text-active);
  }

  .rwx-checkbox input[type='checkbox'] {
    display: none;
  }

  /* Dates */
  .dates-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .date-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .date-field input {
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 12px;
  }

  .hint {
    font-size: 11px;
    color: var(--text-tertiary);
    margin: 0;
  }

  /* Footer */
  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .btn {
    font-size: 12px;
    padding: 6px 16px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    border: 1px solid var(--border-subtle);
  }

  .btn-primary {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .btn-primary:hover {
    opacity: 0.9;
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }

  .btn-link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font-size: 12px;
    text-decoration: underline;
  }

  /* Progress */
  .progress-section {
    padding: 24px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .progress-bar-track {
    width: 100%;
    height: 6px;
    background: var(--bg-secondary);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s ease;
  }

  .progress-text {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .progress-file {
    font-size: 11px;
    color: var(--text-tertiary);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Done */
  .done-section {
    padding: 24px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .success-msg {
    font-size: 13px;
    color: var(--text-success, #4caf50);
    margin: 0;
  }

  .partial-msg {
    font-size: 13px;
    color: var(--text-warning, #ff9800);
    margin: 0;
  }

  .failed-list {
    max-height: 120px;
    overflow-y: auto;
    font-size: 11px;
    color: var(--text-secondary);
    margin: 0;
    padding-left: 16px;
    width: 100%;
  }

  .failed-list li {
    word-break: break-all;
  }
</style>
