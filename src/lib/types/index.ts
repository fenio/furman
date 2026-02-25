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

export type SortField = 'name' | 'size' | 'modified' | 'extension';
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
  | 's3-connect'
  | 's3-manager'
  | 'overwrite'
  | 'search'
  | 'sync'
  | 'preferences'
  | 'properties';

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

export type PanelBackend = 'local' | 's3' | 'archive';

export interface ArchiveInfo {
  archivePath: string;
  internalPath: string;
}

export interface S3ConnectionInfo {
  bucket: string;
  region: string;
  endpoint?: string;
  profile?: string;
  connectionId: string;
}

export interface S3Profile {
  id: string;
  name: string;
  bucket: string;
  region: string;
  endpoint?: string;
  profile?: string;
  credentialType: 'keychain' | 'aws-profile' | 'default';
  accessKeyId?: string;
}
