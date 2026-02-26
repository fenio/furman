<script lang="ts">
  import { onMount } from 'svelte';
  import {
    cfListDistributions, cfGetDistribution, cfCreateDistribution,
    cfUpdateDistribution, cfDeleteDistribution, cfCreateInvalidation, cfListInvalidations,
  } from '$lib/services/cloudfront';
  import type {
    CfDistributionSummary, CfDistribution, CfDistributionConfig,
    CfCustomErrorResponse, CfInvalidation,
  } from '$lib/types';

  interface Props {
    s3ConnectionId: string;
  }

  let { s3ConnectionId }: Props = $props();

  // ── State ───────────────────────────────────────────────────────────────
  let distributions = $state<CfDistributionSummary[]>([]);
  let loading = $state(true);
  let error = $state('');

  let selectedDist = $state<CfDistribution | null>(null);
  let selectedLoading = $state(false);

  let mode = $state<'list' | 'create' | 'edit'>('list');
  let saving = $state(false);
  let message = $state('');

  // Form state
  let formComment = $state('');
  let formEnabled = $state(true);
  let formDefaultRoot = $state('index.html');
  let formPriceClass = $state('PriceClass_All');
  let formHttpVersion = $state('http2');
  let formViewerPolicy = $state('redirect-to-https');
  let formAliases = $state('');
  let formErrorResponses = $state<CfCustomErrorResponse[]>([]);

  // Invalidation state
  let invalidationPaths = $state('/*');
  let invalidating = $state(false);
  let invalidations = $state<CfInvalidation[]>([]);
  let invalidationsLoading = $state(false);
  let showInvalidation = $state(false);

  // Delete state
  let deleting = $state(false);
  let disabling = $state(false);

  // ── Load ────────────────────────────────────────────────────────────────

  onMount(() => { loadDistributions(); });

  async function loadDistributions() {
    loading = true;
    error = '';
    try {
      distributions = await cfListDistributions(s3ConnectionId);
    } catch (e: any) {
      error = e?.toString() ?? 'Failed to load distributions';
    } finally {
      loading = false;
    }
  }

  async function selectDistribution(id: string) {
    selectedLoading = true;
    message = '';
    try {
      selectedDist = await cfGetDistribution(s3ConnectionId, id);
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      selectedLoading = false;
    }
  }

  // ── Create / Edit ─────────────────────────────────────────────────────

  function startCreate() {
    mode = 'create';
    formComment = '';
    formEnabled = true;
    formDefaultRoot = 'index.html';
    formPriceClass = 'PriceClass_All';
    formHttpVersion = 'http2';
    formViewerPolicy = 'redirect-to-https';
    formAliases = '';
    formErrorResponses = [];
    message = '';
  }

  function startEdit() {
    if (!selectedDist) return;
    mode = 'edit';
    const c = selectedDist.config;
    formComment = c.comment;
    formEnabled = c.enabled;
    formDefaultRoot = c.default_root_object;
    formPriceClass = c.price_class;
    formHttpVersion = c.http_version;
    formViewerPolicy = c.viewer_protocol_policy;
    formAliases = c.aliases.join('\n');
    formErrorResponses = c.custom_error_responses.map(e => ({ ...e }));
    message = '';
  }

  function cancelForm() {
    mode = 'list';
    message = '';
  }

  function buildConfig(): CfDistributionConfig {
    const aliases = formAliases.split('\n').map(s => s.trim()).filter(s => s.length > 0);
    return {
      comment: formComment,
      enabled: formEnabled,
      default_root_object: formDefaultRoot,
      price_class: formPriceClass,
      http_version: formHttpVersion,
      viewer_protocol_policy: formViewerPolicy,
      aliases,
      custom_error_responses: formErrorResponses,
    };
  }

  async function saveDistribution() {
    saving = true;
    message = '';
    try {
      const config = buildConfig();
      if (mode === 'create') {
        const created = await cfCreateDistribution(s3ConnectionId, config);
        selectedDist = created;
        message = 'Distribution created';
      } else if (mode === 'edit' && selectedDist) {
        const updated = await cfUpdateDistribution(
          s3ConnectionId, selectedDist.id, config, selectedDist.etag
        );
        selectedDist = updated;
        message = 'Distribution updated';
      }
      mode = 'list';
      await loadDistributions();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  // ── Enable / Disable ──────────────────────────────────────────────────

  async function toggleEnabled() {
    if (!selectedDist) return;
    disabling = true;
    message = '';
    try {
      const newEnabled = !selectedDist.config.enabled;
      const config = { ...selectedDist.config, enabled: newEnabled };
      const updated = await cfUpdateDistribution(
        s3ConnectionId, selectedDist.id, config, selectedDist.etag
      );
      selectedDist = updated;
      message = newEnabled ? 'Distribution enabled' : 'Distribution disabled';
      await loadDistributions();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      disabling = false;
    }
  }

  // ── Delete ────────────────────────────────────────────────────────────

  async function deleteDistribution() {
    if (!selectedDist) return;
    if (selectedDist.config.enabled) {
      message = 'Distribution must be disabled before deletion. Use "Disable" first.';
      return;
    }
    if (selectedDist.status !== 'Deployed') {
      message = 'Wait for status to become "Deployed" before deleting.';
      return;
    }
    deleting = true;
    message = '';
    try {
      await cfDeleteDistribution(s3ConnectionId, selectedDist.id, selectedDist.etag);
      selectedDist = null;
      message = 'Distribution deleted';
      await loadDistributions();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      deleting = false;
    }
  }

  // ── Invalidation ──────────────────────────────────────────────────────

  async function createInvalidation() {
    if (!selectedDist) return;
    invalidating = true;
    message = '';
    try {
      const paths = invalidationPaths.split('\n').map(s => s.trim()).filter(s => s.length > 0);
      await cfCreateInvalidation(s3ConnectionId, selectedDist.id, paths);
      message = 'Invalidation created';
      await loadInvalidations();
    } catch (e: any) {
      message = `Error: ${e}`;
    } finally {
      invalidating = false;
    }
  }

  async function loadInvalidations() {
    if (!selectedDist) return;
    invalidationsLoading = true;
    try {
      invalidations = await cfListInvalidations(s3ConnectionId, selectedDist.id);
    } catch (e: any) {
      message = `Error loading invalidations: ${e}`;
    } finally {
      invalidationsLoading = false;
    }
  }

  function openInvalidation() {
    showInvalidation = true;
    loadInvalidations();
  }

  // ── Error Response Helpers ────────────────────────────────────────────

  function addErrorResponse() {
    formErrorResponses = [...formErrorResponses, {
      error_code: 404,
      response_page_path: '/index.html',
      response_code: '200',
      error_caching_min_ttl: 300,
    }];
  }

  function removeErrorResponse(index: number) {
    formErrorResponses = formErrorResponses.filter((_, i) => i !== index);
  }

  // ── Formatting ────────────────────────────────────────────────────────

  function priceClassLabel(pc: string): string {
    switch (pc) {
      case 'PriceClass_100': return 'US, Canada, Europe';
      case 'PriceClass_200': return 'US, Canada, Europe, Asia, Middle East, Africa';
      case 'PriceClass_All': return 'All Edge Locations';
      default: return pc;
    }
  }

  function copyDomain() {
    if (selectedDist) {
      navigator.clipboard.writeText(selectedDist.domain_name);
      message = 'Domain copied to clipboard';
    }
  }
</script>

{#if mode === 'create' || mode === 'edit'}
  <!-- ── Create / Edit Form ──────────────────────────────────── -->
  <div class="section-title">{mode === 'create' ? 'Create Distribution' : 'Edit Distribution'}</div>
  <div class="cf-form">
    <div class="cf-field">
      <label class="cf-label">Comment</label>
      <input class="cf-input" type="text" bind:value={formComment} placeholder="Description" />
    </div>

    <div class="cf-field-row">
      <label class="cf-checkbox">
        <input type="checkbox" bind:checked={formEnabled} /> Enabled
      </label>
    </div>

    <div class="cf-field">
      <label class="cf-label">Default Root Object</label>
      <input class="cf-input" type="text" bind:value={formDefaultRoot} placeholder="index.html" />
    </div>

    <div class="cf-field">
      <label class="cf-label">Price Class</label>
      <select class="cf-input" bind:value={formPriceClass}>
        <option value="PriceClass_100">US, Canada, Europe</option>
        <option value="PriceClass_200">+ Asia, Middle East, Africa</option>
        <option value="PriceClass_All">All Edge Locations (best performance)</option>
      </select>
    </div>

    <div class="cf-field">
      <label class="cf-label">HTTP Version</label>
      <select class="cf-input" bind:value={formHttpVersion}>
        <option value="http1.1">HTTP/1.1</option>
        <option value="http2">HTTP/2</option>
        <option value="http2and3">HTTP/2 and HTTP/3</option>
      </select>
    </div>

    <div class="cf-field">
      <label class="cf-label">Viewer Protocol Policy</label>
      <select class="cf-input" bind:value={formViewerPolicy}>
        <option value="allow-all">HTTP and HTTPS</option>
        <option value="redirect-to-https">Redirect HTTP to HTTPS</option>
        <option value="https-only">HTTPS Only</option>
      </select>
    </div>

    <div class="cf-field">
      <label class="cf-label">Aliases / CNAMEs <span class="cf-hint">(one per line)</span></label>
      <textarea class="cf-textarea" rows="3" bind:value={formAliases} placeholder="cdn.example.com"></textarea>
    </div>

    <div class="cf-field">
      <label class="cf-label">Custom Error Responses</label>
      {#each formErrorResponses as er, i}
        <div class="cf-error-row">
          <input class="cf-input cf-small" type="number" bind:value={er.error_code} placeholder="Error code" />
          <input class="cf-input cf-small" type="text" bind:value={er.response_page_path} placeholder="/error.html" />
          <input class="cf-input cf-small" type="text" bind:value={er.response_code} placeholder="200" />
          <input class="cf-input cf-small" type="number" bind:value={er.error_caching_min_ttl} placeholder="TTL (s)" />
          <button class="cf-remove-btn" onclick={() => removeErrorResponse(i)} title="Remove">×</button>
        </div>
      {/each}
      <button class="cf-add-btn" onclick={addErrorResponse}>+ Add Error Response</button>
    </div>

    <div class="cf-actions">
      <button class="dialog-btn apply-btn" onclick={saveDistribution} disabled={saving}>
        {saving ? 'Saving...' : (mode === 'create' ? 'Create' : 'Save')}
      </button>
      <button class="dialog-btn" onclick={cancelForm}>Cancel</button>
    </div>
  </div>

{:else}
  <!-- ── Distribution List & Details ─────────────────────────── -->
  <div class="section-title">CloudFront Distributions</div>

  {#if loading}
    <div class="loading">Loading distributions...</div>
  {:else if error}
    <div class="error">{error}</div>
  {:else}
    {#if distributions.length === 0}
      <div class="cf-empty">No CloudFront distributions found for this bucket.</div>
    {:else}
      <div class="cf-dist-list">
        {#each distributions as dist}
          <button
            class="cf-dist-row"
            class:selected={selectedDist?.id === dist.id}
            onclick={() => selectDistribution(dist.id)}
          >
            <div class="cf-dist-info">
              <span class="cf-dist-id">{dist.id}</span>
              <span class="cf-dist-domain">{dist.domain_name}</span>
            </div>
            <div class="cf-dist-meta">
              <span class="cf-badge" class:deployed={dist.status === 'Deployed'} class:progress={dist.status === 'InProgress'}>
                {dist.status}
              </span>
              <span class="cf-badge" class:enabled={dist.enabled} class:disabled={!dist.enabled}>
                {dist.enabled ? 'Enabled' : 'Disabled'}
              </span>
            </div>
          </button>
        {/each}
      </div>
    {/if}

    <div class="cf-list-actions">
      <button class="dialog-btn apply-btn" onclick={startCreate}>Create Distribution</button>
      <button class="dialog-btn apply-btn" onclick={loadDistributions} disabled={loading}>Refresh</button>
    </div>

    <!-- Selected distribution details -->
    {#if selectedLoading}
      <div class="loading">Loading details...</div>
    {:else if selectedDist}
      <div class="cf-detail">
        <div class="section-title">Distribution Details</div>
        <table class="cf-props">
          <tbody>
            <tr><td class="cf-prop-label">ID</td><td class="cf-prop-value mono">{selectedDist.id}</td></tr>
            <tr>
              <td class="cf-prop-label">Domain</td>
              <td class="cf-prop-value mono">
                {selectedDist.domain_name}
                <button class="cf-copy-btn" onclick={copyDomain} title="Copy">⎘</button>
              </td>
            </tr>
            <tr>
              <td class="cf-prop-label">Status</td>
              <td class="cf-prop-value">
                <span class="cf-badge" class:deployed={selectedDist.status === 'Deployed'} class:progress={selectedDist.status === 'InProgress'}>
                  {selectedDist.status}
                </span>
              </td>
            </tr>
            <tr><td class="cf-prop-label">Enabled</td><td class="cf-prop-value">{selectedDist.config.enabled ? 'Yes' : 'No'}</td></tr>
            <tr><td class="cf-prop-label">Comment</td><td class="cf-prop-value">{selectedDist.config.comment || '—'}</td></tr>
            <tr><td class="cf-prop-label">Root Object</td><td class="cf-prop-value">{selectedDist.config.default_root_object || '—'}</td></tr>
            <tr><td class="cf-prop-label">Price Class</td><td class="cf-prop-value">{priceClassLabel(selectedDist.config.price_class)}</td></tr>
            <tr><td class="cf-prop-label">HTTP Version</td><td class="cf-prop-value">{selectedDist.config.http_version}</td></tr>
            <tr><td class="cf-prop-label">Viewer Policy</td><td class="cf-prop-value">{selectedDist.config.viewer_protocol_policy}</td></tr>
            {#if selectedDist.config.aliases.length > 0}
              <tr><td class="cf-prop-label">Aliases</td><td class="cf-prop-value">{selectedDist.config.aliases.join(', ')}</td></tr>
            {/if}
            {#if selectedDist.config.custom_error_responses.length > 0}
              <tr>
                <td class="cf-prop-label">Error Responses</td>
                <td class="cf-prop-value">
                  {#each selectedDist.config.custom_error_responses as er}
                    <div class="cf-error-detail">{er.error_code} → {er.response_page_path ?? '—'} ({er.response_code ?? '—'})</div>
                  {/each}
                </td>
              </tr>
            {/if}
          </tbody>
        </table>

        <div class="cf-detail-actions">
          <button class="dialog-btn apply-btn" onclick={startEdit}>Edit</button>
          <button class="dialog-btn apply-btn" onclick={toggleEnabled} disabled={disabling}>
            {disabling ? '...' : (selectedDist.config.enabled ? 'Disable' : 'Enable')}
          </button>
          <button class="dialog-btn apply-btn" onclick={openInvalidation}>Invalidate Cache</button>
          <button class="dialog-btn cf-delete-btn" onclick={deleteDistribution} disabled={deleting}>
            {deleting ? 'Deleting...' : 'Delete'}
          </button>
        </div>

        <!-- Invalidation Section -->
        {#if showInvalidation}
          <div class="cf-invalidation">
            <div class="section-title">Cache Invalidation</div>
            <div class="cf-field">
              <label class="cf-label">Paths <span class="cf-hint">(one per line)</span></label>
              <textarea class="cf-textarea" rows="3" bind:value={invalidationPaths} placeholder="/*"></textarea>
            </div>
            <div class="cf-actions">
              <button class="dialog-btn apply-btn" onclick={createInvalidation} disabled={invalidating}>
                {invalidating ? 'Invalidating...' : 'Create Invalidation'}
              </button>
            </div>

            {#if invalidationsLoading}
              <div class="loading">Loading invalidations...</div>
            {:else if invalidations.length > 0}
              <div class="cf-inv-list">
                <div class="section-title">Recent Invalidations</div>
                {#each invalidations as inv}
                  <div class="cf-inv-row">
                    <span class="cf-inv-id mono">{inv.id}</span>
                    <span class="cf-badge" class:deployed={inv.status === 'Completed'} class:progress={inv.status === 'InProgress'}>
                      {inv.status}
                    </span>
                    <span class="cf-inv-time">{inv.create_time}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  {/if}
{/if}

{#if message}
  <div class="cf-message" class:cf-error={message.startsWith('Error')}>
    {message}
  </div>
{/if}

<style>
  .cf-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .cf-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .cf-field-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .cf-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .cf-hint {
    font-weight: 400;
    opacity: 0.7;
  }

  .cf-input {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .cf-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .cf-small {
    flex: 1;
    min-width: 0;
  }

  .cf-textarea {
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    resize: vertical;
  }

  .cf-textarea:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .cf-checkbox {
    font-size: 12px;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .cf-error-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .cf-remove-btn {
    background: none;
    border: none;
    color: var(--text-error, #ff6b6b);
    font-size: 16px;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }

  .cf-remove-btn:hover {
    background: rgba(255, 107, 107, 0.1);
  }

  .cf-add-btn {
    background: none;
    border: 1px dashed var(--border-subtle);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: 12px;
    font-family: inherit;
    padding: 4px 12px;
    cursor: pointer;
    align-self: flex-start;
  }

  .cf-add-btn:hover {
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .cf-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .cf-empty {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 12px 0;
  }

  .cf-dist-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 160px;
    overflow-y: auto;
  }

  .cf-dist-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .cf-dist-row:hover {
    background: var(--bg-hover);
    border-color: var(--border-active);
  }

  .cf-dist-row.selected {
    border-color: var(--text-accent);
    background: rgba(110, 168, 254, 0.08);
  }

  .cf-dist-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .cf-dist-id {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .cf-dist-domain {
    font-size: 11px;
    color: var(--text-secondary);
    font-family: var(--font-mono, monospace);
  }

  .cf-dist-meta {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .cf-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }

  .cf-badge.deployed {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .cf-badge.progress {
    background: rgba(255, 193, 7, 0.15);
    color: #ffc107;
  }

  .cf-badge.enabled {
    background: rgba(76, 175, 80, 0.15);
    color: #4caf50;
  }

  .cf-badge.disabled {
    background: rgba(158, 158, 158, 0.15);
    color: #9e9e9e;
  }

  .cf-list-actions {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .cf-detail {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
  }

  .cf-props {
    width: 100%;
    border-collapse: collapse;
  }

  .cf-props td {
    padding: 3px 0;
    font-size: 12px;
    vertical-align: top;
  }

  .cf-prop-label {
    color: var(--text-secondary);
    width: 100px;
    white-space: nowrap;
    padding-right: 12px;
  }

  .cf-prop-value {
    color: var(--text-primary);
    word-break: break-all;
  }

  .cf-prop-value.mono {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .cf-copy-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    padding: 0 4px;
    vertical-align: middle;
  }

  .cf-copy-btn:hover {
    color: var(--text-accent);
  }

  .cf-detail-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .cf-delete-btn {
    background: rgba(255, 107, 107, 0.15) !important;
    border-color: rgba(255, 107, 107, 0.4) !important;
    color: var(--text-error, #ff6b6b) !important;
  }

  .cf-delete-btn:hover {
    background: rgba(255, 107, 107, 0.25) !important;
  }

  .cf-invalidation {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
  }

  .cf-inv-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .cf-inv-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    padding: 4px 0;
  }

  .cf-inv-id {
    font-family: var(--font-mono, monospace);
    color: var(--text-primary);
    font-size: 11px;
  }

  .cf-inv-time {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .cf-error-detail {
    font-size: 11px;
    color: var(--text-secondary);
  }

  .cf-message {
    font-size: 12px;
    color: var(--text-accent);
    padding-top: 4px;
  }

  .cf-message.cf-error {
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
