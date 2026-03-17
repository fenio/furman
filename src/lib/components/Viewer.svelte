<script lang="ts">
  import type { ViewerMode } from '$lib/types';
  import { readFileText, readFileBinary } from '$lib/services/tauri';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { highlightCode, detectLanguage } from '$lib/utils/highlight';

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
  let searchInputEl: HTMLInputElement | undefined = $state(undefined);

  // Search state
  let searchOpen = $state(false);
  let searchQuery = $state('');
  let currentMatchIdx = $state(0);

  const fileName = $derived(path.split('/').pop() ?? path);
  const modeLabel = $derived(mode === 'text' ? 'TEXT' : mode === 'hex' ? 'HEX' : 'IMAGE');
  const imageSrc = $derived(mode === 'image' ? convertFileSrc(path) : '');

  const rawLines = $derived(mode === 'text' && content ? content.split('\n') : []);

  const highlightedLines = $derived.by(() => {
    if (mode !== 'text' || !content) return [];
    const lang = detectLanguage(fileName);
    const html = highlightCode(content, lang);
    return html.split('\n');
  });

  const matchLineIndices = $derived.by(() => {
    if (!searchQuery || !searchOpen || mode !== 'text') return [];
    const q = searchQuery.toLowerCase();
    const indices: number[] = [];
    for (let i = 0; i < rawLines.length; i++) {
      if (rawLines[i].toLowerCase().includes(q)) indices.push(i);
    }
    return indices;
  });

  const currentMatchLine = $derived(
    matchLineIndices.length > 0 ? matchLineIndices[currentMatchIdx] : -1
  );

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
    if (overlayEl && !searchOpen) {
      overlayEl.focus();
    }
  });

  // Scroll to current match when it changes
  $effect(() => {
    const lineIdx = currentMatchLine;
    if (lineIdx < 0 || !contentEl) return;
    const lineEls = contentEl.querySelectorAll('.viewer-line');
    const el = lineEls[lineIdx] as HTMLElement | undefined;
    if (el) {
      el.scrollIntoView({ block: 'center' });
    }
  });

  async function openSearch() {
    searchOpen = true;
    currentMatchIdx = 0;
    await tick();
    searchInputEl?.focus();
    searchInputEl?.select();
  }

  function closeSearch() {
    searchOpen = false;
    searchQuery = '';
    currentMatchIdx = 0;
    overlayEl?.focus();
  }

  function nextMatch() {
    if (matchLineIndices.length === 0) return;
    currentMatchIdx = (currentMatchIdx + 1) % matchLineIndices.length;
  }

  function prevMatch() {
    if (matchLineIndices.length === 0) return;
    currentMatchIdx = (currentMatchIdx - 1 + matchLineIndices.length) % matchLineIndices.length;
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeSearch();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) prevMatch(); else nextMatch();
    } else if (e.key === 'n' && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      // Allow n/N from search input too
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    // Don't intercept when typing in search input
    if (searchOpen && e.target === searchInputEl) return;

    const el = contentEl;

    // Open search
    if ((e.key === 'f' && (e.metaKey || e.ctrlKey)) || (!searchOpen && e.key === '/')) {
      e.preventDefault();
      e.stopPropagation();
      openSearch();
      return;
    }

    // Search navigation (works even when search bar is closed, vim-style)
    if (searchOpen && e.key === 'n' && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      if (e.shiftKey) prevMatch(); else nextMatch();
      return;
    }

    switch (e.key) {
      case 'Escape':
      case 'q':
      case 'Q':
        e.preventDefault();
        e.stopPropagation();
        if (searchOpen) closeSearch();
        else onClose();
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

  // Inline search highlighting: wrap match text in <mark> within highlighted HTML
  function markMatches(htmlLine: string, rawLine: string, query: string): string {
    if (!query) return htmlLine;
    const q = query.toLowerCase();
    const lower = rawLine.toLowerCase();
    // Collect all match positions in raw text
    const positions: [number, number][] = [];
    let start = 0;
    while (true) {
      const idx = lower.indexOf(q, start);
      if (idx < 0) break;
      positions.push([idx, idx + q.length]);
      start = idx + 1;
    }
    if (positions.length === 0) return htmlLine;

    // Walk the highlighted HTML, tracking raw text position
    // We strip HTML tags to measure raw char position, then re-inject marks
    let result = '';
    let rawPos = 0;
    let posIdx = 0;
    let inMark = false;
    let i = 0;

    while (i < htmlLine.length) {
      // Check if we're at an HTML tag
      if (htmlLine[i] === '<') {
        // If we have an open mark, close it before the tag
        if (inMark) {
          result += '</mark>';
          inMark = false;
        }
        // Copy the entire tag
        const end = htmlLine.indexOf('>', i);
        if (end < 0) {
          result += htmlLine.slice(i);
          break;
        }
        result += htmlLine.slice(i, end + 1);
        i = end + 1;
        continue;
      }

      // HTML entity?
      if (htmlLine[i] === '&') {
        const end = htmlLine.indexOf(';', i);
        const entityLen = end >= 0 ? end - i + 1 : 1;
        const rawCharPos = rawPos;

        // Should we open mark here?
        if (!inMark && posIdx < positions.length && rawCharPos >= positions[posIdx][0]) {
          result += '<mark class="viewer-search-match">';
          inMark = true;
        }
        // Should we close mark before this entity?
        if (inMark && posIdx < positions.length && rawCharPos >= positions[posIdx][1]) {
          result += '</mark>';
          inMark = false;
          posIdx++;
          if (!inMark && posIdx < positions.length && rawCharPos >= positions[posIdx][0]) {
            result += '<mark class="viewer-search-match">';
            inMark = true;
          }
        }

        result += htmlLine.slice(i, i + entityLen);
        rawPos++;
        i += entityLen;
        continue;
      }

      // Regular character
      const rawCharPos = rawPos;

      if (!inMark && posIdx < positions.length && rawCharPos >= positions[posIdx][0]) {
        result += '<mark class="viewer-search-match">';
        inMark = true;
      }
      if (inMark && posIdx < positions.length && rawCharPos >= positions[posIdx][1]) {
        result += '</mark>';
        inMark = false;
        posIdx++;
        if (posIdx < positions.length && rawCharPos >= positions[posIdx][0]) {
          result += '<mark class="viewer-search-match">';
          inMark = true;
        }
      }

      result += htmlLine[i];
      rawPos++;
      i++;
    }

    if (inMark) result += '</mark>';
    return result;
  }
</script>

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
    {#if searchOpen && matchLineIndices.length > 0}
      <span class="viewer-match-count">{currentMatchIdx + 1}/{matchLineIndices.length}</span>
    {:else if searchOpen && searchQuery}
      <span class="viewer-match-count viewer-no-match">no matches</span>
    {/if}
    <span class="viewer-help">ESC/Q=Close  PgUp/PgDn=Scroll  ⌘F / /=Search</span>
  </div>

  <!-- Content -->
  <div class="viewer-content" bind:this={contentEl}>
    {#if loading}
      <div class="viewer-loading">Loading...</div>
    {:else if error}
      <div class="viewer-error">Error: {error}</div>
    {:else if mode === 'text'}
      <pre class="viewer-text hljs">{#each highlightedLines as line, i (i)}<div
          class="viewer-line"
          class:search-match-line={searchOpen && matchLineIndices.includes(i)}
          class:search-current-line={i === currentMatchLine}
        ><span class="line-num">{String(i + 1).padStart(5)} </span>{@html searchOpen && searchQuery && matchLineIndices.includes(i) ? markMatches(line, rawLines[i] ?? '', searchQuery) : line}</div>{/each}</pre>
    {:else if mode === 'hex'}
      <pre class="viewer-hex">{#each hexLines as line, i (i)}{line}
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

  <!-- Search bar -->
  {#if searchOpen}
    <div class="viewer-search-bar">
      <span class="search-prompt">/</span>
      <input
        bind:this={searchInputEl}
        bind:value={searchQuery}
        class="search-input"
        placeholder="search..."
        spellcheck="false"
        onkeydown={handleSearchKeydown}
      />
      <span class="search-nav">
        <button onclick={prevMatch} title="Previous (Shift+Enter)">↑</button>
        <button onclick={nextMatch} title="Next (Enter)">↓</button>
        <button onclick={closeSearch} title="Close (Esc)">✕</button>
      </span>
    </div>
  {/if}
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

  .viewer-match-count {
    font-size: 12px;
    color: var(--text-accent);
    background: rgba(110,168,254,0.15);
    border-radius: 4px;
    padding: 2px 8px;
  }

  .viewer-no-match {
    color: var(--error-color, #e06c75);
    background: rgba(224,108,117,0.15);
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

  .viewer-line {
    display: block;
  }

  .search-match-line {
    background: rgba(110,168,254,0.1);
  }

  .search-current-line {
    background: rgba(110,168,254,0.22) !important;
    outline: 1px solid rgba(110,168,254,0.4);
  }

  :global(.viewer-search-match) {
    background: rgba(255, 200, 60, 0.45);
    color: inherit;
    border-radius: 2px;
  }

  .line-num {
    color: var(--text-secondary);
    opacity: 0.3;
    user-select: none;
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

  /* Search bar */
  .viewer-search-bar {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--bg-header);
    border-top: 1px solid var(--border-subtle);
    padding: 6px 12px;
    flex: 0 0 auto;
  }

  .search-prompt {
    color: var(--text-accent);
    font-family: 'Menlo', 'Consolas', 'Courier New', monospace;
    font-size: 13px;
    opacity: 0.7;
  }

  .search-input {
    flex: 1;
    background: var(--bg-surface);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 3px;
    padding: 3px 8px;
    font-family: 'Menlo', 'Consolas', 'Courier New', monospace;
    font-size: 13px;
    outline: none;
  }

  .search-input:focus {
    border-color: rgba(110,168,254,0.6);
  }

  .search-nav {
    display: flex;
    gap: 2px;
  }

  .search-nav button {
    background: var(--bg-surface);
    color: var(--text-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 3px;
    padding: 2px 6px;
    cursor: pointer;
    font-size: 11px;
    line-height: 1.4;
  }

  .search-nav button:hover {
    color: var(--text-primary);
    border-color: rgba(110,168,254,0.5);
  }
</style>
