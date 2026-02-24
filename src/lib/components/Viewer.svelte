<script lang="ts">
  import type { ViewerMode } from '$lib/types';
  import { readFileText, readFileBinary } from '$lib/services/tauri';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  interface Props {
    path: string;
    mode: ViewerMode;
    onClose: () => void;
  }

  let { path, mode, onClose }: Props = $props();

  let content = $state('');
  let hexData = $state<number[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let overlayEl: HTMLDivElement | undefined = $state(undefined);
  let contentEl: HTMLDivElement | undefined = $state(undefined);

  const fileName = $derived(path.split('/').pop() ?? path);
  const modeLabel = $derived(mode === 'text' ? 'TEXT' : mode === 'hex' ? 'HEX' : 'IMAGE');
  const imageSrc = $derived(mode === 'image' ? convertFileSrc(path) : '');

  const lines = $derived.by(() => {
    if (mode !== 'text') return [];
    return content.split('\n');
  });

  const hexLines = $derived.by(() => {
    if (mode !== 'hex' || hexData.length === 0) return [];
    const result: string[] = [];
    const bytesPerLine = 16;
    for (let offset = 0; offset < hexData.length; offset += bytesPerLine) {
      const chunk = hexData.slice(offset, offset + bytesPerLine);
      const offsetStr = offset.toString(16).padStart(8, '0').toUpperCase();
      const hexPart = chunk.map((b) => b.toString(16).padStart(2, '0').toUpperCase()).join(' ');
      const asciiPart = chunk
        .map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.'))
        .join('');
      result.push(
        `${offsetStr}  ${hexPart.padEnd(bytesPerLine * 3 - 1)}  ${asciiPart}`
      );
    }
    return result;
  });

  onMount(async () => {
    try {
      if (mode === 'text') {
        content = await readFileText(path);
      } else if (mode === 'hex') {
        // Read first 64KB for hex view
        hexData = await readFileBinary(path, 0, 65536);
      }
      // Image mode doesn't need loading - it uses a file:// URL
    } catch (err: unknown) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  });

  $effect(() => {
    if (overlayEl) {
      overlayEl.focus();
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    const el = contentEl;
    switch (e.key) {
      case 'Escape':
      case 'q':
      case 'Q':
        e.preventDefault();
        e.stopPropagation();
        onClose();
        break;
      case 'ArrowDown':
      case 'j':
        e.preventDefault();
        if (el) el.scrollTop += 16;
        break;
      case 'ArrowUp':
      case 'k':
        e.preventDefault();
        if (el) el.scrollTop -= 16;
        break;
      case 'PageDown':
      case ' ':
        e.preventDefault();
        if (el) el.scrollTop += el.clientHeight - 16;
        break;
      case 'PageUp':
        e.preventDefault();
        if (el) el.scrollTop -= el.clientHeight - 16;
        break;
      case 'Home':
        e.preventDefault();
        if (el) el.scrollTop = 0;
        break;
      case 'End':
        e.preventDefault();
        if (el) el.scrollTop = el.scrollHeight;
        break;
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="viewer-overlay no-select"
  onkeydown={handleKeydown}
  tabindex="0"
  bind:this={overlayEl}
  role="dialog"
  aria-modal="true"
>
  <!-- Header -->
  <div class="viewer-header">
    <span class="viewer-filename">{fileName}</span>
    <span class="viewer-mode">[{modeLabel}]</span>
    <span class="viewer-help">ESC/Q=Close  PgUp/PgDn=Scroll</span>
  </div>

  <!-- Content -->
  <div class="viewer-content" bind:this={contentEl}>
    {#if loading}
      <div class="viewer-loading">Loading...</div>
    {:else if error}
      <div class="viewer-error">Error: {error}</div>
    {:else if mode === 'text'}
      <pre class="viewer-text">{#each lines as line, i}<span class="line-num">{String(i + 1).padStart(5)} </span>{line}
{/each}</pre>
    {:else if mode === 'hex'}
      <pre class="viewer-hex">{#each hexLines as line}{line}
{/each}</pre>
    {:else if mode === 'image'}
      <div class="viewer-image-container">
        <img
          src={imageSrc}
          alt={fileName}
          class="viewer-image"
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .viewer-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    z-index: 200;
    overflow: hidden;
  }

  .viewer-header {
    display: flex;
    gap: 2ch;
    align-items: center;
    background: var(--bg-header);
    color: var(--text-primary);
    padding: 8px 16px;
    flex: 0 0 auto;
    border-bottom: 1px solid var(--border-subtle);
  }

  .viewer-filename {
    font-weight: 600;
  }

  .viewer-mode {
    color: var(--text-accent);
    background: rgba(110,168,254,0.15);
    border-radius: 4px;
    padding: 2px 8px;
    font-size: 12px;
  }

  .viewer-help {
    margin-left: auto;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .viewer-content {
    flex: 1 1 0;
    overflow: auto;
    padding: 4px;
  }

  .viewer-loading,
  .viewer-error {
    padding: 16px;
    color: var(--text-secondary);
  }

  .viewer-error {
    color: var(--error-color);
  }

  .viewer-text,
  .viewer-hex {
    margin: 0;
    padding: 4px;
    color: var(--text-primary);
    white-space: pre;
    tab-size: 8;
    font-family: 'Menlo', 'Consolas', 'Courier New', monospace;
    font-size: 13px;
  }

  .line-num {
    color: var(--text-secondary);
    opacity: 0.3;
  }

  .viewer-image-container {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100%;
    padding: 16px;
  }

  .viewer-image {
    max-width: 100%;
    max-height: 90vh;
    object-fit: contain;
  }
</style>
