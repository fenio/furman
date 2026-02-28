<script lang="ts">
  import { onMount } from 'svelte';
  import {
    s3ListInventoryConfigurations,
    s3PutInventoryConfiguration,
    s3DeleteInventoryConfiguration,
  } from '$lib/services/s3';
  import type { S3InventoryConfiguration } from '$lib/types';

  interface Props {
    s3ConnectionId: string;
  }

  let { s3ConnectionId }: Props = $props();

  // ── State ───────────────────────────────────────────────────────────────
  let configs = $state<S3InventoryConfiguration[]>([]);
  let loading = $state(true);
  let error = $state('');
  let mode = $state<'list' | 'edit'>('list');
  let saving = $state(false);
  let message = $state('');
  let editingExisting = $state(false);

  // Form state
  let formId = $state('');
  let formEnabled = $state(true);
  let formSchedule = $state('Daily');
  let formVersions = $state('Current');
  let formBucketArn = $state('');
  let formPrefix = $state('');
  let formFormat = $state('CSV');
  let formAccountId = $state('');
  let formFilterPrefix = $state('');
  let formOptionalFields = $state<string[]>([]);

  const OPTIONAL_FIELDS = [
    'Size', 'LastModifiedDate', 'StorageClass', 'ETag',
    'IsMultipartUploaded', 'ReplicationStatus', 'EncryptionStatus',
    'ObjectLockRetainUntilDate', 'ObjectLockMode', 'ObjectLockLegalHoldStatus',
    'IntelligentTieringAccessTier', 'BucketKeyStatus', 'ChecksumAlgorithm',
    'ObjectAccessControlList', 'ObjectOwner',
  ];

  // ── Load ────────────────────────────────────────────────────────────────

  onMount(() => { loadConfigs(); });

  async function loadConfigs() {
    loading = true;
    error = '';
    try {
      configs = await s3ListInventoryConfigurations(s3ConnectionId);
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load inventory configurations';
    } finally {
      loading = false;
    }
  }

  // ── Create / Edit ─────────────────────────────────────────────────────

  function startCreate() {
    mode = 'edit';
    editingExisting = false;
    formId = '';
    formEnabled = true;
    formSchedule = 'Daily';
    formVersions = 'Current';
    formBucketArn = '';
    formPrefix = '';
    formFormat = 'CSV';
    formAccountId = '';
    formFilterPrefix = '';
    formOptionalFields = [];
    message = '';
  }

  function startEdit(config: S3InventoryConfiguration) {
    mode = 'edit';
    editingExisting = true;
    formId = config.id;
    formEnabled = config.enabled;
    formSchedule = config.schedule;
    formVersions = config.included_object_versions;
    formBucketArn = config.destination.bucket_arn;
    formPrefix = config.destination.prefix ?? '';
    formFormat = config.destination.format;
    formAccountId = config.destination.account_id ?? '';
    formFilterPrefix = config.filter_prefix ?? '';
    formOptionalFields = [...config.optional_fields];
    message = '';
  }

  function cancelForm() {
    mode = 'list';
    message = '';
  }

  function toggleField(field: string) {
    if (formOptionalFields.includes(field)) {
      formOptionalFields = formOptionalFields.filter(f => f !== field);
    } else {
      formOptionalFields = [...formOptionalFields, field];
    }
  }

  async function saveConfig() {
    if (!formId.trim()) {
      message = 'Error: Configuration ID is required';
      return;
    }
    if (!formBucketArn.trim()) {
      message = 'Error: Destination bucket ARN is required';
      return;
    }

    saving = true;
    message = '';
    try {
      const config: S3InventoryConfiguration = {
        id: formId.trim(),
        enabled: formEnabled,
        destination: {
          bucket_arn: formBucketArn.trim(),
          prefix: formPrefix.trim() || null,
          format: formFormat,
          account_id: formAccountId.trim() || null,
        },
        schedule: formSchedule,
        included_object_versions: formVersions,
        optional_fields: formOptionalFields,
        filter_prefix: formFilterPrefix.trim() || null,
      };
      await s3PutInventoryConfiguration(s3ConnectionId, config);
      message = editingExisting ? 'Configuration updated' : 'Configuration created';
      mode = 'list';
      await loadConfigs();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  // ── Delete ──────────────────────────────────────────────────────────────

  async function deleteConfig(configId: string) {
    message = '';
    try {
      await s3DeleteInventoryConfiguration(s3ConnectionId, configId);
      message = 'Configuration deleted';
      await loadConfigs();
    } catch (e: any) {
      message = `Error: ${e}`;
    }
  }
</script>

{#if mode === 'edit'}
  <!-- ── Create / Edit Form ──────────────────────────────────── -->
  <div class="section-title">{editingExisting ? 'Edit Inventory Configuration' : 'Create Inventory Configuration'}</div>
  <div class="inv-form">
    <div class="inv-field">
      <label class="inv-label">Configuration ID
        <input class="inv-input" type="text" bind:value={formId} placeholder="my-inventory-config" disabled={editingExisting} />
      </label>
    </div>

    <div class="inv-field-row">
      <label class="inv-checkbox">
        <input type="checkbox" bind:checked={formEnabled} /> Enabled
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Schedule
        <select class="inv-input" bind:value={formSchedule}>
          <option value="Daily">Daily</option>
          <option value="Weekly">Weekly</option>
        </select>
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Included Object Versions
        <select class="inv-input" bind:value={formVersions}>
          <option value="Current">Current</option>
          <option value="All">All</option>
        </select>
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Destination Bucket ARN
        <input class="inv-input" type="text" bind:value={formBucketArn} placeholder="arn:aws:s3:::destination-bucket" />
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Destination Prefix <span class="inv-hint">(optional)</span>
        <input class="inv-input" type="text" bind:value={formPrefix} placeholder="inventory/" />
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Output Format
        <select class="inv-input" bind:value={formFormat}>
          <option value="CSV">CSV</option>
          <option value="ORC">ORC</option>
          <option value="Parquet">Parquet</option>
        </select>
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Destination Account ID <span class="inv-hint">(optional, cross-account)</span>
        <input class="inv-input" type="text" bind:value={formAccountId} placeholder="123456789012" />
      </label>
    </div>

    <div class="inv-field">
      <label class="inv-label">Filter Prefix <span class="inv-hint">(optional)</span>
        <input class="inv-input" type="text" bind:value={formFilterPrefix} placeholder="data/" />
      </label>
    </div>

    <div class="inv-field">
      <span class="inv-label">Optional Fields</span>
      <div class="inv-fields-grid">
        {#each OPTIONAL_FIELDS as field}
          <label class="inv-checkbox">
            <input type="checkbox" checked={formOptionalFields.includes(field)} onchange={() => toggleField(field)} />
            {field}
          </label>
        {/each}
      </div>
    </div>

    <div class="inv-actions">
      <button class="dialog-btn apply-btn" onclick={saveConfig} disabled={saving}>
        {saving ? 'Saving...' : (editingExisting ? 'Save' : 'Create')}
      </button>
      <button class="dialog-btn" onclick={cancelForm}>Cancel</button>
    </div>
  </div>

{:else}
  <!-- ── Configuration List ──────────────────────────────────── -->
  <div class="section-title">Inventory Configurations</div>

  {#if loading}
    <div class="loading">Loading inventory configurations...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else}
    {#if configs.length === 0}
      <div class="inv-empty">No inventory configurations found for this bucket.</div>
    {:else}
      <div class="inv-list">
        {#each configs as config}
          <div class="inv-row">
            <div class="inv-row-info">
              <span class="inv-row-id">{config.id}</span>
              <span class="inv-row-detail">
                {config.schedule} &middot; {config.destination.format} &middot; {config.included_object_versions}
              </span>
            </div>
            <div class="inv-row-meta">
              <span class="inv-badge" class:enabled={config.enabled} class:disabled={!config.enabled}>
                {config.enabled ? 'Enabled' : 'Disabled'}
              </span>
              <button class="dialog-btn apply-btn inv-small-btn" onclick={() => startEdit(config)}>Edit</button>
              <button class="dialog-btn inv-delete-btn inv-small-btn" onclick={() => deleteConfig(config.id)}>Delete</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <div class="inv-list-actions">
      <button class="dialog-btn apply-btn" onclick={startCreate}>Add Configuration</button>
      <button class="dialog-btn apply-btn" onclick={loadConfigs} disabled={loading}>Refresh</button>
    </div>
  {/if}
{/if}

{#if message}
  <div class="inv-message" class:inv-error={message.startsWith('Error')}>
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

  .inv-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .inv-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .inv-field-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .inv-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .inv-hint {
    font-weight: 400;
    opacity: 0.7;
  }

  .inv-input {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .inv-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .inv-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .inv-checkbox {
    font-size: 12px;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .inv-fields-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 4px 12px;
  }

  .inv-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .inv-empty {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 12px 0;
  }

  .inv-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 200px;
    overflow-y: auto;
  }

  .inv-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
  }

  .inv-row-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .inv-row-id {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .inv-row-detail {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .inv-row-meta {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }

  .inv-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .inv-badge.enabled {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .inv-badge.disabled {
    background: rgba(158, 158, 158, 0.15);
    color: #9e9e9e;
  }

  .inv-small-btn {
    padding: 2px 8px !important;
    font-size: 11px !important;
  }

  .inv-delete-btn {
    background: rgba(255, 107, 107, 0.15) !important;
    border-color: rgba(255, 107, 107, 0.4) !important;
    color: var(--text-error, #ff6b6b) !important;
  }

  .inv-delete-btn:hover {
    background: rgba(255, 107, 107, 0.25) !important;
  }

  .inv-list-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .inv-message {
    font-size: 12px;
    color: var(--text-accent);
    padding-top: 4px;
  }

  .inv-message.inv-error {
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
