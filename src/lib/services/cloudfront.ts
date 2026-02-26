import { invoke } from '@tauri-apps/api/core';
import type {
  CfDistributionSummary, CfDistribution, CfDistributionConfig, CfInvalidation,
} from '$lib/types';

export async function cfListDistributions(id: string): Promise<CfDistributionSummary[]> {
  return await invoke<CfDistributionSummary[]>('cf_list_distributions', { id });
}

export async function cfGetDistribution(id: string, distId: string): Promise<CfDistribution> {
  return await invoke<CfDistribution>('cf_get_distribution', { id, distId });
}

export async function cfCreateDistribution(id: string, config: CfDistributionConfig): Promise<CfDistribution> {
  return await invoke<CfDistribution>('cf_create_distribution', { id, config });
}

export async function cfUpdateDistribution(
  id: string,
  distId: string,
  config: CfDistributionConfig,
  etag: string,
): Promise<CfDistribution> {
  return await invoke<CfDistribution>('cf_update_distribution', { id, distId, config, etag });
}

export async function cfDeleteDistribution(id: string, distId: string, etag: string): Promise<void> {
  await invoke('cf_delete_distribution', { id, distId, etag });
}

export async function cfCreateInvalidation(id: string, distId: string, paths: string[]): Promise<CfInvalidation> {
  return await invoke<CfInvalidation>('cf_create_invalidation', { id, distId, paths });
}

export async function cfListInvalidations(id: string, distId: string): Promise<CfInvalidation[]> {
  return await invoke<CfInvalidation[]>('cf_list_invalidations', { id, distId });
}
