import type { S3ObjectVersion } from '#lib/types/index.js';

/** Sum stored payload bytes for one exact object key, not billing overhead. */
export function summarizeVersionSizes(versions: readonly S3ObjectVersion[]) {
  let currentBytes = 0n;
  let previousBytes = 0n;
  let dataVersionCount = 0;
  let previousVersionCount = 0;
  let deleteMarkerCount = 0;

  for (const version of versions) {
    if (version.is_delete_marker) {
      deleteMarkerCount++;
      continue;
    }
    const bytes = BigInt(version.size);
    dataVersionCount++;
    if (version.is_latest) {
      currentBytes += bytes;
    } else {
      previousBytes += bytes;
      previousVersionCount++;
    }
  }

  return {
    currentBytes,
    previousBytes,
    totalBytes: currentBytes + previousBytes,
    dataVersionCount,
    previousVersionCount,
    deleteMarkerCount,
  };
}
