<script lang="ts">
  import { platform } from '$lib/state/platform.svelte';
  import { onMount } from 'svelte';

  interface Props {
    x: number;
    y: number;
    onClose: () => void;
    onAction: (key: string) => void;
    isS3?: boolean;
    isFile?: boolean;
    isArchive?: boolean;
    onEmpty?: boolean;
  }

  let { x, y, onClose, onAction, isS3 = false, isFile = false, isArchive = false, onEmpty = false }: Props = $props();

  let menuEl: HTMLDivElement | undefined = $state(undefined);
  let adjustX = $state(0);
  let adjustY = $state(0);
  const menuX = $derived(x + adjustX);
  const menuY = $derived(y + adjustY);

  onMount(() => {
    if (menuEl) {
      menuEl.focus();
      const rect = menuEl.getBoundingClientRect();
      if (rect.right > window.innerWidth) {
        adjustX = -rect.width;
      }
      if (rect.bottom > window.innerHeight) {
        adjustY = -rect.height;
      }
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  function act(key: string) {
    onAction(key);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="ctx-backdrop"
  role="presentation"
  onclick={onClose}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={menuEl}
    class="ctx-menu no-select"
    role="menu"
    tabindex="-1"
    style="left: {menuX}px; top: {menuY}px"
    onclick={(e) => e.stopPropagation()}
    onkeydown={handleKeydown}
  >
    {#if onEmpty}
      <button class="menu-row" role="menuitem" onclick={() => act('mkdir')}>
        New Folder
        <span class="menu-shortcut">F7</span>
      </button>
      <button class="menu-row" role="menuitem" onclick={() => act('properties')}>
        Properties
        <span class="menu-shortcut">F9</span>
      </button>
    {:else}
      <button class="menu-row" role="menuitem" onclick={() => act('open')}>
        Open
        <span class="menu-shortcut">Enter</span>
      </button>
      <button class="menu-row" role="menuitem" disabled={!isFile} onclick={() => act('view')}>
        View
        <span class="menu-shortcut">F3</span>
      </button>
      <button class="menu-row" role="menuitem" disabled={!isFile} onclick={() => act('edit')}>
        Edit
        <span class="menu-shortcut">F4</span>
      </button>

      <div class="menu-divider"></div>

      <button class="menu-row" role="menuitem" onclick={() => act('copy')}>
        Copy
        <span class="menu-shortcut">F5</span>
      </button>
      <button class="menu-row" role="menuitem" onclick={() => act('move')}>
        Move
        <span class="menu-shortcut">F6</span>
      </button>

      <div class="menu-divider"></div>

      <button class="menu-row" role="menuitem" onclick={() => act('rename')}>
        Rename
        <span class="menu-shortcut">F2</span>
      </button>
      <button class="menu-row" role="menuitem" onclick={() => act('delete')}>
        Delete
        <span class="menu-shortcut">F8</span>
      </button>
      <button class="menu-row" role="menuitem" onclick={() => act('mkdir')}>
        New Folder
        <span class="menu-shortcut">F7</span>
      </button>

      <div class="menu-divider"></div>

      <button class="menu-row" role="menuitem" onclick={() => act('properties')}>
        Properties
        <span class="menu-shortcut">F9</span>
      </button>

      {#if isS3}
        <div class="menu-divider"></div>
        <button class="menu-row" role="menuitem" disabled={!isFile} onclick={() => act('presign')}>
          Presign URL
          <span class="menu-shortcut">{platform.mod}U</span>
        </button>
        <button class="menu-row" role="menuitem" onclick={() => act('copy-uri')}>
          Copy S3 URI
          <span class="menu-shortcut">{platform.mod}K</span>
        </button>
        <button class="menu-row" role="menuitem" onclick={() => act('bulk-storage')}>
          Bulk Storage Class
          <span class="menu-shortcut">{platform.mod}L</span>
        </button>
      {/if}
    {/if}
  </div>
</div>

<style>
  .ctx-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
  }

  .ctx-menu {
    position: fixed;
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-dialog);
    min-width: 180px;
    padding: 6px 0;
    z-index: 91;
  }

  .menu-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 16px;
    font-size: 13px;
    color: var(--text-primary);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .menu-row:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .menu-row:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .menu-shortcut {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.6;
  }

  .menu-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 4px 0;
  }
</style>
