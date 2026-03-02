import type { ColumnId, SortField } from '$lib/types';

export interface ColumnDef {
  id: ColumnId;
  label: string;
  s3Label?: string;
  flex: string;
  textAlign: string;
  sortField: SortField;
  s3SortField?: SortField;
}

export const ALL_COLUMNS: ColumnDef[] = [
  { id: 'name', label: 'Name', flex: '1 1 0', textAlign: 'left', sortField: 'name' },
  { id: 'size', label: 'Size', flex: '0 0 9ch', textAlign: 'right', sortField: 'size' },
  { id: 'modified', label: 'Date', flex: '0 0 16ch', textAlign: 'left', sortField: 'modified' },
  { id: 'extension', label: 'Ext', s3Label: 'Class', flex: '0 0 9ch', textAlign: 'left', sortField: 'extension', s3SortField: 'storage_class' },
  { id: 'permissions', label: 'Perm', flex: '0 0 9ch', textAlign: 'left', sortField: 'name' },
  { id: 'owner', label: 'Owner', flex: '0 0 9ch', textAlign: 'left', sortField: 'name' },
  { id: 'group', label: 'Group', flex: '0 0 9ch', textAlign: 'left', sortField: 'name' },
];

export const DEFAULT_VISIBLE_COLUMNS: ColumnId[] = ['name', 'size', 'modified', 'extension'];
