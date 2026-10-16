import { invoke } from '@tauri-apps/api/core';
import { jobJournalState } from '#lib/state/job-journal.svelte.js';

export type JobAttemptStatus = 'binding' | 'bound' | 'scanning' | 'compared' | 'preparing' | 'prepared' | 'reviewing' | 'awaiting_approval' | 'executing' | 'succeeded' | 'skipped' | 'partial' | 'uncertain' | 'no_changes' | 'discarded' | 'cancelled' | 'failed' | 'limited' | 'interrupted';
export interface JobExecutionCounts { selected_files: number; skipped_files: number; selected_bytes: number; attempted_files: number; acknowledged_files: number; verified_files: number; verified_bytes: number; uncertain_files: number; replacement_files: number; attempted_replacements: number; acknowledged_replacements: number; verified_replacements: number; uncertain_replacements: number; multipart_files: number }
export interface JobHistoryCounters { scanned: number; requests: number; bytes_read: number; s3_bytes_read: number; elapsed_ms: number }
export interface JobAttemptRecord {
  multipart_cleanup_reviews: number;
  run_id: string; job_id: string; job_revision: string; started_ms: number; updated_ms: number; status: JobAttemptStatus;
  binding: JobHistoryCounters; comparison: JobHistoryCounters; preparation: JobHistoryCounters;
  policy: 'create_only' | 'skip_existing' | 'replace_unchanged' | null;
  review: JobHistoryCounters; execution: JobExecutionCounts | null;
}
export interface JobHistoryReport {
  pending_uploads: { id: string; run_id: string; state: 'initiating' | 'active' | 'completing' | 'aborting'; upload_id_known: boolean; attempted_parts: number; acknowledged_parts: number }[];
  revision: string; storage_review_required: boolean; reconciliation_deferred: boolean; in_memory_storage_fault: boolean;
  pauses: Record<string, { run_id: string; reason: 'failed' | 'limited' | 'interrupted' | 'partial' | 'uncertain' }>;
  records: JobAttemptRecord[]; execution_available: false;
}
let failureGeneration = 0;
let requestGeneration = 0;
let appliedGeneration = 0;
function recordFailure(failure: unknown): void { failureGeneration++; jobJournalState.error = failure; }
export function noteJobJournalFailure(failure: unknown): void {
  if (String(failure).includes('[JOB_JOURNAL]')) recordFailure(failure);
}
function accept(report: JobHistoryReport, request: number, failuresAtStart: number): JobHistoryReport {
  const healthy = !report.storage_review_required && !report.in_memory_storage_fault && !Object.keys(report.pauses).length && !(report.pending_uploads?.length);
  // A late startup/refresh reply cannot erase a newer storage failure or review.
  if (request < appliedGeneration || (healthy && failuresAtStart !== failureGeneration)) return report;
  appliedGeneration = request;
  jobJournalState.report = report;
  if (healthy) jobJournalState.error = null;
  return report;
}
export async function loadJobHistory(): Promise<JobHistoryReport> {
  const request = ++requestGeneration; const failures = failureGeneration;
  try { return accept(await invoke<JobHistoryReport>('saved_job_history'), request, failures); }
  catch (failure) { recordFailure(failure); throw failure; }
}
export async function reviewJobHistory(revision: string, jobId: string | null, acknowledged: boolean, clearHistory = false): Promise<JobHistoryReport> {
  if (!acknowledged) throw new Error('Explicit journal review/removal acknowledgement is required.');
  const request = ++requestGeneration; const failures = failureGeneration;
  try { return accept(await invoke<JobHistoryReport>('saved_job_history_review', { expectedRevision: revision, jobId, acknowledged, clearHistory }), request, failures); }
  catch (failure) { recordFailure(failure); throw failure; }
}
export async function reviewMultipartCleanup(revision: string, evidenceId: string, acknowledged: boolean): Promise<JobHistoryReport> {
  if (!acknowledged) throw new Error('Explicit external multipart cleanup review acknowledgement is required; this does not inspect or clean up the provider.');
  const request = ++requestGeneration; const failures = failureGeneration;
  try { return accept(await invoke<JobHistoryReport>('saved_job_multipart_cleanup_review', { expectedRevision: revision, evidenceId, acknowledged }), request, failures); }
  catch (failure) { recordFailure(failure); throw failure; }
}
