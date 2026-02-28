<script lang="ts">
  import { onMount } from 'svelte';
  import {
    s3GetNotificationConfiguration,
    s3PutNotificationConfiguration,
  } from '$lib/services/s3';
  import type { S3NotificationConfiguration, S3NotificationRule } from '$lib/types';

  interface Props {
    s3ConnectionId: string;
  }

  let { s3ConnectionId }: Props = $props();

  // ── State ───────────────────────────────────────────────────────────────
  let config = $state<S3NotificationConfiguration | null>(null);
  let loading = $state(true);
  let error = $state('');
  let mode = $state<'list' | 'edit'>('list');
  let saving = $state(false);
  let message = $state('');

  // Local editable state
  let localRules = $state<S3NotificationRule[]>([]);
  let eventBridgeEnabled = $state(false);
  let dirty = $state(false);
  let editingIndex = $state<number | null>(null);

  // Rule edit form
  let ruleId = $state('');
  let ruleDestType = $state<'sns' | 'sqs' | 'lambda'>('sns');
  let ruleDestArn = $state('');
  let ruleEvents = $state<string[]>([]);
  let ruleFilterPrefix = $state('');
  let ruleFilterSuffix = $state('');

  // ── Event groups ────────────────────────────────────────────────────────

  const EVENT_GROUPS: { label: string; events: { value: string; label: string }[] }[] = [
    {
      label: 'Object Created',
      events: [
        { value: 's3:ObjectCreated:*', label: 'All create events' },
        { value: 's3:ObjectCreated:Put', label: 'Put' },
        { value: 's3:ObjectCreated:Post', label: 'Post' },
        { value: 's3:ObjectCreated:Copy', label: 'Copy' },
        { value: 's3:ObjectCreated:CompleteMultipartUpload', label: 'CompleteMultipartUpload' },
      ],
    },
    {
      label: 'Object Removed',
      events: [
        { value: 's3:ObjectRemoved:*', label: 'All remove events' },
        { value: 's3:ObjectRemoved:Delete', label: 'Delete' },
        { value: 's3:ObjectRemoved:DeleteMarkerCreated', label: 'DeleteMarkerCreated' },
      ],
    },
    {
      label: 'Object Restore',
      events: [
        { value: 's3:ObjectRestore:*', label: 'All restore events' },
        { value: 's3:ObjectRestore:Post', label: 'Post (initiated)' },
        { value: 's3:ObjectRestore:Completed', label: 'Completed' },
        { value: 's3:ObjectRestore:Delete', label: 'Delete' },
      ],
    },
    {
      label: 'Other',
      events: [
        { value: 's3:ReducedRedundancyLostObject', label: 'ReducedRedundancyLostObject' },
        { value: 's3:Replication:*', label: 'Replication' },
        { value: 's3:ObjectTagging:*', label: 'Tagging' },
        { value: 's3:ObjectAcl:Put', label: 'ACL Put' },
        { value: 's3:LifecycleExpiration:*', label: 'Lifecycle Expiration' },
        { value: 's3:IntelligentTiering', label: 'IntelligentTiering' },
      ],
    },
  ];

  // ── Load ────────────────────────────────────────────────────────────────

  onMount(() => { loadConfig(); });

  async function loadConfig() {
    loading = true;
    error = '';
    message = '';
    try {
      config = await s3GetNotificationConfiguration(s3ConnectionId);
      localRules = config.rules.map(r => ({ ...r, events: [...r.events] }));
      eventBridgeEnabled = config.event_bridge_enabled;
      dirty = false;
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load notification configuration';
    } finally {
      loading = false;
    }
  }

  // ── Dirty tracking ─────────────────────────────────────────────────────

  function markDirty() {
    dirty = true;
  }

  function toggleEventBridge() {
    eventBridgeEnabled = !eventBridgeEnabled;
    markDirty();
  }

  // ── Rule CRUD (local) ──────────────────────────────────────────────────

  function startAddRule() {
    editingIndex = null;
    ruleId = '';
    ruleDestType = 'sns';
    ruleDestArn = '';
    ruleEvents = [];
    ruleFilterPrefix = '';
    ruleFilterSuffix = '';
    message = '';
    mode = 'edit';
  }

  function startEditRule(index: number) {
    const rule = localRules[index];
    editingIndex = index;
    ruleId = rule.id ?? '';
    ruleDestType = rule.destination_type as 'sns' | 'sqs' | 'lambda';
    ruleDestArn = rule.destination_arn;
    ruleEvents = [...rule.events];
    ruleFilterPrefix = rule.filter_prefix ?? '';
    ruleFilterSuffix = rule.filter_suffix ?? '';
    message = '';
    mode = 'edit';
  }

  function toggleEvent(event: string) {
    if (ruleEvents.includes(event)) {
      ruleEvents = ruleEvents.filter(e => e !== event);
    } else {
      ruleEvents = [...ruleEvents, event];
    }
  }

  function saveRule() {
    if (!ruleDestArn.trim()) {
      message = 'Error: Destination ARN is required';
      return;
    }
    if (ruleEvents.length === 0) {
      message = 'Error: At least one event must be selected';
      return;
    }

    const rule: S3NotificationRule = {
      id: ruleId.trim() || null,
      destination_type: ruleDestType,
      destination_arn: ruleDestArn.trim(),
      events: [...ruleEvents],
      filter_prefix: ruleFilterPrefix.trim() || null,
      filter_suffix: ruleFilterSuffix.trim() || null,
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

  // ── Save configuration ─────────────────────────────────────────────────

  async function saveConfiguration() {
    saving = true;
    message = '';
    try {
      const newConfig: S3NotificationConfiguration = {
        rules: localRules,
        event_bridge_enabled: eventBridgeEnabled,
      };
      await s3PutNotificationConfiguration(s3ConnectionId, newConfig);
      message = 'Notification configuration saved';
      await loadConfig();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  function destLabel(type: string): string {
    switch (type) {
      case 'sns': return 'SNS';
      case 'sqs': return 'SQS';
      case 'lambda': return 'Lambda';
      default: return type;
    }
  }
</script>

{#if mode === 'edit'}
  <!-- ── Rule Edit Form ──────────────────────────────────────── -->
  <div class="section-title">{editingIndex != null ? 'Edit Notification Rule' : 'Add Notification Rule'}</div>
  <div class="notif-form">
    <div class="notif-field">
      <label class="notif-label">Rule ID <span class="notif-hint">(optional)</span></label>
      <input class="notif-input" type="text" bind:value={ruleId} placeholder="my-notification-rule" />
    </div>

    <div class="notif-field">
      <label class="notif-label">Destination Type</label>
      <select class="notif-input" bind:value={ruleDestType}>
        <option value="sns">SNS Topic</option>
        <option value="sqs">SQS Queue</option>
        <option value="lambda">Lambda Function</option>
      </select>
    </div>

    <div class="notif-field">
      <label class="notif-label">Destination ARN</label>
      <input class="notif-input" type="text" bind:value={ruleDestArn}
        placeholder={ruleDestType === 'sns' ? 'arn:aws:sns:...' : ruleDestType === 'sqs' ? 'arn:aws:sqs:...' : 'arn:aws:lambda:...'} />
    </div>

    <div class="notif-field">
      <label class="notif-label">Events</label>
      <div class="notif-events">
        {#each EVENT_GROUPS as group}
          <div class="notif-event-group">
            <div class="notif-event-group-label">{group.label}</div>
            {#each group.events as evt}
              <label class="notif-checkbox">
                <input type="checkbox" checked={ruleEvents.includes(evt.value)} onchange={() => toggleEvent(evt.value)} />
                {evt.label}
              </label>
            {/each}
          </div>
        {/each}
      </div>
    </div>

    <div class="notif-field">
      <label class="notif-label">Filter Prefix <span class="notif-hint">(optional)</span></label>
      <input class="notif-input" type="text" bind:value={ruleFilterPrefix} placeholder="images/" />
    </div>

    <div class="notif-field">
      <label class="notif-label">Filter Suffix <span class="notif-hint">(optional)</span></label>
      <input class="notif-input" type="text" bind:value={ruleFilterSuffix} placeholder=".jpg" />
    </div>

    <div class="notif-actions">
      <button class="dialog-btn apply-btn" onclick={saveRule}>
        {editingIndex != null ? 'Save Rule' : 'Add Rule'}
      </button>
      <button class="dialog-btn" onclick={cancelRuleEdit}>Cancel</button>
    </div>
  </div>

{:else}
  <!-- ── Configuration List ──────────────────────────────────── -->
  <div class="section-title">Event Notification Configuration</div>

  {#if loading}
    <div class="loading">Loading notification configuration...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else}
    <div class="notif-field-row">
      <label class="notif-checkbox">
        <input type="checkbox" checked={eventBridgeEnabled} onchange={toggleEventBridge} />
        Send notifications to Amazon EventBridge
      </label>
    </div>

    {#if localRules.length === 0}
      <div class="notif-empty">No notification rules configured.</div>
    {:else}
      <div class="notif-list">
        {#each localRules as rule, i}
          <div class="notif-row">
            <div class="notif-row-info">
              <span class="notif-row-id">{rule.id || `Rule ${i + 1}`}</span>
              <span class="notif-row-detail">
                {rule.destination_arn}
                {#if rule.filter_prefix || rule.filter_suffix}
                  &middot;
                  {#if rule.filter_prefix}prefix: {rule.filter_prefix}{/if}
                  {#if rule.filter_prefix && rule.filter_suffix}, {/if}
                  {#if rule.filter_suffix}suffix: {rule.filter_suffix}{/if}
                {/if}
              </span>
            </div>
            <div class="notif-row-meta">
              <span class="notif-dest-badge notif-dest-{rule.destination_type}">{destLabel(rule.destination_type)}</span>
              <span class="notif-event-count">{rule.events.length} event{rule.events.length !== 1 ? 's' : ''}</span>
              <button class="dialog-btn apply-btn notif-small-btn" onclick={() => startEditRule(i)}>Edit</button>
              <button class="dialog-btn notif-delete-btn notif-small-btn" onclick={() => deleteRule(i)}>Delete</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <div class="notif-list-actions">
      <button class="dialog-btn apply-btn" onclick={startAddRule}>Add Rule</button>
      <button class="dialog-btn apply-btn" onclick={saveConfiguration} disabled={saving || !dirty}>
        {saving ? 'Saving...' : 'Save Configuration'}
      </button>
      <button class="dialog-btn apply-btn" onclick={loadConfig} disabled={loading}>Refresh</button>
    </div>
  {/if}
{/if}

{#if message}
  <div class="notif-message" class:notif-error={message.startsWith('Error')}>
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

  .notif-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .notif-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .notif-field-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
  }

  .notif-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .notif-hint {
    font-weight: 400;
    opacity: 0.7;
  }

  .notif-input {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .notif-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .notif-checkbox {
    font-size: 12px;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .notif-events {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .notif-event-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .notif-event-group-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.3px;
    padding-bottom: 2px;
  }

  .notif-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .notif-empty {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 12px 0;
  }

  .notif-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 200px;
    overflow-y: auto;
  }

  .notif-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
  }

  .notif-row-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .notif-row-id {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .notif-row-detail {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .notif-row-meta {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }

  .notif-dest-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .notif-dest-sns {
    background: rgba(255, 152, 0, 0.15);
    color: #ff9800;
  }

  .notif-dest-sqs {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .notif-dest-lambda {
    background: rgba(156, 39, 176, 0.15);
    color: #ce93d8;
  }

  .notif-event-count {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 10px;
    background: rgba(100, 149, 237, 0.15);
    color: #6495ed;
  }

  .notif-small-btn {
    padding: 2px 8px !important;
    font-size: 11px !important;
  }

  .notif-delete-btn {
    background: rgba(255, 107, 107, 0.15) !important;
    border-color: rgba(255, 107, 107, 0.4) !important;
    color: var(--text-error, #ff6b6b) !important;
  }

  .notif-delete-btn:hover {
    background: rgba(255, 107, 107, 0.25) !important;
  }

  .notif-list-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .notif-message {
    font-size: 12px;
    color: var(--text-accent);
    padding-top: 4px;
  }

  .notif-message.notif-error {
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
