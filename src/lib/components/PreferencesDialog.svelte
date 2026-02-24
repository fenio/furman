<script lang="ts">
  import { appState } from '$lib/state/app.svelte.ts';
  import { panels } from '$lib/state/panels.svelte.ts';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

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
    <div class="dialog-body">

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
          checked={appState.startupSound}
          onchange={() => appState.setStartupSound(!appState.startupSound)}
        />
        Startup Sound
      </label>

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
    min-width: 36ch;
    max-width: 50ch;
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
    gap: 10px;
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
    background: rgba(110,168,254,0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110,168,254,0.3);
  }
</style>
