import { errorText } from './error-report.ts';

export interface ListingProgress {
  status: 'loading' | 'complete' | 'cancelled' | 'error';
  pages: number;
  requests: number;
  items: number;
  elapsedMs: number;
  updatedAt: number;
  pending: boolean;
  error: string | null;
  source?: 'network' | 'cache';
  cachedAt?: number;
  fetchedAt?: number;
  cacheRevision?: number;
  cacheStored?: boolean;
}

interface Item { path: string; name: string }
interface Page<T> { entries: T[]; next_token: string | null }

export function listingPosition<T extends Item>(entries: T[], cursorPath?: string, anchorPath?: string, focusName?: string) {
  const focused = focusName ? entries.findIndex(entry => entry.name === focusName) : -1;
  const cursor = entries.findIndex(entry => entry.path === cursorPath);
  const cursorIndex = focused >= 0 ? focused : Math.max(0, cursor);
  const anchor = entries.findIndex(entry => entry.path === anchorPath);
  return { cursorIndex, selectionAnchor: anchor >= 0 ? anchor : cursorIndex, focused: focused >= 0 };
}

/** One request at a time. Cancel/navigation discard any in-flight response.
 * Retry resumes the failed page; it never replays already committed pages. */
export class ProgressiveListing<T extends Item> {
  private active = true;
  private generation = 0;
  private busy = false;
  private token: string | null = null;
  private tokens = new Set<string>();
  private paths = new Set<string>();
  private fetchPage: (token: string | null) => Promise<Page<T>>;
  private onPage: (entries: T[]) => void;
  private onProgress: (progress: ListingProgress) => void;
  private progress: ListingProgress = {
    status: 'loading', pages: 0, requests: 0, items: 0,
    elapsedMs: 0, updatedAt: Date.now(), pending: false, error: null,
  };

  constructor(
    fetchPage: (token: string | null) => Promise<Page<T>>,
    onPage: (entries: T[]) => void,
    onProgress: (progress: ListingProgress) => void,
  ) {
    this.fetchPage = fetchPage;
    this.onPage = onPage;
    this.onProgress = onProgress;
  }

  private update(change: Partial<ListingProgress>) {
    const now = Date.now();
    const elapsedMs = this.progress.elapsedMs + (this.progress.status === 'loading' ? Math.max(0, now - this.progress.updatedAt) : 0);
    this.progress = { ...this.progress, elapsedMs, updatedAt: now, ...change };
    if (this.active) this.onProgress(this.progress);
  }

  cancel() {
    this.generation++;
    this.update({ status: 'cancelled', error: null });
  }

  dispose() {
    this.active = false;
    this.generation++;
  }

  async start() {
    if (!this.active || this.busy || this.progress.status === 'complete') return;
    this.busy = true;
    const generation = this.generation;
    this.update({ status: 'loading', error: null, pending: true });
    try {
      while (this.active && generation === this.generation) {
        this.update({ requests: this.progress.requests + 1 });
        const page = await this.fetchPage(this.token);
        if (!this.active || generation !== this.generation) return;
        if (page.next_token !== null && (!page.next_token || this.tokens.has(page.next_token))) {
          throw new Error('Incomplete listing: repeated or empty continuation token. Refresh the folder.');
        }
        const fresh = page.entries.filter(entry => {
          if (this.paths.has(entry.path)) return false;
          this.paths.add(entry.path);
          return true;
        });
        this.onPage(fresh);
        this.token = page.next_token;
        if (this.token !== null) this.tokens.add(this.token);
        this.update({
          pages: this.progress.pages + 1,
          items: this.progress.items + fresh.filter(entry => entry.name !== '..').length,
          status: this.token === null ? 'complete' : 'loading',
        });
        if (this.token === null) return;
        // Yield between pages so rendering, navigation and Cancel can run.
        await new Promise(resolve => setTimeout(resolve, 0));
      }
    } catch (error) {
      if (this.active && generation === this.generation) {
        this.update({ status: 'error', error: `Listing page ${this.progress.pages + 1} failed\n${errorText(error)}` });
      }
    } finally {
      this.busy = false;
      if (this.active) this.update({ pending: false });
    }
  }
}
