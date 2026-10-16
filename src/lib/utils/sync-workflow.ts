import type { PanelBackend } from '#lib/types/index.js';
import type { ComparisonMode } from './comparison-options.js';

export interface SyncLocation { backend: PanelBackend; path: string; s3Id: string; profileId?: string; bucket?: string; label?: string }
export interface SyncOptions { mode: ComparisonMode; exclusions: string[]; fromComparison?: boolean }
export type SyncPolicy = 'missing' | 'update';

function root(location: SyncLocation): string | null {
  const path = location.path;
  if (location.backend === 'local') {
    if (!path.startsWith('/') || path.includes('\0') || path.split('/').some(part => part === '.' || part === '..')) return null;
    return path.replace(/\/+$/, '') || '/';
  }
  if (location.backend !== 's3' || !location.s3Id || !s3Folder(location)) return null;
  // S3 keys are literal, not URLs: never decode or normalize them.
  return path.replace(/\/+$/, '');
}
export function locationError(location: SyncLocation): string {
  return root(location) === null ? 'Choose an absolute local directory or a connected S3 bucket/folder. Dot segments are not accepted.' : '';
}
export function syncLocationError(source: SyncLocation, destination: SyncLocation): string {
  const invalid = locationError(source) || locationError(destination);
  if (invalid) return invalid;
  const left = root(source)!; const right = root(destination)!;
  // Do not equate equally named buckets at different providers. Native job
  // origin/identity checks remain authoritative for connection aliases.
  const sameNamespace = source.backend === destination.backend && (source.backend === 'local' || source.s3Id === destination.s3Id);
  if (sameNamespace && (left === right || left === '/' || right === '/' || left.startsWith(right + '/') || right.startsWith(left + '/'))) {
    return 'Source and destination must be different, non-overlapping folders. Choose where the files should go.';
  }
  return '';
}
export function s3Folder(location: SyncLocation): { bucket: string; prefix: string } | null {
  if (location.backend !== 's3') return null;
  const match = /^s3:\/\/([^/]+)(?:\/(.*))?$/.exec(location.path);
  if (!match || (location.bucket && location.bucket !== match[1])) return null;
  const prefix = match[2] ?? '';
  if (prefix && !prefix.endsWith('/')) return null;
  return { bucket: match[1], prefix };
}
