import { invoke } from '@tauri-apps/api/core';
import { jobJournalState } from '#lib/state/job-journal.svelte.js';

export type JobAttemptStatus = 'binding' | 'bound' | 'scanning' | 'compared' | 'preparing' | 'prepared' | 'no_changes' | 'discarded' | 'cancelled' | 'failed' | 'limited' | 'interrupted';
export interface JobHistoryCounters { scanned: number; requests: number; bytes_read: number; s3_bytes_read: number; elapsed_ms: number }
export interface JobAttemptRecord {
  run_id: string; job_id: string; job_revision: string; started_ms: number; updated_ms: number; status: JobAttemptStatus;
  binding: JobHistoryCounters; comparison: JobHistoryCounters; preparation: JobHistoryCounters;
  policy: 'create_only' | 'skip_existing' | 'replace_unchanged' | null;
}
export interface JobHistoryReport {
  revision: string; storage_review_required: boolean; reconciliation_deferred: boolean; in_memory_storage_fault: boolean;
  pauses: Record<string, { run_id: string; reason: 'failed' | 'limited' | 'interrupted' }>;
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
  const healthy = !report.storage_review_required && !report.in_memory_storage_fault && !Object.keys(report.pauses).length;
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
