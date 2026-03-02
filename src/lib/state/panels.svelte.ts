import type { FileEntry, SortField, SortDirection, ViewMode, PanelBackend, S3ConnectionInfo, SftpConnectionInfo, ArchiveInfo, GitRepoInfo } from '$lib/types';
import { sortEntries } from '$lib/utils/sort';
import { listDirectory, listArchive, watchDirectory, unwatchDirectory, getGitRepoInfo, getDirectorySize } from '$lib/services/tauri';
import { s3Connect, s3Disconnect, s3ListObjects, s3IsObjectEncrypted } from '$lib/services/s3';
import { sftpConnect, sftpDisconnect, sftpListObjects } from '$lib/services/sftp';
import { appState } from '$lib/state/app.svelte';


let nextTabId = 0;

export class PanelData {
  path = $state('');
  entries = $state<FileEntry[]>([]);
  watchId: string;
  tabId: number;
  cursorIndex = $state(0);
  selectionAnchor = $state(0);
  selectedPaths = $state<Set<string>>(new Set());
  sortField = $state<SortField>(appState.sortField);
  sortDirection = $state<SortDirection>(appState.sortDirection);
  viewMode = $state<ViewMode>('list');
  gridColumns = $state(1);
  filterText = $state('');
  loading = $state(false);
  error = $state<string | null>(null);
  freeSpace = $state(0);
  backend = $state<PanelBackend>('local');
  s3Connection = $state<S3ConnectionInfo | null>(null);
  sftpConnection = $state<SftpConnectionInfo | null>(null);
  archiveInfo = $state<ArchiveInfo | null>(null);
  gitInfo = $state<GitRepoInfo | null>(null);

  sortedEntries = $derived(sortEntries(this.entries, this.sortField, this.sortDirection));

  filteredSortedEntries = $derived.by(() => {
    if (!this.filterText) return this.sortedEntries;
    const pattern = this.filterText;
    const hasGlob = pattern.includes('*') || pattern.includes('?');
    if (hasGlob) {
      const re = globToRegex(pattern);
      return this.sortedEntries.filter(
        (e) => e.name === '..' || re.test(e.name)
      );
    }
    const lower = pattern.toLowerCase();
    return this.sortedEntries.filter(
      (e) => e.name === '..' || e.name.toLowerCase().includes(lower)
    );
  });

  currentEntry = $derived(this.filteredSortedEntries[this.cursorIndex] ?? null);

  selectedCount = $derived(this.selectedPaths.size);

  selectedSize = $derived.by(() => {
    let total = 0;
    for (const entry of this.entries) {
      if (this.selectedPaths.has(entry.path)) {
        if (entry.is_dir) {
          total += this.dirSizeCache[entry.path] ?? 0;
        } else {
          total += entry.size;
        }
      }
    }
    return total;
  });

  /** Cache of computed recursive directory sizes (path → bytes). */
  dirSizeCache = $state<Record<string, number>>({});
  private dirSizePending = new Set<string>();

  /** Cache of encryption status for S3 objects (key → encrypted). */
  encryptionCache = $state<Record<string, boolean>>({});
  private encryptionPending = new Set<string>();
  private encryptionDebounceTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(side: string) {
    this.tabId = nextTabId++;
    this.watchId = `watch-${side}-${this.tabId}`;
  }

  static createTab(side: 'left' | 'right'): PanelData {
    return new PanelData(side);
  }

  /** Compute recursive sizes for any selected directories not yet cached. */
  computeSelectedDirSizes() {
    if (this.backend !== 'local' || !appState.calculateDirSizes) return;
    for (const entry of this.entries) {
      if (
        entry.is_dir &&
        entry.name !== '..' &&
        this.selectedPaths.has(entry.path) &&
        !(entry.path in this.dirSizeCache) &&
        !this.dirSizePending.has(entry.path)
      ) {
        this.dirSizePending.add(entry.path);
        getDirectorySize(entry.path).then((size) => {
          this.dirSizeCache = { ...this.dirSizeCache, [entry.path]: size };
          this.dirSizePending.delete(entry.path);
        }).catch(() => {
          this.dirSizePending.delete(entry.path);
        });
      }
    }
  }

  /** Check encryption status for an S3 object (debounced). */
  checkEncryption(key: string) {
    if (this.backend !== 's3' || !this.s3Connection) return;
    if (key in this.encryptionCache || this.encryptionPending.has(key)) return;
    if (key.endsWith('/')) return; // directories can't be encrypted

    if (this.encryptionDebounceTimer) clearTimeout(this.encryptionDebounceTimer);
    const connectionId = this.s3Connection.connectionId;
    this.encryptionDebounceTimer = setTimeout(() => {
      this.encryptionPending.add(key);
      s3IsObjectEncrypted(connectionId, key).then((encrypted) => {
        this.encryptionCache = { ...this.encryptionCache, [key]: encrypted };
        this.encryptionPending.delete(key);
      }).catch(() => {
        this.encryptionPending.delete(key);
      });
    }, 300);
  }

  clearFilter() {
    this.filterText = '';
  }

  /** Lightweight refresh for file-watcher events: only updates entries if they actually changed. */
  async refresh() {
    if (this.backend !== 'local' || !this.path) return;
    try {
      const listing = await listDirectory(this.path, appState.showHidden);
      if (entriesEqual(this.entries, listing.entries)) return;
      this.freeSpace = listing.free_space;
      this.entries = listing.entries;
      // Preserve cursor position — clamp if entries shrank
      if (this.cursorIndex >= this.filteredSortedEntries.length) {
        this.cursorIndex = Math.max(0, this.filteredSortedEntries.length - 1);
      }
      // Prune selections that no longer exist
      const validPaths = new Set(listing.entries.map(e => e.path));
      let pruned = false;
      for (const p of this.selectedPaths) {
        if (!validPaths.has(p)) { pruned = true; break; }
      }
      if (pruned) {
        this.selectedPaths = new Set([...this.selectedPaths].filter(p => validPaths.has(p)));
      }
    } catch {
      // Ignore — the directory may have been removed; a full loadDirectory will handle errors
    }
  }

  async startWatching() {
    if (this.backend !== 'local' || !this.path) return;
    try { await unwatchDirectory(this.watchId); } catch { /* ignore */ }
    try { await watchDirectory(this.path, this.watchId); } catch { /* ignore */ }
  }

  async stopWatching() {
    try { await unwatchDirectory(this.watchId); } catch { /* ignore */ }
  }

  async loadDirectory(path: string, focusName?: string) {
    this.clearFilter();
    this.dirSizeCache = {};
    this.dirSizePending.clear();
    this.encryptionCache = {};
    this.encryptionPending.clear();

    // If we're in archive mode and the ".." path is a real filesystem path (not archive://),
    // that means we're exiting the archive
    if (this.backend === 'archive' && this.archiveInfo && !path.startsWith('archive://')) {
      this.exitArchive(path, focusName);
      return;
    }

    this.loading = true;
    this.error = null;
    try {
      let listing;
      if (this.backend === 'archive' && this.archiveInfo) {
        // Parse internal path from archive://path#internal
        const internalPath = parseArchiveInternalPath(path);
        listing = await listArchive(this.archiveInfo.archivePath, internalPath);
        this.archiveInfo.internalPath = internalPath;
      } else if (this.backend === 's3' && this.s3Connection) {
        // Extract prefix from s3://bucket/prefix path
        const prefix = s3PathToPrefix(path, this.s3Connection.bucket);
        listing = await s3ListObjects(this.s3Connection.connectionId, prefix);
      } else if (this.backend === 'sftp' && this.sftpConnection) {
        listing = await sftpListObjects(this.sftpConnection.connectionId, path);
      } else {
        listing = await listDirectory(path, appState.showHidden);
      }
      this.path = listing.path;
      this.freeSpace = listing.free_space;
      // Rust backend already provides ".." entry — use entries as-is
      this.entries = listing.entries;
      this.selectedPaths = new Set();
      // Fetch git info non-blocking for local backends
      if (this.backend === 'local') {
        getGitRepoInfo(listing.path).then((info) => {
          this.gitInfo = info;
        }).catch(() => {
          this.gitInfo = null;
        });
      } else {
        this.gitInfo = null;
      }
      // Position cursor on focusName if provided (e.g. directory we just left)
      if (focusName) {
        const sorted = sortEntries(this.entries, this.sortField, this.sortDirection);
        const idx = sorted.findIndex((e) => e.name === focusName);
        this.cursorIndex = idx >= 0 ? idx : 0;
      } else {
        this.cursorIndex = 0;
      }
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading = false;
    }
    this.startWatching();
  }

  async enterArchive(archivePath: string) {
    this.backend = 'archive';
    this.archiveInfo = { archivePath, internalPath: '' };
    await this.loadDirectory(`archive://${archivePath}#`);
  }

  private async exitArchive(realPath: string, focusName?: string) {
    const archiveName = this.archiveInfo
      ? this.archiveInfo.archivePath.replace(/\/+$/, '').split('/').pop() ?? ''
      : '';
    this.backend = 'local';
    this.archiveInfo = null;
    await this.loadDirectory(realPath, focusName ?? archiveName);
  }

  async connectS3(info: S3ConnectionInfo, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, roleArn?: string, externalId?: string, sessionName?: string, sessionDurationSecs?: number, useTransferAcceleration?: boolean, anonymous?: boolean, webIdentityToken?: string, proxyUrl?: string, proxyUsername?: string, proxyPassword?: string) {
    this.loading = true;
    this.error = null;
    try {
      await s3Connect(info.connectionId, info.bucket, info.region, endpoint, profile, accessKey, secretKey, roleArn, externalId, sessionName, sessionDurationSecs, useTransferAcceleration, anonymous, webIdentityToken, proxyUrl, proxyUsername, proxyPassword);
      this.backend = 's3';
      this.s3Connection = info;
      // Load root of the bucket
      await this.loadDirectory(`s3://${info.bucket}/`);
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
      this.loading = false;
    }
  }

  async disconnectS3(homePath?: string) {
    if (this.s3Connection) {
      try {
        await s3Disconnect(this.s3Connection.connectionId);
      } catch {
        // Ignore disconnect errors
      }
    }
    this.backend = 'local';
    this.s3Connection = null;
    // Navigate back to home directory
    await this.loadDirectory(homePath || '/');
  }

  async connectSftp(info: SftpConnectionInfo, password?: string, keyPath?: string, keyPassphrase?: string) {
    this.loading = true;
    this.error = null;
    try {
      const homeDir = await sftpConnect(info.connectionId, info.host, info.port, info.username, password ? 'password' : keyPath ? 'key' : 'agent', password, keyPath, keyPassphrase);
      this.backend = 'sftp';
      this.sftpConnection = info;
      await this.loadDirectory(`sftp://${info.host}:${info.port}${homeDir.startsWith('/') ? '' : '/'}${homeDir}/`);
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
      this.loading = false;
    }
  }

  async disconnectSftp(homePath?: string) {
    if (this.sftpConnection) {
      try {
        await sftpDisconnect(this.sftpConnection.connectionId);
      } catch {
        // Ignore disconnect errors
      }
    }
    this.backend = 'local';
    this.sftpConnection = null;
    await this.loadDirectory(homePath || '/');
  }

  moveCursor(delta: number) {
    const len = this.filteredSortedEntries.length;
    if (len === 0) return;
    let next = this.cursorIndex + delta;
    if (next < 0) next = 0;
    if (next >= len) next = len - 1;
    this.cursorIndex = next;
  }

  moveCursorTo(index: number) {
    const len = this.filteredSortedEntries.length;
    if (len === 0) return;
    if (index < 0) index = 0;
    if (index >= len) index = len - 1;
    this.cursorIndex = index;
    this.selectionAnchor = index;
  }

  toggleSelection(path: string) {
    const next = new Set(this.selectedPaths);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    this.selectedPaths = next;
  }

  selectAll() {
    const next = new Set<string>();
    for (const entry of this.entries) {
      if (entry.name !== '..') {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  deselectAll() {
    this.selectedPaths = new Set();
  }

  invertSelection() {
    const next = new Set<string>();
    for (const entry of this.entries) {
      if (entry.name !== '..' && !this.selectedPaths.has(entry.path)) {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  selectRange(from: number, to: number) {
    const next = new Set<string>();
    const start = Math.min(from, to);
    const end = Math.max(from, to);
    for (let i = start; i <= end; i++) {
      const entry = this.filteredSortedEntries[i];
      if (entry && entry.name !== '..') {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  toggleSort(field: SortField) {
    if (this.sortField === field) {
      this.sortDirection = this.sortDirection === 'asc' ? 'desc' : 'asc';
    } else {
      this.sortField = field;
      this.sortDirection = 'asc';
    }
    appState.sortField = this.sortField;
    appState.sortDirection = this.sortDirection;
    appState.persistConfig();
  }

  toggleViewMode() {
    this.viewMode = this.viewMode === 'list' ? 'icon' : this.viewMode === 'icon' ? 'column' : 'list';
  }

  getSelectedOrCurrent(): string[] {
    if (this.selectedPaths.size > 0) {
      return Array.from(this.selectedPaths);
    }
    const current = this.currentEntry;
    if (current && current.name !== '..') {
      return [current.path];
    }
    return [];
  }
}

function parentPath(p: string): string {
  // Remove trailing slash
  let clean = p.replace(/\/+$/, '');
  const lastSlash = clean.lastIndexOf('/');
  if (lastSlash <= 0) return '/';
  return clean.substring(0, lastSlash);
}

/** Extract the internal path from an archive://path#internal URL. */
function parseArchiveInternalPath(path: string): string {
  const hashIdx = path.indexOf('#');
  if (hashIdx === -1) return '';
  return path.substring(hashIdx + 1);
}

/** Extract the S3 key/prefix from an s3://bucket/key path. */
export function s3PathToPrefix(path: string, bucket: string): string {
  const prefix = `s3://${bucket}/`;
  if (path.startsWith(prefix)) {
    return path.substring(prefix.length);
  }
  return path;
}

/** Convert a glob pattern (with * and ?) to a case-insensitive RegExp. */
function globToRegex(pattern: string): RegExp {
  const escaped = pattern.replace(/([.+^${}()|[\]\\])/g, '\\$1');
  const re = escaped.replace(/\*/g, '.*').replace(/\?/g, '.');
  return new RegExp(`^${re}$`, 'i');
}

/** Fast shallow comparison of two FileEntry arrays. */
function entriesEqual(a: FileEntry[], b: FileEntry[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i], y = b[i];
    if (
      x.name !== y.name ||
      x.path !== y.path ||
      x.size !== y.size ||
      x.is_dir !== y.is_dir ||
      x.modified !== y.modified ||
      x.permissions !== y.permissions ||
      x.git_status !== y.git_status
    ) return false;
  }
  return true;
}

class PanelsState {
  leftTabs = $state<PanelData[]>([PanelData.createTab('left')]);
  rightTabs = $state<PanelData[]>([PanelData.createTab('right')]);
  leftActiveTab = $state(0);
  rightActiveTab = $state(0);
  activePanel = $state<'left' | 'right'>('left');

  // Backwards-compatible getters — return active tab's PanelData
  get left(): PanelData {
    return this.leftTabs[this.leftActiveTab];
  }

  get right(): PanelData {
    return this.rightTabs[this.rightActiveTab];
  }

  active = $derived(this.activePanel === 'left' ? this.leftTabs[this.leftActiveTab] : this.rightTabs[this.rightActiveTab]);
  inactive = $derived(this.activePanel === 'left' ? this.rightTabs[this.rightActiveTab] : this.leftTabs[this.leftActiveTab]);

  switchPanel() {
    this.activePanel = this.activePanel === 'left' ? 'right' : 'left';
  }

  addTab(side: 'left' | 'right'): PanelData {
    const tab = PanelData.createTab(side);
    if (side === 'left') {
      this.leftTabs = [...this.leftTabs, tab];
      this.leftActiveTab = this.leftTabs.length - 1;
    } else {
      this.rightTabs = [...this.rightTabs, tab];
      this.rightActiveTab = this.rightTabs.length - 1;
    }
    return tab;
  }

  closeTab(side: 'left' | 'right', index: number) {
    const tabs = side === 'left' ? this.leftTabs : this.rightTabs;
    if (tabs.length <= 1) return; // Can't close last tab
    const tab = tabs[index];
    tab.stopWatching();
    const next = tabs.filter((_, i) => i !== index);
    if (side === 'left') {
      this.leftTabs = next;
      if (this.leftActiveTab >= next.length) this.leftActiveTab = next.length - 1;
    } else {
      this.rightTabs = next;
      if (this.rightActiveTab >= next.length) this.rightActiveTab = next.length - 1;
    }
  }

  switchTab(side: 'left' | 'right', index: number) {
    if (side === 'left') {
      this.leftActiveTab = index;
    } else {
      this.rightActiveTab = index;
    }
  }

  swapPanels() {
    const tmpTabs = this.leftTabs;
    const tmpActive = this.leftActiveTab;
    this.leftTabs = this.rightTabs;
    this.leftActiveTab = this.rightActiveTab;
    this.rightTabs = tmpTabs;
    this.rightActiveTab = tmpActive;
    // Fix watch IDs
    for (const tab of this.leftTabs) tab.watchId = `watch-left-${tab.tabId}`;
    for (const tab of this.rightTabs) tab.watchId = `watch-right-${tab.tabId}`;
  }
}

export const panels = new PanelsState();
