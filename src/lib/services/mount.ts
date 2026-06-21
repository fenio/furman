import { invoke } from '@tauri-apps/api/core';

/**
 * Mount an SMB or NFS network share via the operating system and return the
 * local mount point. The share is then browsed as a normal local folder.
 *
 * macOS hands the URL to NetFS (mounts under /Volumes, reuses the Keychain);
 * Linux uses GVfs (`gio mount`). NFS is macOS-only here — Linux NFS needs root.
 */
export async function mountNetworkShare(
  protocol: 'smb' | 'nfs',
  host: string,
  share: string,
  username?: string,
  password?: string,
  domain?: string,
): Promise<string> {
  return await invoke<string>('mount_network_share', {
    protocol,
    host,
    share,
    username: username ?? null,
    password: password ?? null,
    domain: domain ?? null,
  });
}
