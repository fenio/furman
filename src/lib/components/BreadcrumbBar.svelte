<script lang="ts">
  import type { PanelBackend, S3ConnectionInfo, SftpConnectionInfo, ArchiveInfo } from '$lib/types';
  import { tick } from 'svelte';

  interface Props {
    path: string;
    backend: PanelBackend;
    s3Connection: S3ConnectionInfo | null;
    sftpConnection: SftpConnectionInfo | null;
    archiveInfo: ArchiveInfo | null;
    homePath: string;
    onNavigate: (path: string) => void;
    canGoBack?: boolean;
    canGoForward?: boolean;
    onGoBack?: () => void;
    onGoForward?: () => void;
  }

  let { path, backend, s3Connection, sftpConnection, archiveInfo, homePath, onNavigate, canGoBack = false, canGoForward = false, onGoBack, onGoForward }: Props = $props();

  let scrollContainer: HTMLDivElement | undefined = $state(undefined);

  interface Segment {
    label: string;
    path: string;
  }

  const segments = $derived.by((): Segment[] => {
    if (backend === 's3' && s3Connection) {
      const bucket = s3Connection.bucket;
      const prefix = `s3://${bucket}/`;
      const key = path.startsWith(prefix) ? path.substring(prefix.length) : '';
      const result: Segment[] = [{ label: `S3: ${bucket}`, path: prefix }];
      if (key) {
        const parts = key.replace(/\/+$/, '').split('/');
        let accumulated = prefix;
        for (const part of parts) {
          accumulated += part + '/';
          result.push({ label: part, path: accumulated });
        }
      }
      return result;
    }

    if (backend === 'sftp' && sftpConnection) {
      const prefix = `sftp://${sftpConnection.host}:${sftpConnection.port}`;
      const remotePath = path.startsWith(prefix) ? path.substring(prefix.length) : '/';
      const cleanRemote = remotePath.replace(/\/+$/, '') || '/';
      const result: Segment[] = [{ label: `${sftpConnection.username}@${sftpConnection.host}`, path: prefix + '/' }];
      if (cleanRemote !== '/') {
        const parts = cleanRemote.split('/').filter(Boolean);
        let accumulated = prefix;
        for (const part of parts) {
          accumulated += '/' + part;
          result.push({ label: part, path: accumulated + '/' });
        }
      }
      return result;
    }

    if (backend === 'archive' && archiveInfo) {
      const archiveName = archiveInfo.archivePath.split('/').pop() ?? 'Archive';
      const result: Segment[] = [{ label: `📦 ${archiveName}`, path: `archive://${archiveInfo.archivePath}#` }];
      if (archiveInfo.internalPath) {
        const parts = archiveInfo.internalPath.replace(/\/+$/, '').split('/');
        let accumulated = '';
        for (const part of parts) {
          accumulated += (accumulated ? '/' : '') + part;
          result.push({ label: part, path: `archive://${archiveInfo.archivePath}#${accumulated}` });
        }
      }
      return result;
    }

    // Local filesystem
    const cleanPath = path.replace(/\/+$/, '') || '/';
    const useHome = homePath && (cleanPath === homePath || cleanPath.startsWith(homePath + '/'));

    if (useHome) {
      const result: Segment[] = [{ label: '~', path: homePath }];
      const remaining = cleanPath.substring(homePath.length);
      if (remaining) {
        const parts = remaining.replace(/^\/+/, '').split('/');
        let accumulated = homePath;
        for (const part of parts) {
          accumulated += '/' + part;
          result.push({ label: part, path: accumulated });
        }
      }
      return result;
    }

    // Absolute path
    const parts = cleanPath.split('/').filter(Boolean);
    const result: Segment[] = [{ label: '/', path: '/' }];
    let accumulated = '';
    for (const part of parts) {
      accumulated += '/' + part;
      result.push({ label: part, path: accumulated });
    }
    return result;
  });

  // Auto-scroll to the right end on path change
  $effect(() => {
    // Track path to re-run on change
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    path;
    tick().then(() => {
      if (scrollContainer) {
        scrollContainer.scrollLeft = scrollContainer.scrollWidth;
      }
    });
  });
</script>

<div class="breadcrumb-bar" bind:this={scrollContainer}>
  <button class="nav-btn" disabled={!canGoBack} onclick={() => onGoBack?.()} title="Back (Alt+←)">◀</button>
  <button class="nav-btn" disabled={!canGoForward} onclick={() => onGoForward?.()} title="Forward (Alt+→)">▶</button>
  {#each segments as segment, i (segment.path)}
    {#if i > 0}
      <span class="breadcrumb-sep">/</span>
    {/if}
    {#if i === segments.length - 1}
      <span class="breadcrumb-segment current">{segment.label}</span>
    {:else}
      <button class="breadcrumb-segment" onclick={() => onNavigate(segment.path)}>
        {segment.label}
      </button>
    {/if}
  {/each}
</div>

<style>
  .breadcrumb-bar {
    display: flex;
    flex-direction: row;
    align-items: center;
    overflow-x: auto;
    white-space: nowrap;
    padding-right: 52px;
    scrollbar-width: none;
    gap: 2px;
  }

  .nav-btn {
    background: none;
    border: none;
    color: var(--text-accent);
    font-size: 10px;
    cursor: pointer;
    padding: 2px 3px;
    line-height: 1;
    opacity: 0.6;
    transition: opacity var(--transition-fast);
    flex-shrink: 0;
  }

  .nav-btn:hover:not(:disabled) {
    opacity: 1;
  }

  .nav-btn:disabled {
    opacity: 0.2;
    cursor: default;
  }

  .breadcrumb-bar::-webkit-scrollbar {
    display: none;
  }

  .breadcrumb-sep {
    color: var(--text-secondary);
    font-size: 12px;
    opacity: 0.5;
    flex-shrink: 0;
  }

  .breadcrumb-segment {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-accent);
    background: none;
    border: none;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .breadcrumb-segment:hover {
    background: var(--bg-hover);
  }

  .breadcrumb-segment.current {
    color: var(--header-text);
    cursor: default;
  }

  .breadcrumb-segment.current:hover {
    background: none;
  }
</style>
