import type { ModalType, ViewerMode, PanelBackend, S3ProviderCapabilities } from '$lib/types';
import type { Theme } from '@tauri-apps/api/window';
import { saveConfig, type Config } from '$lib/services/config';
import { sidebarState } from '$lib/state/sidebar.svelte';
import { workspacesState } from '$lib/state/workspaces.svelte';
import { s3ProfilesState } from '$lib/state/s3profiles.svelte';

class AppState {
  theme = $state<'dark' | 'light'>('dark');
  modal = $state<ModalType>('none');
  viewerPath = $state('');
  viewerMode = $state<ViewerMode>('text');
  viewerContent = $state('');
  editorPath = $state('');
  editorContent = $state('');
  editorDirty = $state(false);
  editorS3ConnectionId = $state('');
  editorS3Key = $state('');
  confirmMessage = $state('');
  confirmCallback = $state<(() => void) | null>(null);
  inputPrompt = $state('');
  inputValue = $state('');
  inputCallback = $state<((value: string) => void) | null>(null);
  menuActive = $state(false);
  iconSize = $state(48);
  startupSound = $state(true);
  showHidden = $state(false);
  calculateDirSizes = $state(true);
  s3ConnectCallback = $state<((bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities) => void) | null>(null);
  searchRoot = $state('');
  searchBackend = $state<PanelBackend>('local');
  searchS3ConnectionId = $state('');
  externalEditor = $state('');
  overwriteFiles = $state<string[]>([]);
  overwriteCallback = $state<((action: 'overwrite' | 'skip') => void) | null>(null);
  propertiesPath = $state('');
  propertiesBackend = $state<PanelBackend>('local');
  propertiesS3ConnectionId = $state('');
  propertiesCapabilities = $state<S3ProviderCapabilities | undefined>(undefined);
  syncSourceBackend = $state<PanelBackend>('local');
  syncSourcePath = $state('');
  syncSourceS3Id = $state('');
  syncDestBackend = $state<PanelBackend>('local');
  syncDestPath = $state('');
  syncDestS3Id = $state('');

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

  showConfirm(message: string, callback: () => void) {
    this.confirmMessage = message;
    this.confirmCallback = callback;
    this.modal = 'confirm';
  }

  showInput(prompt: string, defaultValue: string, callback: (value: string) => void) {
    this.inputPrompt = prompt;
    this.inputValue = defaultValue;
    this.inputCallback = callback;
    this.modal = 'input';
  }

  showS3Connect(callback: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string, provider?: string, customCapabilities?: S3ProviderCapabilities) => void) {
    this.s3ConnectCallback = callback;
    this.modal = 's3-connect';
  }

  showS3Manager() {
    this.modal = 's3-manager';
  }

  showProperties(path: string, backend: PanelBackend, s3ConnectionId?: string, capabilities?: S3ProviderCapabilities) {
    this.propertiesPath = path;
    this.propertiesBackend = backend;
    this.propertiesS3ConnectionId = s3ConnectionId ?? '';
    this.propertiesCapabilities = capabilities;
    this.modal = 'properties';
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
    this.applyTheme();
    this.iconSize = config.iconSize;
    this.startupSound = config.startupSound;
    this.showHidden = config.showHidden;
    this.externalEditor = config.externalEditor;
    this.calculateDirSizes = config.calculateDirSizes;
  }

  toggleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : 'dark';
    this.applyTheme();
    this.persistConfig();
  }

  persistConfig() {
    saveConfig({
      theme: this.theme,
      iconSize: this.iconSize,
      startupSound: this.startupSound,
      showHidden: this.showHidden,
      calculateDirSizes: this.calculateDirSizes,
      externalEditor: this.externalEditor,
      favorites: sidebarState.favorites,
      workspaces: workspacesState.workspaces,
      s3Profiles: s3ProfilesState.profiles,
    });
  }

  closeModal() {
    this.modal = 'none';
    this.confirmMessage = '';
    this.confirmCallback = null;
    this.inputPrompt = '';
    this.inputValue = '';
    this.inputCallback = null;
    this.menuActive = false;
    this.s3ConnectCallback = null;
    this.searchRoot = '';
    this.searchBackend = 'local';
    this.searchS3ConnectionId = '';
    this.overwriteFiles = [];
    this.overwriteCallback = null;
    this.propertiesPath = '';
    this.propertiesBackend = 'local';
    this.propertiesS3ConnectionId = '';
    this.propertiesCapabilities = undefined;
    this.syncSourceBackend = 'local';
    this.syncSourcePath = '';
    this.syncSourceS3Id = '';
    this.syncDestBackend = 'local';
    this.syncDestPath = '';
    this.syncDestS3Id = '';
  }
}

export const appState = new AppState();
