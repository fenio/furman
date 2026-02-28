import { invoke, Channel } from '@tauri-apps/api/core';
import type { DirListing, FileProperties, ProgressEvent, TransferCheckpoint } from '$lib/types';

export async function sftpConnect(
  id: string,
  host: string,
  port: number,
  username: string,
  authMethod: string,
  password?: string,
  keyPath?: string,
  keyPassphrase?: string,
): Promise<string> {
  return await invoke<string>('sftp_connect', {
    id,
    host,
    port,
    username,
    authMethod,
    password: password ?? null,
    keyPath: keyPath ?? null,
    keyPassphrase: keyPassphrase ?? null,
  });
}

export async function sftpDisconnect(id: string): Promise<void> {
  await invoke('sftp_disconnect', { id });
}

export async function sftpListObjects(id: string, path: string): Promise<DirListing> {
  return await invoke<DirListing>('sftp_list_objects', { id, path });
}

export async function sftpDelete(id: string, paths: string[]): Promise<void> {
  await invoke('sftp_delete', { id, paths });
}

export async function sftpRename(id: string, path: string, newName: string): Promise<void> {
  await invoke('sftp_rename', { id, path, newName });
}

export async function sftpCreateFolder(id: string, path: string): Promise<void> {
  await invoke('sftp_create_folder', { id, path });
}

export async function sftpDownload(
  id: string,
  opId: string,
  keys: string[],
  destination: string,
  onProgress: (e: ProgressEvent) => void,
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('sftp_download', {
    id, opId, keys, destination, channel,
  });
}

export async function sftpUpload(
  id: string,
  opId: string,
  sources: string[],
  remotePrefix: string,
  onProgress: (e: ProgressEvent) => void,
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('sftp_upload', {
    id, opId, sources, remotePrefix, channel,
  });
}

export async function sftpDownloadTemp(id: string, path: string): Promise<string> {
  return await invoke<string>('sftp_download_temp', { id, path });
}

export async function sftpPutText(id: string, path: string, content: string): Promise<void> {
  await invoke('sftp_put_text', { id, path, content });
}

export async function sftpHead(id: string, path: string): Promise<FileProperties> {
  return await invoke<FileProperties>('sftp_head', { id, path });
}
