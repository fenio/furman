import { invoke, Channel } from '@tauri-apps/api/core';
import type { DirListing, VolumeInfo, ProgressEvent, SearchEvent, SearchMode, SyncEvent, GitRepoInfo, FileProperties, TransferCheckpoint } from '$lib/types';

export async function listArchive(
  archivePath: string,
  internalPath: string
): Promise<DirListing> {
  return await invoke<DirListing>('list_archive', { archivePath, internalPath });
}

export async function extractArchive(
  id: string,
  archivePath: string,
  internalPaths: string[],
  destination: string,
  onProgress: (e: ProgressEvent) => void
): Promise<void> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  await invoke('extract_archive', { id, archivePath, internalPaths, destination, channel });
}

export async function listDirectory(
  path: string,
  showHidden: boolean
): Promise<DirListing> {
  return await invoke<DirListing>('list_directory', { path, showHidden });
}

export async function createDirectory(path: string): Promise<void> {
  await invoke('create_directory', { path });
}

export async function copyFiles(
  id: string,
  sources: string[],
  destination: string,
  onProgress: (e: ProgressEvent) => void
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('copy_files', { id, sources, destination, channel });
}

export async function moveFiles(
  id: string,
  sources: string[],
  destination: string,
  onProgress: (e: ProgressEvent) => void
): Promise<TransferCheckpoint | null> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onProgress;
  return await invoke<TransferCheckpoint | null>('move_files', { id, sources, destination, channel });
}

export async function cancelFileOperation(id: string): Promise<void> {
  await invoke('cancel_file_operation', { id });
}

export async function pauseFileOperation(id: string): Promise<void> {
  await invoke('pause_file_operation', { id });
}

export async function deleteFiles(
  paths: string[],
  useTrash: boolean
): Promise<void> {
  await invoke('delete_files', { paths, useTrash });
}

export async function checkConflicts(
  sources: string[],
  destination: string
): Promise<string[]> {
  return await invoke<string[]>('check_conflicts', { sources, destination });
}

export async function renameFile(
  path: string,
  newName: string
): Promise<void> {
  await invoke('rename_file', { path, newName });
}

export async function readFileText(path: string): Promise<string> {
  return await invoke<string>('read_file_text', { path });
}

export async function writeFileText(
  path: string,
  content: string
): Promise<void> {
  await invoke('write_file_text', { path, content });
}

export async function readFileBinary(
  path: string,
  offset: number,
  length: number
): Promise<number[]> {
  return await invoke<number[]>('read_file_binary', { path, offset, length });
}

export async function openFileDefault(path: string): Promise<void> {
  await invoke('open_file_default', { path });
}

export async function openInEditor(path: string, editor: string): Promise<void> {
  await invoke('open_in_editor', { path, editor });
}

export async function getDirectorySize(path: string): Promise<number> {
  return await invoke<number>('get_directory_size', { path });
}

export async function getFileProperties(path: string): Promise<FileProperties> {
  return await invoke<FileProperties>('get_file_properties', { path });
}

export async function listVolumes(): Promise<VolumeInfo[]> {
  return await invoke<VolumeInfo[]>('list_volumes');
}

export async function watchDirectory(
  path: string,
  id: string
): Promise<void> {
  await invoke('watch_directory', { path, id });
}

export async function unwatchDirectory(id: string): Promise<void> {
  await invoke('unwatch_directory', { id });
}

export async function terminalSpawn(id: string, cwd: string): Promise<void> {
  await invoke('terminal_spawn', { id, cwd });
}

export async function terminalWrite(id: string, data: string): Promise<void> {
  await invoke('terminal_write', { id, data });
}

export async function terminalResize(id: string, cols: number, rows: number): Promise<void> {
  await invoke('terminal_resize', { id, cols, rows });
}

export async function terminalClose(id: string): Promise<void> {
  await invoke('terminal_close', { id });
}

export async function searchFiles(
  id: string,
  root: string,
  query: string,
  mode: SearchMode,
  onEvent: (e: SearchEvent) => void
): Promise<void> {
  const channel = new Channel<SearchEvent>();
  channel.onmessage = onEvent;
  await invoke('search_files', { id, root, query, mode, channel });
}

export async function cancelSearch(id: string): Promise<void> {
  await invoke('cancel_search', { id });
}

export async function syncDiff(
  id: string,
  sourceBackend: string,
  sourcePath: string,
  sourceS3Id: string,
  destBackend: string,
  destPath: string,
  destS3Id: string,
  excludePatterns: string[],
  compareMode: string,
  onEvent: (e: SyncEvent) => void
): Promise<void> {
  const channel = new Channel<SyncEvent>();
  channel.onmessage = onEvent;
  await invoke('sync_diff', {
    id,
    sourceBackend,
    sourcePath,
    sourceS3Id,
    destBackend,
    destPath,
    destS3Id,
    excludePatterns,
    compareMode,
    channel,
  });
}

export async function cancelSync(id: string): Promise<void> {
  await invoke('cancel_sync', { id });
}

export async function getGitRepoInfo(path: string): Promise<GitRepoInfo | null> {
  return await invoke<GitRepoInfo | null>('git_repo_info', { path });
}

export async function gitPull(path: string): Promise<string> {
  return await invoke<string>('git_pull', { path });
}

export async function gitListBranches(path: string): Promise<string[]> {
  return await invoke<string[]>('git_list_branches', { path });
}

export async function gitCheckout(path: string, branch: string): Promise<string> {
  return await invoke<string>('git_checkout', { path, branch });
}

export async function getLogPath(): Promise<string> {
  return await invoke<string>('get_log_path');
}
