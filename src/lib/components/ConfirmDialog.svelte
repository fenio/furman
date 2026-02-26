<script lang="ts">
  interface Props {
    message: string;
    onConfirm: () => void;
    onCancel: () => void;
    alertOnly?: boolean;
  }

  let { message, onConfirm, onCancel, alertOnly = false }: Props = $props();

  let dialogEl: HTMLDivElement | undefined = $state(undefined);

  $effect(() => {
    if (dialogEl) {
      dialogEl.focus();
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      alertOnly ? onCancel() : onConfirm();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="dialog-overlay no-select"
  onkeydown={handleKeydown}
  tabindex="0"
  bind:this={dialogEl}
  role="dialog"
  aria-modal="true"
>
  <div class="dialog-box">
    <div class="dialog-title">{alertOnly ? 'Error' : 'Confirm'}</div>
    <div class="dialog-body">
      <p class="dialog-message">{message}</p>
      <div class="dialog-buttons">
        {#if alertOnly}
          <button class="dialog-btn primary" onclick={onCancel}>OK</button>
        {:else}
          <button class="dialog-btn primary" onclick={onConfirm}>Yes</button>
          <button class="dialog-btn" onclick={onCancel}>No</button>
        {/if}
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
    margin: 0 0 20px 0;
    text-align: center;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
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
