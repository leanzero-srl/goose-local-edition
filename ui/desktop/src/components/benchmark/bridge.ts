/**
 * Named types for the benchmark catalog/session IPC contract, as SHIPPED by main.ts/preload.ts
 * (802ce86d3). preload's ElectronAPI declares the same shapes inline; these named copies exist so
 * the view's components and tests can speak about a session or a catalog row without re-deriving
 * anonymous types. Structural typing keeps the two in lockstep — a divergence fails typecheck at
 * the `setSessions`/`setCatalog` call sites in BenchmarkView.
 */

/** One published board row for a benchmark era, retrieved from the site — never baked. */
export interface CatalogBaseline {
  label: string;
  score: number;
  model: string;
  title?: string;
  url?: string;
}

export interface CatalogBenchmark {
  scorerVersion: string;
  title: string;
  /** The one benchmark the app can run right now. */
  current: boolean;
  /** Frozen on the site — still viewable, no longer accepting submissions. */
  frozen: boolean;
  baselines: CatalogBaseline[];
}

export interface BenchmarkCatalogResult {
  ok?: boolean;
  fetchedAt?: string;
  /** True when this payload is a disk cache, not a fresh fetch — the view tags it "cached <date>". */
  stale?: boolean;
  benchmarks?: CatalogBenchmark[];
  error?: string;
  detail?: string;
}

export type SessionOutcome = 'running' | 'finished' | 'did_not_finish' | 'did_not_start';

export interface BenchSession {
  /** null while the engine has not yet written .swarm/current-run.json (reconciles ~2s after
   *  launch) — the view keys such a row by its startedAt and refuses to delete it. */
  runId: string | null;
  scorerVersion: string;
  startedAt: string;
  endedAt?: string;
  outcome: SessionOutcome;
  score?: number;
  tiers?: Record<string, number>;
  nodes?: number;
  /** finished AND the stored latest result AND its benchmark is not frozen per the cached catalog. */
  publishable: boolean;
}

/** The 'benchmark-started' payload's version skew fact: the site's current benchmark is newer
 *  than what this app bundles — runnable, but an app update is what catches the board up. */
export interface CatalogMismatch {
  siteCurrent: string;
  bundled: string;
}
