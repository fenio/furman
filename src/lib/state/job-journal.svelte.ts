import type { JobHistoryReport } from '#lib/services/job-journal.js';

export const jobJournalState = $state<{ report: JobHistoryReport | null; error: unknown }>({ report: null, error: null });
