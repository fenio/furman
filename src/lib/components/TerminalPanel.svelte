<script lang="ts">
  import { terminalState } from '$lib/state/terminal.svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { terminalClose } from '$lib/services/tauri';
  import XTerm from './XTerm.svelte';

  function addTerminal() {
    const cwd = panels.active.path || '/';
    terminalState.addInstance(cwd);
  }

  function switchTab(index: number) {
    terminalState.activeIndex = index;
  }

  async function closeTab(id: string, e: MouseEvent) {
    e.stopPropagation();
    await terminalClose(id).catch(() => {});
    terminalState.removeInstance(id);
    if (terminalState.instances.length === 0) {
      terminalState.displayMode = 'none';
    }
  }

  function handleExit(id: string) {
    // Terminal exited naturally — remove it
    terminalState.removeInstance(id);
    if (terminalState.instances.length === 0) {
      terminalState.displayMode = 'none';
    }
  }

  let cwdTimer: ReturnType<typeof setTimeout> | null = null;

  function handleCwdChange(termId: string, cwd: string) {
    // Only sync from the currently visible/active terminal
    if (terminalState.activeInstance?.id !== termId) return;
    if (terminalState.displayMode === 'none') return;

    if (cwdTimer) clearTimeout(cwdTimer);
    cwdTimer = setTimeout(() => {
      // Navigate the active file panel to match the terminal's cwd
      // Skip if already showing this directory to avoid unnecessary re-render
      if (panels.active.path !== cwd) {
        panels.active.loadDirectory(cwd);
      }
    }, 150);
  }
</script>

<div class="terminal-panel">
  <div class="tab-bar">
    {#each terminalState.instances as instance, i}
      <button
        class="tab"
        class:active={i === terminalState.activeIndex}
        onclick={() => switchTab(i)}
      >
        <span class="tab-label">Terminal {i + 1}</span>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <span class="tab-close" role="button" tabindex="-1" onclick={(e) => closeTab(instance.id, e)}>&times;</span>
      </button>
    {/each}
    <button class="tab-add" onclick={addTerminal}>+</button>
  </div>

  <div class="terminal-content">
    {#each terminalState.instances as instance, i (instance.id)}
      <div class="terminal-slot" class:hidden={i !== terminalState.activeIndex}>
        <XTerm
          terminalId={instance.id}
          cwd={instance.cwd || panels.active.path || '/'}
          onExit={handleExit}
          onCwdChange={(cwd) => handleCwdChange(instance.id, cwd)}
        />
      </div>
    {/each}
  </div>
</div>

<style>
  .terminal-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-primary);
  }

  .tab-bar {
    display: flex;
    align-items: center;
    background: var(--bg-header);
    border-bottom: 1px solid var(--border-subtle);
    height: 28px;
    flex-shrink: 0;
    padding: 0 4px;
    gap: 2px;
    user-select: none;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 8px;
    font-size: 12px;
    color: var(--text-secondary);
    border-radius: 3px 3px 0 0;
    cursor: pointer;
    white-space: nowrap;
  }

  .tab.active {
    color: var(--text-primary);
    background: var(--bg-primary);
  }

  .tab:hover {
    color: var(--text-primary);
  }

  .tab-close {
    font-size: 14px;
    line-height: 1;
    opacity: 0.5;
  }

  .tab-close:hover {
    opacity: 1;
  }

  .tab-add {
    padding: 2px 8px;
    font-size: 14px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .tab-add:hover {
    color: var(--text-primary);
  }

  .terminal-content {
    flex: 1 1 0;
    min-height: 0;
    position: relative;
  }

  .terminal-slot {
    position: absolute;
    inset: 0;
  }

  .terminal-slot.hidden {
    visibility: hidden;
    pointer-events: none;
  }
</style>
