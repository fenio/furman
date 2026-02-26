<script lang="ts">
  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let dialogEl: HTMLDivElement | undefined = $state(undefined);

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

  const sections = [
    {
      title: 'Navigation',
      shortcuts: [
        { keys: 'Arrow Keys', desc: 'Move cursor' },
        { keys: 'Home / End', desc: 'Jump to first / last entry' },
        { keys: 'Page Up / Down', desc: 'Scroll by page' },
        { keys: 'Enter', desc: 'Open file or enter directory' },
        { keys: 'Backspace', desc: 'Go to parent directory' },
        { keys: 'Tab', desc: 'Switch active panel' },
        { keys: 'Space', desc: 'Quick look' },
      ],
    },
    {
      title: 'Selection',
      shortcuts: [
        { keys: 'Shift + Arrow', desc: 'Extend selection' },
        { keys: 'Shift + Home / End', desc: 'Select to start / end' },
        { keys: 'Insert', desc: 'Toggle select and move down' },
      ],
    },
    {
      title: 'File Operations',
      shortcuts: [
        { keys: 'F2 / \u2318R', desc: 'Rename' },
        { keys: 'F3 / \u2318\u21E73', desc: 'View file' },
        { keys: 'F4 / \u2318E', desc: 'Edit file' },
        { keys: 'F5 / \u2318C', desc: 'Copy to other panel' },
        { keys: 'F6 / \u2318M', desc: 'Move to other panel' },
        { keys: 'Shift+F6', desc: 'Rename (alt)' },
        { keys: 'F7 / \u2318N', desc: 'Create directory' },
        { keys: 'F8 / \u2318D / \u2318\u232B', desc: 'Delete' },
        { keys: 'F9 / \u2318I', desc: 'Properties' },
        { keys: 'F10 / \u2318Q', desc: 'Quit' },
      ],
    },
    {
      title: 'Panels & Layout',
      shortcuts: [
        { keys: '\u2318P', desc: 'Toggle single / dual pane' },
        { keys: '\u2318B', desc: 'Toggle sidebar' },
        { keys: '\u2318J', desc: 'Toggle transfer panel' },
        { keys: '\u2318Y', desc: 'Sync directories' },
      ],
    },
    {
      title: 'S3',
      shortcuts: [
        { keys: '\u2318S', desc: 'Connect / disconnect S3' },
        { keys: '\u2318U', desc: 'Presigned URL' },
        { keys: '\u2318K', desc: 'Copy S3 URI' },
        { keys: '\u2318L', desc: 'Bulk storage class change' },
      ],
    },
    {
      title: 'Search & Terminal',
      shortcuts: [
        { keys: '\u2318F', desc: 'Search files' },
        { keys: '\u2318T', desc: 'Toggle bottom terminal' },
        { keys: '\u2318\u21E7T', desc: 'Toggle in-pane terminal' },
        { keys: '\u2318`', desc: 'Toggle quake terminal' },
        { keys: 'Esc', desc: 'Hide quake / clear filter' },
      ],
    },
    {
      title: 'Other',
      shortcuts: [
        { keys: '\u2318\u21E7L', desc: 'Toggle dark / light theme' },
        { keys: '\u2318/', desc: 'Show this cheatsheet' },
        { keys: 'Type any letter', desc: 'Quick filter' },
      ],
    },
  ];
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
    <div class="dialog-body">
      <div class="shortcuts-grid">
        {#each sections as section}
          <div class="section">
            <div class="section-title">{section.title}</div>
            {#each section.shortcuts as shortcut}
              <div class="shortcut-row">
                <kbd class="keys">{shortcut.keys}</kbd>
                <span class="desc">{shortcut.desc}</span>
              </div>
            {/each}
          </div>
        {/each}
      </div>
      <div class="dialog-buttons">
        <button class="dialog-btn primary" onclick={onClose}>Close</button>
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
    max-width: 90ch;
    max-height: 85vh;
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

  .dialog-body {
    padding: 16px 24px 20px;
    overflow-y: auto;
  }

  .shortcuts-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px 32px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-accent);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 4px;
  }

  .shortcut-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 13px;
    line-height: 1.7;
  }

  .keys {
    flex-shrink: 0;
    min-width: 12ch;
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

  .dialog-buttons {
    display: flex;
    justify-content: center;
    margin-top: 16px;
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
