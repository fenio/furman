import { Channel, invoke } from '@tauri-apps/api/core';
import { noteJobJournalFailure } from '#lib/services/job-journal.js';
import type { ScanLimits, ScanOutcome, ScanProgress } from '#lib/types/index.js';
import type { JobExecutionCounts, JobAttemptStatus } from '#lib/services/job-journal.js';
import { jobExecutionState } from '#lib/state/job-execution.svelte.js';
import { beginS3Mutation } from '#lib/state/navigation-cache.svelte.js';

export interface JobRunStatus {
  run_id: string;
  job_id: string | null;
  phase: 'binding' | 'bound' | 'scanning' | 'complete' | 'preparing' | 'prepared' | 'reviewing_execution' | 'awaiting_execution_approval' | 'executing' | 'stopped';
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
  execution_available: boolean;
  preview: JobCheckedPreview | null;
  execution_review: JobExecutionReview | null;
  execution: JobExecutionResult | null;
}
export interface JobExecutionReview { approval_id: string; preview: JobCheckedPreview; source: string; destination: string; exclusions: string[]; destination_sse_c: boolean; s3_source: boolean; preserved_tag_count: number; source_aes256_files: number; source_sse_c_files: number }
export interface JobExecutionResult { status: JobAttemptStatus; counts: JobExecutionCounts; outcome_recorded: boolean; message: string | null }
export type JobPreviewPolicy = 'create_only' | 'skip_existing' | 'replace_unchanged';
export interface JobCheckedPreview {
  id: string; run_id: string; manifest_id: string; job_id: string; revision: string;
  policy: JobPreviewPolicy; files: number; skipped: number; bytes: number;
  max_snapshot_bytes: number; expires_seconds: number; relay: boolean;
  entries: { entry_id: string; relative_path: string; destination_key: string; size: number; replaces: boolean }[];
  entries_truncated: boolean; verification_required: true; execution_available: false;
  can_review_execution: boolean; execution_blocker: string | null;
}
export interface JobManifestRow {
  id: string; relative_path: string; status: 'new' | 'modified' | 'destination_only';
  source_size: number | null; destination_size: number | null;
}

async function jobInvoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    const result = await invoke<T>(command, args);
    const status = result as Partial<JobRunStatus> | null;
    if (status?.phase === 'stopped' && (status.outcome === 'failed' || status.outcome === 'limited')) {
      noteJobJournalFailure(`[JOB_JOURNAL] Job ${status.outcome}; inspect the diagnostic/history and acknowledge any listed review pause before a new run. ${status.message ?? ''}`);
    }
    return result;
  }
  catch (failure) { noteJobJournalFailure(failure); throw failure; }
}

export function bindJobSession(opId: string, jobId: string, revision: string, approved: boolean): Promise<JobRunStatus> {
  if (!approved) return Promise.reject(new Error('Explicit approval is required before job keychain access and read-only connection validation.'));
  return jobInvoke('saved_job_bind', { opId, jobId, revision, approved });
}
export function scanJobSession(runId: string, onProgress: (progress: ScanProgress) => void): Promise<JobRunStatus> {
  const channel = new Channel<ScanProgress>(); channel.onmessage = onProgress;
  return jobInvoke('saved_job_scan', { runId, channel });
}
export function releaseJobSession(handle: string): Promise<void> { return jobInvoke('saved_job_release', { handle }); }
export function jobSessionStatus(runId: string): Promise<JobRunStatus> { return jobInvoke('saved_job_status', { runId }); }
export function jobManifestPage(runId: string, manifestId: string, offset: number): Promise<JobManifestRow[]> {
  return jobInvoke('saved_job_manifest_page', { runId, manifestId, offset });
}
export function prepareJobPreview(runId: string, manifestId: string, entryIds: string[], policy: JobPreviewPolicy, maxSnapshotBytes: number, approved: boolean, onProgress: (progress: ScanProgress) => void): Promise<JobRunStatus> {
  if (!approved) return Promise.reject(new Error('Explicit approval is required before checked snapshot preparation and additional read/download costs.'));
  const channel = new Channel<ScanProgress>(); channel.onmessage = onProgress;
  return jobInvoke('saved_job_prepare', { request: { run_id: runId, manifest_id: manifestId, entry_ids: [...entryIds], policy, max_snapshot_bytes: maxSnapshotBytes, approved }, channel });
}
export function reviewJobExecution(runId: string, previewId: string): Promise<JobRunStatus> {
  return jobInvoke('saved_job_execution_review', { request: { run_id: runId, preview_id: previewId } });
}
let monitoringExecution = false;
export async function executeJobPreview(runId: string, approvalId: string, approved: boolean, acknowledgeExclusions: boolean): Promise<JobRunStatus> {
  if (!approved || !acknowledgeExclusions) throw new Error('Explicit single-use execution and attribute-exclusion acknowledgement is required.');
  if (monitoringExecution) throw new Error('A native execution outcome is still being monitored; wait for it or inspect local history.');
  monitoringExecution = true;
  jobExecutionState.error = null;
  // Private job clients have no ordinary connection IDs: invalidate all display
  // views conservatively, including failed/uncertain outcomes.
  const endMutation = beginS3Mutation();
  try {
    let status = await jobInvoke<JobRunStatus>('saved_job_execute', { request: { run_id: runId, approval_id: approvalId, approved, acknowledge_exclusions: acknowledgeExclusions } });
    jobExecutionState.status = status;
    // Monitor only explicitly admitted work, independently of its dialog lifetime.
    // These are local status reads, never mutation retries or restart resume.
    while (status.phase === 'executing') {
      await new Promise(resolve => setTimeout(resolve, 500));
      status = await jobSessionStatus(runId); jobExecutionState.status = status;
    }
    return status;
  } catch (failure) {
    jobExecutionState.error = failure;
    noteJobJournalFailure(`[JOB_JOURNAL] Execution admission/monitoring failed. Inspect local history and destination; sent writes may have committed. Nothing retries. ${String(failure)}`);
    throw failure;
  } finally { endMutation(); monitoringExecution = false; }
}
