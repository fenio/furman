import type { PanelBackend } from '$lib/types';

export type ClipboardMode = 'copy' | 'cut';

class ClipboardState {
  paths = $state<string[]>([]);
  mode = $state<ClipboardMode>('copy');
  sourceBackend = $state<PanelBackend>('local');
  sourceS3ConnectionId = $state('');
  sourceSftpConnectionId = $state('');

  isEmpty = $derived(this.paths.length === 0);

  copy(paths: string[], backend: PanelBackend, opts?: { s3ConnectionId?: string; sftpConnectionId?: string }) {
    this.paths = paths;
    this.mode = 'copy';
    this.sourceBackend = backend;
    this.sourceS3ConnectionId = opts?.s3ConnectionId ?? '';
    this.sourceSftpConnectionId = opts?.sftpConnectionId ?? '';
  }

  cut(paths: string[], backend: PanelBackend, opts?: { s3ConnectionId?: string; sftpConnectionId?: string }) {
    this.paths = paths;
    this.mode = 'cut';
    this.sourceBackend = backend;
    this.sourceS3ConnectionId = opts?.s3ConnectionId ?? '';
    this.sourceSftpConnectionId = opts?.sftpConnectionId ?? '';
  }

  clear() {
    this.paths = [];
    this.mode = 'copy';
    this.sourceBackend = 'local';
    this.sourceS3ConnectionId = '';
    this.sourceSftpConnectionId = '';
  }
}

export const clipboardState = new ClipboardState();
