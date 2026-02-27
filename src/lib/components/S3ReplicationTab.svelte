<script lang="ts">
  import { onMount } from 'svelte';
  import {
    s3GetReplicationConfiguration,
    s3PutReplicationConfiguration,
    s3DeleteReplicationConfiguration,
  } from '$lib/services/s3';
  import type { S3ReplicationConfiguration, S3ReplicationRule } from '$lib/types';

  interface Props {
    s3ConnectionId: string;
  }

  let { s3ConnectionId }: Props = $props();

  // ── State ───────────────────────────────────────────────────────────────
  let config = $state<S3ReplicationConfiguration | null>(null);
  let loading = $state(true);
  let error = $state('');
  let mode = $state<'list' | 'edit'>('list');
  let saving = $state(false);
  let message = $state('');

  // Local editable state
  let formRole = $state('');
  let localRules = $state<S3ReplicationRule[]>([]);
  let dirty = $state(false);
  let editingIndex = $state<number | null>(null);

  // Rule edit form
  let ruleId = $state('');
  let rulePriority = $state('');
  let ruleStatus = $state('Enabled');
  let ruleFilterPrefix = $state('');
  let ruleDestBucket = $state('');
  let ruleStorageClass = $state('');
  let ruleAccount = $state('');
  let ruleKmsKeyId = $state('');
  let ruleDeleteMarkers = $state(false);

  const STORAGE_CLASSES = [
    '', 'STANDARD', 'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING',
    'GLACIER', 'DEEP_ARCHIVE', 'GLACIER_IR', 'REDUCED_REDUNDANCY',
  ];

  // ── Load ────────────────────────────────────────────────────────────────

  onMount(() => { loadConfig(); });

  async function loadConfig() {
    loading = true;
    error = '';
    message = '';
    try {
      config = await s3GetReplicationConfiguration(s3ConnectionId);
      if (config) {
        formRole = config.role;
        localRules = config.rules.map(r => ({ ...r, destination: { ...r.destination } }));
      } else {
        formRole = '';
        localRules = [];
      }
      dirty = false;
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load replication configuration';
    } finally {
      loading = false;
    }
  }

  // ── Dirty tracking ─────────────────────────────────────────────────────

  function markDirty() {
    dirty = true;
  }

  function handleRoleInput() {
    markDirty();
  }

  // ── Rule CRUD (local) ──────────────────────────────────────────────────

  function startAddRule() {
    editingIndex = null;
    ruleId = '';
    rulePriority = '';
    ruleStatus = 'Enabled';
    ruleFilterPrefix = '';
    ruleDestBucket = '';
    ruleStorageClass = '';
    ruleAccount = '';
    ruleKmsKeyId = '';
    ruleDeleteMarkers = false;
    message = '';
    mode = 'edit';
  }

  function startEditRule(index: number) {
    const rule = localRules[index];
    editingIndex = index;
    ruleId = rule.id ?? '';
    rulePriority = rule.priority != null ? String(rule.priority) : '';
    ruleStatus = rule.status;
    ruleFilterPrefix = rule.filter_prefix ?? '';
    ruleDestBucket = rule.destination.bucket_arn;
    ruleStorageClass = rule.destination.storage_class ?? '';
    ruleAccount = rule.destination.account ?? '';
    ruleKmsKeyId = rule.destination.kms_key_id ?? '';
    ruleDeleteMarkers = rule.delete_marker_replication;
    message = '';
    mode = 'edit';
  }

  function saveRule() {
    if (!ruleDestBucket.trim()) {
      message = 'Error: Destination Bucket ARN is required';
      return;
    }

    const rule: S3ReplicationRule = {
      id: ruleId.trim() || null,
      priority: rulePriority.trim() ? parseInt(rulePriority, 10) : null,
      status: ruleStatus,
      filter_prefix: ruleFilterPrefix.trim() || null,
      destination: {
        bucket_arn: ruleDestBucket.trim(),
        storage_class: ruleStorageClass || null,
        account: ruleAccount.trim() || null,
        kms_key_id: ruleKmsKeyId.trim() || null,
      },
      delete_marker_replication: ruleDeleteMarkers,
    };

    if (editingIndex != null) {
      localRules[editingIndex] = rule;
      localRules = [...localRules];
    } else {
      localRules = [...localRules, rule];
    }

    markDirty();
    mode = 'list';
    message = '';
  }

  function cancelRuleEdit() {
    mode = 'list';
    message = '';
  }

  function deleteRule(index: number) {
    localRules = localRules.filter((_, i) => i !== index);
    markDirty();
  }

  // ── Save / Delete configuration ────────────────────────────────────────

  async function saveConfiguration() {
    if (!formRole.trim()) {
      message = 'Error: IAM Role ARN is required';
      return;
    }
    if (localRules.length === 0) {
      message = 'Error: At least one replication rule is required';
      return;
    }

    saving = true;
    message = '';
    try {
      const newConfig: S3ReplicationConfiguration = {
        role: formRole.trim(),
        rules: localRules,
      };
      await s3PutReplicationConfiguration(s3ConnectionId, newConfig);
      message = 'Replication configuration saved';
      await loadConfig();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  async function deleteConfiguration() {
    saving = true;
    message = '';
    try {
      await s3DeleteReplicationConfiguration(s3ConnectionId);
      message = 'Replication configuration deleted';
      await loadConfig();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }
</script>

{#if mode === 'edit'}
  <!-- ── Rule Edit Form ──────────────────────────────────────── -->
  <div class="section-title">{editingIndex != null ? 'Edit Replication Rule' : 'Add Replication Rule'}</div>
  <div class="repl-form">
    <div class="repl-field">
      <label class="repl-label">Rule ID <span class="repl-hint">(optional)</span></label>
      <input class="repl-input" type="text" bind:value={ruleId} placeholder="my-replication-rule" />
    </div>

    <div class="repl-field">
      <label class="repl-label">Priority <span class="repl-hint">(optional, integer)</span></label>
      <input class="repl-input" type="number" bind:value={rulePriority} placeholder="0" />
    </div>

    <div class="repl-field">
      <label class="repl-label">Status</label>
      <select class="repl-input" bind:value={ruleStatus}>
        <option value="Enabled">Enabled</option>
        <option value="Disabled">Disabled</option>
      </select>
    </div>

    <div class="repl-field">
      <label class="repl-label">Filter Prefix <span class="repl-hint">(optional)</span></label>
      <input class="repl-input" type="text" bind:value={ruleFilterPrefix} placeholder="data/" />
    </div>

    <div class="repl-field">
      <label class="repl-label">Destination Bucket ARN</label>
      <input class="repl-input" type="text" bind:value={ruleDestBucket} placeholder="arn:aws:s3:::destination-bucket" />
    </div>

    <div class="repl-field">
      <label class="repl-label">Storage Class Override <span class="repl-hint">(optional)</span></label>
      <select class="repl-input" bind:value={ruleStorageClass}>
        {#each STORAGE_CLASSES as sc}
          <option value={sc}>{sc || '(same as source)'}</option>
        {/each}
      </select>
    </div>

    <div class="repl-field">
      <label class="repl-label">Destination Account ID <span class="repl-hint">(optional, cross-account)</span></label>
      <input class="repl-input" type="text" bind:value={ruleAccount} placeholder="123456789012" />
    </div>

    <div class="repl-field">
      <label class="repl-label">KMS Key ID <span class="repl-hint">(optional, for SSE-KMS)</span></label>
      <input class="repl-input" type="text" bind:value={ruleKmsKeyId} placeholder="arn:aws:kms:..." />
    </div>

    <div class="repl-field-row">
      <label class="repl-checkbox">
        <input type="checkbox" bind:checked={ruleDeleteMarkers} /> Replicate delete markers
      </label>
    </div>

    <div class="repl-actions">
      <button class="dialog-btn apply-btn" onclick={saveRule}>
        {editingIndex != null ? 'Save Rule' : 'Add Rule'}
      </button>
      <button class="dialog-btn" onclick={cancelRuleEdit}>Cancel</button>
    </div>
  </div>

{:else}
  <!-- ── Configuration List ──────────────────────────────────── -->
  <div class="section-title">Replication Configuration</div>

  {#if loading}
    <div class="loading">Loading replication configuration...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else}
    <div class="repl-field">
      <label class="repl-label">IAM Role ARN</label>
      <input class="repl-input" type="text" bind:value={formRole} oninput={handleRoleInput} placeholder="arn:aws:iam::123456789012:role/replication-role" />
    </div>

    {#if localRules.length === 0}
      <div class="repl-empty">No replication rules configured.</div>
    {:else}
      <div class="repl-list">
        {#each localRules as rule, i}
          <div class="repl-row">
            <div class="repl-row-info">
              <span class="repl-row-id">{rule.id || `Rule ${i + 1}`}</span>
              <span class="repl-row-detail">
                {rule.destination.bucket_arn}
                {#if rule.destination.storage_class}&middot; {rule.destination.storage_class}{/if}
                {#if rule.filter_prefix}&middot; prefix: {rule.filter_prefix}{/if}
              </span>
            </div>
            <div class="repl-row-meta">
              {#if rule.priority != null}
                <span class="repl-priority">P{rule.priority}</span>
              {/if}
              <span class="repl-badge" class:enabled={rule.status === 'Enabled'} class:disabled={rule.status !== 'Enabled'}>
                {rule.status}
              </span>
              {#if rule.delete_marker_replication}
                <span class="repl-dm-badge">DM</span>
              {/if}
              <button class="dialog-btn apply-btn repl-small-btn" onclick={() => startEditRule(i)}>Edit</button>
              <button class="dialog-btn repl-delete-btn repl-small-btn" onclick={() => deleteRule(i)}>Delete</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <div class="repl-list-actions">
      <button class="dialog-btn apply-btn" onclick={startAddRule}>Add Rule</button>
      <button class="dialog-btn apply-btn" onclick={saveConfiguration} disabled={saving || !dirty}>
        {saving ? 'Saving...' : 'Save Configuration'}
      </button>
      {#if config}
        <button class="dialog-btn repl-delete-btn" onclick={deleteConfiguration} disabled={saving}>
          Delete All Replication
        </button>
      {/if}
      <button class="dialog-btn apply-btn" onclick={loadConfig} disabled={loading}>Refresh</button>
    </div>
  {/if}
{/if}

{#if message}
  <div class="repl-message" class:repl-error={message.startsWith('Error')}>
    {message}
  </div>
{/if}

<style>
  .repl-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .repl-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .repl-field-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .repl-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .repl-hint {
    font-weight: 400;
    opacity: 0.7;
  }

  .repl-input {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .repl-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .repl-checkbox {
    font-size: 12px;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .repl-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .repl-empty {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 12px 0;
  }

  .repl-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 200px;
    overflow-y: auto;
  }

  .repl-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
  }

  .repl-row-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .repl-row-id {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .repl-row-detail {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repl-row-meta {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }

  .repl-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .repl-badge.enabled {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .repl-badge.disabled {
    background: rgba(158, 158, 158, 0.15);
    color: #9e9e9e;
  }

  .repl-priority {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 10px;
    background: rgba(100, 149, 237, 0.15);
    color: #6495ed;
  }

  .repl-dm-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 10px;
    background: rgba(255, 193, 7, 0.15);
    color: #ffc107;
  }

  .repl-small-btn {
    padding: 2px 8px !important;
    font-size: 11px !important;
  }

  .repl-delete-btn {
    background: rgba(255, 107, 107, 0.15) !important;
    border-color: rgba(255, 107, 107, 0.4) !important;
    color: var(--text-error, #ff6b6b) !important;
  }

  .repl-delete-btn:hover {
    background: rgba(255, 107, 107, 0.25) !important;
  }

  .repl-list-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .repl-message {
    font-size: 12px;
    color: var(--text-accent);
    padding-top: 4px;
  }

  .repl-message.repl-error {
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
