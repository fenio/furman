import type { ModalType, ViewerMode, ProgressEvent } from '$lib/types';
import type { Theme } from '@tauri-apps/api/window';
import { saveConfig, type Config } from '$lib/services/config';
import { sidebarState } from '$lib/state/sidebar.svelte';
import { workspacesState } from '$lib/state/workspaces.svelte';

class AppState {
  theme = $state<'dark' | 'light'>('dark');
  modal = $state<ModalType>('none');
  viewerPath = $state('');
  viewerMode = $state<ViewerMode>('text');
  viewerContent = $state('');
  editorPath = $state('');
  editorContent = $state('');
  editorDirty = $state(false);
  confirmMessage = $state('');
  confirmCallback = $state<(() => void) | null>(null);
  inputPrompt = $state('');
  inputValue = $state('');
  inputCallback = $state<((value: string) => void) | null>(null);
  progressData = $state<ProgressEvent | null>(null);
  menuActive = $state(false);
  iconSize = $state(48);
  startupSound = $state(true);
  showHidden = $state(false);
  s3ConnectCallback = $state<((bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string) => void) | null>(null);
  searchRoot = $state('');
  externalEditor = $state('');

  showSearch(root: string) {
    this.searchRoot = root;
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

  showProgress() {
    this.progressData = null;
    this.modal = 'progress';
  }

  showS3Connect(callback: (bucket: string, region: string, endpoint?: string, profile?: string, accessKey?: string, secretKey?: string) => void) {
    this.s3ConnectCallback = callback;
    this.modal = 's3-connect';
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
      externalEditor: this.externalEditor,
      favorites: sidebarState.favorites,
      workspaces: workspacesState.workspaces,
    });
  }

  closeModal() {
    this.modal = 'none';
    this.confirmMessage = '';
    this.confirmCallback = null;
    this.inputPrompt = '';
    this.inputValue = '';
    this.inputCallback = null;
    this.progressData = null;
    this.menuActive = false;
    this.s3ConnectCallback = null;
    this.searchRoot = '';
  }
}

export const appState = new AppState();
