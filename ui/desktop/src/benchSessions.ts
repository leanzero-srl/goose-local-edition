/**
 * Benchmark session index — the PURE logic over the rows main.ts persists to
 * `~/.config/goose/benchmark/sessions.json`. File IO, slot inspection and the IPC handlers live in
 * main.ts; everything here is a plain function over data so the rules are unit-testable outside
 * Electron.
 *
 * The truth-layer contract these rules serve (Mihai 2026-08-31, "sessions with honest states"):
 * - 'running' is asserted only by the live process (`activeBenchRun`), NEVER by the index alone —
 *   a crash leaves a 'running' row behind, and the reader re-derives the honest outcome from where
 *   the run's data actually lives, then stamps the correction back.
 * - A session's data lives in the live-run slot (`runs/build/swarm-<n>node-r0`, `slot: true`) until
 *   the NEXT launch archives it to `sessions/<runId>/`. The slot stays in place between runs on
 *   purpose: the loop-state scripts read `runs/build/swarm-3node-r0` as THE live-run location.
 */

export type BenchSessionOutcome = 'running' | 'finished' | 'did_not_finish' | 'did_not_start';

export interface BenchSessionRow {
  /** null until `<slot>/.swarm/current-run.json` appears — the engine writes it before any phase. */
  runId: string | null;
  scorerVersion: string;
  startedAt: string;
  endedAt?: string;
  outcome: BenchSessionOutcome;
  score?: number;
  tiers?: Record<string, number>;
  nodes?: number;
  /** Data still lives in the live-run slot, not yet moved to sessions/<runId>. */
  slot?: boolean;
  slotDir?: string;
}

/** The one rule for what a slot's contents testify: a verdict is a finished run; engine events
 *  without a verdict are a run that started and died; neither is a launch that never reached OPEN. */
export const outcomeFromSlot = (hasVerdict: boolean, hasRunEvents: boolean): BenchSessionOutcome =>
  hasVerdict ? 'finished' : hasRunEvents ? 'did_not_finish' : 'did_not_start';

/** One launch is named by the pair minted in the benchmark-run handler: its startedAt ISO and the
 *  slot it launched into. runId cannot name a launch — it is unknown until the engine starts. */
export interface BenchLaunchKey {
  startedAt: string;
  slotDir: string;
}

export const findLaunchRow = (rows: BenchSessionRow[], key: BenchLaunchKey): number =>
  rows.findIndex((r) => r.startedAt === key.startedAt && r.slotDir === key.slotDir);

export interface ArchivedSlotFacts {
  runId: string;
  slotDir: string;
  startedAt: string;
  outcome: BenchSessionOutcome;
  endedAt: string;
  score?: number;
  tiers?: Record<string, number>;
  scorerVersion?: string;
}

/**
 * Fold an archived slot into the index. Matches the run's existing row by runId first (the
 * reconcile poll may have stamped it), then by the ONE row still marked `slot` for that slot dir —
 * every launch into a slot ends with exactly one such row, and archiving clears the mark. A slot
 * with no row at all (pre-sessions era) gets a fresh row from the slot's own facts. Fields the
 * close handler already stamped (score/tiers/scorerVersion/nodes) survive when the slot derivation
 * has nothing better.
 */
export const upsertArchivedRow = (
  rows: BenchSessionRow[],
  archived: ArchivedSlotFacts
): BenchSessionRow[] => {
  const idx = rows.findIndex(
    (r) =>
      (r.runId != null && r.runId === archived.runId) ||
      (r.slot === true && r.slotDir === archived.slotDir)
  );
  const prior = idx >= 0 ? rows[idx] : null;
  const merged: BenchSessionRow = {
    runId: archived.runId,
    scorerVersion: archived.scorerVersion ?? prior?.scorerVersion ?? 'unknown',
    startedAt: prior?.startedAt ?? archived.startedAt,
    endedAt: archived.endedAt,
    outcome: archived.outcome,
    ...(archived.score != null
      ? { score: archived.score }
      : prior?.score != null
        ? { score: prior.score }
        : {}),
    ...(archived.tiers ? { tiers: archived.tiers } : prior?.tiers ? { tiers: prior.tiers } : {}),
    ...(prior?.nodes != null ? { nodes: prior.nodes } : {}),
    slot: false,
  };
  const next = rows.slice();
  if (idx >= 0) next[idx] = merged;
  else next.push(merged);
  return next;
};

// ── Site catalog (leanzero.net) shapes ──────────────────────────────────────────────────────────

export interface BenchCatalogBaseline {
  label: string;
  score: number;
  model: string;
  title?: string;
  url?: string;
}

export interface BenchCatalogBenchmark {
  scorerVersion: string;
  title: string;
  current: boolean;
  frozen: boolean;
  baselines: BenchCatalogBaseline[];
}

/** Non-null when the site's CURRENT benchmark is not the one this app bundles — the run still
 *  launches the bundled newest (the app cannot run a spec it does not ship), and the view says
 *  "the site's current benchmark needs an app update". */
export const catalogMismatchOf = (
  benchmarks: BenchCatalogBenchmark[] | null | undefined,
  bundledScorer: string
): { siteCurrent: string; bundled: string } | null => {
  const current = (benchmarks ?? []).find((b) => b?.current === true);
  if (!current || typeof current.scorerVersion !== 'string') return null;
  if (current.scorerVersion === bundledScorer) return null;
  return { siteCurrent: current.scorerVersion, bundled: bundledScorer };
};

/** The client-side half of the server's frozen gate: the same refusal shape the server returns,
 *  produced from the cached catalog without burning the POST. The server stays the authority —
 *  it refuses too — this only saves the round trip and keeps the message identical offline. */
export const frozenPublishRefusal = (
  benchmarks: BenchCatalogBenchmark[] | null | undefined,
  scorerVersion: string
): { ok: false; status: 'error'; message: string; error: string } | null => {
  if (!scorerVersion) return null;
  const hit = (benchmarks ?? []).find(
    (b) => b?.frozen === true && b.scorerVersion === scorerVersion
  );
  if (!hit) return null;
  const message = `benchmark ${hit.title || hit.scorerVersion} is frozen — submissions closed`;
  return { ok: false, status: 'error', message, error: message };
};
