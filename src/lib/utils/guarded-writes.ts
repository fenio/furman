import type { WritePolicy, WritePreview, WriteSelection } from '#lib/services/s3.js';
import type { SyncEntry } from '#lib/types/index.js';

export function writeNeedsApproval(policy: WritePolicy, existing: number): boolean {
  return policy === 'overwrite' || (policy === 'replace_unchanged' && existing > 0);
}

export function validWritePolicy(policy: WritePolicy, preview: (Pick<WritePreview, 'existing' | 'unguardable'> & { guard_supported?: boolean }) | null): boolean {
  if (preview?.guard_supported === false && policy !== 'overwrite') return false;
  return preview !== null && !(policy === 'create_only' && preview.existing > 0)
    && !(policy === 'replace_unchanged' && preview.unguardable > 0);
}

export function syncWriteSelections(entries: SyncEntry[], sourcePath: string, prefix: string): WriteSelection[] {
  const base = sourcePath.endsWith('/') ? sourcePath : sourcePath + '/';
  const isHash = (value: string) => /^[a-f0-9]{64}$/.test(value);
  return entries.filter((entry) => entry.status === 'new' || entry.status === 'modified').map((entry) => ({
    path: base + entry.relative_path,
    destination_key: prefix + entry.relative_path,
    // Legacy SyncEntry names hold SHA-256 in checksum mode, NOT S3 ETags.
    expected_sha256: isHash(entry.source_etag) ? entry.source_etag : undefined,
    expected_destination_sha256: isHash(entry.dest_etag) ? entry.dest_etag : undefined,
    expected_destination: entry.status === 'new' ? { type: 'missing' as const } : undefined,
  }));
}
