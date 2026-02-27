export interface FileEntry {
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  is_symlink: boolean;
  symlink_target: string | null;
  modified: number; // epoch ms
  permissions: number; // unix mode
  owner: string;
  group: string;
  extension: string | null;
  git_status: string | null;
  storage_class: string | null;
}

export interface DirListing {
  path: string;
  entries: FileEntry[];
  total_size: number;
  free_space: number;
}

export interface VolumeInfo {
  name: string;
  mount_point: string;
  total_space: number;
  free_space: number;
  fs_type: string;
}

export interface ProgressEvent {
  id: string;
  bytes_done: number;
  bytes_total: number;
  current_file: string;
  files_done: number;
  files_total: number;
}

export type TerminalDisplayMode = 'none' | 'bottom' | 'in-pane' | 'quake';

export interface TerminalOutput {
  id: string;
  data: string;
}

export interface TerminalExit {
  id: string;
  code: number | null;
}

export type SortField = 'name' | 'size' | 'modified' | 'extension' | 'storage_class';
export type SortDirection = 'asc' | 'desc';
export type ViewMode = 'list' | 'icon';
export type ViewerMode = 'text' | 'image' | 'hex';
export type ModalType =
  | 'none'
  | 'confirm'
  | 'input'
  | 'viewer'
  | 'editor'
  | 'menu'
  | 'volume-selector'
  | 's3-manager'
  | 'overwrite'
  | 'search'
  | 'sync'
  | 'preferences'
  | 'properties'
  | 'batch-edit'
  | 'shortcuts';

export type SearchMode = 'name' | 'content';

export interface SearchResult {
  type: 'Result';
  path: string;
  name: string;
  size: number;
  is_dir: boolean;
  line_number: number | null;
  snippet: string | null;
}

export interface SearchDone {
  type: 'Done';
  total_found: number;
  cancelled: boolean;
}

export type SearchEvent = SearchResult | SearchDone;

export interface SyncEntry {
  relative_path: string;
  status: 'new' | 'modified' | 'deleted' | 'same';
  source_size: number;
  dest_size: number;
  source_modified: number;
  dest_modified: number;
  source_etag: string;
  dest_etag: string;
}

export type SyncEvent =
  | ({ type: 'Entry' } & SyncEntry)
  | { type: 'Progress'; scanned: number }
  | { type: 'Done'; total: number; new_count: number; modified: number; deleted: number };

export interface GitRepoInfo {
  branch: string;
  ahead: number;
  behind: number;
  dirty: boolean;
}

export interface FileProperties {
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  is_symlink: boolean;
  symlink_target: string | null;
  created: number; // epoch ms
  modified: number; // epoch ms
  accessed: number; // epoch ms
  permissions: number; // unix mode
  owner: string;
  group: string;
  kind: string; // "File", "Directory", or "Symlink"
}

export interface S3ObjectProperties {
  key: string;
  size: number;
  modified: number; // epoch ms
  content_type: string | null;
  etag: string | null;
  storage_class: string | null;
  restore_status: string | null;
  version_id: string | null;
}

export interface S3ObjectVersion {
  version_id: string;
  is_latest: boolean;
  is_delete_marker: boolean;
  size: number;
  modified: number; // epoch ms
  etag: string | null;
  storage_class: string | null;
}

export interface S3Bucket {
  name: string;
  created: number; // epoch ms
}

export interface S3BucketVersioning {
  status: string;
  mfa_delete: string;
}

export interface S3EncryptionRule {
  sse_algorithm: string;
  kms_key_id: string | null;
  bucket_key_enabled: boolean;
}

export interface S3BucketEncryption {
  rules: S3EncryptionRule[];
}

export interface S3ObjectMetadata {
  content_type: string | null;
  content_disposition: string | null;
  cache_control: string | null;
  content_encoding: string | null;
  custom: Record<string, string>;
}

export interface S3Tag {
  key: string;
  value: string;
}

export interface S3MultipartUpload {
  key: string;
  upload_id: string;
  initiated: number;
}

export interface S3LifecycleTransition {
  days: number;
  storage_class: string;
}

export interface S3LifecycleRule {
  id: string;
  prefix: string;
  enabled: boolean;
  transitions: S3LifecycleTransition[];
  expiration_days: number | null;
  noncurrent_transitions: S3LifecycleTransition[];
  noncurrent_expiration_days: number | null;
  abort_incomplete_days: number | null;
}

export type TransferCheckpoint = {
  files_completed: string[];
  bytes_done: number;
  bytes_total: number;
  files_done: number;
  files_total: number;
};

export type S3UploadCheckpoint = {
  files_completed: string[];
  current_file_upload_id: string | null;
  current_file_key: string | null;
  completed_parts: { part_number: number; etag: string }[];
  bytes_done: number;
  bytes_total: number;
  files_done: number;
  files_total: number;
};

export interface S3CorsRule {
  allowed_origins: string[];
  allowed_methods: string[];
  allowed_headers: string[];
  expose_headers: string[];
  max_age_seconds: number | null;
}

export interface S3PublicAccessBlock {
  block_public_acls: boolean;
  ignore_public_acls: boolean;
  block_public_policy: boolean;
  restrict_public_buckets: boolean;
}

export interface S3AclGrant {
  grantee_type: string;
  grantee_id: string | null;
  grantee_uri: string | null;
  grantee_email: string | null;
  grantee_display_name: string | null;
  permission: string;
}

export interface S3BucketAcl {
  owner_id: string;
  owner_display_name: string | null;
  grants: S3AclGrant[];
}

export interface S3BucketWebsite {
  enabled: boolean;
  index_document: string;
  error_document: string | null;
}

export interface S3BucketLogging {
  enabled: boolean;
  target_bucket: string | null;
  target_prefix: string | null;
}

export interface S3BucketOwnership {
  object_ownership: string;
}

export interface S3ObjectLockConfig {
  enabled: boolean;
  default_retention_mode: string | null;  // "GOVERNANCE" | "COMPLIANCE"
  default_retention_days: number | null;
  default_retention_years: number | null;
}

export interface S3ObjectRetention {
  mode: string | null;              // "GOVERNANCE" | "COMPLIANCE"
  retain_until_date: string | null; // ISO-8601
}

export interface S3ObjectLegalHold {
  status: string; // "ON" | "OFF"
}

export interface KmsKeyInfo {
  key_id: string;
  arn: string;
  alias: string | null;
}

// ── S3 Inventory Types ──────────────────────────────────────────────────────

export interface S3InventoryDestination {
  bucket_arn: string;
  prefix: string | null;
  format: string;
  account_id: string | null;
}

export interface S3InventoryConfiguration {
  id: string;
  enabled: boolean;
  destination: S3InventoryDestination;
  schedule: string;
  included_object_versions: string;
  optional_fields: string[];
  filter_prefix: string | null;
}

// ── S3 Replication Types ─────────────────────────────────────────────────────

export interface S3ReplicationDestination {
  bucket_arn: string;
  storage_class: string | null;
  account: string | null;
  kms_key_id: string | null;
}

export interface S3ReplicationRule {
  id: string | null;
  priority: number | null;
  status: string;
  filter_prefix: string | null;
  destination: S3ReplicationDestination;
  delete_marker_replication: boolean;
}

export interface S3ReplicationConfiguration {
  role: string;
  rules: S3ReplicationRule[];
}

// ── S3 Event Notification Types ──────────────────────────────────────────────

export interface S3NotificationRule {
  id: string | null;
  destination_type: string;
  destination_arn: string;
  events: string[];
  filter_prefix: string | null;
  filter_suffix: string | null;
}

export interface S3NotificationConfiguration {
  rules: S3NotificationRule[];
  event_bridge_enabled: boolean;
}

// ── S3 Access Point Types ────────────────────────────────────────────────────

export interface S3AccessPoint {
  name: string;
  access_point_arn: string;
  alias: string;
  bucket: string;
  network_origin: string; // "Internet" | "VPC"
  vpc_id: string | null;
}

export interface S3AccessPointDetail {
  name: string;
  access_point_arn: string;
  alias: string;
  bucket: string;
  network_origin: string;
  vpc_id: string | null;
  public_access_block: S3PublicAccessBlock | null;
  creation_date: string | null;
  endpoints: Record<string, string>;
}

// ── CloudFront Types ────────────────────────────────────────────────────────

export interface CfDistributionSummary {
  id: string;
  domain_name: string;
  status: string;
  enabled: boolean;
  comment: string;
  last_modified: string;
  price_class: string;
  http_version: string;
  aliases: string[];
}

export interface CfCustomErrorResponse {
  error_code: number;
  response_page_path: string | null;
  response_code: string | null;
  error_caching_min_ttl: number | null;
}

export interface CfDistributionConfig {
  comment: string;
  enabled: boolean;
  default_root_object: string;
  price_class: string;
  http_version: string;
  viewer_protocol_policy: string;
  aliases: string[];
  custom_error_responses: CfCustomErrorResponse[];
}

export interface CfDistribution {
  id: string;
  domain_name: string;
  status: string;
  etag: string;
  config: CfDistributionConfig;
}

export interface CfInvalidation {
  id: string;
  status: string;
  create_time: string;
  paths: string[];
}

export type PanelBackend = 'local' | 's3' | 'archive';

export interface ArchiveInfo {
  archivePath: string;
  internalPath: string;
}

export interface S3ProviderCapabilities {
  versioning: boolean;
  lifecycleRules: boolean;
  cors: boolean;
  bucketPolicy: boolean;
  acl: boolean;
  publicAccessBlock: boolean;
  encryption: boolean;
  storageClasses: string[];
  glacierRestore: boolean;
  presignedUrls: boolean;
  objectMetadata: boolean;
  objectTags: boolean;
  bucketTags: boolean;
  multipartUploadCleanup: boolean;
  websiteHosting: boolean;
  requesterPays: boolean;
  objectOwnership: boolean;
  serverAccessLogging: boolean;
  objectLock: boolean;
  listBuckets: boolean;
  cloudfront: boolean;
  inventory: boolean;
  replication: boolean;
  eventNotifications: boolean;
  accessPoints: boolean;
}

export interface S3ConnectionInfo {
  bucket: string;
  region: string;
  endpoint?: string;
  profile?: string;
  connectionId: string;
  provider?: string;
  capabilities?: S3ProviderCapabilities;
}

export interface S3Bookmark {
  id: string;
  name: string;
  profileId: string;
  path: string; // e.g. "s3://mybucket/data/reports/"
}

export interface S3Profile {
  id: string;
  name: string;
  bucket: string;
  region: string;
  endpoint?: string;
  profile?: string;
  credentialType: 'keychain' | 'aws-profile' | 'default' | 'anonymous' | 'oidc';
  accessKeyId?: string;
  provider?: string;
  customCapabilities?: S3ProviderCapabilities;
  roleArn?: string;
  externalId?: string;
  sessionName?: string;
  sessionDurationSecs?: number;
  useTransferAcceleration?: boolean;
  oidcIssuerUrl?: string;
  oidcClientId?: string;
  oidcScopes?: string;
  defaultClientEncryption?: boolean;
  encryptionCipher?: 'aes-256-gcm' | 'chacha20-poly1305';
  kdfMemoryCost?: number;     // KiB (default: 19456)
  kdfTimeCost?: number;       // iterations (default: 2)
  kdfParallelism?: number;    // threads (default: 1)
  autoEncryptMinSize?: number; // bytes; skip encryption if all files < this (0 = always)
  autoEncryptExtensions?: string[]; // only encrypt files with these extensions (empty = all)
  proxyUrl?: string;       // "http://proxy:8080" or "system" for env auto-detect
  proxyUsername?: string;
  // proxyPassword stored in keychain as "furman-s3-proxy-{profile.id}"
}
