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
  | 'progress'
  | 'viewer'
  | 'editor'
  | 'menu'
  | 'volume-selector'
  | 's3-connect'
  | 's3-manager'
  | 'overwrite'
  | 'search'
  | 'preferences';

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

export interface GitRepoInfo {
  branch: string;
  ahead: number;
  behind: number;
  dirty: boolean;
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
