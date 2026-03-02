<script lang="ts">
  import type { PanelData } from '$lib/state/panels.svelte';

  interface Props {
    tabs: PanelData[];
    activeIndex: number;
    side: 'left' | 'right';
    onSwitch: (index: number) => void;
    onClose: (index: number) => void;
    onAdd: () => void;
  }

  let { tabs, activeIndex, side, onSwitch, onClose, onAdd }: Props = $props();

  function getTabLabel(panel: PanelData): string {
    if (panel.backend === 's3' && panel.s3Connection) {
      const prefix = panel.path.replace(/^s3:\/\/[^/]+\/?/, '').replace(/\/+$/, '');
      return prefix ? prefix.split('/').pop()! : panel.s3Connection.bucket;
    }
    if (panel.backend === 'sftp' && panel.sftpConnection) {
      const segments = panel.path.replace(/\/+$/, '').split('/');
      return segments[segments.length - 1] || panel.sftpConnection.host;
    }
    if (panel.backend === 'archive' && panel.archiveInfo) {
      return panel.archiveInfo.archivePath.split('/').pop() ?? 'Archive';
    }
    const segments = panel.path.replace(/\/+$/, '').split('/');
    return segments[segments.length - 1] || '/';
  }

  function handleAuxClick(e: MouseEvent, index: number) {
    if (e.button === 1) {
      e.preventDefault();
      onClose(index);
    }
  }
</script>

<div class="tab-bar" role="tablist">
  {#each tabs as tab, i (tab.tabId)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="tab"
      class:active={i === activeIndex}
      role="tab"
      tabindex="-1"
      aria-selected={i === activeIndex}
      title={tab.path}
      onclick={() => onSwitch(i)}
      onauxclick={(e) => handleAuxClick(e, i)}
    >
      <span class="tab-label">{getTabLabel(tab)}</span>
      {#if tabs.length > 1}
        <button
          class="tab-close"
          onclick={(e) => { e.stopPropagation(); onClose(i); }}
          title="Close tab"
        >&times;</button>
      {/if}
    </div>
  {/each}
  <button class="tab-add" onclick={onAdd} title="New tab">+</button>
</div>

<style>
  .tab-bar {
    display: flex;
    flex-direction: row;
    align-items: stretch;
    height: 28px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border-subtle);
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
  }

  .tab-bar::-webkit-scrollbar {
    display: none;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 10px;
    font-size: 11px;
    font-family: inherit;
    color: var(--text-secondary);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    white-space: nowrap;
    min-width: 0;
    max-width: 160px;
    flex-shrink: 1;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab:hover {
    color: var(--text-primary);
  }

  .tab.active {
    color: var(--text-accent);
    border-bottom-color: var(--text-accent);
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .tab-close {
    flex-shrink: 0;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    opacity: 0.4;
    transition: opacity var(--transition-fast);
  }

  .tab-close:hover {
    opacity: 1;
    color: var(--error-color);
  }

  .tab-add {
    flex-shrink: 0;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 8px;
    opacity: 0.4;
    transition: opacity var(--transition-fast);
  }

  .tab-add:hover {
    opacity: 1;
    color: var(--text-accent);
  }
</style>
