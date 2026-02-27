<script lang="ts">
  import { untrack } from 'svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { s3ProfilesState } from '$lib/state/s3profiles.svelte';
  import S3ConnectDialog from '$lib/components/S3ConnectDialog.svelte';
  import { resolveCapabilities, getProviderIcon } from '$lib/data/s3-providers';
  import { error } from '$lib/services/log';
  import { oidcRefresh } from '$lib/services/s3';
  import type { S3Profile, S3ConnectionInfo, S3ProviderCapabilities } from '$lib/types';

  interface Props {
    onClose: () => void;
    initialTab?: 'saved' | 'connect';
    onConnect?: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities, roleArn?: string, externalId?: string, sessionName?: string, sessionDurationSecs?: number, useTransferAcceleration?: boolean, anonymous?: boolean, webIdentityToken?: string, proxyUrl?: string, proxyUsername?: string, proxyPassword?: string) => void;
  }

  let { onClose, initialTab = 'saved', onConnect: onConnectProp }: Props = $props();

  let activeTab = $state(untrack(() => initialTab));
  let view = $state<'list' | 'edit'>('list');
  let editingProfile = $state<S3Profile | undefined>(undefined);
  let connectError = $state('');

  function handleAddNew() {
    activeTab = 'connect';
  }

  function handleEdit(profile: S3Profile) {
    editingProfile = profile;
    view = 'edit';
  }

  function handleDelete(profile: S3Profile) {
    appState.showConfirm(`Delete connection "${profile.name}"?`, async () => {
      if (profile.credentialType === 'keychain') {
        await s3ProfilesState.deleteSecret(profile.id);
      }
      s3ProfilesState.removeProfile(profile.id);
    });
  }

  async function handleConnect(
    bucket: string,
    region: string,
    endpoint?: string,
    profile?: string,
    accessKey?: string,
    secretKey?: string,
    provider?: string,
    customCapabilities?: S3ProviderCapabilities,
    roleArn?: string,
    externalId?: string,
    sessionName?: string,
    sessionDurationSecs?: number,
    useTransferAcceleration?: boolean,
    anonymous?: boolean,
    webIdentityToken?: string,
    proxyUrl?: string,
    proxyUsername?: string,
    proxyPassword?: string,
  ) {
    connectError = '';
    const panel = panels.active;
    const connectionId = `s3-${Date.now()}`;
    const caps = resolveCapabilities({ provider, customCapabilities });
    const info: S3ConnectionInfo = { bucket, region, connectionId, provider, capabilities: caps };
    if (endpoint) info.endpoint = endpoint;
    if (profile) info.profile = profile;
    try {
      await panel.connectS3(info, endpoint, profile, accessKey, secretKey, roleArn, externalId, sessionName, sessionDurationSecs, useTransferAcceleration, anonymous, webIdentityToken, proxyUrl, proxyUsername, proxyPassword);
      onClose();
    } catch (err: unknown) {
      connectError = err instanceof Error ? err.message : String(err);
      error(String(err));
    }
  }

  async function handleConnectFromProfile(p: S3Profile) {
    connectError = '';
    let secretKey: string | undefined;
    let accessKey: string | undefined = p.accessKeyId;

    // Load proxy password from keychain
    let proxyPassword: string | undefined;
    if (p.proxyUrl && p.proxyUrl !== 'system') {
      try {
        const pp = await s3ProfilesState.getSecret(p.id + ':proxy');
        if (pp) proxyPassword = pp;
      } catch { /* ignore */ }
    }

    if (p.credentialType === 'anonymous') {
      await handleConnect(
        p.bucket, p.region, p.endpoint,
        undefined, undefined, undefined,
        p.provider, p.customCapabilities,
        undefined, undefined, undefined, undefined, undefined,
        true,
        undefined,
        p.proxyUrl, p.proxyUsername, proxyPassword,
      );
      return;
    }

    if (p.credentialType === 'oidc') {
      // Try silent refresh first
      try {
        const refreshToken = await s3ProfilesState.getSecret(p.id + ':oidc-refresh');
        if (refreshToken && p.oidcIssuerUrl && p.oidcClientId) {
          try {
            const result = await oidcRefresh(p.oidcIssuerUrl, p.oidcClientId, refreshToken);
            // Store new refresh token
            if (result.refresh_token) {
              await s3ProfilesState.saveSecret(p.id + ':oidc-refresh', result.refresh_token);
            }
            await handleConnect(
              p.bucket, p.region, p.endpoint,
              undefined, undefined, undefined,
              p.provider, p.customCapabilities,
              p.roleArn, p.externalId, p.sessionName, p.sessionDurationSecs,
              undefined, false,
              result.id_token,
              p.proxyUrl, p.proxyUsername, proxyPassword,
            );
            return;
          } catch {
            // Silent refresh failed — fall through to edit for re-auth
          }
        }
      } catch {
        // No stored refresh token
      }
      // Interactive: open edit dialog for re-authentication
      editingProfile = p;
      view = 'edit';
      return;
    }

    if (p.credentialType === 'keychain' && p.accessKeyId) {
      try {
        const secret = await s3ProfilesState.getSecret(p.id);
        if (secret) {
          secretKey = secret;
        }
      } catch (err: unknown) {
        connectError = 'Failed to retrieve credentials from keychain';
        error(String(err));
        return;
      }
    }

    await handleConnect(
      p.bucket,
      p.region,
      p.endpoint,
      p.profile,
      accessKey,
      secretKey,
      p.provider,
      p.customCapabilities,
      p.roleArn,
      p.externalId,
      p.sessionName,
      p.sessionDurationSecs,
      p.useTransferAcceleration,
      undefined,
      undefined,
      p.proxyUrl,
      p.proxyUsername,
      proxyPassword,
    );
  }

  async function handleSave(profileData: Omit<S3Profile, 'id'> & { id?: string }, secretKey?: string) {
    const isEdit = !!profileData.id;
    const id = profileData.id || `s3-profile-${Date.now()}`;
    const profile: S3Profile = { ...profileData, id } as S3Profile;

    if (secretKey && profile.credentialType === 'keychain') {
      await s3ProfilesState.saveSecret(id, secretKey);
    }

    if (isEdit) {
      s3ProfilesState.updateProfile(profile);
    } else {
      s3ProfilesState.addProfile(profile);
    }

    view = 'list';
    editingProfile = undefined;
  }

  function handleDialogCancel() {
    view = 'list';
    editingProfile = undefined;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (activeTab === 'saved' && view !== 'list') {
        view = 'list';
        editingProfile = undefined;
      } else {
        onClose();
      }
    }
  }
</script>

{#if view === 'edit' && editingProfile}
  <S3ConnectDialog
    saveMode={true}
    initialData={editingProfile}
    onConnect={handleConnect}
    onCancel={handleDialogCancel}
    onSave={handleSave}
  />
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
      <div class="dialog-title">S3 Connections</div>

      <div class="tab-bar">
        <button class="tab-btn" class:active={activeTab === 'saved'} onclick={() => { activeTab = 'saved'; }}>Saved</button>
        <button class="tab-btn" class:active={activeTab === 'connect'} onclick={() => { activeTab = 'connect'; }}>New Connection</button>
      </div>

      {#if activeTab === 'saved'}
        <div class="dialog-body">
          {#if connectError}
            <div class="error-msg">{connectError}</div>
          {/if}

          {#if s3ProfilesState.profiles.length === 0}
            <div class="empty-state">
              <div class="empty-text">No saved connections</div>
            </div>
          {:else}
            <div class="profile-list">
              {#each s3ProfilesState.profiles as p (p.id)}
                <div class="profile-row">
                  <img class="profile-icon" src={getProviderIcon(p.provider ?? 'aws')} alt="" />
                  <div class="profile-info">
                    <div class="profile-name">{p.name}</div>
                    <div class="profile-detail">{p.bucket} — {p.region}{p.endpoint ? ` — ${p.endpoint}` : ''}</div>
                  </div>
                  <div class="profile-actions">
                    <button class="action-btn connect" onclick={() => handleConnectFromProfile(p)}>Connect</button>
                    <button class="action-btn" onclick={() => handleEdit(p)}>Edit</button>
                    <button class="action-btn danger" onclick={() => handleDelete(p)}>Delete</button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
        <div class="dialog-footer">
          <button class="dialog-btn primary" onclick={handleAddNew}>Add New</button>
          <button class="dialog-btn" onclick={onClose}>Close</button>
        </div>
      {:else}
        <S3ConnectDialog
          embedded={true}
          onConnect={onConnectProp ?? handleConnect}
          onCancel={onClose}
        />
      {/if}
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

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--dialog-border);
    padding: 0 16px;
  }

  .tab-btn {
    padding: 6px 16px;
    font-size: 12px;
    font-family: inherit;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-btn.active {
    border-bottom: 2px solid var(--text-accent);
    color: var(--text-accent);
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    overflow-y: auto;
  }

  .dialog-footer {
    display: flex;
    justify-content: center;
    gap: 10px;
    padding: 16px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .error-msg {
    font-size: 12px;
    color: var(--warning-color);
    padding: 8px 12px;
    background: rgba(255, 100, 100, 0.1);
    border-radius: var(--radius-sm);
    border: 1px solid rgba(255, 100, 100, 0.2);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 24px 0;
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .profile-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 320px;
    overflow-y: auto;
  }

  .profile-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .profile-row:hover {
    background: var(--bg-hover);
  }

  .profile-icon {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
  }

  .profile-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .profile-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .profile-detail {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .profile-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .action-btn {
    padding: 4px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .action-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .action-btn.connect {
    background: rgba(110,168,254,0.15);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .action-btn.connect:hover {
    background: rgba(110,168,254,0.25);
  }

  .action-btn.danger:hover {
    border-color: var(--warning-color);
    color: var(--warning-color);
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

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .dialog-btn.primary {
    background: rgba(110,168,254,0.2);
    border-color: var(--border-active);
    color: var(--text-accent);
  }

  .dialog-btn.primary:hover {
    background: rgba(110,168,254,0.3);
  }
</style>
