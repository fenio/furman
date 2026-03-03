import { panels } from '$lib/state/panels.svelte';
import { appState } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { sidebarState } from '$lib/state/sidebar.svelte';
import { workspacesState } from '$lib/state/workspaces.svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
import { sftpBookmarksState } from '$lib/state/sftpbookmarks.svelte';
import { keychainGet } from '$lib/services/keychain';
import { resolveCapabilities } from '$lib/data/s3-providers';
import { error } from '$lib/services/log';
import type { S3Bookmark, SftpBookmark, S3ConnectionInfo } from '$lib/types';

// ── Sidebar action types ────────────────────────────────────────────────────

export type SidebarAction =
  | { type: 'favorite'; path: string }
  | { type: 'add-favorite' }
  | { type: 'workspace'; name: string; leftPath: string; rightPath: string; activePanel: 'left' | 'right'; leftTabs?: string[]; rightTabs?: string[]; leftActiveTab?: number; rightActiveTab?: number }
  | { type: 'save-workspace' }
  | { type: 's3-bookmark'; id: string; name: string; profileId: string; path: string }
  | { type: 'sftp-bookmark'; id: string; name: string; profileId: string; path: string }
  | { type: 'volume'; mountPoint: string }
  | { type: 's3'; panel: 'left' | 'right'; bucket: string }
  | { type: 'theme' };

// ── Sidebar items builder ───────────────────────────────────────────────────

export function buildSidebarItems(): SidebarAction[] {
  const list: SidebarAction[] = [];
  for (const fav of sidebarState.favorites) {
    list.push({ type: 'favorite', path: fav.path });
  }
  list.push({ type: 'add-favorite' });
  for (const ws of workspacesState.workspaces) {
    list.push({ type: 'workspace', name: ws.name, leftPath: ws.leftPath, rightPath: ws.rightPath, activePanel: ws.activePanel, leftTabs: ws.leftTabs, rightTabs: ws.rightTabs, leftActiveTab: ws.leftActiveTab, rightActiveTab: ws.rightActiveTab });
  }
  list.push({ type: 'save-workspace' });
  for (const bm of s3BookmarksState.bookmarks) {
    list.push({ type: 's3-bookmark', id: bm.id, name: bm.name, profileId: bm.profileId, path: bm.path });
  }
  for (const bm of sftpBookmarksState.bookmarks) {
    list.push({ type: 'sftp-bookmark', id: bm.id, name: bm.name, profileId: bm.profileId, path: bm.path });
  }
  for (const vol of sidebarState.volumes) {
    list.push({ type: 'volume', mountPoint: vol.mount_point });
  }
  if (panels.left.s3Connection) {
    list.push({ type: 's3', panel: 'left', bucket: panels.left.s3Connection.bucket });
  }
  if (panels.right.s3Connection) {
    list.push({ type: 's3', panel: 'right', bucket: panels.right.s3Connection.bucket });
  }
  list.push({ type: 'theme' });
  return list;
}

// ── Bookmark navigation ─────────────────────────────────────────────────────

export async function navigateBookmark(bm: S3Bookmark) {
  sidebarState.blur();
  const profile = connectionsState.s3Profiles.find((p) => p.id === bm.profileId);
  if (!profile) {
    statusState.setMessage('S3 profile not found — save the connection as a profile first');
    return;
  }

  const panel = panels.active;
  const bmBucket = bm.path.replace(/^s3:\/\//, '').split('/')[0];

  if (panel.backend === 's3' && panel.s3Connection && panel.s3Connection.bucket === bmBucket) {
    await panel.loadDirectory(bm.path);
    return;
  }

  let secretKey: string | undefined;
  const accessKey: string | undefined = profile.accessKeyId;
  if (profile.credentialType === 'keychain' && profile.accessKeyId) {
    try {
      const secret = await keychainGet(profile.id);
      if (secret) secretKey = secret;
    } catch (err: unknown) {
      error(String(err));
      appState.showAlert('Failed to retrieve credentials from keychain');
      return;
    }
  }

  const connectionId = `s3-${Date.now()}`;
  const caps = resolveCapabilities({ provider: profile.provider, customCapabilities: profile.customCapabilities });
  const info: S3ConnectionInfo = { bucket: profile.bucket, region: profile.region, connectionId, provider: profile.provider, capabilities: caps };
  if (profile.endpoint) info.endpoint = profile.endpoint;
  if (profile.profile) info.profile = profile.profile;

  try {
    await panel.connectS3(info, profile.endpoint, profile.profile, accessKey, secretKey, profile.roleArn, profile.externalId, profile.sessionName, profile.sessionDurationSecs, profile.useTransferAcceleration);
    if (bm.path !== `s3://${profile.bucket}/`) {
      await panel.loadDirectory(bm.path);
    }
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Failed to connect: ' + String(err));
  }
}

export async function navigateSftpBookmark(bm: SftpBookmark) {
  sidebarState.blur();
  const profile = connectionsState.sftpProfiles.find((p) => p.id === bm.profileId);
  if (!profile) {
    statusState.setMessage('SFTP profile not found — save the connection as a profile first');
    return;
  }

  const panel = panels.active;

  if (panel.backend === 'sftp' && panel.sftpConnection &&
      panel.sftpConnection.host === profile.host &&
      panel.sftpConnection.port === profile.port) {
    await panel.loadDirectory(bm.path);
    return;
  }

  let password: string | undefined;
  if (profile.authMethod === 'password') {
    try {
      const secret = await keychainGet(profile.id);
      if (secret) password = secret;
    } catch (err: unknown) {
      error(String(err));
      appState.showAlert('Failed to retrieve credentials from keychain');
      return;
    }
  }

  try {
    const connectionId = `sftp-${Date.now()}`;
    await panel.connectSftp(
      { connectionId, host: profile.host, port: profile.port, username: profile.username },
      password,
      profile.keyPath,
    );
    if (bm.path !== `sftp://${profile.host}:${profile.port}/`) {
      await panel.loadDirectory(bm.path);
    }
  } catch (err: unknown) {
    error(String(err));
    appState.showAlert('Failed to connect: ' + String(err));
  }
}

// ── Tab / workspace restoration ─────────────────────────────────────────────

export function restoreTabsForSide(side: 'left' | 'right', paths: string[], activeIdx: number) {
  while ((side === 'left' ? panels.leftTabs : panels.rightTabs).length > paths.length) {
    panels.closeTab(side, (side === 'left' ? panels.leftTabs : panels.rightTabs).length - 1);
  }
  const loads: Promise<void>[] = [];
  for (let i = 0; i < paths.length; i++) {
    if (i >= (side === 'left' ? panels.leftTabs : panels.rightTabs).length) {
      panels.addTab(side);
    }
    loads.push((side === 'left' ? panels.leftTabs : panels.rightTabs)[i].loadDirectory(paths[i]));
  }
  if (side === 'left') panels.leftActiveTab = activeIdx;
  else panels.rightActiveTab = activeIdx;
  Promise.all(loads);
}

export function activateSidebarItem(action: SidebarAction) {
  if (!action) return;
  switch (action.type) {
    case 'favorite':
      sidebarState.blur();
      panels.active.loadDirectory(action.path);
      break;
    case 'add-favorite': {
      const path = panels.active.path;
      const name = path.replace(/\/+$/, '').split('/').pop() || path;
      sidebarState.addFavorite(name, path);
      break;
    }
    case 'workspace':
      sidebarState.blur();
      panels.activePanel = action.activePanel;
      if (action.leftTabs && action.leftTabs.length > 0) {
        restoreTabsForSide('left', action.leftTabs, action.leftActiveTab ?? 0);
      } else {
        panels.left.loadDirectory(action.leftPath);
      }
      if (action.rightTabs && action.rightTabs.length > 0) {
        restoreTabsForSide('right', action.rightTabs, action.rightActiveTab ?? 0);
      } else {
        panels.right.loadDirectory(action.rightPath);
      }
      break;
    case 'save-workspace':
      sidebarState.blur();
      appState.showInput('Workspace name:', '', (name) => {
        appState.closeModal();
        if (!name) return;
        workspacesState.save({
          name,
          leftPath: panels.left.path,
          rightPath: panels.right.path,
          activePanel: panels.activePanel,
          leftTabs: panels.leftTabs.map(t => t.path),
          rightTabs: panels.rightTabs.map(t => t.path),
          leftActiveTab: panels.leftActiveTab,
          rightActiveTab: panels.rightActiveTab,
        });
      });
      break;
    case 's3-bookmark':
      navigateBookmark({ id: action.id, name: action.name, profileId: action.profileId, path: action.path });
      break;
    case 'sftp-bookmark':
      navigateSftpBookmark({ id: action.id, name: action.name, profileId: action.profileId, path: action.path });
      break;
    case 'volume':
      sidebarState.blur();
      panels.active.loadDirectory(action.mountPoint);
      break;
    case 's3':
      sidebarState.blur();
      panels.activePanel = action.panel;
      panels.active.loadDirectory(`s3://${action.bucket}/`);
      break;
    case 'theme':
      appState.toggleTheme();
      break;
  }
}
