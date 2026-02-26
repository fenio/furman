<script lang="ts">
  import { appState } from '$lib/state/app.svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { transfersState } from '$lib/state/transfers.svelte';
  import { getLogPath, openFileDefault } from '$lib/services/tauri';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let activeTab = $state<'general' | 's3'>('general');

  const sizes = [
    { label: 'Small', value: 32 },
    { label: 'Medium', value: 48 },
    { label: 'Large', value: 64 },
    { label: 'Extra Large', value: 96 },
  ];

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  async function openLogFolder() {
    const logPath = await getLogPath();
    await openFileDefault(logPath);
  }

  function toggleShowHidden() {
    appState.setShowHidden(!appState.showHidden);
    panels.left.loadDirectory(panels.left.path);
    panels.right.loadDirectory(panels.right.path);
  }
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Preferences</div>
    <div class="tab-bar">
      <button class="tab-btn" class:active={activeTab === 'general'} onclick={() => { activeTab = 'general'; }}>General</button>
      <button class="tab-btn" class:active={activeTab === 's3'} onclick={() => { activeTab = 's3'; }}>S3</button>
    </div>
    <div class="dialog-body">

      {#if activeTab === 'general'}
        <div class="section-title">Appearance</div>

        <div class="pref-row">
          <span class="pref-label">Theme</span>
          <button class="toggle-btn" onclick={() => appState.toggleTheme()}>
            {appState.theme === 'dark' ? 'Dark' : 'Light'}
          </button>
        </div>

        <div class="pref-row column">
          <span class="pref-label">Icon Size</span>
          <div class="radio-group">
            {#each sizes as s}
              <label class="radio-label" class:active={appState.iconSize === s.value}>
                <input
                  type="radio"
                  name="iconSize"
                  value={s.value}
                  checked={appState.iconSize === s.value}
                  onchange={() => appState.setIconSize(s.value)}
                />
                {s.label}
                <span class="size-hint">{s.value}px</span>
              </label>
            {/each}
          </div>
        </div>

        <div class="section-title">Behavior</div>

        <label class="pref-row checkbox">
          <input
            type="checkbox"
            checked={appState.showHidden}
            onchange={toggleShowHidden}
          />
          Show Hidden Files
        </label>

        <label class="pref-row checkbox">
          <input
            type="checkbox"
            checked={appState.calculateDirSizes}
            onchange={() => appState.setCalculateDirSizes(!appState.calculateDirSizes)}
          />
          Calculate Directory Sizes on Selection
        </label>

        <label class="pref-row checkbox">
          <input
            type="checkbox"
            checked={appState.startupSound}
            onchange={() => appState.setStartupSound(!appState.startupSound)}
          />
          Startup Sound
        </label>

        <div class="pref-row column">
          <span class="pref-label">External Editor</span>
          <input
            class="pref-input"
            type="text"
            autocomplete="off"
            placeholder="e.g. code, vim, subl"
            value={appState.externalEditor}
            oninput={(e) => appState.setExternalEditor((e.target as HTMLInputElement).value)}
          />
          <span class="pref-hint">Leave empty to use built-in editor</span>
        </div>

        <div class="section-title">Diagnostics</div>

        <div class="pref-row">
          <span class="pref-label">Log Files</span>
          <button class="toggle-btn" onclick={openLogFolder}>Open Log Folder</button>
        </div>
        <span class="pref-hint">Share log files when reporting issues</span>
      {/if}

      {#if activeTab === 's3'}
        <div class="section-title">Transfers</div>

        <div class="pref-row">
          <span class="pref-label">Concurrent Transfers</span>
          <select class="pref-select" value={transfersState.maxConcurrent} onchange={(e) => appState.setMaxConcurrent(Number((e.target as HTMLSelectElement).value))}>
            <option value={1}>1</option>
            <option value={2}>2</option>
            <option value={3}>3</option>
            <option value={4}>4</option>
            <option value={5}>5</option>
          </select>
        </div>

        <div class="pref-row column">
          <span class="pref-label">Bandwidth Limit</span>
          <select class="pref-select full-width" value={transfersState.bandwidthLimit} onchange={(e) => appState.setBandwidthLimit(Number((e.target as HTMLSelectElement).value))}>
            <option value={0}>Unlimited</option>
            <option value={131072}>128 KB/s</option>
            <option value={524288}>512 KB/s</option>
            <option value={1048576}>1 MB/s</option>
            <option value={5242880}>5 MB/s</option>
            <option value={10485760}>10 MB/s</option>
            <option value={52428800}>50 MB/s</option>
            <option value={104857600}>100 MB/s</option>
          </select>
        </div>

        <div class="section-title">Security</div>

        <label class="pref-row checkbox">
          <input
            type="checkbox"
            checked={appState.secureTempCleanup}
            onchange={() => appState.setSecureTempCleanup(!appState.secureTempCleanup)}
          />
          Secure Temp Cleanup
        </label>
        <span class="pref-hint">Overwrite encrypted temp files with zeros before deleting</span>
      {/if}

    </div>
    <div class="dialog-footer">
      <button class="dialog-btn primary" onclick={onClose}>Close</button>
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
    width: 72ch;
    height: 85vh;
    max-width: 90vw;
    max-height: 900px;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
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

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--dialog-border);
  }

  .tab-btn {
    padding: 6px 16px;
    font-size: 12px;
    font-family: inherit;
    font-weight: 500;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-btn.active {
    border-bottom: 2px solid var(--text-accent);
    color: var(--text-accent);
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    flex: 1;
    overflow-y: auto;
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

  .pref-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
    color: var(--text-primary);
  }

  .pref-row.column {
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }

  .pref-row.checkbox {
    justify-content: flex-start;
    gap: 8px;
    cursor: pointer;
  }

  .pref-label {
    font-size: 13px;
  }

  .toggle-btn {
    padding: 4px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .toggle-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .radio-group {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    width: 100%;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .radio-label:hover {
    background: var(--bg-hover);
  }

  .radio-label.active {
    border-color: var(--border-active);
    background: rgba(110,168,254,0.1);
    color: var(--text-accent);
  }

  .radio-label input[type="radio"] {
    display: none;
  }

  .size-hint {
    font-size: 10px;
    color: var(--text-secondary);
    opacity: 0.6;
  }

  .pref-input {
    width: 100%;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .pref-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .pref-hint {
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.6;
  }

  .pref-select {
    padding: 4px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .pref-select:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .pref-select.full-width {
    width: 100%;
  }

  .dialog-footer {
    display: flex;
    justify-content: center;
    padding: 16px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
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
    background: rgba(110,168,254,0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110,168,254,0.3);
  }
</style>
