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

export function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return '';
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(1)} GB/s`;
}

export function formatEta(bytesRemaining: number, bytesPerSec: number): string {
  if (bytesPerSec <= 0 || bytesRemaining <= 0) return '';
  const secs = Math.round(bytesRemaining / bytesPerSec);
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h ${m}m`;
}

export function getFileIcon(entry: FileEntry): string {
  if (entry.name === '..') return ' ..';
  if (entry.is_dir) return '<DIR>';
  if (entry.is_symlink) return '<LNK>';
  return '    ';
}
