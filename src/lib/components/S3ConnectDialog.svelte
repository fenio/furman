<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { s3CheckCredentials, s3ListBuckets, s3CreateBucket, s3DeleteBucket } from '$lib/services/s3';
  import { S3_PROVIDERS, getProvider, inferProviderFromEndpoint } from '$lib/data/s3-providers';
  import type { S3ProviderProfile, S3ProviderRegion } from '$lib/data/s3-providers';
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
  let encryptionCipher = $state<'aes-256-gcm' | 'chacha20-poly1305'>(init?.encryptionCipher ?? 'aes-256-gcm');
  let kdfMemoryCost = $state(init?.kdfMemoryCost ?? 19456);
  let kdfTimeCost = $state(init?.kdfTimeCost ?? 2);
  let kdfParallelism = $state(init?.kdfParallelism ?? 1);
  let autoEncryptMinSize = $state(init?.autoEncryptMinSize ?? 0);
  let autoEncryptExtensions = $state(init?.autoEncryptExtensions?.join(', ') ?? '');
  let showEncryptionSettings = $state(false);

  // Custom capabilities (for 'custom' provider)
  let customCaps = $state<S3ProviderCapabilities>(init?.customCapabilities ?? { ...getProvider('custom').capabilities });

  // Create / delete bucket
  let showCreateForm = $state(false);
  let newBucketName = $state('');
  let creatingBucket = $state(false);
  let createError = $state('');
  let deletingBucket = $state<string | null>(null);

  // Provider search combobox
  let providerQuery = $state('');
  let providerDropdownOpen = $state(false);
  let providerHighlight = $state(-1);
  let providerInputEl: HTMLInputElement | undefined = $state(undefined);
  let providerListEl: HTMLDivElement | undefined = $state(undefined);

  // Region selector
  let selectedRegionId = $state('_custom');

  const isEditing = !!init;

  const currentProvider = $derived(getProvider(selectedProvider));
  const providerRegions = $derived(currentProvider.regions ?? []);

  const filteredProviders = $derived.by(() => {
    const q = providerQuery.toLowerCase().trim();
    if (!q) return S3_PROVIDERS;
    return S3_PROVIDERS.filter(p => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q));
  });

  function selectProvider(p: S3ProviderProfile) {
    selectedProvider = p.id;
    providerQuery = '';
    providerDropdownOpen = false;
    providerHighlight = -1;
    if (p.regionHint && !region.trim()) {
      region = p.regionHint;
    }
    if (p.id === 'custom') {
      customCaps = { ...getProvider('custom').capabilities };
    }
    // Reset region selector when switching providers
    selectedRegionId = '_custom';
  }

  function handleProviderInputFocus() {
    providerDropdownOpen = true;
    providerHighlight = -1;
  }

  function handleProviderInputBlur(e: FocusEvent) {
    // Don't close if clicking within the dropdown
    const related = e.relatedTarget as HTMLElement | null;
    if (related && providerListEl?.contains(related)) return;
    providerDropdownOpen = false;
    providerQuery = '';
    providerHighlight = -1;
  }

  function handleProviderInputKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (!providerDropdownOpen) {
        providerDropdownOpen = true;
      }
      providerHighlight = Math.min(providerHighlight + 1, filteredProviders.length - 1);
      scrollHighlightIntoView();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      providerHighlight = Math.max(providerHighlight - 1, 0);
      scrollHighlightIntoView();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      e.stopPropagation();
      if (providerDropdownOpen && providerHighlight >= 0 && providerHighlight < filteredProviders.length) {
        selectProvider(filteredProviders[providerHighlight]);
      } else if (providerDropdownOpen && filteredProviders.length === 1) {
        selectProvider(filteredProviders[0]);
      }
    } else if (e.key === 'Escape') {
      if (providerDropdownOpen) {
        e.preventDefault();
        e.stopPropagation();
        providerDropdownOpen = false;
        providerQuery = '';
        providerHighlight = -1;
      }
    }
  }

  function scrollHighlightIntoView() {
    requestAnimationFrame(() => {
      const el = providerListEl?.querySelector('.provider-option.highlighted');
      el?.scrollIntoView({ block: 'nearest' });
    });
  }

  function handleRegionChange(e: Event) {
    const value = (e.target as HTMLSelectElement).value;
    selectedRegionId = value;
    if (value === '_custom') return; // user will type manually

    const r = providerRegions.find(r => r.id === value || `${r.id}::${r.name}` === value);
    if (!r) return;
    if (r.endpoint) {
      endpoint = r.endpoint;
    }
    if (r.id) {
      region = r.id;
    }
  }

  function handleEndpointBlur() {
    const inferred = inferProviderFromEndpoint(endpoint.trim() || undefined);
    if (inferred !== selectedProvider && inferred !== 'custom') {
      selectedProvider = inferred;
      providerQuery = '';
      selectedRegionId = '_custom';
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
      ...(defaultEncryption && encryptionCipher !== 'aes-256-gcm' ? { encryptionCipher } : {}),
      ...(defaultEncryption && kdfMemoryCost !== 19456 ? { kdfMemoryCost } : {}),
      ...(defaultEncryption && kdfTimeCost !== 2 ? { kdfTimeCost } : {}),
      ...(defaultEncryption && kdfParallelism !== 1 ? { kdfParallelism } : {}),
      ...(defaultEncryption && autoEncryptMinSize > 0 ? { autoEncryptMinSize } : {}),
      ...(defaultEncryption && autoEncryptExtensions.trim() ? { autoEncryptExtensions: autoEncryptExtensions.split(',').map(s => s.trim()).filter(Boolean) } : {}),
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
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="provider-row">
          <img class="provider-icon" src={currentProvider.icon} alt={currentProvider.name} />
          <div class="provider-combobox" onkeydown={handleProviderInputKeydown}>
            <input
              type="text"
              class="dialog-input provider-search"
              autocomplete="off"
              placeholder={currentProvider.name}
              bind:value={providerQuery}
              bind:this={providerInputEl}
              onfocus={handleProviderInputFocus}
              onblur={handleProviderInputBlur}
            />
            {#if providerDropdownOpen}
              <div class="provider-dropdown" bind:this={providerListEl}>
                {#each filteredProviders as p, i}
                  <button
                    class="provider-option"
                    class:highlighted={i === providerHighlight}
                    class:selected={p.id === selectedProvider}
                    onmousedown={(e) => { e.preventDefault(); selectProvider(p); }}
                    onmouseenter={() => { providerHighlight = i; }}
                  >
                    <img class="provider-option-icon" src={p.icon} alt="" />
                    <span class="provider-option-name">{p.name}</span>
                  </button>
                {:else}
                  <div class="provider-option-empty">No matching providers</div>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </div>

      {#if providerRegions.length > 0}
        <label class="field-label">
          Region Preset
          <select class="dialog-input" value={selectedRegionId} onchange={handleRegionChange}>
            <option value="_custom">(Custom / manual entry)</option>
            {#each providerRegions as r}
              <option value={r.id === '' ? `${r.id}::${r.name}` : r.id}>
                {r.name}{r.id ? ` (${r.id})` : ''}
              </option>
            {/each}
          </select>
          <span class="field-hint">Pick a region to auto-fill endpoint and region, or choose Custom to enter your own.</span>
        </label>
      {/if}

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

      {#if defaultEncryption}
        <div class="caps-section">
          <button class="caps-toggle" onclick={() => { showEncryptionSettings = !showEncryptionSettings; }}>
            Encryption Settings {showEncryptionSettings ? '\u25B4' : '\u25BE'}
          </button>
          {#if showEncryptionSettings}
            <div class="encryption-settings">
              <label class="field-label">
                Cipher
                <select class="dialog-input" bind:value={encryptionCipher}>
                  <option value="aes-256-gcm">AES-256-GCM (default)</option>
                  <option value="chacha20-poly1305">ChaCha20-Poly1305</option>
                </select>
              </label>

              <div class="kdf-grid">
                <label class="field-label">
                  KDF Memory (KiB)
                  <select class="dialog-input" bind:value={kdfMemoryCost}>
                    <option value={8192}>8 MiB (faster)</option>
                    <option value={19456}>19 MiB (default)</option>
                    <option value={65536}>64 MiB</option>
                    <option value={131072}>128 MiB (stronger)</option>
                  </select>
                </label>
                <label class="field-label">
                  KDF Iterations
                  <select class="dialog-input" bind:value={kdfTimeCost}>
                    <option value={1}>1 (faster)</option>
                    <option value={2}>2 (default)</option>
                    <option value={4}>4</option>
                    <option value={8}>8 (stronger)</option>
                  </select>
                </label>
                <label class="field-label">
                  KDF Parallelism
                  <select class="dialog-input" bind:value={kdfParallelism}>
                    <option value={1}>1 (default)</option>
                    <option value={2}>2</option>
                    <option value={4}>4</option>
                  </select>
                </label>
              </div>
              <span class="field-hint">Higher KDF values = slower but more resistant to brute force</span>

              <label class="field-label">
                Auto-encrypt min size
                <select class="dialog-input" bind:value={autoEncryptMinSize}>
                  <option value={0}>Always encrypt (default)</option>
                  <option value={1024}>Skip if all files &lt; 1 KB</option>
                  <option value={10240}>Skip if all files &lt; 10 KB</option>
                  <option value={102400}>Skip if all files &lt; 100 KB</option>
                  <option value={1048576}>Skip if all files &lt; 1 MB</option>
                </select>
              </label>

              <label class="field-label">
                Encrypt only extensions (comma-separated)
                <input
                  type="text"
                  class="dialog-input"
                  bind:value={autoEncryptExtensions}
                  placeholder="e.g. pdf, docx, xlsx (empty = all)"
                />
                <span class="field-hint">Only trigger encryption when files match these extensions</span>
              </label>
            </div>
          {/if}
        </div>
      {/if}

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

  .provider-combobox {
    flex: 1;
    position: relative;
  }

  .provider-search {
    cursor: text;
  }

  .provider-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    max-height: 260px;
    overflow-y: auto;
    background: var(--dialog-bg);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-dialog);
    z-index: 10;
    display: flex;
    flex-direction: column;
  }

  .provider-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
  }

  .provider-option:hover,
  .provider-option.highlighted {
    background: var(--bg-hover);
  }

  .provider-option.selected {
    color: var(--text-accent);
  }

  .provider-option-icon {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
  }

  .provider-option-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .provider-option-empty {
    padding: 8px 10px;
    color: var(--text-secondary);
    font-size: 12px;
    text-align: center;
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

  .encryption-settings {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-left: 8px;
    border-left: 2px solid var(--border-subtle);
  }

  .kdf-grid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 8px;
  }
</style>
