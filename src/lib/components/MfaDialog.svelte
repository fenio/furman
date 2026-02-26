<script lang="ts">
  interface Props {
    title: string;
    onSubmit: (mfa: string) => void;
    onCancel: () => void;
  }

  let { title, onSubmit, onCancel }: Props = $props();

  let serial = $state('');
  let code = $state('');
  let serialEl: HTMLInputElement | undefined = $state(undefined);

  $effect(() => {
    if (serialEl) {
      serialEl.focus();
    }
  });

  function handleSubmit() {
    if (!serial.trim() || !code.trim()) return;
    onSubmit(`${serial.trim()} ${code.trim()}`);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      handleSubmit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="dialog-box">
    <div class="dialog-title">{title}</div>
    <div class="dialog-body">
      <label class="field-label" for="mfa-serial">MFA Device Serial</label>
      <input
        id="mfa-serial"
        type="text"
        class="dialog-input"
        autocomplete="off"
        placeholder="arn:aws:iam::123456789012:mfa/user"
        bind:value={serial}
        bind:this={serialEl}
        onkeydown={handleKeydown}
      />
      <label class="field-label" for="mfa-code">MFA Token Code</label>
      <input
        id="mfa-code"
        type="text"
        class="dialog-input"
        autocomplete="off"
        placeholder="123456"
        maxlength="6"
        bind:value={code}
        onkeydown={handleKeydown}
      />
      <div class="dialog-buttons">
        <button
          class="dialog-btn primary"
          onclick={handleSubmit}
          disabled={!serial.trim() || !code.trim()}
        >OK</button>
        <button class="dialog-btn" onclick={onCancel}>Cancel</button>
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
    max-width: 80ch;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
  }

  .dialog-body {
    padding: 20px 24px;
  }

  .field-label {
    display: block;
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: 4px;
  }

  .dialog-input {
    width: 100%;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    font-family: inherit;
    font-size: 14px;
    margin-bottom: 16px;
  }

  .dialog-input:focus {
    border-color: var(--border-active);
    box-shadow: 0 0 0 1px rgba(110,168,254,0.3);
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    gap: 10px;
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

  .dialog-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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
