import { invoke, Channel } from '@tauri-apps/api/core';
import type { DirListing, ProgressEvent, S3Bucket, S3BucketAcl, S3BucketEncryption, S3BucketVersioning, S3CorsRule, S3LifecycleRule, S3MultipartUpload, S3ObjectMetadata, S3ObjectProperties, S3ObjectVersion, S3PublicAccessBlock, S3Tag, SearchEvent, TransferCheckpoint } from '$lib/types';

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
  secretKey?: string,
  roleArn?: string,
  externalId?: string,
  sessionName?: string,
  sessionDurationSecs?: number,
): Promise<void> {
  await invoke('s3_connect', {
    id,
    bucket,
    region,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
    roleArn: roleArn || null,
    externalId: externalId || null,
    sessionName: sessionName || null,
    sessionDurationSecs: sessionDurationSecs ?? null,
  });
}

export async function s3ListBuckets(
  region: string,
  endpoint?: string,
  profile?: string,
  accessKey?: string,
  secretKey?: string,
  roleArn?: string,
  externalId?: string,
  sessionName?: string,
  sessionDurationSecs?: number,
): Promise<S3Bucket[]> {
  return await invoke<S3Bucket[]>('s3_list_buckets', {
    region,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
    roleArn: roleArn || null,
    externalId: externalId || null,
    sessionName: sessionName || null,
    sessionDurationSecs: sessionDurationSecs ?? null,
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
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('s3_download', { id, opId, keys, destination, channel });
}

export async function s3Upload(
  id: string,
  opId: string,
  sources: string[],
  destPrefix: string,
  onProgress: (e: ProgressEvent) => void
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('s3_upload', { id, opId, sources, destPrefix, channel });
}

export async function s3CopyObjects(
  srcId: string,
  opId: string,
  srcKeys: string[],
  destId: string,
  destPrefix: string,
  onProgress: (e: ProgressEvent) => void
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('s3_copy_objects', { srcId, opId, srcKeys, destId, destPrefix, channel });
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

export async function s3PresignUrl(id: string, key: string, expiresInSecs: number): Promise<string> {
  return await invoke<string>('s3_presign_url', { id, key, expiresInSecs });
}

export async function s3DownloadToTemp(
  id: string,
  key: string
): Promise<string> {
  return await invoke<string>('s3_download_temp', { id, key });
}

export async function s3PutText(id: string, key: string, content: string): Promise<void> {
  await invoke('s3_put_text', { id, key, content });
}

export async function s3ChangeStorageClass(
  id: string,
  key: string,
  targetClass: string,
): Promise<void> {
  await invoke('s3_change_storage_class', { id, key, targetClass });
}

export async function s3RestoreObject(
  id: string,
  key: string,
  days: number,
  tier: string,
): Promise<void> {
  await invoke('s3_restore_object', { id, key, days, tier });
}

export async function s3ListObjectVersions(
  id: string,
  key: string,
): Promise<S3ObjectVersion[]> {
  return await invoke<S3ObjectVersion[]>('s3_list_object_versions', { id, key });
}

export async function s3DownloadVersion(
  id: string,
  key: string,
  versionId: string,
): Promise<string> {
  return await invoke<string>('s3_download_version', { id, key, versionId });
}

export async function s3RestoreVersion(
  id: string,
  key: string,
  versionId: string,
): Promise<void> {
  await invoke('s3_restore_version', { id, key, versionId });
}

export async function s3DeleteVersion(
  id: string,
  key: string,
  versionId: string,
): Promise<void> {
  await invoke('s3_delete_version', { id, key, versionId });
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

// ── Bucket Management ───────────────────────────────────────────────────────

export async function s3CreateBucket(
  region: string,
  bucketName: string,
  endpoint?: string,
  profile?: string,
  accessKey?: string,
  secretKey?: string,
  roleArn?: string,
  externalId?: string,
  sessionName?: string,
  sessionDurationSecs?: number,
): Promise<void> {
  await invoke('s3_create_bucket', {
    region,
    bucketName,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
    roleArn: roleArn || null,
    externalId: externalId || null,
    sessionName: sessionName || null,
    sessionDurationSecs: sessionDurationSecs ?? null,
  });
}

export async function s3DeleteBucket(
  region: string,
  bucketName: string,
  endpoint?: string,
  profile?: string,
  accessKey?: string,
  secretKey?: string,
  roleArn?: string,
  externalId?: string,
  sessionName?: string,
  sessionDurationSecs?: number,
): Promise<void> {
  await invoke('s3_delete_bucket', {
    region,
    bucketName,
    endpoint: endpoint || null,
    profile: profile || null,
    accessKey: accessKey || null,
    secretKey: secretKey || null,
    roleArn: roleArn || null,
    externalId: externalId || null,
    sessionName: sessionName || null,
    sessionDurationSecs: sessionDurationSecs ?? null,
  });
}

export async function s3GetBucketVersioning(id: string): Promise<S3BucketVersioning> {
  return await invoke<S3BucketVersioning>('s3_get_bucket_versioning', { id });
}

export async function s3PutBucketVersioning(id: string, enabled: boolean): Promise<void> {
  await invoke('s3_put_bucket_versioning', { id, enabled });
}

export async function s3GetBucketEncryption(id: string): Promise<S3BucketEncryption> {
  return await invoke<S3BucketEncryption>('s3_get_bucket_encryption', { id });
}

// ── Object Metadata ─────────────────────────────────────────────────────────

export async function s3GetObjectMetadata(id: string, key: string): Promise<S3ObjectMetadata> {
  return await invoke<S3ObjectMetadata>('s3_get_object_metadata', { id, key });
}

export async function s3PutObjectMetadata(
  id: string,
  key: string,
  contentType: string | null,
  contentDisposition: string | null,
  cacheControl: string | null,
  contentEncoding: string | null,
  custom: Record<string, string>,
): Promise<void> {
  await invoke('s3_put_object_metadata', {
    id,
    key,
    contentType: contentType || null,
    contentDisposition: contentDisposition || null,
    cacheControl: cacheControl || null,
    contentEncoding: contentEncoding || null,
    custom,
  });
}

// ── Tagging ─────────────────────────────────────────────────────────────────

export async function s3GetObjectTags(id: string, key: string): Promise<S3Tag[]> {
  return await invoke<S3Tag[]>('s3_get_object_tags', { id, key });
}

export async function s3PutObjectTags(id: string, key: string, tags: S3Tag[]): Promise<void> {
  await invoke('s3_put_object_tags', { id, key, tags });
}

export async function s3GetBucketTags(id: string): Promise<S3Tag[]> {
  return await invoke<S3Tag[]>('s3_get_bucket_tags', { id });
}

export async function s3PutBucketTags(id: string, tags: S3Tag[]): Promise<void> {
  await invoke('s3_put_bucket_tags', { id, tags });
}

// ── Multipart Upload Cleanup ────────────────────────────────────────────────

export async function s3ListMultipartUploads(id: string, prefix?: string): Promise<S3MultipartUpload[]> {
  return await invoke<S3MultipartUpload[]>('s3_list_multipart_uploads', {
    id,
    prefix: prefix || null,
  });
}

export async function s3AbortMultipartUpload(id: string, key: string, uploadId: string): Promise<void> {
  await invoke('s3_abort_multipart_upload', { id, key, uploadId });
}

// ── Lifecycle Rules ──────────────────────────────────────────────────────────

export async function s3GetBucketLifecycle(id: string): Promise<S3LifecycleRule[]> {
  return await invoke<S3LifecycleRule[]>('s3_get_bucket_lifecycle', { id });
}

export async function s3PutBucketLifecycle(id: string, rules: S3LifecycleRule[]): Promise<void> {
  await invoke('s3_put_bucket_lifecycle', { id, rules });
}

// ── CORS Configuration ───────────────────────────────────────────────────────

export async function s3GetBucketCors(id: string): Promise<S3CorsRule[]> {
  return await invoke<S3CorsRule[]>('s3_get_bucket_cors', { id });
}

export async function s3PutBucketCors(id: string, rules: S3CorsRule[]): Promise<void> {
  await invoke('s3_put_bucket_cors', { id, rules });
}

// ── Bulk Storage Class Change ────────────────────────────────────────────────

export async function s3BulkChangeStorageClass(id: string, keys: string[], targetClass: string): Promise<string[]> {
  return await invoke<string[]>('s3_bulk_change_storage_class', { id, keys, targetClass });
}

// ── Public Access Block ──────────────────────────────────────────────────────

export async function s3GetPublicAccessBlock(id: string): Promise<S3PublicAccessBlock> {
  return await invoke<S3PublicAccessBlock>('s3_get_public_access_block', { id });
}

export async function s3PutPublicAccessBlock(id: string, config: S3PublicAccessBlock): Promise<void> {
  await invoke('s3_put_public_access_block', { id, config });
}

// ── Bucket Policy ────────────────────────────────────────────────────────────

export async function s3GetBucketPolicy(id: string): Promise<string> {
  return await invoke<string>('s3_get_bucket_policy', { id });
}

export async function s3PutBucketPolicy(id: string, policy: string): Promise<void> {
  await invoke('s3_put_bucket_policy', { id, policy });
}

// ── Bucket ACL (Read-Only) ───────────────────────────────────────────────────

export async function s3GetBucketAcl(id: string): Promise<S3BucketAcl> {
  return await invoke<S3BucketAcl>('s3_get_bucket_acl', { id });
}

// ── Bandwidth Throttling ────────────────────────────────────────────────────

export async function s3SetBandwidthLimit(bytesPerSec: number): Promise<void> {
  await invoke('s3_set_bandwidth_limit', { bytesPerSec });
}
