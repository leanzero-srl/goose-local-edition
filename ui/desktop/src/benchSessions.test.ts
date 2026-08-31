import { describe, expect, it } from 'vitest';
import {
  outcomeFromSlot,
  findLaunchRow,
  upsertArchivedRow,
  catalogMismatchOf,
  frozenPublishRefusal,
  type BenchSessionRow,
  type BenchCatalogBenchmark,
} from './benchSessions';

const SLOT = '/ud/benchmark/runs/build/swarm-3node-r0';

const provisional = (over: Partial<BenchSessionRow> = {}): BenchSessionRow => ({
  runId: null,
  scorerVersion: 'sb-7.0-rc',
  startedAt: '2026-08-31T10:00:00.000Z',
  outcome: 'running',
  nodes: 3,
  slot: true,
  slotDir: SLOT,
  ...over,
});

describe('outcomeFromSlot — the one rule for what a data dir testifies', () => {
  it('verdict beats everything; events without a verdict did not finish; neither never started', () => {
    expect(outcomeFromSlot(true, true)).toBe('finished');
    expect(outcomeFromSlot(true, false)).toBe('finished');
    expect(outcomeFromSlot(false, true)).toBe('did_not_finish');
    expect(outcomeFromSlot(false, false)).toBe('did_not_start');
  });
});

describe('findLaunchRow — a launch is named by startedAt + slotDir, never runId', () => {
  it('finds the row minted for this launch and not a sibling', () => {
    const rows = [
      provisional({ startedAt: '2026-08-30T09:00:00.000Z', slot: false }),
      provisional(),
    ];
    expect(findLaunchRow(rows, { startedAt: '2026-08-31T10:00:00.000Z', slotDir: SLOT })).toBe(1);
    expect(findLaunchRow(rows, { startedAt: '2026-08-31T10:00:00.000Z', slotDir: '/elsewhere' })).toBe(-1);
  });
});

describe('upsertArchivedRow — archiving a slot folds into the index without duplicating', () => {
  it('reconciles the provisional running row (runId still null) into the archived row', () => {
    const rows = [provisional()];
    const next = upsertArchivedRow(rows, {
      runId: 'run-abc',
      slotDir: SLOT,
      startedAt: '2026-08-31T10:00:01.000Z',
      outcome: 'finished',
      endedAt: '2026-08-31T12:00:00.000Z',
      score: 0.61,
      tiers: { A: 1, B: 0.5, C: 0.4, D: 0.6 },
      scorerVersion: 'sb-7.0-rc',
    });
    expect(next).toHaveLength(1);
    expect(next[0].runId).toBe('run-abc');
    expect(next[0].outcome).toBe('finished');
    expect(next[0].score).toBe(0.61);
    // The handler's startedAt (the provisional row's) wins over the engine's later stamp.
    expect(next[0].startedAt).toBe('2026-08-31T10:00:00.000Z');
    // Archived data no longer lives in the slot.
    expect(next[0].slot).toBe(false);
    // The provisional row's nodes survives — the slot derivation cannot know it.
    expect(next[0].nodes).toBe(3);
  });

  it('matches by runId when the reconcile poll already stamped it', () => {
    const rows = [provisional({ runId: 'run-abc', outcome: 'finished', score: 0.7 })];
    const next = upsertArchivedRow(rows, {
      runId: 'run-abc',
      slotDir: SLOT,
      startedAt: '2026-08-31T10:00:01.000Z',
      outcome: 'finished',
      endedAt: '2026-08-31T12:00:00.000Z',
      // No score from the slot derivation — the close handler's stamped score must survive.
    });
    expect(next).toHaveLength(1);
    expect(next[0].score).toBe(0.7);
  });

  it('appends a fresh row for a pre-sessions-era slot with no row at all', () => {
    const next = upsertArchivedRow([], {
      runId: 'run-old',
      slotDir: SLOT,
      startedAt: '2026-08-29T08:00:00.000Z',
      outcome: 'did_not_finish',
      endedAt: '2026-08-29T09:00:00.000Z',
    });
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({
      runId: 'run-old',
      outcome: 'did_not_finish',
      scorerVersion: 'unknown',
      slot: false,
    });
  });

  it('never steals an ARCHIVED sibling row that shares the slot dir', () => {
    // Every 3-node run uses the same slot; only the one row still marked slot:true owns it.
    const archived = provisional({ runId: 'run-old', outcome: 'finished', slot: false });
    const current = provisional({ startedAt: '2026-08-31T11:00:00.000Z' });
    const next = upsertArchivedRow([archived, current], {
      runId: 'run-new',
      slotDir: SLOT,
      startedAt: '2026-08-31T11:00:05.000Z',
      outcome: 'did_not_finish',
      endedAt: '2026-08-31T11:30:00.000Z',
    });
    expect(next).toHaveLength(2);
    expect(next[0].runId).toBe('run-old');
    expect(next[0].outcome).toBe('finished');
    expect(next[1].runId).toBe('run-new');
    expect(next[1].outcome).toBe('did_not_finish');
  });
});

const CATALOG: BenchCatalogBenchmark[] = [
  {
    scorerVersion: 'sb-6.0',
    title: 'VendorSync Pro',
    current: false,
    frozen: true,
    baselines: [{ label: 'Claude Opus 5', score: 0.91, model: 'claude-opus-5' }],
  },
  {
    scorerVersion: 'sb-7.0-rc',
    title: 'Meridian Payments Console',
    current: true,
    frozen: false,
    baselines: [],
  },
];

describe('catalogMismatchOf — the "site needs an app update" signal', () => {
  it('is null when the site current matches the bundled scorer', () => {
    expect(catalogMismatchOf(CATALOG, 'sb-7.0-rc')).toBeNull();
  });

  it('names both sides when they differ — the run still launches the bundled newest', () => {
    expect(catalogMismatchOf(CATALOG, 'sb-6.0')).toEqual({
      siteCurrent: 'sb-7.0-rc',
      bundled: 'sb-6.0',
    });
  });

  it('is null with no catalog or no current entry — absence is not a mismatch claim', () => {
    expect(catalogMismatchOf(null, 'sb-7.0-rc')).toBeNull();
    expect(catalogMismatchOf([], 'sb-7.0-rc')).toBeNull();
    expect(
      catalogMismatchOf(
        CATALOG.map((b) => ({ ...b, current: false })),
        'sb-6.0'
      )
    ).toBeNull();
  });
});

describe('frozenPublishRefusal — the server-shaped refusal, without burning the POST', () => {
  it('refuses a frozen benchmark with the server error shape', () => {
    const refusal = frozenPublishRefusal(CATALOG, 'sb-6.0');
    expect(refusal).not.toBeNull();
    expect(refusal).toMatchObject({ ok: false, status: 'error' });
    expect(refusal?.message).toBe('benchmark VendorSync Pro is frozen — submissions closed');
    // `error` mirrors `message` so the existing publish-error rendering shows it unchanged.
    expect(refusal?.error).toBe(refusal?.message);
  });

  it('lets an open benchmark, an unknown scorer and a missing catalog through to the server', () => {
    expect(frozenPublishRefusal(CATALOG, 'sb-7.0-rc')).toBeNull();
    expect(frozenPublishRefusal(CATALOG, 'sb-9.9')).toBeNull();
    expect(frozenPublishRefusal(null, 'sb-6.0')).toBeNull();
    expect(frozenPublishRefusal(CATALOG, '')).toBeNull();
  });
});
