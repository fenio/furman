<script lang="ts">
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import FilePanel from './FilePanel.svelte';
  import TerminalPanel from './TerminalPanel.svelte';

  interface Props {
    onEntryActivate?: (index: number) => void;
    onDrop?: (sourceSide: 'left' | 'right', shiftKey: boolean) => void;
  }

  let { onEntryActivate, onDrop }: Props = $props();

  const isSingle = $derived(appState.layoutMode === 'single');
  const isInPane = $derived(terminalState.displayMode === 'in-pane');
  const replaceLeft = $derived(isInPane && terminalState.inPaneSlot === 'left');
  const replaceRight = $derived(isInPane && terminalState.inPaneSlot === 'right');
</script>

<div class="dual-panel no-select">
  {#if isSingle}
    {#if panels.activePanel === 'left'}
      {#if replaceLeft}
        <div class="in-pane-terminal">
          <TerminalPanel />
        </div>
      {:else}
        <FilePanel
          panel={panels.left}
          isActive={true}
          side="left"
          onActivate={() => { panels.activePanel = 'left'; }}
          {onEntryActivate}
          {onDrop}
        />
      {/if}
    {:else}
      {#if replaceRight}
        <div class="in-pane-terminal">
          <TerminalPanel />
        </div>
      {:else}
        <FilePanel
          panel={panels.right}
          isActive={true}
          side="right"
          onActivate={() => { panels.activePanel = 'right'; }}
          {onEntryActivate}
          {onDrop}
        />
      {/if}
    {/if}
  {:else}
    {#if replaceLeft}
      <div class="in-pane-terminal">
        <TerminalPanel />
      </div>
    {:else}
      <FilePanel
        panel={panels.left}
        isActive={panels.activePanel === 'left'}
        side="left"
        onActivate={() => { panels.activePanel = 'left'; }}
        {onEntryActivate}
        {onDrop}
      />
    {/if}
    {#if replaceRight}
      <div class="in-pane-terminal">
        <TerminalPanel />
      </div>
    {:else}
      <FilePanel
        panel={panels.right}
        isActive={panels.activePanel === 'right'}
        side="right"
        onActivate={() => { panels.activePanel = 'right'; }}
        {onEntryActivate}
        {onDrop}
      />
    {/if}
  {/if}
</div>

<style>
  .dual-panel {
    display: flex;
    flex-direction: row;
    flex: 1 1 0;
    min-height: 0;
    gap: 6px;
    padding: 0 6px 6px 6px;
  }

  .in-pane-terminal {
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
    margin: 4px;
  }
</style>
