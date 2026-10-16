import type { ScanLimits, ScanOutcome, ScanProgress } from '../types/index.js';

export const DEFAULT_SCAN_LIMITS: ScanLimits = { max_objects: 100000, max_requests: 1000, max_seconds: 300, max_results: 1000 };
export const SCAN_RANGES: Record<keyof ScanLimits, [number, number]> = {
  max_objects: [1, 10000000], max_requests: [1, 1000000], max_seconds: [1, 86400], max_results: [1, 10000],
};
export function validScanLimits(limits: ScanLimits): boolean {
  return Object.entries(SCAN_RANGES).every(([key, [min, max]]) => {
    const value = limits[key as keyof ScanLimits];
    return Number.isSafeInteger(value) && value >= min && value <= max;
  });
}
export function scanLimitsFromConfig(value: unknown): ScanLimits {
  if (!value || typeof value !== 'object' || !validScanLimits(value as ScanLimits)) return { ...DEFAULT_SCAN_LIMITS };
  const limits = value as ScanLimits;
  return { max_objects: limits.max_objects, max_requests: limits.max_requests, max_seconds: limits.max_seconds, max_results: limits.max_results };
}
export function scanSummary(progress: ScanProgress, outcome?: ScanOutcome, showMatches = true): string {
  const state = outcome === 'complete' ? 'Complete' : outcome ? `Incomplete — ${outcome}` : 'Scanning';
  return `${state}: ${progress.scanned.toLocaleString()} scanned${showMatches ? ` · ${progress.matched.toLocaleString()} matched` : ''} · ${progress.requests.toLocaleString()} remote operations · ${Math.floor(progress.elapsed_ms / 1000)}s`;
}
export function canUseScan(outcome: ScanOutcome | null, scanning: boolean): boolean {
  return outcome === 'complete' && !scanning;
}
