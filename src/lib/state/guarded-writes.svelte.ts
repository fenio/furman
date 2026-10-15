import type { WriteSelection } from '#lib/services/s3.js';

export interface GuardedWriteRequest {
  sourceId: string | null;
  destinationId: string;
  selections: WriteSelection[];
  prefix: string;
  destination: string;
  verify: boolean;
  /** Only run follow-up deletion after every selected write succeeded. */
  onComplete?: () => Promise<void>;
}

export const guardedWrites = $state<{ request: GuardedWriteRequest | null }>({ request: null });

export function requestGuardedWrite(request: GuardedWriteRequest) {
  if (guardedWrites.request) throw new Error('A write preview is already open. Close it before preparing another.');
  guardedWrites.request = request;
}
