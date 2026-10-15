import type { FileEntry } from '#lib/types/index.js';
import { NavigationCache } from '#lib/utils/navigation-cache.js';

// FileEntry is a flat record, with no credentials, payloads or nested metadata.
export const s3NavigationCache = new NavigationCache<FileEntry>();
export const navigationCacheState = $state({ revision: 0 });
export function clearS3NavigationCache() {
  s3NavigationCache.clear();
  navigationCacheState.revision = s3NavigationCache.revision;
}
export function beginS3Mutation() {
  const end = s3NavigationCache.beginMutation();
  navigationCacheState.revision = s3NavigationCache.revision;
  return () => { end(); navigationCacheState.revision = s3NavigationCache.revision; };
}
