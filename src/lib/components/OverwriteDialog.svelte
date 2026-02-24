<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    files: string[];
    onOverwrite: () => void;
    onSkip: () => void;
    onCancel: () => void;
  }

  let { files, onOverwrite, onSkip, onCancel }: Props = $props();

  let btnOverwrite: HTMLButtonElement | undefined = $state(undefined);
  let btnSkip: HTMLButtonElement | undefined = $state(undefined);
  let btnCancel: HTMLButtonElement | undefined = $state(undefined);
  let focusIdx = $state(0);

  function getBtns() {
    return [btnOverwrite, btnSkip, btnCancel].filter(Boolean) as HTMLButtonElement[];
  }

  function focusCurrent() {
    const btns = getBtns();
    btns[focusIdx]?.focus();
  }

  onMount(() => {
    focusCurrent();

    function onKey(e: KeyboardEvent) {
      const btns = getBtns();
      if (e.key === 'Tab') {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.shiftKey) {
          focusIdx = focusIdx <= 0 ? btns.length - 1 : focusIdx - 1;
        } else {
          focusIdx = focusIdx >= btns.length - 1 ? 0 : focusIdx + 1;
        }
        focusCurrent();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        e.stopImmediatePropagation();
        btns[focusIdx]?.click();
      } else if (e.key === 's' || e.key === 'S') {
        e.preventDefault();
        e.stopImmediatePropagation();
        onSkip();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        onCancel();
      }
    }

    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
>
  <div class="dialog-box">
    <div class="dialog-title">File Conflict</div>
    <div class="dialog-body">
      <p class="dialog-message">
        {files.length} file(s) already exist at destination:
      </p>
      <div class="file-list">
        {#each files as f}
          <div class="file-item">{f}</div>
        {/each}
      </div>
      <div class="dialog-buttons">
        <button class="dialog-btn danger" bind:this={btnOverwrite} onclick={onOverwrite}>Overwrite</button>
        <button class="dialog-btn primary" bind:this={btnSkip} onclick={onSkip}>Skip <span class="key-hint">S</span></button>
        <button class="dialog-btn" bind:this={btnCancel} onclick={onCancel}>Cancel <span class="key-hint">Esc</span></button>
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
    max-width: 80ch;
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
  }

  .dialog-message {
    color: var(--text-primary);
    margin: 0 0 12px 0;
    text-align: center;
    font-size: 13px;
  }

  .file-list {
    max-height: 200px;
    overflow-y: auto;
    margin-bottom: 16px;
    padding: 8px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .file-item {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 3px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    gap: 10px;
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

  .dialog-btn:focus {
    outline: 2px solid var(--border-active);
    outline-offset: 2px;
  }

  .dialog-btn.primary {
    background: rgba(110,168,254,0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110,168,254,0.3);
  }

  .dialog-btn.danger {
    background: rgba(255,100,100,0.15);
    border-color: rgba(255,100,100,0.4);
    color: var(--warning-color);
  }

  .dialog-btn.danger:hover {
    background: rgba(255,100,100,0.25);
    border-color: rgba(255,100,100,0.6);
  }

  .key-hint {
    font-size: 10px;
    opacity: 0.5;
    margin-left: 4px;
  }
</style>
