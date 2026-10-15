import { invoke } from '@tauri-apps/api/core';
import type { ComparisonMode } from '#lib/utils/comparison-options.js';

export interface ComparisonPreset {
  id: string;
  name: string;
  compare_mode: ComparisonMode;
  exclude_patterns: string[];
}
export interface PresetReport { revision: string; presets: ComparisonPreset[] }

export function loadComparisonPresets(): Promise<PresetReport> {
  return invoke('comparison_presets_load');
}

export function saveComparisonPresets(expectedRevision: string, presets: ComparisonPreset[]): Promise<PresetReport> {
  return invoke('comparison_presets_save', {
    expectedRevision,
    // Allowlist fields: never pass connection state, scans or write approvals.
    presets: presets.map(preset => ({ id: preset.id, name: preset.name, compare_mode: preset.compare_mode, exclude_patterns: [...preset.exclude_patterns] })),
  });
}
