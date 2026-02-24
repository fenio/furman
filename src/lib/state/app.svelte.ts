import type { ModalType, ViewerMode, ProgressEvent } from '$lib/types';
import type { Theme } from '@tauri-apps/api/window';

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

  showSearch(root: string) {
    this.searchRoot = root;
    this.modal = 'search';
  }

  showPreferences() {
    this.modal = 'preferences';
  }

  setShowHidden(val: boolean) {
    this.showHidden = val;
    localStorage.setItem('showHidden', String(val));
  }

  setStartupSound(val: boolean) {
    this.startupSound = val;
    localStorage.setItem('startupSound', String(val));
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
    localStorage.setItem('iconSize', String(size));
  }

  private applyTheme() {
    document.documentElement.setAttribute('data-theme', this.theme);
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      getCurrentWindow().setTheme(this.theme as Theme).catch(() => {});
    }).catch(() => {});
  }

  initSettings() {
    const saved = localStorage.getItem('theme');
    if (saved === 'dark' || saved === 'light') {
      this.theme = saved;
    } else if (window.matchMedia('(prefers-color-scheme: light)').matches) {
      this.theme = 'light';
    } else {
      this.theme = 'dark';
    }
    this.applyTheme();

    const savedSize = localStorage.getItem('iconSize');
    if (savedSize) this.iconSize = parseInt(savedSize, 10) || 48;

    const savedSound = localStorage.getItem('startupSound');
    if (savedSound === 'false') this.startupSound = false;

    const savedHidden = localStorage.getItem('showHidden');
    if (savedHidden === 'true') this.showHidden = true;
  }

  toggleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : 'dark';
    this.applyTheme();
    localStorage.setItem('theme', this.theme);
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
