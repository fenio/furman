import { invoke } from '@tauri-apps/api/core';

export interface JobConfiguration {
  bucket: string; region: string; endpoint: string | null; provider: string;
  proxy_endpoint: string | null; proxy_auth: boolean; sse_customer_key_enabled: boolean; acceleration: boolean;
}
export interface JobS3Pin { profile_id: string; generation: string; configuration: JobConfiguration; prefix: string }
export type JobSourcePin = { kind: 'local'; path: string; device: string; inode: string } | { kind: 's3'; pin: JobS3Pin };
export interface SavedJobDefinition {
  id: string; revision: string; name: string; source: JobSourcePin; destination: JobS3Pin;
  exclude_patterns: string[]; compare_mode: 'sha256'; operation: 'copy_update_only';
}
export interface SavedJobsReport {
  revision: string;
  configuration_revision: string;
  execution_available: false;
  jobs: { definition: SavedJobDefinition; status: 'definition_valid' | 'needs_rebind'; reason: string }[];
  profiles: { id: string; name: string; configuration: JobConfiguration | null; unavailable: string | null }[];
}
export type JobSourceRequest = { kind: 'local'; path: string } | { kind: 's3'; profile_id: string; prefix: string };
export interface SaveJobRequest {
  expected_revision: string; expected_configuration_revision: string; id: string | null; name: string; source: JobSourceRequest;
  destination_profile_id: string; destination_prefix: string; exclude_patterns: string[]; acknowledged: boolean;
}
export function listSavedJobs(): Promise<SavedJobsReport> { return invoke('saved_jobs_list'); }
export function saveJobDefinition(request: SaveJobRequest): Promise<SavedJobsReport> {
  return invoke('saved_jobs_save', { request: {
    expected_revision: request.expected_revision, expected_configuration_revision: request.expected_configuration_revision, id: request.id, name: request.name,
    source: request.source.kind === 'local' ? { kind: 'local', path: request.source.path } : { kind: 's3', profile_id: request.source.profile_id, prefix: request.source.prefix },
    destination_profile_id: request.destination_profile_id, destination_prefix: request.destination_prefix,
    exclude_patterns: [...request.exclude_patterns], acknowledged: request.acknowledged,
  } });
}
export function deleteJobDefinition(expectedRevision: string, id: string, acknowledged: boolean): Promise<SavedJobsReport> {
  return invoke('saved_jobs_delete', { expectedRevision, id, acknowledged });
}
