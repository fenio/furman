<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { s3CheckCredentials, s3ListBuckets, s3CreateBucket, s3DeleteBucket } from '$lib/services/s3';
  import { S3_PROVIDERS, getProvider, inferProviderFromEndpoint } from '$lib/data/s3-providers';
  import type { S3Bucket, S3Profile, S3ProviderCapabilities } from '$lib/types';

  interface Props {
    onConnect: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities, roleArn?: string, externalId?: string, sessionName?: string, sessionDurationSecs?: number, useTransferAcceleration?: boolean) => void;
    onCancel: () => void;
    saveMode?: boolean;
    initialData?: S3Profile;
    onSave?: (profile: Omit<S3Profile, 'id'> & { id?: string }, secretKey?: string) => void;
    embedded?: boolean;
  }

  let { onConnect, onCancel, saveMode = false, initialData, onSave, embedded = false }: Props = $props();

  const init = untrack(() => initialData);

  let name = $state(init?.name ?? '');
  let bucket = $state(init?.bucket ?? '');
  let region = $state(init?.region ?? 'us-east-1');
  let endpoint = $state(init?.endpoint ?? '');
  let profile = $state(init?.profile ?? '');
  let accessKey = $state(init?.accessKeyId ?? '');
  let secretKey = $state('');
  let selectedProvider = $state(init?.provider ?? 'aws');
  let useDefaultCreds = $state(init?.credentialType !== 'keychain');
  let hasDefaultCreds = $state(true);
  let checking = $state(true);
  let bucketEl: HTMLInputElement | undefined = $state(undefined);
  let nameEl: HTMLInputElement | undefined = $state(undefined);
  let buckets = $state<S3Bucket[]>([]);
  let browsing = $state(false);
  let browseError = $state('');
  let showBucketList = $state(false);
  let showCustomCaps = $state(false);

  // AssumeRole
  let roleArn = $state(init?.roleArn ?? '');
  let externalIdVal = $state(init?.externalId ?? '');
  let sessionDuration = $state(init?.sessionDurationSecs ?? 3600);
  let showAssumeRole = $state(!!(init?.roleArn));
  let useAcceleration = $state(init?.useTransferAcceleration ?? false);
  let defaultEncryption = $state(init?.defaultClientEncryption ?? false);

  // Custom capabilities (for 'custom' provider)
  let customCaps = $state<S3ProviderCapabilities>(init?.customCapabilities ?? { ...getProvider('custom').capabilities });

  // Create / delete bucket
  let showCreateForm = $state(false);
  let newBucketName = $state('');
  let creatingBucket = $state(false);
  let createError = $state('');
  let deletingBucket = $state<string | null>(null);

  const isEditing = !!init;

  const currentProvider = $derived(getProvider(selectedProvider));

  function handleProviderChange(e: Event) {
    const id = (e.target as HTMLSelectElement).value;
    selectedProvider = id;
    const p = getProvider(id);
    if (p.regionHint && !region.trim()) {
      region = p.regionHint;
    }
    if (id === 'custom') {
      customCaps = { ...getProvider('custom').capabilities };
    }
  }

  function handleEndpointBlur() {
    const inferred = inferProviderFromEndpoint(endpoint.trim() || undefined);
    if (inferred !== selectedProvider && inferred !== 'custom') {
      selectedProvider = inferred;
      const p = getProvider(inferred);
      if (p.regionHint && (!region.trim() || region === 'us-east-1')) {
        region = p.regionHint;
      }
    }
  }

  onMount(async () => {
    try {
      hasDefaultCreds = await s3CheckCredentials();
    } catch {
      hasDefaultCreds = false;
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
    const credentialType = !useDefaultCreds && accessKey.trim() ? 'keychain' as const : profile.trim() ? 'aws-profile' as const : 'default' as const;
    return {
      ...(init ? { id: init.id } : {}),
      name: name.trim(),
      bucket: bucket.trim(),
      region: region.trim() || 'us-east-1',
      ...(endpoint.trim() ? { endpoint: endpoint.trim() } : {}),
      ...(profile.trim() ? { profile: profile.trim() } : {}),
      credentialType,
      ...(credentialType === 'keychain' && accessKey.trim() ? { accessKeyId: accessKey.trim() } : {}),
      provider: selectedProvider,
      ...(selectedProvider === 'custom' ? { customCapabilities: { ...customCaps } } : {}),
      ...(roleArn.trim() ? { roleArn: roleArn.trim() } : {}),
      ...(externalIdVal.trim() ? { externalId: externalIdVal.trim() } : {}),
      ...(roleArn.trim() ? { sessionDurationSecs: sessionDuration } : {}),
      ...(useAcceleration ? { useTransferAcceleration: true } : {}),
      ...(defaultEncryption ? { defaultClientEncryption: true } : {}),
    };
  }

  function handleConnect() {
    if (!bucket.trim()) return;
    onConnect(
      bucket.trim(),
      region.trim() || 'us-east-1',
      endpoint.trim() || undefined,
      profile.trim() || undefined,
      !useDefaultCreds && accessKey.trim() ? accessKey.trim() : undefined,
      !useDefaultCreds && secretKey.trim() ? secretKey.trim() : undefined,
      selectedProvider,
      selectedProvider === 'custom' ? { ...customCaps } : undefined,
      roleArn.trim() || undefined,
      externalIdVal.trim() || undefined,
      roleArn.trim() ? undefined : undefined, // sessionName uses default
      roleArn.trim() ? sessionDuration : undefined,
      useAcceleration || undefined,
    );
  }

  function handleSaveAndConnect() {
    if (!bucket.trim() || !name.trim()) return;
    const p = buildProfile();
    const sk = !useDefaultCreds && secretKey.trim() ? secretKey.trim() : undefined;
    onSave?.(p, sk);
    onConnect(
      bucket.trim(),
      region.trim() || 'us-east-1',
      endpoint.trim() || undefined,
      profile.trim() || undefined,
      !useDefaultCreds && accessKey.trim() ? accessKey.trim() : undefined,
      !useDefaultCreds && secretKey.trim() ? secretKey.trim() : undefined,
      selectedProvider,
      selectedProvider === 'custom' ? { ...customCaps } : undefined,
      roleArn.trim() || undefined,
      externalIdVal.trim() || undefined,
      roleArn.trim() ? undefined : undefined,
      roleArn.trim() ? sessionDuration : undefined,
      useAcceleration || undefined,
    );
  }

  function handleSave() {
    if (!bucket.trim() || !name.trim()) return;
    const p = buildProfile();
    const sk = !useDefaultCreds && secretKey.trim() ? secretKey.trim() : undefined;
    onSave?.(p, sk);
    onCancel();
  }

  async function handleBrowse() {
    browsing = true;
    browseError = '';
    try {
      const [ep, prof, ak, sk, rArn, extId, sName, sDur] = currentCredArgs();
      buckets = await s3ListBuckets(
        region.trim() || 'us-east-1',
        ep, prof, ak, sk, rArn, extId, sName, sDur,
      );
      showBucketList = true;
    } catch (e: any) {
      browseError = e?.message ?? String(e);
      showBucketList = false;
    } finally {
      browsing = false;
    }
  }

  function selectBucket(name: string) {
    bucket = name;
    showBucketList = false;
  }

  function currentCredArgs(): [string | undefined, string | undefined, string | undefined, string | undefined, string | undefined, string | undefined, string | undefined, number | undefined] {
    return [
      endpoint.trim() || undefined,
      profile.trim() || undefined,
      !useDefaultCreds && accessKey.trim() ? accessKey.trim() : undefined,
      !useDefaultCreds && secretKey.trim() ? secretKey.trim() : undefined,
      roleArn.trim() || undefined,
      externalIdVal.trim() || undefined,
      undefined, // sessionName
      roleArn.trim() ? sessionDuration : undefined,
    ];
  }

  async function handleCreateBucket() {
    if (!newBucketName.trim()) return;
    creatingBucket = true;
    createError = '';
    try {
      const r = region.trim() || 'us-east-1';
      const [ep, prof, ak, sk, rArn, extId, sName, sDur] = currentCredArgs();
      await s3CreateBucket(r, newBucketName.trim(), ep, prof, ak, sk, rArn, extId, sName, sDur);
      // Refresh bucket list
      buckets = await s3ListBuckets(r, ep, prof, ak, sk, rArn, extId, sName, sDur);
      showBucketList = true;
      newBucketName = '';
      showCreateForm = false;
    } catch (e: any) {
      createError = e?.message ?? String(e);
    } finally {
      creatingBucket = false;
    }
  }

  async function handleDeleteBucket(bucketName: string) {
    if (!confirm(`Delete bucket "${bucketName}"? The bucket must be empty.`)) return;
    deletingBucket = bucketName;
    browseError = '';
    try {
      const r = region.trim() || 'us-east-1';
      const [ep, prof, ak, sk, rArn, extId, sName, sDur] = currentCredArgs();
      await s3DeleteBucket(r, bucketName, ep, prof, ak, sk, rArn, extId, sName, sDur);
      buckets = buckets.filter(b => b.name !== bucketName);
      if (bucket === bucketName) bucket = '';
    } catch (e: any) {
      browseError = e?.message ?? String(e);
    } finally {
      deletingBucket = null;
    }
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

{#snippet formBody()}
    <div class="dialog-body">
      <div class="field-label">
        Provider
        <div class="provider-row">
          <img class="provider-icon" src={currentProvider.icon} alt={currentProvider.name} />
          <select class="dialog-input provider-select" value={selectedProvider} onchange={handleProviderChange}>
            {#each S3_PROVIDERS as p}
              <option value={p.id}>{p.name}</option>
            {/each}
          </select>
        </div>
      </div>

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

      <div class="field-label">
        Bucket
        <div class="bucket-row">
          <input
            type="text"
            class="dialog-input"
            autocomplete="off"
            bind:value={bucket}
            bind:this={bucketEl}
            placeholder="my-bucket-name"
          />
          <button class="dialog-btn browse-btn" onclick={handleBrowse} disabled={browsing}>
            {browsing ? 'Loading...' : 'Browse'}
          </button>
        </div>
        {#if showBucketList && buckets.length > 0}
          <div class="bucket-list">
            {#each buckets as b}
              <div class="bucket-item-row">
                <button class="bucket-item" onclick={() => selectBucket(b.name)}>
                  <span class="bucket-name">{b.name}</span>
                  {#if b.created}
                    <span class="bucket-date">{new Date(b.created).toLocaleDateString()}</span>
                  {/if}
                </button>
                <button
                  class="bucket-delete-btn"
                  onclick={() => handleDeleteBucket(b.name)}
                  disabled={deletingBucket === b.name}
                  title="Delete bucket"
                >&times;</button>
              </div>
            {/each}
          </div>
        {:else if showBucketList && buckets.length === 0}
          <span class="field-hint">No buckets found.</span>
        {/if}
        {#if showBucketList}
          {#if showCreateForm}
            <div class="create-bucket-form">
              <input
                type="text"
                class="dialog-input create-input"
                bind:value={newBucketName}
                placeholder="new-bucket-name"
                onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); handleCreateBucket(); } }}
              />
              <button class="dialog-btn create-btn" onclick={handleCreateBucket} disabled={creatingBucket || !newBucketName.trim()}>
                {creatingBucket ? 'Creating...' : 'Create'}
              </button>
              <button class="dialog-btn" onclick={() => { showCreateForm = false; createError = ''; }}>Cancel</button>
            </div>
            {#if createError}
              <span class="browse-error">{createError}</span>
            {/if}
          {:else}
            <button class="dialog-btn create-trigger" onclick={() => { showCreateForm = true; createError = ''; }}>
              + Create Bucket
            </button>
          {/if}
        {/if}
        {#if browseError}
          <span class="browse-error">{browseError}</span>
        {/if}
      </div>

      <label class="field-label">
        Region
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={region}
          placeholder={currentProvider.regionHint || 'us-east-1'}
        />
      </label>

      <label class="field-label">
        Endpoint {selectedProvider === 'aws' ? '(optional)' : ''}
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={endpoint}
          placeholder={currentProvider.endpointHint || 'https://us-east-1.linodeobjects.com'}
          onblur={handleEndpointBlur}
        />
        <span class="field-hint">Leave empty for AWS. Provider is auto-detected from endpoint.</span>
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
        <span class="field-hint">SSO profiles work if you've run `aws sso login`</span>
      </label>

      {#if !checking && hasDefaultCreds}
        <div class="creds-toggle">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={useDefaultCreds} />
            Use default credentials
          </label>
          <span class="creds-status ok">Default credentials found</span>
        </div>
      {/if}

      <label class="field-label">
        Access Key
        <input
          type="text"
          class="dialog-input"
          autocomplete="off"
          bind:value={accessKey}
          placeholder="AKIA..."
          disabled={useDefaultCreds && hasDefaultCreds}
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
          disabled={useDefaultCreds && hasDefaultCreds}
        />
      </label>

      {#if !endpoint.trim()}
        <div class="creds-toggle">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={useAcceleration} />
            Transfer Acceleration
          </label>
          <span class="field-hint">Route transfers through CloudFront edge locations</span>
        </div>
      {/if}

      <div class="creds-toggle">
        <label class="checkbox-label">
          <input type="checkbox" bind:checked={defaultEncryption} />
          Client-side encryption by default
        </label>
        <span class="field-hint">Prompt for password when uploading to this bucket</span>
      </div>

      <div class="caps-section">
        <button class="caps-toggle" onclick={() => { showAssumeRole = !showAssumeRole; }}>
          AssumeRole (optional) {showAssumeRole ? '\u25B4' : '\u25BE'}
        </button>
        {#if showAssumeRole}
          <label class="field-label">
            Role ARN
            <input
              type="text"
              class="dialog-input"
              autocomplete="off"
              bind:value={roleArn}
              placeholder="arn:aws:iam::123456789012:role/RoleName"
            />
          </label>
          <label class="field-label">
            External ID (optional)
            <input
              type="text"
              class="dialog-input"
              autocomplete="off"
              bind:value={externalIdVal}
              placeholder="External ID"
            />
          </label>
          <label class="field-label">
            Session Duration
            <select class="dialog-input" bind:value={sessionDuration}>
              <option value={900}>15 minutes</option>
              <option value={1800}>30 minutes</option>
              <option value={3600}>1 hour</option>
              <option value={7200}>2 hours</option>
              <option value={14400}>4 hours</option>
              <option value={43200}>12 hours</option>
            </select>
          </label>
        {/if}
      </div>

      {#if selectedProvider === 'custom'}
        <div class="caps-section">
          <button class="caps-toggle" onclick={() => { showCustomCaps = !showCustomCaps; }}>
            Capabilities {showCustomCaps ? '\u25B4' : '\u25BE'}
          </button>
          {#if showCustomCaps}
            <div class="caps-grid">
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.versioning} /> Versioning</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.lifecycleRules} /> Lifecycle Rules</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.cors} /> CORS</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.bucketPolicy} /> Bucket Policy</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.acl} /> ACL</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.publicAccessBlock} /> Public Access Block</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.encryption} /> Encryption</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.glacierRestore} /> Glacier Restore</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.presignedUrls} /> Presigned URLs</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.objectMetadata} /> Object Metadata</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.objectTags} /> Object Tags</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.bucketTags} /> Bucket Tags</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.multipartUploadCleanup} /> Multipart Cleanup</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.websiteHosting} /> Website Hosting</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.requesterPays} /> Requester Pays</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.objectOwnership} /> Object Ownership</label>
              <label class="caps-checkbox"><input type="checkbox" bind:checked={customCaps.serverAccessLogging} /> Access Logging</label>
            </div>
          {/if}
        </div>
      {/if}

    </div>
{/snippet}

{#snippet formButtons()}
    <div class="dialog-footer">
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
{/snippet}

{#if embedded}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="embedded-wrapper" role="group" onkeydown={handleKeydown}>
    {@render formBody()}
    {@render formButtons()}
  </div>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="dialog-overlay no-select"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onkeydown={handleKeydown}
  >
    <div class="dialog-box">
      <div class="dialog-title">{isEditing ? 'Edit S3 Connection' : 'Connect to S3-Compatible Storage'}</div>
      {@render formBody()}
      {@render formButtons()}
    </div>
  </div>
{/if}

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
    height: 85vh;
    max-width: 90vw;
    max-height: 900px;
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
  }

  .embedded-wrapper {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    overflow-y: auto;
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

  .dialog-input:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .dialog-footer {
    display: flex;
    justify-content: center;
    gap: 10px;
    padding: 16px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
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

  .bucket-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .bucket-row .dialog-input {
    flex: 1;
  }

  .browse-btn {
    flex-shrink: 0;
    white-space: nowrap;
  }

  .bucket-list {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
  }

  .bucket-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
  }

  .bucket-item:hover {
    background: var(--bg-hover);
  }

  .bucket-name {
    font-family: inherit;
  }

  .bucket-date {
    color: var(--text-secondary);
    font-size: 11px;
    margin-left: 12px;
    flex-shrink: 0;
  }

  .browse-error {
    font-size: 11px;
    color: var(--warning-color);
  }

  .bucket-item-row {
    display: flex;
    align-items: center;
  }

  .bucket-item-row .bucket-item {
    flex: 1;
  }

  .bucket-delete-btn {
    padding: 2px 8px;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 16px;
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
    flex-shrink: 0;
  }

  .bucket-item-row:hover .bucket-delete-btn {
    opacity: 1;
  }

  .bucket-delete-btn:hover {
    color: var(--text-error, #ff6b6b);
  }

  .bucket-delete-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .create-bucket-form {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .create-input {
    flex: 1;
    padding: 6px 10px !important;
    font-size: 13px !important;
  }

  .create-btn {
    flex-shrink: 0;
    white-space: nowrap;
  }

  .create-trigger {
    align-self: flex-start;
    font-size: 12px;
    padding: 4px 12px;
  }

  .provider-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .provider-icon {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
  }

  .provider-select {
    flex: 1;
    cursor: pointer;
  }

  .caps-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .caps-toggle {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    padding: 4px 0;
    font-family: inherit;
  }

  .caps-toggle:hover {
    color: var(--text-primary);
  }

  .caps-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 16px;
  }

  .caps-checkbox {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }
</style>
