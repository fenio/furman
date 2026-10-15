import type { S3ActivitySnapshot } from '#lib/services/s3.js';

/** Deliberate field allowlist: never export arbitrary IPC objects or UI state. */
export function activityExport(snapshot: S3ActivitySnapshot): string {
  return JSON.stringify({
    schema_version: snapshot.schema_version, app_version: snapshot.app_version,
    capacity: snapshot.capacity, evicted: snapshot.evicted,
    records: snapshot.records.map(record => ({
      id: record.id, started_ms: record.started_ms, operation: record.operation,
      duration_ms: record.duration_ms, outcome: record.outcome, http_status: record.http_status,
      request_id: record.request_id, attempts: record.attempts, retries: record.retries,
    })),
  }, null, 2);
}
