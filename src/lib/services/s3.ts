import { invoke, Channel } from '@tauri-apps/api/core';
import type { DirListing, ProgressEvent, S3Bucket, S3ObjectProperties, SearchEvent } from '$lib/types';

export async function s3CheckCredentials(): Promise<boolean> {
  return await invoke<boolean>('s3_check_credentials');
}

export async function s3Connect(
  id: string,
  bucket: string,
  region: string,
  endpoint?: string,
  profile?: string,
  accessKey?: string,
  secretKey?: string
): Promise<void> {
  await invoke('s3_connect', {
    id,
    bucket,
    region,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
  });
}

export async function s3ListBuckets(
  region: string,
  endpoint?: string,
  profile?: string,
  accessKey?: string,
  secretKey?: string,
): Promise<S3Bucket[]> {
  return await invoke<S3Bucket[]>('s3_list_buckets', {
    region,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
  });
}

export async function s3Disconnect(id: string): Promise<void> {
  await invoke('s3_disconnect', { id });
}

export async function s3ListObjects(
  id: string,
  prefix: string
): Promise<DirListing> {
  return await invoke<DirListing>('s3_list_objects', { id, prefix });
}

export async function s3Download(
  id: string,
  opId: string,
  keys: string[],
  destination: string,
  onProgress: (e: ProgressEvent) => void
): Promise<void> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  await invoke('s3_download', { id, opId, keys, destination, channel });
}

export async function s3Upload(
  id: string,
  opId: string,
  sources: string[],
  destPrefix: string,
  onProgress: (e: ProgressEvent) => void
): Promise<void> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  await invoke('s3_upload', { id, opId, sources, destPrefix, channel });
}

export async function s3CopyObjects(
  srcId: string,
  opId: string,
  srcKeys: string[],
  destId: string,
  destPrefix: string,
  onProgress: (e: ProgressEvent) => void
): Promise<void> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  await invoke('s3_copy_objects', { srcId, opId, srcKeys, destId, destPrefix, channel });
}

export async function s3HeadObject(
  id: string,
  key: string
): Promise<S3ObjectProperties> {
  return await invoke<S3ObjectProperties>('s3_head_object', { id, key });
}

export async function s3DeleteObjects(
  id: string,
  keys: string[]
): Promise<void> {
  await invoke('s3_delete_objects', { id, keys });
}

export async function s3CreateFolder(id: string, key: string): Promise<void> {
  await invoke('s3_create_folder', { id, key });
}

export async function s3RenameObject(id: string, key: string, newName: string): Promise<void> {
  await invoke('s3_rename_object', { id, key, newName });
}

export async function s3SearchObjects(
  id: string,
  searchId: string,
  prefix: string,
  query: string,
  onEvent: (e: SearchEvent) => void,
): Promise<void> {
  const channel = new Channel<SearchEvent>();
  channel.onmessage = onEvent;
  await invoke('s3_search_objects', { id, searchId, prefix, query, channel });
}
