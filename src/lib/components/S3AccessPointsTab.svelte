<script lang="ts">
  import { onMount } from 'svelte';
  import {
    s3ListAccessPoints,
    s3GetAccessPoint,
    s3CreateAccessPoint,
    s3DeleteAccessPoint,
    s3GetAccessPointPolicy,
    s3PutAccessPointPolicy,
    s3DeleteAccessPointPolicy,
  } from '$lib/services/s3';
  import type { S3AccessPoint, S3AccessPointDetail, S3PublicAccessBlock } from '$lib/types';

  interface Props {
    s3ConnectionId: string;
  }

  let { s3ConnectionId }: Props = $props();

  // ── State ───────────────────────────────────────────────────────────────
  let mode = $state<'list' | 'create' | 'detail'>('list');
  let loading = $state(true);
  let error = $state('');
  let message = $state('');
  let accessPoints = $state<S3AccessPoint[]>([]);

  // Create form
  let createName = $state('');
  let createNetwork = $state<'Internet' | 'VPC'>('Internet');
  let createVpcId = $state('');
  let createBlockPublicAcls = $state(true);
  let createIgnorePublicAcls = $state(true);
  let createBlockPublicPolicy = $state(true);
  let createRestrictPublicBuckets = $state(true);
  let creating = $state(false);

  // Detail view
  let detail = $state<S3AccessPointDetail | null>(null);
  let detailLoading = $state(false);
  let policy = $state('');
  let savedPolicy = $state('');
  let policyLoading = $state(false);
  let policySaving = $state(false);
  let policyDirty = $derived(policy !== savedPolicy);
  let policyError = $state('');
  let deleting = $state(false);

  // ── Load ────────────────────────────────────────────────────────────────

  onMount(() => { loadList(); });

  async function loadList() {
    loading = true;
    error = '';
    message = '';
    try {
      accessPoints = await s3ListAccessPoints(s3ConnectionId);
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load access points';
    } finally {
      loading = false;
    }
  }

  // ── Detail ──────────────────────────────────────────────────────────────

  async function showDetail(name: string) {
    mode = 'detail';
    detailLoading = true;
    detail = null;
    policy = '';
    savedPolicy = '';
    policyError = '';
    message = '';
    try {
      const [ap, pol] = await Promise.all([
        s3GetAccessPoint(s3ConnectionId, name),
        s3GetAccessPointPolicy(s3ConnectionId, name),
      ]);
      detail = ap;
      policy = pol;
      savedPolicy = pol;
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load access point detail';
      mode = 'list';
    } finally {
      detailLoading = false;
    }
  }

  // ── Policy ──────────────────────────────────────────────────────────────

  function validateJson(text: string): boolean {
    if (!text.trim()) return true;
    try {
      JSON.parse(text);
      return true;
    } catch {
      return false;
    }
  }

  async function savePolicy() {
    if (!detail) return;
    if (!validateJson(policy)) {
      policyError = 'Invalid JSON';
      return;
    }
    policySaving = true;
    policyError = '';
    try {
      await s3PutAccessPointPolicy(s3ConnectionId, detail.name, policy);
      savedPolicy = policy;
      message = 'Policy saved';
    } catch (e: any) {
      policyError = `Error: ${e}`;
    } finally {
      policySaving = false;
    }
  }

  async function deletePolicy() {
    if (!detail) return;
    policySaving = true;
    policyError = '';
    try {
      await s3DeleteAccessPointPolicy(s3ConnectionId, detail.name);
      policy = '';
      savedPolicy = '';
      message = 'Policy deleted';
    } catch (e: any) {
      policyError = `Error: ${e}`;
    } finally {
      policySaving = false;
    }
  }

  // ── Create ──────────────────────────────────────────────────────────────

  function startCreate() {
    createName = '';
    createNetwork = 'Internet';
    createVpcId = '';
    createBlockPublicAcls = true;
    createIgnorePublicAcls = true;
    createBlockPublicPolicy = true;
    createRestrictPublicBuckets = true;
    message = '';
    error = '';
    mode = 'create';
  }

  async function submitCreate() {
    const name = createName.trim();
    if (name.length < 3 || name.length > 255) {
      message = 'Error: Name must be 3–255 characters';
      return;
    }
    creating = true;
    message = '';
    try {
      const pab: S3PublicAccessBlock = {
        block_public_acls: createBlockPublicAcls,
        ignore_public_acls: createIgnorePublicAcls,
        block_public_policy: createBlockPublicPolicy,
        restrict_public_buckets: createRestrictPublicBuckets,
      };
      await s3CreateAccessPoint(
        s3ConnectionId,
        name,
        createNetwork === 'VPC' ? createVpcId.trim() || undefined : undefined,
        pab,
      );
      message = `Access point "${name}" created`;
      mode = 'list';
      await loadList();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      creating = false;
    }
  }

  // ── Delete ──────────────────────────────────────────────────────────────

  async function confirmDelete(name: string) {
    if (!confirm(`Delete access point "${name}"? This cannot be undone.`)) return;
    deleting = true;
    message = '';
    try {
      await s3DeleteAccessPoint(s3ConnectionId, name);
      message = `Access point "${name}" deleted`;
      mode = 'list';
      await loadList();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      deleting = false;
    }
  }

  function truncateArn(arn: string): string {
    if (arn.length <= 60) return arn;
    return arn.slice(0, 30) + '…' + arn.slice(-25);
  }
</script>

{#if mode === 'create'}
  <!-- ── Create Form ──────────────────────────────────────────── -->
  <div class="section-title">Create Access Point</div>
  <div class="ap-form">
    <div class="ap-field">
      <label class="ap-label">Name</label>
      <input class="ap-input" type="text" bind:value={createName}
        placeholder="my-access-point" minlength="3" maxlength="255" />
    </div>

    <div class="ap-field">
      <label class="ap-label">Network Origin</label>
      <select class="ap-input" bind:value={createNetwork}>
        <option value="Internet">Internet</option>
        <option value="VPC">VPC</option>
      </select>
    </div>

    {#if createNetwork === 'VPC'}
      <div class="ap-field">
        <label class="ap-label">VPC ID</label>
        <input class="ap-input" type="text" bind:value={createVpcId} placeholder="vpc-0123456789abcdef0" />
      </div>
    {/if}

    <div class="ap-field">
      <label class="ap-label">Public Access Block</label>
      <div class="ap-checkboxes">
        <label class="ap-checkbox">
          <input type="checkbox" bind:checked={createBlockPublicAcls} />
          Block public ACLs
        </label>
        <label class="ap-checkbox">
          <input type="checkbox" bind:checked={createIgnorePublicAcls} />
          Ignore public ACLs
        </label>
        <label class="ap-checkbox">
          <input type="checkbox" bind:checked={createBlockPublicPolicy} />
          Block public policy
        </label>
        <label class="ap-checkbox">
          <input type="checkbox" bind:checked={createRestrictPublicBuckets} />
          Restrict public buckets
        </label>
      </div>
    </div>

    <div class="ap-actions">
      <button class="dialog-btn apply-btn" onclick={submitCreate} disabled={creating}>
        {creating ? 'Creating...' : 'Create'}
      </button>
      <button class="dialog-btn" onclick={() => { mode = 'list'; message = ''; }}>Cancel</button>
    </div>
  </div>

{:else if mode === 'detail'}
  <!-- ── Detail View ──────────────────────────────────────────── -->
  <div class="ap-detail-header">
    <button class="dialog-btn" onclick={() => { mode = 'list'; message = ''; }}>Back</button>
    <span class="section-title" style="margin: 0;">Access Point: {detail?.name ?? '...'}</span>
  </div>

  {#if detailLoading}
    <div class="loading">Loading access point details...</div>
  {:else if detail}
    <div class="ap-info-grid">
      <div class="ap-info-row"><span class="ap-info-label">ARN</span><span class="ap-info-value ap-mono">{detail.access_point_arn}</span></div>
      <div class="ap-info-row"><span class="ap-info-label">Alias</span><span class="ap-info-value">{detail.alias}</span></div>
      <div class="ap-info-row"><span class="ap-info-label">Bucket</span><span class="ap-info-value">{detail.bucket}</span></div>
      <div class="ap-info-row">
        <span class="ap-info-label">Network</span>
        <span class="ap-info-value">
          <span class="ap-network-badge ap-network-{detail.network_origin.toLowerCase()}">{detail.network_origin}</span>
          {#if detail.vpc_id}<span class="ap-vpc-id">{detail.vpc_id}</span>{/if}
        </span>
      </div>
      {#if detail.creation_date}
        <div class="ap-info-row"><span class="ap-info-label">Created</span><span class="ap-info-value">{detail.creation_date}</span></div>
      {/if}
    </div>

    {#if detail.public_access_block}
      <div class="section-title" style="margin-top: 12px;">Public Access Block</div>
      <div class="ap-checkboxes ap-readonly">
        <label class="ap-checkbox"><input type="checkbox" checked={detail.public_access_block.block_public_acls} disabled /> Block public ACLs</label>
        <label class="ap-checkbox"><input type="checkbox" checked={detail.public_access_block.ignore_public_acls} disabled /> Ignore public ACLs</label>
        <label class="ap-checkbox"><input type="checkbox" checked={detail.public_access_block.block_public_policy} disabled /> Block public policy</label>
        <label class="ap-checkbox"><input type="checkbox" checked={detail.public_access_block.restrict_public_buckets} disabled /> Restrict public buckets</label>
      </div>
    {/if}

    {#if Object.keys(detail.endpoints).length > 0}
      <div class="section-title" style="margin-top: 12px;">Endpoints</div>
      <div class="ap-info-grid">
        {#each Object.entries(detail.endpoints) as [key, value]}
          <div class="ap-info-row"><span class="ap-info-label">{key}</span><span class="ap-info-value ap-mono">{value}</span></div>
        {/each}
      </div>
    {/if}

    <div class="section-title" style="margin-top: 12px;">Access Point Policy</div>
    {#if policyLoading}
      <div class="loading">Loading policy...</div>
    {:else}
      <textarea
        class="ap-policy-editor"
        bind:value={policy}
        placeholder={'{"Version": "2012-10-17", "Statement": [...]}'}
        rows="8"
      ></textarea>
      <div class="ap-actions">
        <button class="dialog-btn apply-btn" onclick={savePolicy} disabled={policySaving || !policyDirty || !validateJson(policy)}>
          {policySaving ? 'Saving...' : 'Save Policy'}
        </button>
        <button class="dialog-btn ap-delete-btn" onclick={deletePolicy} disabled={policySaving || !savedPolicy}>
          Delete Policy
        </button>
      </div>
      {#if policyError}
        <div class="ap-message ap-error">{policyError}</div>
      {/if}
    {/if}

    <div class="ap-actions" style="margin-top: 12px; border-top: 1px solid var(--border-subtle); padding-top: 10px;">
      <button class="dialog-btn ap-delete-btn" onclick={() => { if (detail) confirmDelete(detail.name); }} disabled={deleting}>
        {deleting ? 'Deleting...' : 'Delete Access Point'}
      </button>
    </div>
  {/if}

{:else}
  <!-- ── List View ────────────────────────────────────────────── -->
  <div class="section-title">Access Points</div>

  {#if loading}
    <div class="loading">Loading access points...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else if accessPoints.length === 0}
    <div class="ap-empty">No access points configured for this bucket.</div>
  {:else}
    <div class="ap-list">
      {#each accessPoints as ap}
        <div class="ap-row">
          <div class="ap-row-info">
            <span class="ap-row-name">{ap.name}</span>
            <span class="ap-row-arn">{truncateArn(ap.access_point_arn)}</span>
          </div>
          <div class="ap-row-meta">
            <span class="ap-network-badge ap-network-{ap.network_origin.toLowerCase()}">{ap.network_origin}</span>
            {#if ap.vpc_id}<span class="ap-vpc-id">{ap.vpc_id}</span>{/if}
            <button class="dialog-btn apply-btn ap-small-btn" onclick={() => showDetail(ap.name)}>Details</button>
            <button class="dialog-btn ap-delete-btn ap-small-btn" onclick={() => confirmDelete(ap.name)}>Delete</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="ap-list-actions">
    <button class="dialog-btn apply-btn" onclick={startCreate}>Create Access Point</button>
    <button class="dialog-btn apply-btn" onclick={loadList} disabled={loading}>Refresh</button>
  </div>
{/if}

{#if message}
  <div class="ap-message" class:ap-error={message.startsWith('Error')}>
    {message}
  </div>
{/if}

<style>
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

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .dialog-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .apply-btn {
    align-self: flex-start;
    padding: 6px 18px;
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-sm);
    color: var(--text-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .apply-btn:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  .ap-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .ap-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ap-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .ap-input {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .ap-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .ap-checkboxes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .ap-readonly {
    opacity: 0.7;
  }

  .ap-checkbox {
    font-size: 12px;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .ap-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .ap-empty {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 12px 0;
  }

  .ap-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 240px;
    overflow-y: auto;
  }

  .ap-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
  }

  .ap-row-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .ap-row-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .ap-row-arn {
    font-size: 11px;
    color: var(--text-secondary);
    font-family: var(--font-mono, monospace);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ap-row-meta {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }

  .ap-network-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .ap-network-internet {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .ap-network-vpc {
    background: rgba(33, 150, 243, 0.15);
    color: #2196f3;
  }

  .ap-vpc-id {
    font-size: 11px;
    color: var(--text-secondary);
    font-family: var(--font-mono, monospace);
  }

  .ap-small-btn {
    padding: 2px 8px !important;
    font-size: 11px !important;
  }

  .ap-delete-btn {
    background: rgba(255, 107, 107, 0.15) !important;
    border-color: rgba(255, 107, 107, 0.4) !important;
    color: var(--text-error, #ff6b6b) !important;
  }

  .ap-delete-btn:hover {
    background: rgba(255, 107, 107, 0.25) !important;
  }

  .ap-list-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .ap-detail-header {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .ap-info-grid {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ap-info-row {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .ap-info-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    min-width: 80px;
    flex-shrink: 0;
  }

  .ap-info-value {
    font-size: 12px;
    color: var(--text-primary);
    word-break: break-all;
  }

  .ap-mono {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .ap-policy-editor {
    width: 100%;
    min-height: 120px;
    padding: 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    resize: vertical;
    box-sizing: border-box;
  }

  .ap-policy-editor:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .ap-message {
    font-size: 12px;
    color: var(--text-accent);
    padding-top: 4px;
  }

  .ap-message.ap-error {
    color: var(--text-error, #ff6b6b);
  }

  .loading {
    text-align: center;
    padding: 12px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .error {
    text-align: center;
    padding: 12px;
    font-size: 12px;
    color: var(--text-error, #ff6b6b);
  }
</style>
