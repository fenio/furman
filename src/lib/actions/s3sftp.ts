import { panels } from '$lib/state/panels.svelte';
import { appState } from '$lib/state/app.svelte';
import { statusState } from '$lib/state/status.svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
import { sftpBookmarksState } from '$lib/state/sftpbookmarks.svelte';
import { s3PresignUrl, s3BulkChangeStorageClass } from '$lib/services/s3';
import { resolveCapabilities } from '$lib/data/s3-providers';
import { error } from '$lib/services/log';
import type { S3ConnectionInfo } from '$lib/types';

export function handlePresignUrl() {
  const active = panels.active;
  const entry = active.currentEntry;
  if (!entry || entry.name === '..' || entry.is_dir) return;
  if (active.backend !== 's3' || !active.s3Connection) return;

  const connectionId = active.s3Connection.connectionId;
  appState.showInput('Link expires in (minutes):', '60', async (val: string) => {
    appState.closeModal();
    const minutes = parseInt(val, 10);
    if (!minutes || minutes <= 0) return;
    try {
      const url = await s3PresignUrl(connectionId, entry.path, minutes * 60);
      await navigator.clipboard.writeText(url);
      statusState.setMessage('Presigned URL copied to clipboard');
    } catch (err: unknown) {
      error(String(err));
    }
  });
}

export async function handleCopyS3Uri() {
  const active = panels.active;
  if (active.backend !== 's3') return;
  const entry = active.currentEntry;
  if (!entry || entry.name === '..') return;
  try {
    await navigator.clipboard.writeText(entry.path);
    statusState.setMessage(`Copied: ${entry.path}`);
  } catch (err: unknown) {
    error(String(err));
  }
}

export function handleBulkStorageClassChange() {
  const active = panels.active;
  if (active.backend !== 's3' || !active.s3Connection) return;
  const caps = active.s3Connection.capabilities;
  if (caps && caps.storageClasses.length <= 1) return;
  const selected = active.getSelectedOrCurrent();
  if (selected.length === 0) return;

  const connectionId = active.s3Connection.connectionId;
  const defaultClass = caps && caps.storageClasses.length > 1 ? caps.storageClasses[1] : 'STANDARD_IA';
  appState.showInput('Target storage class (e.g. STANDARD_IA, GLACIER):', defaultClass, async (targetClass: string) => {
    appState.closeModal();
    if (!targetClass) return;
    try {
      const failed = await s3BulkChangeStorageClass(connectionId, selected, targetClass);
      if (failed.length === 0) {
        statusState.setMessage(`Storage class changed to ${targetClass} for ${selected.length} object(s)`);
      } else {
        appState.showAlert(`${selected.length - failed.length} succeeded, ${failed.length} failed`);
      }
      await active.loadDirectory(active.path);
    } catch (err: unknown) {
      error(String(err));
    }
  });
}

export function handleBucketProperties() {
  const active = panels.active;
  if (active.backend !== 's3' || !active.s3Connection) return;
  appState.showProperties(
    `s3://${active.s3Connection.bucket}/`,
    active.backend,
    {
      s3ConnectionId: active.s3Connection.connectionId,
      capabilities: active.s3Connection.capabilities,
      s3Connection: active.s3Connection,
    },
  );
}

export function handleBookmarkS3() {
  const active = panels.active;
  if (active.backend !== 's3' || !active.s3Connection) return;

  const conn = active.s3Connection;
  const profile = connectionsState.s3Profiles.find((p) =>
    p.bucket === conn.bucket &&
    p.region === conn.region &&
    (p.endpoint ?? '') === (conn.endpoint ?? ''),
  );

  if (!profile) {
    statusState.setMessage('Save this connection as a profile first');
    return;
  }

  const pathSegments = active.path.replace(/\/+$/, '').split('/');
  const defaultName = pathSegments[pathSegments.length - 1] || conn.bucket;

  appState.showInput('Bookmark name:', defaultName, (name) => {
    appState.closeModal();
    if (!name) return;
    s3BookmarksState.add({
      id: Date.now().toString(36),
      name,
      profileId: profile.id,
      path: active.path,
    });
    statusState.setMessage(`Bookmarked: ${name}`);
  });
}

export function handleBookmarkSftp() {
  const active = panels.active;
  if (active.backend !== 'sftp' || !active.sftpConnection) return;

  const conn = active.sftpConnection;
  const profile = connectionsState.sftpProfiles.find((p) =>
    p.host === conn.host &&
    p.port === conn.port &&
    p.username === conn.username,
  );

  if (!profile) {
    statusState.setMessage('Save this connection as a profile first');
    return;
  }

  const pathSegments = active.path.replace(/\/+$/, '').split('/');
  const defaultName = pathSegments[pathSegments.length - 1] || conn.host;

  appState.showInput('Bookmark name:', defaultName, (name) => {
    appState.closeModal();
    if (!name) return;
    sftpBookmarksState.add({
      id: Date.now().toString(36),
      name,
      profileId: profile.id,
      path: active.path,
    });
    statusState.setMessage(`Bookmarked: ${name}`);
  });
}

export function handleProperties() {
  const active = panels.active;
  const entry = active.currentEntry;
  if (!entry) return;
  // If cursor is on '..' in an S3 bucket, show bucket-level properties
  if (entry.name === '..') {
    if (active.backend === 's3' && active.s3Connection) {
      appState.showProperties(
        `s3://${active.s3Connection.bucket}/`,
        active.backend,
        {
          s3ConnectionId: active.s3Connection.connectionId,
          capabilities: active.s3Connection.capabilities,
          s3Connection: active.s3Connection,
        },
      );
    } else if (active.backend === 'sftp' && active.sftpConnection) {
      appState.showProperties(
        active.path,
        active.backend,
        {
          sftpConnectionId: active.sftpConnection.connectionId,
          sftpConnection: active.sftpConnection,
        },
      );
    }
    return;
  }
  // Multi-selection: batch edit (S3 = S3BatchEdit, local/sftp = LocalBatchEdit)
  if (active.selectedPaths.size > 1) {
    if (active.backend === 's3' && active.s3Connection) {
      const keys = [...active.selectedPaths].filter(p => !p.endsWith('/'));
      if (keys.length > 0) {
        appState.showBatchEdit(
          keys,
          active.s3Connection.connectionId,
          active.s3Connection.capabilities,
        );
        return;
      }
    }
    if (active.backend === 'local' || active.backend === 'sftp') {
      const paths = [...active.selectedPaths];
      if (paths.length > 0) {
        appState.showLocalBatchEdit(
          paths,
          active.backend,
          active.sftpConnection?.connectionId,
        );
        return;
      }
    }
  }
  appState.showProperties(
    entry.path,
    active.backend,
    {
      s3ConnectionId: active.s3Connection?.connectionId,
      capabilities: active.s3Connection?.capabilities,
      s3Connection: active.s3Connection ?? undefined,
      sftpConnectionId: active.sftpConnection?.connectionId,
      sftpConnection: active.sftpConnection ?? undefined,
      archiveInfo: active.archiveInfo ?? undefined,
    },
  );
}

export function _handleS3Connect() {
  const panel = panels.active;
  appState.showConnect(async (bucket, region, endpoint, profile, accessKey, secretKey, provider, customCapabilities) => {
    const connectionId = `s3-${Date.now()}`;
    const caps = resolveCapabilities({ provider, customCapabilities });
    const info: S3ConnectionInfo = { bucket, region, connectionId, provider, capabilities: caps };
    if (endpoint) info.endpoint = endpoint;
    if (profile) info.profile = profile;
    try {
      await panel.connectS3(info, endpoint, profile, accessKey, secretKey);
    } catch (err: unknown) {
      error(String(err));
    }
  });
}

export function handleQuit() {
  appState.showConfirm('Quit Furman?', async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    } catch {
      window.close();
    }
  });
}
