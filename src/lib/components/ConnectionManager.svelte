<script lang="ts">
  import { untrack } from 'svelte';
  import { panels } from '$lib/state/panels.svelte';
  import { appState } from '$lib/state/app.svelte';
  import { connectionsState } from '$lib/state/connections.svelte';
  import S3ConnectDialog from '$lib/components/S3ConnectDialog.svelte';
  import SftpConnectDialog from '$lib/components/SftpConnectDialog.svelte';
  import { resolveCapabilities, getProviderIcon } from '$lib/data/s3-providers';
  import { error } from '$lib/services/log';
  import { oidcRefresh } from '$lib/services/s3';
  import type { S3Profile, S3ConnectionInfo, SftpConnectionInfo, S3ProviderCapabilities, SftpProfile, ConnectionProfile } from '$lib/types';

  interface Props {
    onClose: () => void;
    initialTab?: 'saved' | 'connect';
    initialData?: Partial<S3Profile>;
    onConnect?: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities, roleArn?: string, externalId?: string, sessionName?: string, sessionDurationSecs?: number, useTransferAcceleration?: boolean, anonymous?: boolean, webIdentityToken?: string, proxyUrl?: string, proxyUsername?: string, proxyPassword?: string) => void;
  }

  let { onClose, initialTab = 'saved', initialData, onConnect: onConnectProp }: Props = $props();

  type Selection =
    | { mode: 'saved' }
    | { mode: 'new'; protocol: 's3' | 'sftp' }
    | { mode: 'edit'; profile: S3Profile }
    | { mode: 'edit-sftp'; profile: SftpProfile };

  let selected = $state<Selection>(untrack(() =>
    initialTab === 'connect' ? { mode: 'new', protocol: 's3' } : { mode: 'saved' }
  ));
  let connectError = $state('');
  let dialogKey = $state(0);

  function handleEdit(profile: ConnectionProfile) {
    if (profile.type === 's3') {
      selected = { mode: 'edit', profile };
      dialogKey++;
    } else if (profile.type === 'sftp') {
      selected = { mode: 'edit-sftp', profile };
      dialogKey++;
    }
  }

  function handleDelete(profile: ConnectionProfile) {
    appState.showConfirm(`Delete connection "${profile.name}"?`, async () => {
      if (profile.type === 's3' && profile.credentialType === 'keychain') {
        await connectionsState.deleteSecret(profile.id);
      } else if (profile.type === 'sftp' && profile.authMethod === 'password') {
        await connectionsState.deleteSecret(profile.id);
      }
      connectionsState.removeProfile(profile.id);
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

  async function handleConnectSftp(
    host: string,
    port: number,
    username: string,
    authMethod: string,
    password?: string,
    keyPath?: string,
    keyPassphrase?: string,
  ) {
    connectError = '';
    const panel = panels.active;
    const connectionId = `sftp-${Date.now()}`;
    const info: SftpConnectionInfo = { connectionId, host, port, username };
    try {
      await panel.connectSftp(info, password, keyPath, keyPassphrase);
      onClose();
    } catch (err: unknown) {
      connectError = err instanceof Error ? err.message : String(err);
      error(String(err));
    }
  }

  async function handleSaveSftp(profileData: Omit<SftpProfile, 'id'> & { id?: string }, password?: string) {
    const isEdit = !!profileData.id;
    const id = profileData.id || `sftp-profile-${Date.now()}`;
    const profile: SftpProfile = { ...profileData, id, type: 'sftp' } as SftpProfile;

    if (password && profile.authMethod === 'password') {
      await connectionsState.saveSecret(id, password);
    }

    if (isEdit) {
      connectionsState.updateProfile(profile);
    } else {
      connectionsState.addProfile(profile);
    }

    selected = { mode: 'saved' };
  }

  async function handleConnectFromProfile(p: ConnectionProfile) {
    if (p.type === 'sftp') {
      connectError = '';
      let password: string | undefined;
      if (p.authMethod === 'password') {
        try {
          const secret = await connectionsState.getSecret(p.id);
          if (secret) password = secret;
        } catch (err: unknown) {
          connectError = 'Failed to retrieve password from keychain';
          error(String(err));
          return;
        }
      }
      await handleConnectSftp(
        p.host,
        p.port,
        p.username,
        p.authMethod,
        password,
        p.keyPath,
      );
      return;
    }
    if (p.type !== 's3') return;
    connectError = '';
    let secretKey: string | undefined;
    let accessKey: string | undefined = p.accessKeyId;

    // Load proxy password from keychain
    let proxyPassword: string | undefined;
    if (p.proxyUrl && p.proxyUrl !== 'system') {
      try {
        const pp = await connectionsState.getSecret(p.id + ':proxy');
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
        const refreshToken = await connectionsState.getSecret(p.id + ':oidc-refresh');
        if (refreshToken && p.oidcIssuerUrl && p.oidcClientId) {
          try {
            const result = await oidcRefresh(p.oidcIssuerUrl, p.oidcClientId, refreshToken);
            // Store new refresh token
            if (result.refresh_token) {
              await connectionsState.saveSecret(p.id + ':oidc-refresh', result.refresh_token);
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
      selected = { mode: 'edit', profile: p };
      dialogKey++;
      return;
    }

    if (p.credentialType === 'keychain' && p.accessKeyId) {
      try {
        const secret = await connectionsState.getSecret(p.id);
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
    const profile: S3Profile = { ...profileData, id, type: 's3' } as S3Profile;

    if (secretKey && profile.credentialType === 'keychain') {
      await connectionsState.saveSecret(id, secretKey);
    }

    if (isEdit) {
      connectionsState.updateProfile(profile);
    } else {
      connectionsState.addProfile(profile);
    }

    selected = { mode: 'saved' };
  }

  function handleDialogCancel() {
    selected = { mode: 'saved' };
  }

  function protocolBadge(p: ConnectionProfile): string {
    return p.type.toUpperCase();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (selected.mode === 'edit' || selected.mode === 'edit-sftp') {
        selected = { mode: 'saved' };
      } else {
        onClose();
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Connections</div>

    <div class="dialog-content">
      <div class="manager-sidebar">
        <div class="sidebar-section-label">SAVED</div>
        <button
          class="sidebar-item"
          class:active={selected.mode === 'saved' || selected.mode === 'edit' || selected.mode === 'edit-sftp'}
          onclick={() => { selected = { mode: 'saved' }; }}
        >
          Connections
          {#if connectionsState.profiles.length > 0}
            <span class="sidebar-badge">{connectionsState.profiles.length}</span>
          {/if}
        </button>

        <div class="sidebar-section-label">NEW</div>
        <button
          class="sidebar-item"
          class:active={selected.mode === 'new'}
          onclick={() => { selected = { mode: 'new', protocol: 's3' }; dialogKey++; }}
        >
          S3
          <span class="sidebar-hint">Amazon S3 & compatible</span>
        </button>
        <button
          class="sidebar-item"
          class:active={selected.mode === 'new' && selected.protocol === 'sftp'}
          onclick={() => { selected = { mode: 'new', protocol: 'sftp' }; dialogKey++; }}
        >
          SFTP
          <span class="sidebar-hint">SSH File Transfer</span>
        </button>
      </div>

      <div class="manager-main">
        {#if selected.mode === 'saved'}
          <div class="main-body">
            {#if connectError}
              <div class="error-msg">{connectError}</div>
            {/if}

            {#if connectionsState.profiles.length === 0}
              <div class="empty-state">
                <div class="empty-text">No saved connections</div>
                <button class="dialog-btn primary" onclick={() => { selected = { mode: 'new', protocol: 's3' }; dialogKey++; }}>
                  New S3 Connection
                </button>
              </div>
            {:else}
              <div class="profile-list">
                {#each connectionsState.profiles as p (p.id)}
                  <div class="profile-row">
                    {#if p.type === 's3'}
                      <img class="profile-icon" src={getProviderIcon(p.provider ?? 'aws')} alt="" />
                    {:else}
                      <span class="profile-icon-placeholder">&#128274;</span>
                    {/if}
                    <span class="protocol-badge">{protocolBadge(p)}</span>
                    <div class="profile-info">
                      <div class="profile-name">{p.name}</div>
                      <div class="profile-detail">
                        {#if p.type === 's3'}
                          {p.bucket} — {p.region}{p.endpoint ? ` — ${p.endpoint}` : ''}
                        {:else}
                          {p.host}:{p.port} — {p.username}
                        {/if}
                      </div>
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
          <div class="main-footer">
            <button class="dialog-btn" onclick={onClose}>Close</button>
          </div>
        {:else if selected.mode === 'edit'}
          {#key dialogKey}
            <S3ConnectDialog
              embedded={true}
              saveMode={true}
              initialData={selected.profile}
              onConnect={handleConnect}
              onCancel={handleDialogCancel}
              onSave={handleSave}
            />
          {/key}
        {:else if selected.mode === 'edit-sftp'}
          {#key dialogKey}
            <SftpConnectDialog
              embedded={true}
              saveMode={true}
              initialData={selected.profile}
              onConnect={handleConnectSftp}
              onCancel={handleDialogCancel}
              onSave={handleSaveSftp}
            />
          {/key}
        {:else if selected.mode === 'new' && selected.protocol === 'sftp'}
          {#key dialogKey}
            <SftpConnectDialog
              embedded={true}
              saveMode={true}
              onConnect={handleConnectSftp}
              onCancel={onClose}
              onSave={handleSaveSftp}
            />
          {/key}
        {:else}
          {#key dialogKey}
            <S3ConnectDialog
              embedded={true}
              saveMode={true}
              initialData={initialData as S3Profile | undefined}
              onConnect={onConnectProp ?? handleConnect}
              onCancel={onClose}
              onSave={handleSave}
            />
          {/key}
        {/if}
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
    width: 110ch;
    height: 90vh;
    max-width: 95vw;
    max-height: 1000px;
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
    flex-shrink: 0;
  }

  .dialog-content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* --- Sidebar --- */
  .manager-sidebar {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid var(--dialog-border);
    padding: 8px 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .sidebar-section-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-secondary);
    padding: 10px 16px 4px;
  }

  .sidebar-item {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px 16px;
    background: none;
    border: none;
    text-align: left;
    font-family: inherit;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast);
    position: relative;
  }

  .sidebar-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .sidebar-item.active {
    background: rgba(110,168,254,0.12);
    color: var(--text-accent);
  }

  .sidebar-badge {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    font-size: 10px;
    font-weight: 600;
    min-width: 18px;
    height: 18px;
    line-height: 18px;
    text-align: center;
    border-radius: 9px;
    background: rgba(110,168,254,0.15);
    color: var(--text-accent);
  }

  .sidebar-hint {
    font-size: 11px;
    font-weight: 400;
    color: var(--text-secondary);
  }

  .sidebar-item.active .sidebar-hint {
    color: var(--text-accent);
    opacity: 0.7;
  }

  /* --- Main area --- */
  .manager-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .main-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    overflow-y: auto;
  }

  .main-footer {
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
    padding: 40px 0;
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .profile-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
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

  .profile-icon-placeholder {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
  }

  .protocol-badge {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.5px;
    padding: 1px 5px;
    border-radius: 3px;
    background: rgba(110,168,254,0.15);
    color: var(--text-accent);
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
