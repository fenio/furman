import { Channel, invoke } from '@tauri-apps/api/core';
import type { ScanLimits, ScanOutcome, ScanProgress } from '#lib/types/index.js';

export interface JobRunStatus {
  run_id: string;
  job_id: string | null;
  phase: 'binding' | 'bound' | 'scanning' | 'complete' | 'stopped';
  expires_seconds: number;
  outcome: ScanOutcome | null;
  message: string | null;
  progress: ScanProgress;
  summary: { new: number; modified: number; destination_only: number; same: number } | null;
  manifest_id: string | null;
  source_instance: string | null;
  destination_instance: string | null;
  source_namespace: string | null;
  destination_namespace: string | null;
  limits: ScanLimits | null;
  execution_available: false;
}
export interface JobManifestRow {
  id: string; relative_path: string; status: 'new' | 'modified' | 'destination_only';
  source_size: number | null; destination_size: number | null;
}

export function bindJobSession(opId: string, jobId: string, revision: string, approved: boolean): Promise<JobRunStatus> {
  if (!approved) return Promise.reject(new Error('Explicit approval is required before job keychain access and read-only connection validation.'));
  return invoke('saved_job_bind', { opId, jobId, revision, approved });
}
export function scanJobSession(runId: string, onProgress: (progress: ScanProgress) => void): Promise<JobRunStatus> {
  const channel = new Channel<ScanProgress>(); channel.onmessage = onProgress;
  return invoke('saved_job_scan', { runId, channel });
}
export function releaseJobSession(handle: string): Promise<void> { return invoke('saved_job_release', { handle }); }
export function jobSessionStatus(runId: string): Promise<JobRunStatus> { return invoke('saved_job_status', { runId }); }
export function jobManifestPage(runId: string, manifestId: string, offset: number): Promise<JobManifestRow[]> {
  return invoke('saved_job_manifest_page', { runId, manifestId, offset });
}
