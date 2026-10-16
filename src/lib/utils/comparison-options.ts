import type { PanelBackend } from '#lib/types/index.js';

export type ComparisonMode = 'size_mtime' | 'sha256';

export function defaultComparisonMode(left: PanelBackend, right: PanelBackend): ComparisonMode {
  return left !== 'local' || right !== 'local' ? 'sha256' : 'size_mtime';
}

export function canComparePanes(left: PanelBackend, right: PanelBackend): boolean {
  const supported = (backend: PanelBackend) => ['local', 's3', 'sftp', 'archive'].includes(backend);
  return supported(left) && supported(right);
}

export function parseComparisonExcludes(text: string): string[] {
  return text.split(',').map(pattern => pattern.trim()).filter(Boolean);
}
