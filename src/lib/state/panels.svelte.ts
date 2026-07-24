import type { FileEntry, SortField, SortDirection, ViewMode, PanelBackend, S3ConnectionInfo, SftpConnectionInfo, ArchiveInfo, GitRepoInfo, DirListEvent } from '$lib/types';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { sortEntries } from '$lib/utils/sort';
import { listDirectory, listDirectoryStreamed, listArchive, watchDirectory, unwatchDirectory, getGitRepoInfo, getDirectorySize, cleanupTempPath } from '$lib/services/tauri';
import { s3Connect, s3Disconnect, s3ListObjects, s3IsObjectEncrypted } from '$lib/services/s3';
import { sftpConnect, sftpDisconnect, sftpListObjects } from '$lib/services/sftp';
import { mountNetworkShare } from '$lib/services/mount';
import { appState } from '$lib/state/app.svelte';
import { comparisonState, type ComparisonSide } from '$lib/state/comparison.svelte';

/// Threshold above which we use streamed directory listing.
const STREAM_THRESHOLD = 50_000;
/// Debounce delay (ms) for filter text in large directories.
const FILTER_DEBOUNCE_MS = 150;


let nextTabId = 0;

export class PanelData {
  path = $state('');
  entries = $state<FileEntry[]>([]);
  watchId: string;
  tabId: number;
  side: ComparisonSide;
  cursorIndex = $state(0);
  selectionAnchor = $state(0);
  selectedPaths = $state<Set<string>>(new SvelteSet());
  sortField = $state<SortField>(appState.sortField);
  sortDirection = $state<SortDirection>(appState.sortDirection);
  viewMode = $state<ViewMode>(appState.defaultViewMode);
  gridColumns = $state(1);
  filterText = $state('');
  /** Raw filter input — immediately updated by the UI input binding. */
  filterInput = $state('');
  /** True once a streamed listing has finished (entries are sorted). */
  streamComplete = $state(true);
  private filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  loading = $state(false);
  error = $state<string | null>(null);
  freeSpace = $state(0);
  backend = $state<PanelBackend>('local');
  s3Connection = $state<S3ConnectionInfo | null>(null);
  sftpConnection = $state<SftpConnectionInfo | null>(null);
  archiveInfo = $state<ArchiveInfo | null>(null);
  gitInfo = $state<GitRepoInfo | null>(null);

  // Directory history (browser-like back/forward)
  history = $state<string[]>([]);
  historyIndex = $state(-1);
  private navigatingHistory = false;

  canGoBack = $derived(this.historyIndex > 0);
  canGoForward = $derived(this.historyIndex < this.history.length - 1);

  sortedEntries = $derived(
    this.streamComplete
      ? sortEntries(this.entries, this.sortField, this.sortDirection)
      : this.entries
  );

  private cachedGlobPattern = '';
  private cachedGlobRegex: RegExp | null = null;

  filteredSortedEntries = $derived.by(() => {
    let result = this.sortedEntries;
    if (this.filterText) {
      const pattern = this.filterText;
      const hasGlob = pattern.includes('*') || pattern.includes('?');
      if (hasGlob) {
        if (pattern !== this.cachedGlobPattern) {
          this.cachedGlobPattern = pattern;
          this.cachedGlobRegex = globToRegex(pattern);
        }
        const re = this.cachedGlobRegex!;
        result = result.filter((e) => e.name === '..' || re.test(e.name));
      } else {
        const lower = pattern.toLowerCase();
        result = result.filter(
          (e) => e.name === '..' || e.name.toLowerCase().includes(lower)
        );
      }
    }
    if (comparisonState.active && comparisonState.filterFor(this.side) !== 'all') {
      result = result.filter((entry) => comparisonState.matchesFilter(this.side, this.path, entry));
    }
    return result;
  });

  currentEntry = $derived(this.filteredSortedEntries[this.cursorIndex] ?? null);

  selectedCount = $derived(this.selectedPaths.size);

  // Path→entry index for O(1) lookups (rebuilt when entries change)
  private entryByPath = $derived.by(() => {
    const map = new SvelteMap<string, FileEntry>();
    for (const e of this.entries) map.set(e.path, e);
    return map;
  });

  selectedSize = $derived.by(() => {
    let total = 0;
    for (const path of this.selectedPaths) {
      const entry = this.entryByPath.get(path);
      if (!entry) continue;
      if (entry.is_dir) {
        total += this.dirSizeCache[entry.path] ?? 0;
      } else {
        total += entry.size;
      }
    }
    return total;
  });

  /** Cache of computed recursive directory sizes (path → bytes). */
  dirSizeCache = $state<Record<string, number>>({});
  private dirSizePending = new SvelteSet<string>();

  /** Cache of encryption status for S3 objects (key → encrypted). */
  encryptionCache = $state<Record<string, boolean>>({});
  private encryptionPending = new SvelteSet<string>();
  private encryptionDebounceTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(side: ComparisonSide) {
    this.side = side;
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
          this.dirSizeCache[entry.path] = size;
          this.dirSizePending.delete(entry.path);
        }).catch(() => {
          this.dirSizePending.delete(entry.path);
        });
      }
    }
  }

  private static readonly MAX_ENCRYPTION_CONCURRENT = 5;

  /** Check encryption status for an S3 object (debounced, concurrency-limited). */
  checkEncryption(key: string) {
    if (this.backend !== 's3' || !this.s3Connection) return;
    if (key in this.encryptionCache || this.encryptionPending.has(key)) return;
    if (key.endsWith('/')) return; // directories can't be encrypted
    if (this.encryptionPending.size >= PanelData.MAX_ENCRYPTION_CONCURRENT) return;

    if (this.encryptionDebounceTimer) clearTimeout(this.encryptionDebounceTimer);
    const connectionId = this.s3Connection.connectionId;
    this.encryptionDebounceTimer = setTimeout(() => {
      if (this.encryptionPending.size >= PanelData.MAX_ENCRYPTION_CONCURRENT) return;
      this.encryptionPending.add(key);
      s3IsObjectEncrypted(connectionId, key).then((encrypted) => {
        this.encryptionCache[key] = encrypted;
        this.encryptionPending.delete(key);
      }).catch(() => {
        this.encryptionPending.delete(key);
      });
    }, 300);
  }

  clearFilter() {
    this.filterText = '';
    this.filterInput = '';
    if (this.filterDebounceTimer) {
      clearTimeout(this.filterDebounceTimer);
      this.filterDebounceTimer = null;
    }
  }

  /** Update filter text from raw input, with debouncing for large directories. */
  setFilterInput(value: string) {
    this.filterInput = value;
    if (this.entries.length > STREAM_THRESHOLD) {
      if (this.filterDebounceTimer) clearTimeout(this.filterDebounceTimer);
      this.filterDebounceTimer = setTimeout(() => {
        this.filterText = this.filterInput;
        this.filterDebounceTimer = null;
      }, FILTER_DEBOUNCE_MS);
    } else {
      this.filterText = value;
    }
  }

  /** Lightweight refresh for file-watcher events: only updates entries if they actually changed. */
  async refresh() {
    if (this.backend !== 'local' || !this.path) return;
    try {
      const listing = await listDirectory(this.path, appState.showHidden);
      if (entriesEqual(this.entries, listing.entries)) return;
      if (comparisonState.active) comparisonState.stopComparison();
      this.freeSpace = listing.free_space;
      // Preserve cursor by name across entry changes
      const cursorName = this.filteredSortedEntries[this.cursorIndex]?.name;
      this.entries = listing.entries;
      if (cursorName) {
        const idx = this.filteredSortedEntries.findIndex((e) => e.name === cursorName);
        const newIdx = idx >= 0 ? idx : Math.max(0, this.filteredSortedEntries.length - 1);
        this.cursorIndex = newIdx;
      } else if (this.cursorIndex >= this.filteredSortedEntries.length) {
        this.cursorIndex = Math.max(0, this.filteredSortedEntries.length - 1);
      }
      // Prune selections that no longer exist
      const validPaths = new SvelteSet(listing.entries.map(e => e.path));
      let pruned = false;
      for (const p of this.selectedPaths) {
        if (!validPaths.has(p)) { pruned = true; break; }
      }
      if (pruned) {
        this.selectedPaths = new SvelteSet([...this.selectedPaths].filter(p => validPaths.has(p)));
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
    this.streamComplete = true;

    // If we're in archive mode and the ".." path is a real filesystem path (not archive://),
    // that means we're exiting the archive
    if (this.backend === 'archive' && this.archiveInfo && !path.startsWith('archive://')) {
      this.exitArchive(path, focusName);
      return;
    }

    if (comparisonState.active) {
      if (path === this.path || !comparisonState.containsPanelPath(this.side, path)) {
        comparisonState.stopComparison();
      }
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
        // Use streamed listing for local directories
        await this.loadDirectoryStreamed(path, focusName);
        return;
      }
      this.path = listing.path;
      this.freeSpace = listing.free_space;
      // Rust backend already provides ".." entry — use entries as-is
      this.entries = listing.entries;
      this.selectedPaths = new SvelteSet();
      this.gitInfo = null;
      // Position cursor on focusName if provided (e.g. directory we just left)
      if (focusName) {
        const idx = this.filteredSortedEntries.findIndex((e) => e.name === focusName);
        this.cursorIndex = idx >= 0 ? idx : 0;
      } else {
        this.cursorIndex = 0;
      }
      this.selectionAnchor = this.cursorIndex;
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading = false;
    }
    // Track history
    if (!this.navigatingHistory && !this.error) {
      this.history = [...this.history.slice(0, this.historyIndex + 1), this.path];
      this.historyIndex = this.history.length - 1;
    }
    this.navigatingHistory = false;
    this.startWatching();
  }

  /** Load a local directory using streamed listing for progressive rendering. */
  private async loadDirectoryStreamed(path: string, focusName?: string) {
    this.streamComplete = false;
    this.entries = [];
    this.selectedPaths = new SvelteSet();
    this.cursorIndex = 0;
    this.selectionAnchor = 0;

    try {
      await listDirectoryStreamed(path, appState.showHidden, (event: DirListEvent) => {
        if (event.type === 'Batch') {
          // Append entries progressively
          this.entries = [...this.entries, ...event.entries];
          // Clear loading after first batch arrives
          if (this.loading) {
            this.loading = false;
          }
        } else if (event.type === 'Done') {
          this.path = event.path;
          this.freeSpace = event.free_space;

          // Apply git statuses if available
          if (event.git_statuses) {
            const statuses = event.git_statuses;
            this.entries = this.entries.map(e => {
              const status = statuses[e.name];
              if (status) {
                return { ...e, git_status: status };
              }
              return e;
            });
          }

          // Mark stream as complete — triggers sorting in $derived
          this.streamComplete = true;

          // Position cursor on focusName if provided
          if (focusName) {
            const idx = this.filteredSortedEntries.findIndex((e) => e.name === focusName);
            this.cursorIndex = idx >= 0 ? idx : 0;
          } else {
            this.cursorIndex = 0;
          }
          this.selectionAnchor = this.cursorIndex;

          // Fetch git info non-blocking
          getGitRepoInfo(event.path).then((info) => {
            this.gitInfo = info;
          }).catch(() => {
            this.gitInfo = null;
          });
        }
      });
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
      this.loading = false;
      this.streamComplete = true;
      return;
    }

    // Track history
    if (!this.navigatingHistory && !this.error) {
      this.history = [...this.history.slice(0, this.historyIndex + 1), this.path];
      this.historyIndex = this.history.length - 1;
    }
    this.navigatingHistory = false;
    this.startWatching();
  }

  goBack() {
    if (this.historyIndex <= 0) return;
    this.historyIndex--;
    this.navigatingHistory = true;
    const target = this.history[this.historyIndex];
    this.loadDirectory(target);
  }

  goForward() {
    if (this.historyIndex >= this.history.length - 1) return;
    this.historyIndex++;
    this.navigatingHistory = true;
    const target = this.history[this.historyIndex];
    this.loadDirectory(target);
  }

  private clearHistory() {
    this.history = [];
    this.historyIndex = -1;
    this.navigatingHistory = false;
  }

  async enterArchive(archivePath: string, remoteOrigin?: ArchiveInfo['remoteOrigin']) {
    if (comparisonState.active) comparisonState.stopComparison();
    this.releaseTemporaryArchive();
    this.clearHistory();
    this.backend = 'archive';
    this.archiveInfo = {
      archivePath,
      internalPath: '',
      remoteOrigin,
      temporaryPath: remoteOrigin ? archivePath : undefined,
    };
    await this.loadDirectory(`archive://${archivePath}#`);
  }

  private async exitArchive(realPath: string, focusName?: string) {
    this.clearHistory();
    const origin = this.archiveInfo?.remoteOrigin;
    const temporaryPath = this.archiveInfo?.temporaryPath;
    const archiveName = this.archiveInfo
      ? this.archiveInfo.archivePath.replace(/\/+$/, '').split('/').pop() ?? ''
      : '';
    this.archiveInfo = null;
    if (temporaryPath) cleanupTempPath(temporaryPath).catch(() => {});

    if (origin) {
      // Restore remote backend connection
      this.backend = origin.backend;
      this.s3Connection = origin.s3Connection ?? null;
      this.sftpConnection = origin.sftpConnection ?? null;
      await this.loadDirectory(origin.path, origin.remoteName);
    } else {
      this.backend = 'local';
      await this.loadDirectory(realPath, focusName ?? archiveName);
    }
  }

  private releaseTemporaryArchive() {
    const temporaryPath = this.archiveInfo?.temporaryPath;
    this.archiveInfo = null;
    if (temporaryPath) cleanupTempPath(temporaryPath).catch(() => {});
  }

  async connectS3(info: S3ConnectionInfo, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, roleArn?: string, externalId?: string, sessionName?: string, sessionDurationSecs?: number, useTransferAcceleration?: boolean, anonymous?: boolean, webIdentityToken?: string, proxyUrl?: string, proxyUsername?: string, proxyPassword?: string) {
    if (comparisonState.active) comparisonState.stopComparison();
    this.clearHistory();
    this.loading = true;
    this.error = null;
    try {
      await s3Connect(info.connectionId, info.bucket, info.region, endpoint, profile, accessKey, secretKey, roleArn, externalId, sessionName, sessionDurationSecs, useTransferAcceleration, anonymous, webIdentityToken, proxyUrl, proxyUsername, proxyPassword);
      this.releaseTemporaryArchive();
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
    if (comparisonState.active) comparisonState.stopComparison();
    this.releaseTemporaryArchive();
    this.clearHistory();
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

  async connectSftp(info: SftpConnectionInfo, password?: string, keyPath?: string, keyPassphrase?: string, agentSocket?: string) {
    if (comparisonState.active) comparisonState.stopComparison();
    this.clearHistory();
    this.loading = true;
    this.error = null;
    try {
      const homeDir = await sftpConnect(info.connectionId, info.host, info.port, info.username, password ? 'password' : keyPath ? 'key' : 'agent', password, keyPath, keyPassphrase, agentSocket, appState.sftpInactivityTimeout, appState.sftpKeepaliveInterval, appState.sftpOperationTimeout);
      this.releaseTemporaryArchive();
      this.backend = 'sftp';
      this.sftpConnection = info;
      await this.loadDirectory(`sftp://${info.host}:${info.port}${homeDir.startsWith('/') ? '' : '/'}${homeDir}/`);
    } catch (err: unknown) {
      this.error = err instanceof Error ? err.message : String(err);
      this.loading = false;
    }
  }

  /// Mount an SMB/NFS share through the OS and navigate this panel to it as a
  /// local folder. Throws on failure so the caller can surface the error.
  async mountShare(protocol: 'smb' | 'nfs', host: string, share: string, username?: string, password?: string, domain?: string) {
    if (comparisonState.active) comparisonState.stopComparison();
    this.loading = true;
    this.error = null;
    try {
      const mountPoint = await mountNetworkShare(protocol, host, share, username, password, domain);
      this.releaseTemporaryArchive();
      this.clearHistory();
      this.backend = 'local';
      await this.loadDirectory(mountPoint);
    } catch (err: unknown) {
      this.loading = false;
      throw err;
    }
  }

  async disconnectSftp(homePath?: string) {
    if (comparisonState.active) comparisonState.stopComparison();
    this.releaseTemporaryArchive();
    this.clearHistory();
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
    const next = new SvelteSet(this.selectedPaths);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    this.selectedPaths = next;
  }

  selectAll() {
    const next = new SvelteSet<string>();
    for (const entry of this.filteredSortedEntries) {
      if (entry.name !== '..') {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  deselectAll() {
    this.selectedPaths = new SvelteSet();
  }

  invertSelection() {
    const next = new SvelteSet<string>();
    for (const entry of this.filteredSortedEntries) {
      if (entry.name !== '..' && !this.selectedPaths.has(entry.path)) {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  selectByPattern(pattern: string) {
    const re = globToRegex(pattern);
    const next = new SvelteSet(this.selectedPaths);
    for (const entry of this.filteredSortedEntries) {
      if (entry.name !== '..' && re.test(entry.name)) {
        next.add(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  deselectByPattern(pattern: string) {
    const re = globToRegex(pattern);
    const next = new SvelteSet(this.selectedPaths);
    for (const entry of this.filteredSortedEntries) {
      if (re.test(entry.name)) {
        next.delete(entry.path);
      }
    }
    this.selectedPaths = next;
  }

  selectRange(from: number, to: number) {
    const next = new SvelteSet<string>();
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

function _parentPath(p: string): string {
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
    if (comparisonState.active) comparisonState.stopComparison();
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
    const activeIndex = side === 'left' ? this.leftActiveTab : this.rightActiveTab;
    if (comparisonState.active && index === activeIndex) comparisonState.stopComparison();
    const tab = tabs[index];
    tab.stopWatching();
    if (tab.archiveInfo?.temporaryPath) {
      cleanupTempPath(tab.archiveInfo.temporaryPath).catch(() => {});
    }
    const next = tabs.filter((_, i) => i !== index);
    if (side === 'left') {
      this.leftTabs = next;
      if (index < this.leftActiveTab) this.leftActiveTab--;
      else if (this.leftActiveTab >= next.length) this.leftActiveTab = next.length - 1;
    } else {
      this.rightTabs = next;
      if (index < this.rightActiveTab) this.rightActiveTab--;
      else if (this.rightActiveTab >= next.length) this.rightActiveTab = next.length - 1;
    }
  }

  switchTab(side: 'left' | 'right', index: number) {
    const activeIndex = side === 'left' ? this.leftActiveTab : this.rightActiveTab;
    if (comparisonState.active && index !== activeIndex) comparisonState.stopComparison();
    if (side === 'left') {
      this.leftActiveTab = index;
    } else {
      this.rightActiveTab = index;
    }
  }

  swapPanels() {
    if (comparisonState.active) comparisonState.stopComparison();
    const tmpTabs = this.leftTabs;
    const tmpActive = this.leftActiveTab;
    this.leftTabs = this.rightTabs;
    this.leftActiveTab = this.rightActiveTab;
    this.rightTabs = tmpTabs;
    this.rightActiveTab = tmpActive;
    // Fix watch IDs
    for (const tab of this.leftTabs) {
      tab.side = 'left';
      tab.watchId = `watch-left-${tab.tabId}`;
    }
    for (const tab of this.rightTabs) {
      tab.side = 'right';
      tab.watchId = `watch-right-${tab.tabId}`;
    }
  }
}

export const panels = new PanelsState();
