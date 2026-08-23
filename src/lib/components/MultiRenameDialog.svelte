<script lang="ts">
  import type { FileEntry, PanelBackend } from '$lib/types';
  import { SvelteMap } from 'svelte/reactivity';
  import { renameFile } from '$lib/services/tauri';
  import { s3RenameObject } from '$lib/services/s3';
  import { sftpRename } from '$lib/services/sftp';
  import { error as logError } from '$lib/services/log';

  interface Props {
    entries: FileEntry[];
    backend: PanelBackend;
    s3ConnectionId?: string;
    sftpConnectionId?: string;
    onClose: () => void;
    onDone: () => void;
  }

  let { entries, backend, s3ConnectionId, sftpConnectionId, onClose, onDone }: Props = $props();

  type Phase = 'edit' | 'progress' | 'done';
  let phase = $state<Phase>('edit');

  // Pattern controls
  let findText = $state('');
  let replaceText = $state('');
  let useRegex = $state(false);
  let regexHelpOpen = $state(false);
  let prefix = $state('');
  let suffix = $state('');
  let numberingEnabled = $state(false);
  let numberStart = $state(1);
  let numberStep = $state(1);
  let numberDigits = $state(2);
  let caseTransform = $state<'none' | 'upper' | 'lower' | 'title'>('none');

  // Per-row overrides
  let overrides = $state<Record<string, string>>({});

  // Progress state
  let filesDone = $state(0);
  let currentFile = $state('');
  let cancelled = $state(false);
  let errors = $state<{ name: string; error: string }[]>([]);
  let successCount = $state(0);

  let dialogEl: HTMLDivElement | undefined = $state(undefined);

  $effect(() => {
    if (dialogEl) dialogEl.focus();
  });

  function splitNameExt(name: string): { stem: string; ext: string } {
    if (name.startsWith('.') && !name.includes('.', 1)) return { stem: name, ext: '' };
    const dot = name.lastIndexOf('.');
    if (dot <= 0) return { stem: name, ext: '' };
    return { stem: name.substring(0, dot), ext: name.substring(dot) };
  }

  function applyTransform(text: string, transform: typeof caseTransform): string {
    switch (transform) {
      case 'upper': return text.toUpperCase();
      case 'lower': return text.toLowerCase();
      case 'title': return text.replace(/\b\w/g, (c) => c.toUpperCase());
      default: return text;
    }
  }

  function computeNewName(entry: FileEntry, index: number): string {
    const { stem, ext } = splitNameExt(entry.name);
    let newStem = stem;

    // Find/Replace
    if (findText) {
      if (useRegex) {
        try {
          const re = new RegExp(findText, 'g');
          newStem = newStem.replace(re, replaceText);
        } catch {
          // Invalid regex — skip
        }
      } else {
        // Global string replace
        newStem = newStem.split(findText).join(replaceText);
      }
    }

    // Prefix / Suffix
    if (prefix) newStem = prefix + newStem;
    if (suffix) newStem = newStem + suffix;

    // Numbering
    if (numberingEnabled) {
      const num = numberStart + index * numberStep;
      const numStr = String(num).padStart(numberDigits, '0');
      newStem = newStem + numStr;
    }

    // Case transform
    newStem = applyTransform(newStem, caseTransform);

    return newStem + ext;
  }

  const preview = $derived.by(() => {
    return entries.map((entry, i) => {
      const override = overrides[entry.path];
      const computed = computeNewName(entry, i);
      const newName = override !== undefined ? override : computed;
      return { entry, newName, changed: newName !== entry.name };
    });
  });

  const conflicts = $derived.by(() => {
    const names = new SvelteMap<string, number>();
    const result: boolean[] = [];
    for (const item of preview) {
      const count = (names.get(item.newName) ?? 0) + 1;
      names.set(item.newName, count);
      result.push(item.newName.includes('/') || item.newName === '');
    }
    // Mark duplicates
    for (let i = 0; i < preview.length; i++) {
      if ((names.get(preview[i].newName) ?? 0) > 1) {
        result[i] = true;
      }
    }
    return result;
  });

  const hasConflicts = $derived(conflicts.some(Boolean));
  const hasChanges = $derived(preview.some((p) => p.changed));

  function handleOverrideInput(path: string, value: string, original: string, index: number) {
    const computed = computeNewName(entries[index], index);
    if (value === computed) {
      const next = { ...overrides };
      delete next[path];
      overrides = next;
    } else {
      overrides = { ...overrides, [path]: value };
    }
  }

  async function handleApply() {
    phase = 'progress';
    filesDone = 0;
    errors = [];
    successCount = 0;
    cancelled = false;

    for (let i = 0; i < preview.length; i++) {
      if (cancelled) break;
      const item = preview[i];
      if (!item.changed) {
        filesDone = i + 1;
        continue;
      }
      currentFile = item.entry.name;
      try {
        if (backend === 's3' && s3ConnectionId) {
          await s3RenameObject(s3ConnectionId, item.entry.path, item.newName);
        } else if (backend === 'sftp' && sftpConnectionId) {
          await sftpRename(sftpConnectionId, item.entry.path, item.newName);
        } else {
          await renameFile(item.entry.path, item.newName);
        }
        successCount++;
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        logError(msg);
        errors = [...errors, { name: item.entry.name, error: msg }];
      }
      filesDone = i + 1;
    }
    phase = 'done';
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (regexHelpOpen) regexHelpOpen = false;
      else if (phase === 'edit') onClose();
      else if (phase === 'progress') cancelled = true;
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (regexHelpOpen && !target.closest('.regex-help')) regexHelpOpen = false;
    if (target.classList.contains('dialog-overlay')) {
      if (phase === 'edit') onClose();
    }
  }
</script>

<div
  class="dialog-overlay no-select"
  tabindex="0"
  role="dialog"
  aria-modal="true"
  bind:this={dialogEl}
  onkeydown={handleKeydown}
  onclick={handleOverlayClick}
>
  <div class="dialog-box">
    <div class="dialog-title">Batch Rename ({entries.length} files)</div>

    {#if phase === 'edit'}
      <div class="dialog-body">
        <div class="controls">
          <div class="control-row">
            <span class="control-label">Find:</span>
            <input class="control-input" bind:value={findText} placeholder="Text to find..." />
            <label class="regex-toggle">
              <input type="checkbox" bind:checked={useRegex} /> Regex
            </label>
            <span class="regex-help">
              <button
                class="regex-help-btn"
                type="button"
                aria-label="Regex help"
                aria-expanded={regexHelpOpen}
                aria-controls="regex-help-popup"
                onclick={() => { regexHelpOpen = !regexHelpOpen; }}
              >?</button>
              {#if regexHelpOpen}
                <span id="regex-help-popup" class="regex-help-popup" role="note">
                  <strong>JavaScript regular expressions</strong>
                  <span>Patterns are global, case-sensitive, and apply to the name before its extension. Do not add <code>/</code> delimiters. Use <code>$1</code>, <code>$2</code>, etc. for captured groups.</span>
                  <span class="regex-example">
                    <span>Pad one-digit names</span>
                    <code>Find: ^(\d)(m)$</code>
                    <code>Replace: 0$1$2</code>
                    <span>2m.webp &rarr; 02m.webp</span>
                  </span>
                  <span class="regex-example">
                    <span>Remove a suffix</span>
                    <code>Find: -copy$</code>
                    <code>Replace: (empty)</code>
                    <span>report-copy.pdf &rarr; report.pdf</span>
                  </span>
                </span>
              {/if}
            </span>
          </div>
          <div class="control-row">
            <span class="control-label">Replace:</span>
            <input class="control-input" bind:value={replaceText} placeholder="Replace with..." />
          </div>
          <div class="control-row">
            <span class="control-label">Prefix:</span>
            <input class="control-input short" bind:value={prefix} placeholder="Before name" />
            <span class="control-label">Suffix:</span>
            <input class="control-input short" bind:value={suffix} placeholder="After name" />
          </div>
          <div class="control-row">
            <label class="regex-toggle">
              <input type="checkbox" bind:checked={numberingEnabled} /> Numbering
            </label>
            {#if numberingEnabled}
              <span class="control-label small">Start:</span>
              <input class="control-input tiny" type="number" bind:value={numberStart} min="0" />
              <span class="control-label small">Step:</span>
              <input class="control-input tiny" type="number" bind:value={numberStep} min="1" />
              <span class="control-label small">Digits:</span>
              <input class="control-input tiny" type="number" bind:value={numberDigits} min="1" max="10" />
            {/if}
          </div>
          <div class="control-row">
            <span class="control-label">Case:</span>
            <select class="control-select" bind:value={caseTransform}>
              <option value="none">None</option>
              <option value="upper">UPPER CASE</option>
              <option value="lower">lower case</option>
              <option value="title">Title Case</option>
            </select>
          </div>
        </div>

        <div class="preview-table">
          <div class="preview-header">
            <span class="preview-col original">Original Name</span>
            <span class="preview-col new">New Name</span>
          </div>
          <div class="preview-scroll">
            {#each preview as item, i (item.entry.path)}
              <div class="preview-row" class:conflict={conflicts[i]} class:changed={item.changed}>
                <span class="preview-col original">{item.entry.name}</span>
                <input
                  class="preview-input"
                  value={item.newName}
                  oninput={(e) => handleOverrideInput(item.entry.path, e.currentTarget.value, item.entry.name, i)}
                />
              </div>
            {/each}
          </div>
        </div>
      </div>

      <div class="dialog-footer">
        <button class="dialog-btn" onclick={onClose}>Cancel</button>
        <button class="dialog-btn primary" disabled={!hasChanges || hasConflicts} onclick={handleApply}>
          Apply ({preview.filter(p => p.changed).length})
        </button>
      </div>

    {:else if phase === 'progress'}
      <div class="dialog-body progress-body">
        <div class="progress-bar-container">
          <div class="progress-bar" style="width: {(filesDone / entries.length) * 100}%"></div>
        </div>
        <div class="progress-text">{filesDone} / {entries.length}</div>
        <div class="progress-file">{currentFile}</div>
      </div>
      <div class="dialog-footer">
        <button class="dialog-btn" onclick={() => { cancelled = true; }}>Cancel</button>
      </div>

    {:else}
      <div class="dialog-body progress-body">
        <div class="done-summary">
          {successCount} renamed{errors.length > 0 ? `, ${errors.length} error(s)` : ''}
          {cancelled ? ' (cancelled)' : ''}
        </div>
        {#if errors.length > 0}
          <div class="error-list">
            {#each errors as err (err.name)}
              <div class="error-item">
                <span class="error-name">{err.name}</span>: {err.error}
              </div>
            {/each}
          </div>
        {/if}
      </div>
      <div class="dialog-footer">
        <button class="dialog-btn primary" onclick={onDone}>Close</button>
      </div>
    {/if}
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
    max-width: 90vw;
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
    padding: 16px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
  }

  .controls {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .control-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .control-label {
    flex: 0 0 auto;
    font-size: 12px;
    color: var(--text-secondary);
    min-width: 5ch;
  }

  .control-label.small {
    min-width: auto;
    font-size: 11px;
  }

  .control-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    padding: 4px 8px;
  }

  .control-input:focus {
    border-color: var(--border-active);
    outline: none;
  }

  .control-input.short {
    flex: 0 1 14ch;
  }

  .control-input.tiny {
    flex: 0 0 6ch;
    width: 6ch;
  }

  .control-select {
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    padding: 4px 8px;
  }

  .regex-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .regex-toggle input {
    margin: 0;
  }

  .regex-help {
    position: relative;
    flex-shrink: 0;
  }

  .regex-help-btn {
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: 50%;
    background: var(--bg-surface);
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 11px;
    font-weight: 600;
    line-height: 16px;
    cursor: pointer;
  }

  .regex-help-btn:hover,
  .regex-help-btn:focus-visible,
  .regex-help-btn[aria-expanded="true"] {
    color: var(--text-accent);
    border-color: var(--border-active);
    outline: none;
  }

  .regex-help-popup {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 290px;
    max-width: calc(100vw - 64px);
    padding: 12px;
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-md);
    background: var(--dialog-bg);
    box-shadow: var(--shadow-dialog);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1.45;
    white-space: normal;
  }

  .regex-help-popup strong {
    color: var(--text-primary);
    font-size: 12px;
  }

  .regex-help-popup code {
    padding: 1px 4px;
    border-radius: 3px;
    background: var(--bg-surface);
    color: var(--text-accent);
    font-family: monospace;
  }

  .regex-example {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
  }

  .regex-example > span:first-child {
    color: var(--text-primary);
    font-weight: 600;
  }

  .preview-table {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .preview-header {
    display: flex;
    background: var(--bg-header);
    padding: 4px 8px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border-subtle);
  }

  .preview-col {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preview-col.original {
    flex: 0 0 45%;
  }

  .preview-col.new {
    flex: 0 0 55%;
  }

  .preview-scroll {
    max-height: 40vh;
    overflow-y: auto;
  }

  .preview-row {
    display: flex;
    align-items: center;
    padding: 2px 8px;
    font-size: 12px;
    border-bottom: 1px solid color-mix(in srgb, var(--border-subtle) 50%, transparent);
  }

  .preview-row .preview-col.original {
    color: var(--text-secondary);
  }

  .preview-row.changed .preview-input {
    color: var(--success-color, #4ec990);
  }

  .preview-row.conflict .preview-input {
    color: var(--error-color);
  }

  .preview-input {
    flex: 0 0 55%;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    padding: 2px 4px;
  }

  .preview-input:focus {
    border-color: var(--border-active);
    outline: none;
    background: var(--bg-surface);
  }

  .progress-body {
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 32px 24px;
  }

  .progress-bar-container {
    width: 100%;
    height: 6px;
    background: var(--bg-surface);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar {
    height: 100%;
    background: var(--border-active);
    transition: width 0.2s;
    border-radius: 3px;
  }

  .progress-text {
    font-size: 14px;
    color: var(--text-primary);
    font-weight: 600;
  }

  .progress-file {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .done-summary {
    font-size: 14px;
    color: var(--text-primary);
    font-weight: 600;
    text-align: center;
  }

  .error-list {
    max-height: 30vh;
    overflow-y: auto;
    width: 100%;
  }

  .error-item {
    font-size: 12px;
    color: var(--error-color);
    padding: 2px 0;
  }

  .error-name {
    font-weight: 600;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .dialog-btn {
    padding: 8px 20px;
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

  .dialog-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .dialog-btn.primary {
    background: rgba(110,168,254,0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover:not(:disabled) {
    background: rgba(110,168,254,0.3);
  }
</style>
