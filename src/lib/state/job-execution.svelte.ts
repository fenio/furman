import type { JobRunStatus } from '#lib/services/job-runs.js';

// Runtime-only visibility, never persisted authority or a restored approval.
export const jobExecutionState = $state<{ status: JobRunStatus | null; error: unknown }>({ status: null, error: null });
