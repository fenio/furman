<script lang="ts">
  import { operationsState } from '$lib/state/operations.svelte';

  interface Props {
    onUndo: () => void;
  }

  let { onUndo }: Props = $props();

  const lastOp = $derived(operationsState.history[0]);

  const message = $derived.by(() => {
    if (!lastOp) return '';
    switch (lastOp.type) {
      case 'delete':
        return `Deleted ${lastOp.trashItems?.length ?? lastOp.sourcePaths?.length ?? 0} file(s)`;
      case 'rename':
        return `Renamed ${lastOp.originalName} → ${lastOp.newName}`;
      case 'move':
        return `Moved ${lastOp.sourcePaths?.length ?? 0} file(s)`;
      default:
        return 'Operation completed';
    }
  });

  const canUndo = $derived(
    lastOp && !lastOp.undone && lastOp.backend === 'local'
  );
</script>

{#if operationsState.toastVisible && lastOp}
  <div class="undo-toast" role="status">
    <span class="toast-message">{message}</span>
    {#if canUndo}
      <button class="toast-undo" onclick={onUndo}>Undo</button>
    {/if}
    <button class="toast-dismiss" onclick={() => operationsState.dismissToast()}>×</button>
  </div>
{/if}

<style>
  .undo-toast {
    position: fixed;
    bottom: 52px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-md);
    padding: 8px 16px;
    box-shadow: var(--shadow-dialog);
    z-index: 180;
    font-size: 13px;
    color: var(--text-primary);
    animation: toast-slide-in 0.2s ease-out;
  }

  @keyframes toast-slide-in {
    from {
      transform: translateX(-50%) translateY(20px);
      opacity: 0;
    }
    to {
      transform: translateX(-50%) translateY(0);
      opacity: 1;
    }
  }

  .toast-message {
    white-space: nowrap;
  }

  .toast-undo {
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    color: var(--text-accent);
    border-radius: var(--radius-sm);
    padding: 3px 10px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .toast-undo:hover {
    background: rgba(110, 168, 254, 0.35);
  }

  .toast-dismiss {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 16px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    font-family: inherit;
  }

  .toast-dismiss:hover {
    color: var(--text-primary);
  }
</style>
