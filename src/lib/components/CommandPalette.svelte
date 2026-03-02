<script lang="ts">
  import type { Command } from '$lib/state/commands.svelte';

  interface Props {
    commands: Command[];
    onClose: () => void;
  }

  let { commands, onClose }: Props = $props();

  let query = $state('');
  let cursorIndex = $state(0);
  let inputEl: HTMLInputElement | undefined = $state(undefined);
  let resultsEl: HTMLDivElement | undefined = $state(undefined);

  const filteredCommands = $derived.by(() => {
    const enabled = commands.filter((c) => c.enabled?.() !== false);
    if (!query) return enabled;
    const lower = query.toLowerCase();
    return enabled.filter((c) => c.label.toLowerCase().includes(lower));
  });

  $effect(() => {
    inputEl?.focus();
  });

  // Clamp cursor when filtered results change
  $effect(() => {
    const len = filteredCommands.length;
    if (cursorIndex >= len) {
      cursorIndex = Math.max(0, len - 1);
    }
  });

  function scrollCursorIntoView() {
    if (!resultsEl) return;
    const rows = resultsEl.querySelectorAll('.cmd-row');
    const row = rows[cursorIndex] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  }

  function executeCommand(index: number) {
    const cmd = filteredCommands[index];
    if (!cmd) return;
    onClose();
    // Defer execution so the palette closes before the command runs
    requestAnimationFrame(() => cmd.execute());
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      cursorIndex = Math.min(cursorIndex + 1, filteredCommands.length - 1);
      scrollCursorIntoView();
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      cursorIndex = Math.max(cursorIndex - 1, 0);
      scrollCursorIntoView();
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (filteredCommands.length > 0) {
        executeCommand(cursorIndex);
      }
      return;
    }
  }

  function handleInput() {
    cursorIndex = 0;
  }

  const categoryColors: Record<string, string> = {
    'File': 'var(--git-modified)',
    'Navigation': 'var(--text-accent)',
    'Panel': 'var(--git-added)',
    'Terminal': 'var(--git-untracked)',
    'Connection': 'var(--git-renamed)',
    'S3': 'var(--git-conflict)',
    'Search': 'var(--text-accent)',
    'Display': 'var(--git-ignored)',
  };
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="palette-overlay no-select"
  onkeydown={handleKeydown}
  tabindex="0"
  role="dialog"
  aria-modal="true"
  onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) onClose(); }}
>
  <div class="palette-box">
    <input
      bind:this={inputEl}
      bind:value={query}
      oninput={handleInput}
      class="palette-input"
      placeholder="Type a command..."
      spellcheck="false"
      autocomplete="off"
    />
    <div class="palette-results" bind:this={resultsEl}>
      {#each filteredCommands as cmd, i}
        <button
          class="cmd-row"
          class:cursor-active={i === cursorIndex}
          onclick={() => executeCommand(i)}
          onmouseenter={() => { cursorIndex = i; }}
        >
          <span class="cmd-category" style="background: {categoryColors[cmd.category] ?? 'var(--text-secondary)'}">
            {cmd.category}
          </span>
          <span class="cmd-label">{cmd.label}</span>
          {#if cmd.shortcut}
            <span class="cmd-shortcut">{cmd.shortcut}</span>
          {/if}
        </button>
      {:else}
        <div class="cmd-empty">No matching commands</div>
      {/each}
    </div>
  </div>
</div>

<style>
  .palette-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 60px;
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    z-index: 200;
  }

  .palette-box {
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-lg);
    width: 56ch;
    max-width: 90vw;
    max-height: 60vh;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .palette-input {
    width: 100%;
    padding: 12px 16px;
    font-size: 14px;
    font-family: inherit;
    color: var(--text-primary);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--dialog-border);
    outline: none;
    box-sizing: border-box;
  }

  .palette-input::placeholder {
    color: var(--text-secondary);
    opacity: 0.6;
  }

  .palette-results {
    overflow-y: auto;
    max-height: 50vh;
  }

  .cmd-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 16px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .cmd-row:hover,
  .cmd-row.cursor-active {
    background: var(--bg-hover);
  }

  .cmd-category {
    flex: 0 0 auto;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--bg-primary);
    padding: 1px 6px;
    border-radius: 3px;
    min-width: 5ch;
    text-align: center;
  }

  .cmd-label {
    flex: 1 1 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cmd-shortcut {
    flex: 0 0 auto;
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.7;
    font-family: inherit;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: 3px;
    padding: 1px 6px;
  }

  .cmd-empty {
    padding: 16px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 13px;
  }
</style>
