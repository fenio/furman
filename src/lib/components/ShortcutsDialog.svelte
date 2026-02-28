<script lang="ts">
  import { platform } from '$lib/state/platform.svelte';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let dialogEl: HTMLDivElement | undefined = $state(undefined);
  let activeTab = $state('general');

  $effect(() => {
    if (dialogEl) {
      dialogEl.focus();
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  const tabs = [
    {
      id: 'general',
      label: 'General',
      groups: [
        {
          title: 'Navigation',
          shortcuts: [
            { keys: '\u2191 \u2193 \u2190 \u2192', desc: 'Move cursor' },
            { keys: 'Home / End', desc: 'Jump to first / last' },
            { keys: 'Page Up / Down', desc: 'Scroll by page' },
            { keys: 'Enter', desc: 'Open file or enter directory' },
            { keys: 'Backspace', desc: 'Go to parent directory' },
            { keys: 'Tab', desc: 'Switch active panel' },
            { keys: 'Space', desc: 'Toggle select and move down' },
          ],
        },
        {
          title: 'Selection',
          shortcuts: [
            { keys: 'Shift + Arrow', desc: 'Extend selection' },
            { keys: 'Shift + Home / End', desc: 'Select to start / end' },
            { keys: 'Insert', desc: 'Toggle select and move down' },
            { keys: 'Type any letter', desc: 'Quick filter' },
            { keys: 'Esc', desc: 'Clear filter' },
          ],
        },
      ],
    },
    {
      id: 'files',
      label: 'Files',
      groups: [
        {
          title: 'File Operations',
          shortcuts: [
            { keys: `F2 / ${platform.mod}R`, desc: 'Rename' },
            { keys: `F3 / ${platform.mod}3`, desc: 'View file' },
            { keys: `F4 / ${platform.mod}E`, desc: 'Edit file' },
            { keys: `F5 / ${platform.mod}C`, desc: 'Copy to other panel' },
            { keys: `F6 / ${platform.mod}M`, desc: 'Move to other panel' },
            { keys: 'Shift+F6', desc: 'Rename (alt)' },
            { keys: `F7 / ${platform.mod}N`, desc: 'Create directory' },
            { keys: `F8 / ${platform.mod}${platform.backspace}`, desc: 'Delete' },
            { keys: `F9 / ${platform.mod}I`, desc: 'Properties' },
            { keys: `F10 / ${platform.mod}Q`, desc: 'Quit' },
          ],
        },
      ],
    },
    {
      id: 'panels',
      label: 'Panels',
      groups: [
        {
          title: 'Layout',
          shortcuts: [
            { keys: `${platform.mod}P`, desc: 'Toggle single / dual pane' },
            { keys: `${platform.mod}B`, desc: 'Toggle sidebar' },
            { keys: `${platform.mod}D`, desc: 'Save workspace' },
            { keys: `${platform.mod}J`, desc: 'Toggle transfer panel' },
            { keys: `${platform.mod}Y`, desc: 'Sync directories' },
            { keys: `${platform.mod}${platform.shift}L`, desc: 'Toggle dark / light theme' },
          ],
        },
        {
          title: 'Terminal',
          shortcuts: [
            { keys: `${platform.mod}T`, desc: 'Bottom terminal' },
            { keys: `${platform.mod}${platform.shift}T`, desc: 'In-pane terminal' },
            { keys: `${platform.mod}\``, desc: 'Quake console' },
            { keys: 'Esc', desc: 'Hide quake console' },
          ],
        },
        {
          title: 'Search',
          shortcuts: [
            { keys: `${platform.mod}F`, desc: 'Search files' },
            { keys: `${platform.mod}/`, desc: 'This cheatsheet' },
          ],
        },
      ],
    },
    {
      id: 's3',
      label: 'S3',
      groups: [
        {
          title: 'S3 Operations',
          shortcuts: [
            { keys: `${platform.mod}S`, desc: 'Connect / disconnect S3' },
            { keys: `${platform.mod}${platform.shift}I`, desc: 'Bucket properties' },
            { keys: `${platform.mod}D`, desc: 'Bookmark S3 path' },
            { keys: `${platform.mod}U`, desc: 'Presigned URL' },
            { keys: `${platform.mod}K`, desc: 'Copy S3 URI' },
            { keys: `${platform.mod}L`, desc: 'Bulk storage class change' },
          ],
        },
      ],
    },
  ];

  const currentTab = $derived(tabs.find((t) => t.id === activeTab)!);
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
    <div class="dialog-title">Keyboard Shortcuts</div>
    <div class="tab-bar">
      {#each tabs as tab}
        <button
          class="tab-btn"
          class:active={activeTab === tab.id}
          onclick={() => { activeTab = tab.id; }}
        >{tab.label}</button>
      {/each}
    </div>
    <div class="dialog-body">
      {#each currentTab.groups as group}
        <div class="section-title">{group.title}</div>
        {#each group.shortcuts as shortcut}
          <div class="shortcut-row">
            <kbd class="keys">{shortcut.keys}</kbd>
            <span class="desc">{shortcut.desc}</span>
          </div>
        {/each}
      {/each}
    </div>
    <div class="dialog-footer">
      <button class="dialog-btn primary" onclick={onClose}>Close</button>
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
    width: 72ch;
    height: 85vh;
    max-width: 90vw;
    max-height: 900px;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .tab-btn {
    flex: 1;
    padding: 8px 12px;
    font-size: 12px;
    font-family: inherit;
    color: var(--text-secondary);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-btn.active {
    color: var(--text-accent);
    border-bottom-color: var(--text-accent);
  }

  .dialog-body {
    padding: 16px 24px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
    flex: 1 1 0;
    min-height: 0;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    opacity: 0.7;
    padding-top: 4px;
  }

  .section-title:first-child {
    padding-top: 0;
  }

  .shortcut-row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    font-size: 13px;
    line-height: 1.8;
  }

  .keys {
    flex: 0 0 19ch;
    font-family: inherit;
    font-size: 11px;
    color: var(--text-secondary);
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: 3px;
    padding: 1px 6px;
    text-align: center;
  }

  .desc {
    color: var(--text-primary);
  }

  .dialog-footer {
    display: flex;
    justify-content: center;
    padding: 12px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
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
