<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { s3CheckCredentials } from '$lib/services/s3';
  import type { S3Profile } from '$lib/types';

  interface Props {
    onConnect: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string) => void;
    onCancel: () => void;
    saveMode?: boolean;
    initialData?: S3Profile;
    onSave?: (profile: Omit<S3Profile, 'id'> & { id?: string }, secretKey?: string) => void;
  }

  let { onConnect, onCancel, saveMode = false, initialData, onSave }: Props = $props();

  const init = untrack(() => initialData);

  let name = $state(init?.name ?? '');
  let bucket = $state(init?.bucket ?? '');
  let region = $state(init?.region ?? 'us-east-1');
  let endpoint = $state(init?.endpoint ?? '');
  let profile = $state(init?.profile ?? '');
  let accessKey = $state(init?.accessKeyId ?? '');
  let secretKey = $state('');
  let showManualCreds = $state(init?.credentialType === 'keychain' || false);
  let hasDefaultCreds = $state(true);
  let checking = $state(true);
  let bucketEl: HTMLInputElement | undefined = $state(undefined);
  let nameEl: HTMLInputElement | undefined = $state(undefined);

  const isEditing = !!init;

  onMount(async () => {
    try {
      hasDefaultCreds = await s3CheckCredentials();
      if (!hasDefaultCreds && !isEditing) {
        showManualCreds = true;
      }
    } catch {
      hasDefaultCreds = false;
      if (!isEditing) showManualCreds = true;
    } finally {
      checking = false;
    }
    if (saveMode && nameEl) {
      nameEl.focus();
    } else if (bucketEl) {
      bucketEl.focus();
    }
  });

  function buildProfile(): Omit<S3Profile, 'id'> & { id?: string } {
    const credentialType = showManualCreds && accessKey.trim() ? 'keychain' as const : profile.trim() ? 'aws-profile' as const : 'default' as const;
    return {
      ...(init ? { id: init.id } : {}),
      name: name.trim(),
      bucket: bucket.trim(),
      region: region.trim() || 'us-east-1',
      ...(endpoint.trim() ? { endpoint: endpoint.trim() } : {}),
      ...(profile.trim() ? { profile: profile.trim() } : {}),
      credentialType,
      ...(credentialType === 'keychain' && accessKey.trim() ? { accessKeyId: accessKey.trim() } : {}),
    };
  }

  function handleConnect() {
    if (!bucket.trim()) return;
    onConnect(
      bucket.trim(),
      region.trim() || 'us-east-1',
      endpoint.trim() || undefined,
      profile.trim() || undefined,
      showManualCreds && accessKey.trim() ? accessKey.trim() : undefined,
      showManualCreds && secretKey.trim() ? secretKey.trim() : undefined,
    );
  }

  function handleSaveAndConnect() {
    if (!bucket.trim() || !name.trim()) return;
    const p = buildProfile();
    const sk = showManualCreds && secretKey.trim() ? secretKey.trim() : undefined;
    onSave?.(p, sk);
    onConnect(
      bucket.trim(),
      region.trim() || 'us-east-1',
      endpoint.trim() || undefined,
      profile.trim() || undefined,
      showManualCreds && accessKey.trim() ? accessKey.trim() : undefined,
      showManualCreds && secretKey.trim() ? secretKey.trim() : undefined,
    );
  }

  function handleSave() {
    if (!bucket.trim() || !name.trim()) return;
    const p = buildProfile();
    const sk = showManualCreds && secretKey.trim() ? secretKey.trim() : undefined;
    onSave?.(p, sk);
    onCancel();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      if (saveMode) {
        handleSaveAndConnect();
      } else {
        handleConnect();
      }
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
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">{isEditing ? 'Edit S3 Connection' : 'Connect to S3-Compatible Storage'}</div>
    <div class="dialog-body">
      {#if saveMode}
        <label class="field-label">
          Connection Name
          <input
            type="text"
            class="dialog-input"
            autocomplete="off"
            bind:value={name}
            bind:this={nameEl}
            placeholder="My S3 Bucket"
          />
        </label>
      {/if}

      <label class="field-label">
        Bucket
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={bucket}
          bind:this={bucketEl}
          placeholder="my-bucket-name"
        />
      </label>

      <label class="field-label">
        Region
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={region}
          placeholder="us-east-1"
        />
      </label>

      <label class="field-label">
        Endpoint (optional — for non-AWS providers)
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={endpoint}
          placeholder="https://us-east-1.linodeobjects.com"
        />
        <span class="field-hint">Linode, DigitalOcean, MinIO, Backblaze, etc. Leave empty for AWS.</span>
      </label>

      <label class="field-label">
        Profile (optional)
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={profile}
          placeholder="default"
        />
      </label>

      <div class="creds-toggle">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={showManualCreds} />
          Manual credentials
        </label>
        {#if checking}
          <span class="creds-status">Checking credentials...</span>
        {:else if hasDefaultCreds}
          <span class="creds-status ok">Default credentials found</span>
        {:else}
          <span class="creds-status warn">No default credentials</span>
        {/if}
      </div>

      {#if showManualCreds}
        <label class="field-label">
          Access Key
          <input
            type="text"
            class="dialog-input"
            autocomplete="off"
            bind:value={accessKey}
            placeholder="AKIA..."
          />
        </label>

        <label class="field-label">
          Secret Key
          <input
            type="password"
            class="dialog-input"
            autocomplete="off"
            bind:value={secretKey}
            placeholder={isEditing ? 'Leave empty to keep current' : 'secret'}
          />
        </label>
      {/if}

      <div class="dialog-buttons">
        {#if saveMode}
          <button class="dialog-btn primary" onclick={handleSaveAndConnect} disabled={!bucket.trim() || !name.trim()}>Save & Connect</button>
          {#if !isEditing}
            <button class="dialog-btn" onclick={handleConnect} disabled={!bucket.trim()}>Connect Without Saving</button>
          {:else}
            <button class="dialog-btn" onclick={handleSave} disabled={!bucket.trim() || !name.trim()}>Save</button>
          {/if}
        {:else}
          <button class="dialog-btn primary" onclick={handleConnect} disabled={!bucket.trim()}>Connect</button>
        {/if}
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
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .field-label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .field-hint {
    font-size: 11px;
    color: var(--text-secondary);
    opacity: 0.7;
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
    box-sizing: border-box;
  }

  .dialog-input:focus {
    border-color: var(--border-active);
    box-shadow: 0 0 0 1px rgba(110,168,254,0.3);
  }

  .creds-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 0;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }

  .creds-status {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .creds-status.ok {
    color: var(--success-color);
  }

  .creds-status.warn {
    color: var(--warning-color);
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    gap: 10px;
    margin-top: 4px;
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
