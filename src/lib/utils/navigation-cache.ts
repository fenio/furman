/** Display-only complete listings. Never used by native operation preflights.
 * Keys include a live connection identity; invalidation advances the revision
 * so a pending older listing cannot repopulate the cache after a write. */
export class NavigationCache<T extends object> {
  private views = new Map<string, { entries: T[]; savedAt: number; weight: number }>();
  private entries = 0;
  private weight = 0;
  private mutations = 0;
  revision = 0;
  readonly limits: Readonly<{ views: number; entries: number; weight: number; ttlMs: number }>;
  private clock: () => number;
  constructor(limits = { views: 32, entries: 50_000, weight: 16 * 1024 * 1024, ttlMs: 60_000 }, clock = Date.now) {
    this.limits = Object.freeze({ ...limits }); this.clock = clock;
  }

  private remove(key: string) {
    const value = this.views.get(key);
    if (!value) return;
    this.entries -= value.entries.length;
    this.weight -= value.weight;
    this.views.delete(key);
  }
  private prune() {
    const now = this.clock();
    for (const [key, view] of this.views) if (now < view.savedAt || now - view.savedAt >= this.limits.ttlMs) this.remove(key);
  }
  clear() {
    this.views.clear(); this.entries = 0; this.weight = 0; this.revision++;
  }
  beginMutation() {
    this.mutations++; this.clear();
    let ended = false;
    return () => { if (!ended) { ended = true; this.mutations--; this.clear(); } };
  }
  get(key: string) {
    this.prune();
    if (this.mutations) return null;
    const view = this.views.get(key);
    if (!view) return null;
    this.views.delete(key); this.views.set(key, view); // LRU, without extending TTL.
    return { entries: view.entries.map(entry => ({ ...entry })), savedAt: view.savedAt };
  }
  put(key: string, entries: T[], revision: number) {
    if (revision !== this.revision || this.mutations || entries.length > this.limits.entries) return false;
    // Conservative UTF-16 string storage + per-row overhead estimate. This is
    // a cache admission budget, not a measurement of the JS engine's heap.
    let weight = key.length * 2;
    if (weight > this.limits.weight) return false;
    for (const entry of entries) {
      weight += 512;
      for (const value of Object.values(entry)) if (typeof value === 'string') weight += value.length * 2;
      if (weight > this.limits.weight) return false;
    }
    this.prune(); this.remove(key);
    while (this.views.size && (this.views.size >= this.limits.views || this.entries + entries.length > this.limits.entries || this.weight + weight > this.limits.weight)) this.remove(this.views.keys().next().value!);
    if (this.limits.views < 1) return false;
    this.views.set(key, { entries: entries.map(entry => ({ ...entry })), savedAt: this.clock(), weight });
    this.entries += entries.length; this.weight += weight;
    return true;
  }
  stats() {
    this.prune();
    return { views: this.views.size, entries: this.entries, estimatedBytes: this.weight, revision: this.revision, pendingMutations: this.mutations };
  }
}

/** Non-read commands default to invalidation, including newly added commands.
 * Unknown commands may sacrifice hit rate, but cannot silently retain stale views. */
export function mayChangeS3(command: string): boolean {
  return !/^(s3_(get_|head_|list_|download|search_|is_|check_|presign_|prepare_write$|discard_write$|set_|activity_)|oidc_|cloudfront_list_|cloudfront_get_)/.test(command);
}
