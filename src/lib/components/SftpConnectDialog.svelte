<script lang="ts">
  import { untrack } from 'svelte';
  import type { SftpProfile } from '$lib/types';

  interface Props {
    onConnect: (host: string, port: number, username: string, authMethod: string, password?: string, keyPath?: string, keyPassphrase?: string) => void;
    onCancel: () => void;
    saveMode?: boolean;
    initialData?: SftpProfile;
    onSave?: (profile: Omit<SftpProfile, 'id'> & { id?: string }, password?: string) => void;
    embedded?: boolean;
  }

  let { onConnect, onCancel, saveMode = false, initialData, onSave, embedded = false }: Props = $props();

  const init = untrack(() => initialData);

  let name = $state(init?.name ?? '');
  let host = $state(init?.host ?? '');
  let port = $state(init?.port ?? 22);
  let username = $state(init?.username ?? '');
  let authMethod = $state<'password' | 'key' | 'agent'>(init?.authMethod ?? 'password');
  let password = $state('');
  let keyPath = $state(init?.keyPath ?? '');
  let keyPassphrase = $state('');
  let connecting = $state(false);
  let connectError = $state('');

  function canConnect(): boolean {
    if (!host || !username) return false;
    if (authMethod === 'password' && !password) return false;
    if (authMethod === 'key' && !keyPath) return false;
    return true;
  }

  async function handleConnect() {
    if (!canConnect()) return;
    connecting = true;
    connectError = '';
    try {
      onConnect(
        host,
        port,
        username,
        authMethod,
        authMethod === 'password' ? password : undefined,
        authMethod === 'key' ? keyPath : undefined,
        authMethod === 'key' ? keyPassphrase || undefined : undefined,
      );
    } catch (err: unknown) {
      connectError = err instanceof Error ? err.message : String(err);
    } finally {
      connecting = false;
    }
  }

  async function handleSaveAndConnect() {
    if (!canConnect() || !name) return;
    connecting = true;
    connectError = '';
    try {
      if (onSave) {
        const profile: Omit<SftpProfile, 'id'> & { id?: string } = {
          type: 'sftp',
          name,
          host,
          port,
          username,
          authMethod,
          keyPath: authMethod === 'key' ? keyPath : undefined,
        };
        if (init?.id) profile.id = init.id;
        onSave(profile, authMethod === 'password' ? password : undefined);
      }
      onConnect(
        host,
        port,
        username,
        authMethod,
        authMethod === 'password' ? password : undefined,
        authMethod === 'key' ? keyPath : undefined,
        authMethod === 'key' ? keyPassphrase || undefined : undefined,
      );
    } catch (err: unknown) {
      connectError = err instanceof Error ? err.message : String(err);
    } finally {
      connecting = false;
    }
  }

  function handleSaveOnly() {
    if (!name) return;
    if (onSave) {
      const profile: Omit<SftpProfile, 'id'> & { id?: string } = {
        type: 'sftp',
        name,
        host,
        port,
        username,
        authMethod,
        keyPath: authMethod === 'key' ? keyPath : undefined,
      };
      if (init?.id) profile.id = init.id;
      onSave(profile, authMethod === 'password' ? password : undefined);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && canConnect()) {
      e.preventDefault();
      if (saveMode && name) {
        handleSaveAndConnect();
      } else {
        handleConnect();
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="sftp-dialog" class:embedded onkeydown={handleKeydown} role="form">
  <div class="main-body">
    {#if connectError}
      <div class="error-msg">{connectError}</div>
    {/if}

    {#if saveMode}
      <div class="field">
        <label for="sftp-name">Connection Name</label>
        <input id="sftp-name" type="text" bind:value={name} placeholder="My Server" />
      </div>
    {/if}

    <div class="field-row">
      <div class="field" style="flex:3">
        <label for="sftp-host">Host</label>
        <input id="sftp-host" type="text" bind:value={host} placeholder="example.com" />
      </div>
      <div class="field" style="flex:1">
        <label for="sftp-port">Port</label>
        <input id="sftp-port" type="number" bind:value={port} min="1" max="65535" />
      </div>
    </div>

    <div class="field">
      <label for="sftp-username">Username</label>
      <input id="sftp-username" type="text" bind:value={username} placeholder="user" />
    </div>

    <div class="field">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label>Authentication</label>
      <div class="auth-options">
        <label class="radio-label">
          <input type="radio" bind:group={authMethod} value="password" />
          Password
        </label>
        <label class="radio-label">
          <input type="radio" bind:group={authMethod} value="key" />
          SSH Key
        </label>
        <label class="radio-label">
          <input type="radio" bind:group={authMethod} value="agent" />
          SSH Agent
        </label>
      </div>
    </div>

    {#if authMethod === 'password'}
      <div class="field">
        <label for="sftp-password">Password</label>
        <input id="sftp-password" type="password" bind:value={password} placeholder="Enter password" />
      </div>
    {:else if authMethod === 'key'}
      <div class="field">
        <label for="sftp-keypath">Key File</label>
        <input id="sftp-keypath" type="text" bind:value={keyPath} placeholder="~/.ssh/id_rsa" />
      </div>
      <div class="field">
        <label for="sftp-passphrase">Key Passphrase <span class="optional">(optional)</span></label>
        <input id="sftp-passphrase" type="password" bind:value={keyPassphrase} placeholder="Enter passphrase" />
      </div>
    {/if}
  </div>

  <div class="main-footer">
    {#if saveMode}
      <button class="dialog-btn primary" onclick={handleSaveAndConnect} disabled={!canConnect() || !name || connecting}>
        {connecting ? 'Connecting...' : 'Save & Connect'}
      </button>
      {#if !init?.id}
        <button class="dialog-btn" onclick={handleConnect} disabled={!canConnect() || connecting}>
          {connecting ? 'Connecting...' : 'Connect Without Saving'}
        </button>
      {:else}
        <button class="dialog-btn" onclick={handleSaveOnly} disabled={!name}>Save</button>
      {/if}
    {:else}
      <button class="dialog-btn primary" onclick={handleConnect} disabled={!canConnect() || connecting}>
        {connecting ? 'Connecting...' : 'Connect'}
      </button>
    {/if}
    <button class="dialog-btn" onclick={onCancel}>Cancel</button>
  </div>
</div>

<style>
  .sftp-dialog {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .main-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    flex: 1;
    overflow-y: auto;
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
    font-weight: 500;
    color: var(--text-secondary);
  }

  .field-row {
    display: flex;
    gap: 12px;
  }

  .field input[type="text"],
  .field input[type="password"],
  .field input[type="number"] {
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
