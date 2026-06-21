<script lang="ts">
  interface Props {
    onConnect: (protocol: 'smb' | 'nfs', host: string, share: string, username?: string, password?: string, domain?: string) => void;
    onCancel: () => void;
    embedded?: boolean;
  }

  let { onConnect, onCancel, embedded = false }: Props = $props();

  let protocol = $state<'smb' | 'nfs'>('smb');
  let host = $state('');
  let share = $state('');
  let username = $state('');
  let password = $state('');
  let domain = $state('');
  let connecting = $state(false);
  let connectError = $state('');

  function canConnect(): boolean {
    return !!host && !!share;
  }

  async function handleConnect() {
    if (!canConnect()) return;
    connecting = true;
    connectError = '';
    try {
      onConnect(
        protocol,
        host.trim(),
        share.trim(),
        protocol === 'smb' && username ? username : undefined,
        protocol === 'smb' && password ? password : undefined,
        protocol === 'smb' && domain ? domain : undefined,
      );
    } catch (err: unknown) {
      connectError = err instanceof Error ? err.message : String(err);
    } finally {
      connecting = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && canConnect()) {
      e.preventDefault();
      handleConnect();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="share-dialog" class:embedded onkeydown={handleKeydown} role="form">
  <div class="main-body">
    <div class="conn-tab-bar">
      <button class="conn-tab-btn active">Network Share</button>
    </div>

    {#if connectError}
      <div class="error-msg">{connectError}</div>
    {/if}

    <div class="field">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label>Protocol</label>
      <div class="auth-options">
        <label class="radio-label">
          <input type="radio" bind:group={protocol} value="smb" />
          SMB / CIFS
        </label>
        <label class="radio-label">
          <input type="radio" bind:group={protocol} value="nfs" />
          NFS
        </label>
      </div>
    </div>

    <div class="field">
      <label for="share-host">Server</label>
      <input id="share-host" type="text" bind:value={host} placeholder="nas.local or 192.168.1.10" />
    </div>

    <div class="field">
      <label for="share-path">{protocol === 'nfs' ? 'Export Path' : 'Share Name'}</label>
      <input id="share-path" type="text" bind:value={share} placeholder={protocol === 'nfs' ? '/exports/data' : 'media'} />
    </div>

    {#if protocol === 'smb'}
      <div class="field-row">
        <div class="field" style="flex:2">
          <label for="share-username">Username <span class="optional">(optional)</span></label>
          <input id="share-username" type="text" bind:value={username} placeholder="guest if empty" />
        </div>
        <div class="field" style="flex:1">
          <label for="share-domain">Domain <span class="optional">(optional)</span></label>
          <input id="share-domain" type="text" bind:value={domain} placeholder="WORKGROUP" />
        </div>
      </div>
      <div class="field">
        <label for="share-password">Password <span class="optional">(optional)</span></label>
        <input id="share-password" type="password" bind:value={password} placeholder="Enter password" />
      </div>
    {:else}
      <div class="hint-msg">
        NFS mounts have no credentials. On Linux, NFS requires mounting manually with root —
        this works on macOS only.
      </div>
    {/if}
  </div>

  <div class="main-footer">
    <button class="dialog-btn primary" onclick={handleConnect} disabled={!canConnect() || connecting}>
      {connecting ? 'Mounting...' : 'Mount'}
    </button>
    <button class="dialog-btn" onclick={onCancel}>Cancel</button>
  </div>
</div>

<style>
  .share-dialog {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }

  .main-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    overflow-y: auto;
  }

  .conn-tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: 8px;
    flex-shrink: 0;
  }

  .conn-tab-btn {
    padding: 6px 16px;
    font-size: 12px;
    font-family: inherit;
    font-weight: 500;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    cursor: default;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .conn-tab-btn.active {
    border-bottom: 2px solid var(--text-accent);
    color: var(--text-accent);
  }

  .main-footer {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 10px;
    padding: 16px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .field-row {
    display: flex;
    gap: 12px;
  }

  .field input[type="text"],
  .field input[type="password"] {
    padding: 7px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
  }

  .field input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .auth-options {
    display: flex;
    gap: 16px;
    padding: 4px 0;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .optional {
    font-weight: 400;
    opacity: 0.6;
  }

  .error-msg {
    font-size: 12px;
    color: var(--warning-color);
    padding: 8px 12px;
    background: rgba(255, 100, 100, 0.1);
    border-radius: var(--radius-sm);
    border: 1px solid rgba(255, 100, 100, 0.2);
  }

  .hint-msg {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 8px 12px;
    background: var(--bg-hover);
    border-radius: var(--radius-sm);
    line-height: 1.4;
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

  .dialog-btn:hover:not(:disabled) {
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
