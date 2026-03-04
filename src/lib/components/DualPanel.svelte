<script lang="ts">
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { terminalState } from '$lib/state/terminal.svelte';
  import FilePanel from './FilePanel.svelte';
  import TerminalPanel from './TerminalPanel.svelte';
  import TabBar from './TabBar.svelte';
  import ComparisonBar from './ComparisonBar.svelte';
  import PreviewPane from './PreviewPane.svelte';
  import { comparisonState } from '$lib/state/comparison.svelte';
  import { previewState } from '$lib/state/preview.svelte';
  import DiskUsagePane from './DiskUsagePane.svelte';

  interface Props {
    onEntryActivate?: (index: number) => void;
    onDrop?: (sourceSide: 'left' | 'right', shiftKey: boolean) => void;
  }

  let { onEntryActivate, onDrop }: Props = $props();

  const isSingle = $derived(appState.layoutMode === 'single');
  const isInPane = $derived(terminalState.displayMode === 'in-pane');
  const replaceLeft = $derived(isInPane && terminalState.inPaneSlot === 'left');
  const replaceRight = $derived(isInPane && terminalState.inPaneSlot === 'right');

  // Preview replaces the inactive panel
  const previewReplacesLeft = $derived(previewState.visible && panels.activePanel === 'right');
  const previewReplacesRight = $derived(previewState.visible && panels.activePanel === 'left');

  // Disk usage replaces the inactive panel
  const duReplacesLeft = $derived(appState.diskUsagePanel === 'left');
  const duReplacesRight = $derived(appState.diskUsagePanel === 'right');
</script>

<div class="dual-panel no-select">
  {#if isSingle}
    {#if panels.activePanel === 'left'}
      {#if replaceLeft}
        <div class="in-pane-terminal">
          <TerminalPanel />
        </div>
      {:else}
        <div class="panel-column">
          {#if panels.leftTabs.length > 1}
            <TabBar
              tabs={panels.leftTabs}
              activeIndex={panels.leftActiveTab}
              side="left"
              onSwitch={(i) => panels.switchTab('left', i)}
              onClose={(i) => panels.closeTab('left', i)}
              onAdd={() => { const path = panels.left.path; const tab = panels.addTab('left'); tab.loadDirectory(path); }}
            />
          {/if}
          <FilePanel
            panel={panels.left}
            isActive={true}
            side="left"
            onActivate={() => { panels.activePanel = 'left'; }}
            {onEntryActivate}
            {onDrop}
          />
        </div>
      {/if}
    {:else}
      {#if replaceRight}
        <div class="in-pane-terminal">
          <TerminalPanel />
        </div>
      {:else}
        <div class="panel-column">
          {#if panels.rightTabs.length > 1}
            <TabBar
              tabs={panels.rightTabs}
              activeIndex={panels.rightActiveTab}
              side="right"
              onSwitch={(i) => panels.switchTab('right', i)}
              onClose={(i) => panels.closeTab('right', i)}
              onAdd={() => { const path = panels.right.path; const tab = panels.addTab('right'); tab.loadDirectory(path); }}
            />
          {/if}
          <FilePanel
            panel={panels.right}
            isActive={true}
            side="right"
            onActivate={() => { panels.activePanel = 'right'; }}
            {onEntryActivate}
            {onDrop}
          />
        </div>
      {/if}
    {/if}
    {#if previewState.visible}
      <div class="panel-column">
        <PreviewPane
          entry={panels.active.currentEntry}
          backend={panels.active.backend}
          panelPath={panels.active.path}
          otherEntry={panels.inactive.currentEntry}
          otherBackend={panels.inactive.backend}
        />
      </div>
    {/if}
  {:else}
    <!-- Left slot: terminal > disk-usage > preview > file panel -->
    {#if replaceLeft}
      <div class="in-pane-terminal">
        <TerminalPanel />
      </div>
    {:else if duReplacesLeft}
      <div class="panel-column">
        <DiskUsagePane
          path={appState.diskUsagePath}
          title={appState.diskUsageTitle}
          onClose={() => appState.closeDiskUsage()}
        />
      </div>
    {:else if previewReplacesLeft}
      <div class="panel-column">
        <PreviewPane
          entry={panels.active.currentEntry}
          backend={panels.active.backend}
          panelPath={panels.active.path}
          otherEntry={panels.inactive.currentEntry}
          otherBackend={panels.inactive.backend}
        />
      </div>
    {:else}
      <div class="panel-column">
        {#if panels.leftTabs.length > 1}
          <TabBar
            tabs={panels.leftTabs}
            activeIndex={panels.leftActiveTab}
            side="left"
            onSwitch={(i) => panels.switchTab('left', i)}
            onClose={(i) => panels.closeTab('left', i)}
            onAdd={() => { const path = panels.left.path; const tab = panels.addTab('left'); tab.loadDirectory(path); }}
          />
        {/if}
        {#if comparisonState.active}
          <ComparisonBar />
        {/if}
        <FilePanel
          panel={panels.left}
          isActive={panels.activePanel === 'left'}
          side="left"
          onActivate={() => { panels.activePanel = 'left'; }}
          {onEntryActivate}
          {onDrop}
        />
      </div>
    {/if}
    <!-- Right slot: terminal > disk-usage > preview > file panel -->
    {#if replaceRight}
      <div class="in-pane-terminal">
        <TerminalPanel />
      </div>
    {:else if duReplacesRight}
      <div class="panel-column">
        <DiskUsagePane
          path={appState.diskUsagePath}
          title={appState.diskUsageTitle}
          onClose={() => appState.closeDiskUsage()}
        />
      </div>
    {:else if previewReplacesRight}
      <div class="panel-column">
        <PreviewPane
          entry={panels.active.currentEntry}
          backend={panels.active.backend}
          panelPath={panels.active.path}
          otherEntry={panels.inactive.currentEntry}
          otherBackend={panels.inactive.backend}
        />
      </div>
    {:else}
      <div class="panel-column">
        {#if panels.rightTabs.length > 1}
          <TabBar
            tabs={panels.rightTabs}
            activeIndex={panels.rightActiveTab}
            side="right"
            onSwitch={(i) => panels.switchTab('right', i)}
            onClose={(i) => panels.closeTab('right', i)}
            onAdd={() => { const path = panels.right.path; const tab = panels.addTab('right'); tab.loadDirectory(path); }}
          />
        {/if}
        {#if comparisonState.active}
          <ComparisonBar />
        {/if}
        <FilePanel
          panel={panels.right}
          isActive={panels.activePanel === 'right'}
          side="right"
          onActivate={() => { panels.activePanel = 'right'; }}
          {onEntryActivate}
          {onDrop}
        />
      </div>
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

  .panel-column {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    width: 0;
    min-width: 0;
    min-height: 0;
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
