<script lang="ts">
  import { terminalState } from '$lib/state/terminal.svelte';
  import { platform } from '$lib/state/platform.svelte';

  interface FnKey {
    key: string;
    label: string;
    shortcut: string;
    custom?: () => void;
  }

  const fnKeys: FnKey[] = [
    { key: '1', label: 'Term', shortcut: `${platform.mod}T`, custom: () => terminalState.toggle('bottom') },
    { key: '2', label: 'Rename', shortcut: `${platform.mod}R` },
    { key: '3', label: 'View', shortcut: `${platform.mod}3` },
    { key: '4', label: 'Edit', shortcut: `${platform.mod}E` },
    { key: '5', label: 'Copy', shortcut: `${platform.mod}C` },
    { key: '6', label: 'Move', shortcut: `${platform.mod}M` },
    { key: '7', label: 'MkDir', shortcut: `${platform.mod}N` },
    { key: '8', label: 'Delete', shortcut: `${platform.mod}D` },
    { key: '9', label: 'Props', shortcut: `${platform.mod}I` },
    { key: '10', label: 'Quit', shortcut: `${platform.mod}Q` }
  ];

  function handleClick(fk: FnKey) {
    if (fk.custom) {
      fk.custom();
    } else {
      // Dispatch a synthetic F-key event so the global handler in +layout.svelte picks it up
      window.dispatchEvent(new KeyboardEvent('keydown', { key: `F${fk.key}`, bubbles: true }));
    }
  }
</script>

<div class="function-bar no-select">
  {#each fnKeys as fk (fk.key)}
    <button class="fn-btn" onclick={() => handleClick(fk)}>
      <span class="fn-label">{fk.label}</span>
      {#if fk.shortcut}
        <span class="fn-shortcut">{fk.shortcut}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .function-bar {
    display: flex;
    flex-direction: row;
    background: var(--fn-bar-bg);
    flex: 0 0 auto;
    padding: 6px 8px;
    gap: 4px;
    border-top: 1px solid var(--border-subtle);
  }

  .fn-btn {
    flex: 1 1 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    background: var(--fn-btn-bg);
    border: 1px solid var(--fn-btn-border);
    border-radius: var(--radius-sm);
    padding: 2px 6px;
    cursor: pointer;
    height: 30px;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .fn-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .fn-btn:active {
    background: var(--cursor-bg);
  }

  .fn-label {
    color: var(--fn-bar-text);
    font-size: 13px;
  }

  .fn-shortcut {
    color: var(--text-secondary);
    font-size: 10px;
    opacity: 0.9;
  }
</style>
