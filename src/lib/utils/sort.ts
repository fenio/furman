import type { FileEntry, SortField, SortDirection } from '$lib/types';

const nameCollator = new Intl.Collator(undefined, { sensitivity: 'base' });

function compareField(
  a: FileEntry,
  b: FileEntry,
  field: SortField,
  direction: SortDirection
): number {
  const dir = direction === 'asc' ? 1 : -1;

  switch (field) {
    case 'name':
      return dir * nameCollator.compare(a.name, b.name);
    case 'size':
      return dir * (a.size - b.size);
    case 'modified':
      return dir * (a.modified - b.modified);
    case 'extension': {
      const extA = a.extension ?? '';
      const extB = b.extension ?? '';
      const cmp = nameCollator.compare(extA, extB);
      if (cmp !== 0) return dir * cmp;
      return dir * nameCollator.compare(a.name, b.name);
    }
    case 'storage_class': {
      const scA = a.storage_class ?? '';
      const scB = b.storage_class ?? '';
      const cmp = nameCollator.compare(scA, scB);
      if (cmp !== 0) return dir * cmp;
      return dir * nameCollator.compare(a.name, b.name);
    }
    default:
      return 0;
  }
}

export function sortEntries(
  entries: FileEntry[],
  field: SortField,
  direction: SortDirection
): FileEntry[] {
  const dotdot: FileEntry[] = [];
  const dirs: FileEntry[] = [];
  const files: FileEntry[] = [];

  for (const entry of entries) {
    if (entry.name === '..') {
      dotdot.push(entry);
    } else if (entry.is_dir) {
      dirs.push(entry);
    } else {
      files.push(entry);
    }
  }

  dirs.sort((a, b) => compareField(a, b, field, direction));
  files.sort((a, b) => compareField(a, b, field, direction));

  return [...dotdot, ...dirs, ...files];
}
