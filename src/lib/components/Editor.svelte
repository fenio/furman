<script lang="ts">
  import { readFileText, writeFileText } from '$lib/services/tauri';
  import { s3PutText } from '$lib/services/s3';
  import { appState } from '$lib/state/app.svelte';
  import { onMount } from 'svelte';

  interface Props {
    path: string;
    onClose: () => void;
  }

  let { path, onClose }: Props = $props();

  let content = $state('');
  let originalContent = $state('');
  let dirty = $state(false);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let textareaEl: HTMLTextAreaElement | undefined = $state(undefined);
  let wrapperEl: HTMLDivElement | undefined = $state(undefined);

  const fileName = $derived(path.split('/').pop() ?? path);

  const lineCount = $derived(content.split('\n').length);

  const lineNumbers = $derived.by(() => {
    const count = lineCount;
    const lines: string[] = [];
    for (let i = 1; i <= count; i++) {
      lines.push(String(i));
    }
    return lines.join('\n');
  });

  onMount(async () => {
    try {
      content = await readFileText(path);
      originalContent = content;
    } catch (err: unknown) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  });

  $effect(() => {
    dirty = content !== originalContent;
    appState.editorDirty = dirty;
  });

  $effect(() => {
    if (textareaEl && !loading) {
      textareaEl.focus();
    }
  });

  async function save() {
    if (saving) return;
    saving = true;
    try {
      await writeFileText(path, content);
      if (appState.editorS3ConnectionId) {
        await s3PutText(
          appState.editorS3ConnectionId,
          appState.editorS3Key,
          content,
        );
      }
      originalContent = content;
      dirty = false;
    } catch (err: unknown) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      saving = false;
    }
  }

  function handleClose() {
    if (dirty) {
      appState.showConfirm('File has been modified. Discard changes?', () => {
        appState.closeModal();
        onClose();
      });
    } else {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      handleClose();
      return;
    }

    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault();
      e.stopPropagation();
      save();
      return;
    }

    if (e.key === 'F2') {
      e.preventDefault();
      e.stopPropagation();
      save();
      return;
    }
  }

  function handleScroll() {
    // Sync line numbers scroll with textarea scroll
    const lineNumEl = wrapperEl?.querySelector('.editor-line-numbers') as HTMLElement | null;
    if (lineNumEl && textareaEl) {
      lineNumEl.scrollTop = textareaEl.scrollTop;
    }
  }
</script>

<div
  class="editor-overlay no-select"
  onkeydown={handleKeydown}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <!-- Header -->
  <div class="editor-header">
    <span class="editor-filename">{fileName}</span>
    {#if dirty}
      <span class="editor-modified">[Modified]</span>
    {/if}
    {#if saving}
      <span class="editor-saving">[Saving...]</span>
    {/if}
    <span class="editor-help">Ctrl+S/F2=Save  ESC=Close</span>
  </div>

  <!-- Content -->
  {#if loading}
    <div class="editor-loading">Loading...</div>
  {:else if error && !content}
    <div class="editor-error">Error: {error}</div>
  {:else}
    <div class="editor-body" bind:this={wrapperEl}>
      <pre class="editor-line-numbers">{lineNumbers}</pre>
      <textarea
        class="editor-textarea"
        bind:value={content}
        bind:this={textareaEl}
        onscroll={handleScroll}
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        {...{ autocorrect: 'off' }}
      ></textarea>
    </div>
  {/if}

  {#if error && content}
    <div class="editor-status-error">Error: {error}</div>
  {/if}
</div>

<style>
  .editor-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    z-index: 200;
  }

  .editor-header {
    display: flex;
    gap: 2ch;
    background: var(--bg-header);
    color: var(--text-primary);
    padding: 4px 12px;
    flex: 0 0 auto;
    border-bottom: 1px solid var(--border-subtle);
  }

  .editor-filename {
    font-weight: 600;
  }

  .editor-modified {
    color: var(--error-color);
  }

  .editor-saving {
    color: var(--text-accent);
  }

  .editor-help {
    margin-left: auto;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .editor-loading,
  .editor-error {
    padding: 16px;
    color: var(--text-secondary);
  }

  .editor-error {
    color: var(--error-color);
  }

  .editor-body {
    display: flex;
    flex-direction: row;
    flex: 1 1 0;
    min-height: 0;
    overflow: hidden;
  }

  .editor-line-numbers {
    flex: 0 0 5ch;
    margin: 0;
    padding: 4px 4px 4px 0;
    text-align: right;
    color: var(--text-secondary);
    background: var(--bg-panel);
    overflow: hidden;
    border-right: 1px solid var(--border-subtle);
    line-height: inherit;
    font-family: 'Menlo', 'Consolas', 'Courier New', monospace;
    font-size: 13px;
  }

  .editor-textarea {
    flex: 1 1 0;
    min-width: 0;
    resize: none;
    border: none;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-family: 'Menlo', 'Consolas', 'Courier New', monospace;
    font-size: 13px;
    line-height: inherit;
    padding: 4px;
    margin: 0;
    tab-size: 4;
    white-space: pre;
    overflow: auto;
  }

  .editor-textarea:focus {
    outline: none;
  }

  .editor-status-error {
    background: var(--error-bg);
    color: var(--error-color);
    padding: 2px 12px;
    flex: 0 0 auto;
    font-size: 12px;
    border-top: 1px solid var(--border-subtle);
  }
</style>
