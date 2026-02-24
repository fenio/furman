import type { FileEntry } from '$lib/types';

export function formatSize(bytes: number): string {
  if (bytes < 0) return '0';
  if (bytes < 1024) return `${bytes}`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}G`;
}

export function formatDate(epochMs: number): string {
  if (!epochMs) return '';
  const d = new Date(epochMs);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  const hours = String(d.getHours()).padStart(2, '0');
  const minutes = String(d.getMinutes()).padStart(2, '0');
  return `${year}-${month}-${day} ${hours}:${minutes}`;
}

export function formatPermissions(mode: number): string {
  const chars = ['r', 'w', 'x'];
  let result = '';
  for (let i = 2; i >= 0; i--) {
    const bits = (mode >> (i * 3)) & 7;
    for (let j = 2; j >= 0; j--) {
      result += bits & (1 << j) ? chars[2 - j] : '-';
    }
  }
  return result;
}

export function getFileIcon(entry: FileEntry): string {
  if (entry.name === '..') return ' ..';
  if (entry.is_dir) return '<DIR>';
  if (entry.is_symlink) return '<LNK>';
  return '    ';
}
