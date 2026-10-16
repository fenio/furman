import { Channel, invoke } from '@tauri-apps/api/core';
import type { ScanLimits, ScanOutcome, ScanProgress } from '#lib/types/index.js';

export interface JobRunStatus {
  run_id: string;
  job_id: string | null;
  phase: 'binding' | 'bound' | 'scanning' | 'complete' | 'preparing' | 'prepared' | 'stopped';
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
  preview: JobCheckedPreview | null;
}
export type JobPreviewPolicy = 'create_only' | 'skip_existing' | 'replace_unchanged';
export interface JobCheckedPreview {
  id: string; run_id: string; manifest_id: string; job_id: string; revision: string;
  policy: JobPreviewPolicy; files: number; skipped: number; bytes: number;
  max_snapshot_bytes: number; expires_seconds: number; relay: boolean;
  entries: { entry_id: string; relative_path: string; destination_key: string; size: number; replaces: boolean }[];
  entries_truncated: boolean; verification_required: true; execution_available: false;
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
export function prepareJobPreview(runId: string, manifestId: string, entryIds: string[], policy: JobPreviewPolicy, maxSnapshotBytes: number, approved: boolean, onProgress: (progress: ScanProgress) => void): Promise<JobRunStatus> {
  if (!approved) return Promise.reject(new Error('Explicit approval is required before checked snapshot preparation and additional read/download costs.'));
  const channel = new Channel<ScanProgress>(); channel.onmessage = onProgress;
  return invoke('saved_job_prepare', { request: { run_id: runId, manifest_id: manifestId, entry_ids: [...entryIds], policy, max_snapshot_bytes: maxSnapshotBytes, approved }, channel });
}
