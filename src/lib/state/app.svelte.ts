import type { ModalType, ViewerMode, PanelBackend, S3ProviderCapabilities, S3ConnectionInfo, SftpConnectionInfo, S3Profile, SortField, SortDirection, ArchiveInfo } from '$lib/types';
import type { Theme } from '@tauri-apps/api/window';
import { saveConfig, type Config } from '$lib/services/config';
import { sidebarState } from '$lib/state/sidebar.svelte';
import { workspacesState } from '$lib/state/workspaces.svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import { s3BookmarksState } from '$lib/state/s3bookmarks.svelte';
import { sftpBookmarksState } from '$lib/state/sftpbookmarks.svelte';
import { transfersState } from '$lib/state/transfers.svelte';
import { s3SetBandwidthLimit } from '$lib/services/s3';

class AppState {
  theme = $state<'dark' | 'light'>('dark');
  layoutMode = $state<'dual' | 'single'>('dual');
  modal = $state<ModalType>('none');
  viewerPath = $state('');
  viewerMode = $state<ViewerMode>('text');
  viewerContent = $state('');
  editorPath = $state('');
  editorContent = $state('');
  editorDirty = $state(false);
  editorS3ConnectionId = $state('');
  editorS3Key = $state('');
  editorSftpConnectionId = $state('');
  editorSftpPath = $state('');
  confirmMessage = $state('');
  confirmCallback = $state<(() => void) | null>(null);
  confirmAlertOnly = $state(false);
  inputPrompt = $state('');
  inputValue = $state('');
  inputCallback = $state<((value: string) => void) | null>(null);
  inputType = $state<'text' | 'password'>('text');
  menuActive = $state(false);
  iconSize = $state(48);
  startupSound = $state(true);
  showHidden = $state(false);
  calculateDirSizes = $state(true);
  connectCallback = $state<((bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities) => void) | null>(null);
  connectionManagerTab = $state<'saved' | 'connect'>('saved');
  connectionManagerInitialData = $state<Partial<S3Profile> | undefined>(undefined);
  searchRoot = $state('');
  searchBackend = $state<PanelBackend>('local');
  searchS3ConnectionId = $state('');
  sortField = $state<SortField>('name');
  sortDirection = $state<SortDirection>('asc');
  externalEditor = $state('');
  overwriteFiles = $state<string[]>([]);
  overwriteCallback = $state<((action: 'overwrite' | 'skip') => void) | null>(null);
  propertiesPath = $state('');
  propertiesBackend = $state<PanelBackend>('local');
  propertiesS3ConnectionId = $state('');
  propertiesSftpConnectionId = $state('');
  propertiesCapabilities = $state<S3ProviderCapabilities | undefined>(undefined);
  propertiesS3Connection = $state<S3ConnectionInfo | undefined>(undefined);
  propertiesSftpConnection = $state<SftpConnectionInfo | undefined>(undefined);
  propertiesArchiveInfo = $state<ArchiveInfo | undefined>(undefined);
  syncSourceBackend = $state<PanelBackend>('local');
  syncSourcePath = $state('');
  syncSourceS3Id = $state('');
  syncDestBackend = $state<PanelBackend>('local');
  syncDestPath = $state('');
  syncDestS3Id = $state('');
  secureTempCleanup = $state(false);
  syncExcludePatterns = $state('.DS_Store, Thumbs.db, .git/**');
  batchEditKeys = $state<string[]>([]);
  batchEditS3ConnectionId = $state('');
  batchEditCapabilities = $state<S3ProviderCapabilities | undefined>(undefined);

  showSearch(root: string, backend: PanelBackend = 'local', s3ConnectionId: string = '') {
    this.searchRoot = root;
    this.searchBackend = backend;
    this.searchS3ConnectionId = s3ConnectionId;
    this.modal = 'search';
  }

  showPreferences() {
    this.modal = 'preferences';
  }

  setExternalEditor(val: string) {
    this.externalEditor = val;
    this.persistConfig();
  }

  setShowHidden(val: boolean) {
    this.showHidden = val;
    this.persistConfig();
  }

  setCalculateDirSizes(val: boolean) {
    this.calculateDirSizes = val;
    this.persistConfig();
  }

  setStartupSound(val: boolean) {
    this.startupSound = val;
    this.persistConfig();
  }

  setSecureTempCleanup(val: boolean) {
    this.secureTempCleanup = val;
    this.persistConfig();
  }

  setMaxConcurrent(val: number) {
    transfersState.maxConcurrent = val;
    this.persistConfig();
  }

  setBandwidthLimit(val: number) {
    transfersState.bandwidthLimit = val;
    s3SetBandwidthLimit(val).catch(() => {});
    this.persistConfig();
  }

  toggleLayout() {
    this.layoutMode = this.layoutMode === 'dual' ? 'single' : 'dual';
    this.persistConfig();
  }

  showConfirm(message: string, callback: () => void) {
    this.confirmMessage = message;
    this.confirmCallback = callback;
    this.confirmAlertOnly = false;
    this.modal = 'confirm';
  }

  showAlert(message: string) {
    this.confirmMessage = message;
    this.confirmCallback = null;
    this.confirmAlertOnly = true;
    this.modal = 'confirm';
  }

  showInput(prompt: string, defaultValue: string, callback: (value: string) => void, type: 'text' | 'password' = 'text') {
    this.inputPrompt = prompt;
    this.inputValue = defaultValue;
    this.inputCallback = callback;
    this.inputType = type;
    this.modal = 'input';
  }

  showConnect(callback: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities) => void) {
    this.connectCallback = callback;
    this.connectionManagerTab = 'connect';
    this.modal = 'connection-manager';
  }

  showConnectionManager() {
    this.connectionManagerTab = 'saved';
    this.connectionManagerInitialData = undefined;
    this.modal = 'connection-manager';
  }

  showConnectionManagerSave(initialData: Partial<S3Profile>) {
    this.connectionManagerTab = 'connect';
    this.connectionManagerInitialData = initialData;
    this.modal = 'connection-manager';
  }

  showProperties(path: string, backend: PanelBackend, opts?: {
    s3ConnectionId?: string;
    capabilities?: S3ProviderCapabilities;
    s3Connection?: S3ConnectionInfo;
    sftpConnectionId?: string;
    sftpConnection?: SftpConnectionInfo;
    archiveInfo?: ArchiveInfo;
  }) {
    this.propertiesPath = path;
    this.propertiesBackend = backend;
    this.propertiesS3ConnectionId = opts?.s3ConnectionId ?? '';
    this.propertiesSftpConnectionId = opts?.sftpConnectionId ?? '';
    this.propertiesCapabilities = opts?.capabilities;
    this.propertiesS3Connection = opts?.s3Connection;
    this.propertiesSftpConnection = opts?.sftpConnection;
    this.propertiesArchiveInfo = opts?.archiveInfo;
    this.modal = 'properties';
  }

  showBatchEdit(keys: string[], s3ConnectionId: string, capabilities?: S3ProviderCapabilities) {
    this.batchEditKeys = keys;
    this.batchEditS3ConnectionId = s3ConnectionId;
    this.batchEditCapabilities = capabilities;
    this.modal = 'batch-edit';
  }

  showOverwrite(files: string[], callback: (action: 'overwrite' | 'skip') => void) {
    this.overwriteFiles = files;
    this.overwriteCallback = callback;
    this.modal = 'overwrite';
  }

  showSync(
    source: { backend: PanelBackend; path: string; s3Id: string },
    dest: { backend: PanelBackend; path: string; s3Id: string },
  ) {
    this.syncSourceBackend = source.backend;
    this.syncSourcePath = source.path;
    this.syncSourceS3Id = source.s3Id;
    this.syncDestBackend = dest.backend;
    this.syncDestPath = dest.path;
    this.syncDestS3Id = dest.s3Id;
    this.modal = 'sync';
  }

  setIconSize(size: number) {
    this.iconSize = size;
    this.persistConfig();
  }

  private applyTheme() {
    document.documentElement.setAttribute('data-theme', this.theme);
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      getCurrentWindow().setTheme(this.theme as Theme).catch(() => {});
    }).catch(() => {});
  }

  initSettings(config: Config) {
    this.theme = config.theme;
    this.layoutMode = config.layoutMode;
    this.applyTheme();
    this.iconSize = config.iconSize;
    this.startupSound = config.startupSound;
    this.showHidden = config.showHidden;
    this.externalEditor = config.externalEditor;
    this.calculateDirSizes = config.calculateDirSizes;
    this.sortField = config.sortField ?? 'name';
    this.sortDirection = config.sortDirection ?? 'asc';
    transfersState.bandwidthLimit = config.bandwidthLimit ?? 0;
    transfersState.maxConcurrent = config.maxConcurrent ?? 2;
    this.secureTempCleanup = config.secureTempCleanup ?? false;
    this.syncExcludePatterns = config.syncExcludePatterns ?? '.DS_Store, Thumbs.db, .git/**';
    s3SetBandwidthLimit(transfersState.bandwidthLimit).catch(() => {});
  }

  toggleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : 'dark';
    this.applyTheme();
    this.persistConfig();
  }

  persistConfig() {
    saveConfig({
      theme: this.theme,
      layoutMode: this.layoutMode,
      iconSize: this.iconSize,
      startupSound: this.startupSound,
      showHidden: this.showHidden,
      calculateDirSizes: this.calculateDirSizes,
      externalEditor: this.externalEditor,
      favorites: sidebarState.favorites,
      workspaces: workspacesState.workspaces,
      connections: connectionsState.profiles,
      s3Bookmarks: s3BookmarksState.bookmarks,
      sftpBookmarks: sftpBookmarksState.bookmarks,
      bandwidthLimit: transfersState.bandwidthLimit,
      maxConcurrent: transfersState.maxConcurrent,
      secureTempCleanup: this.secureTempCleanup,
      sortField: this.sortField,
      sortDirection: this.sortDirection,
      syncExcludePatterns: this.syncExcludePatterns,
    });
  }

  closeModal() {
    this.modal = 'none';
    this.confirmMessage = '';
    this.confirmCallback = null;
    this.confirmAlertOnly = false;
    this.inputPrompt = '';
    this.inputValue = '';
    this.inputCallback = null;
    this.inputType = 'text';
    this.menuActive = false;
    this.connectCallback = null;
    this.connectionManagerInitialData = undefined;
    this.searchRoot = '';
    this.searchBackend = 'local';
    this.searchS3ConnectionId = '';
    this.overwriteFiles = [];
    this.overwriteCallback = null;
    this.propertiesPath = '';
    this.propertiesBackend = 'local';
    this.propertiesS3ConnectionId = '';
    this.propertiesSftpConnectionId = '';
    this.propertiesCapabilities = undefined;
    this.propertiesS3Connection = undefined;
    this.propertiesSftpConnection = undefined;
    this.propertiesArchiveInfo = undefined;
    this.batchEditKeys = [];
    this.batchEditS3ConnectionId = '';
    this.batchEditCapabilities = undefined;
    this.syncSourceBackend = 'local';
    this.syncSourcePath = '';
    this.syncSourceS3Id = '';
    this.syncDestBackend = 'local';
    this.syncDestPath = '';
    this.syncDestS3Id = '';
  }
}

export const appState = new AppState();
