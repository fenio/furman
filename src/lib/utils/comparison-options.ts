import type { PanelBackend } from '#lib/types/index.js';

export type ComparisonMode = 'size_mtime' | 'sha256';

export function defaultComparisonMode(left: PanelBackend, right: PanelBackend): ComparisonMode {
  return left === 's3' || right === 's3' ? 'sha256' : 'size_mtime';
}

export function canComparePanes(left: PanelBackend, right: PanelBackend): boolean {
  return (left === 'local' || left === 's3') && (right === 'local' || right === 's3');
}

export function parseComparisonExcludes(text: string): string[] {
  return text.split(',').map(pattern => pattern.trim()).filter(Boolean);
}
