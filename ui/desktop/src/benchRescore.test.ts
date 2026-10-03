import { describe, expect, it } from 'vitest';
import {
  publishRunMeta,
  publishedIsCurrent,
  rescoreEligibility,
  retryScoringEligibility,
} from './benchRescore';
import type { BenchSessionRow } from './benchSessions';
const row: BenchSessionRow = {
  runId: 'cloud-test',
  scorerVersion: 'sb-7.1',
  startedAt: '2026-09-20T20:00:00Z',
  outcome: 'did_not_finish',
};
const receipt = {
  schemaVersion: 1,
  scorerVersion: 'sb-7.1',
  runId: row.runId,
  startedAt: row.startedAt,
  agent: { exit: 0, timed_out: false, secs: 498.2 },
  sourceInventory: { 'app.py': 'hash' },
  contracts: { 'spec.md': 'hash' },
  fixture_seed: '0123456789abcdef',
  vendor_port: 8850,
};
it('offers scoring retry only for this completed failed session', () => {
  expect(retryScoringEligibility(row, receipt)).toEqual({ ready: true });
  for (const candidate of [
    null,
    { usage: { status: 'recorded' } },
    { ...receipt, runId: 'other' },
    { ...receipt, agent: { ...receipt.agent, exit: 1 } },
    { ...receipt, startedAt: 'other' },
  ])
    expect(retryScoringEligibility(row, candidate).ready).toBe(false);
  expect(retryScoringEligibility({ ...row, outcome: 'running' }, receipt).ready).toBe(false);
  expect(retryScoringEligibility({ ...row, outcome: 'finished' }, receipt).ready).toBe(false);
});
it('offers the same retry to an SB7.2 session with its own SB7.2 receipt, never across tiers', () => {
  const sb72 = { ...row, scorerVersion: 'sb-7.2' };
  expect(retryScoringEligibility(sb72, { ...receipt, scorerVersion: 'sb-7.2' })).toEqual({
    ready: true,
  });
  // A receipt from the other payments tier proves nothing about this session.
  expect(retryScoringEligibility(sb72, receipt).ready).toBe(false);
  expect(retryScoringEligibility(row, { ...receipt, scorerVersion: 'sb-7.2' }).ready).toBe(false);
  for (const [scorerVersion, shown] of [
    ['sb-7.2-rc', 'Gauntlet 7.2 rc'],
    ['sb-7.0-rc', 'Gauntlet 7.0 rc'],
    ['sb-8.0-rc', 'Gauntlet 8.0 rc'],
  ])
    expect(retryScoringEligibility({ ...row, scorerVersion }, receipt).reason).toBe(
      `Only Gauntlet 7.1, Gauntlet 7.2 and Forge 1.0 runs can be rescored; this run is ${shown}.`
    );
});
it('keeps legacy missing completion evidence explicit rather than using usage or transcript prose', () => {
  expect(retryScoringEligibility(row, null).reason).toContain('No completed-build receipt');
  expect(retryScoringEligibility({ ...row, scorerVersion: 'sb-7.1-rc' }, receipt).ready).toBe(
    false
  );
});

it('offers the same retry to a Forge session with its own forge-1.0 receipt — never an rc identity', () => {
  const forgeRow = { ...row, scorerVersion: 'forge-1.0' };
  const forgeReceipt = { ...receipt, scorerVersion: 'forge-1.0' };
  expect(retryScoringEligibility(forgeRow, forgeReceipt)).toEqual({ ready: true });
  // A Gauntlet receipt proves nothing about a Forge session, and the reverse.
  expect(retryScoringEligibility(forgeRow, receipt).ready).toBe(false);
  expect(retryScoringEligibility(row, forgeReceipt).ready).toBe(false);
  expect(
    retryScoringEligibility({ ...row, scorerVersion: 'forge-1.0-rc' }, forgeReceipt).reason
  ).toBe(
    'Only Gauntlet 7.1, Gauntlet 7.2 and Forge 1.0 runs can be rescored; this run is Forge 1.0 rc.'
  );
});

it('re-scores a FINISHED Forge run only for a missing clip — its rc row against its forge-1.0 receipt', () => {
  const finished = { ...row, scorerVersion: 'forge-1.0-rc', outcome: 'finished' as const };
  const forgeReceipt = { ...receipt, scorerVersion: 'forge-1.0' };
  expect(retryScoringEligibility(finished, forgeReceipt, true)).toEqual({ ready: true });
  // Without the clip fact a finished rc row is no rescorable identity at all.
  expect(retryScoringEligibility(finished, forgeReceipt).reason).toBe(
    'Only Gauntlet 7.1, Gauntlet 7.2 and Forge 1.0 runs can be rescored; this run is Forge 1.0 rc.'
  );
  // The exemption is Forge's alone: a finished Gauntlet run is never re-scored this way.
  expect(
    retryScoringEligibility({ ...row, outcome: 'finished', scorerVersion: 'sb-7.2' }, receipt, true)
      .ready
  ).toBe(false);
});

describe('Re-score a FINISHED run (owner 2026-10-03: rescore all runs and republish)', () => {
  const finished = { ...row, outcome: 'finished' as const, score: 0.42 };

  it('is offered for a finished run with its own completed-build receipt', () => {
    expect(rescoreEligibility(finished, receipt)).toEqual({ ready: true });
    expect(
      rescoreEligibility(
        { ...finished, scorerVersion: 'sb-7.2' },
        { ...receipt, scorerVersion: 'sb-7.2' }
      )
    ).toEqual({ ready: true });
  });

  it('is not offered without a receipt that proves THIS build completed', () => {
    expect(rescoreEligibility(finished, null)).toEqual({
      ready: false,
      reason: 'No completed-build receipt was recorded for this run.',
    });
    for (const candidate of [
      { ...receipt, runId: 'other' },
      { ...receipt, startedAt: 'other' },
      { ...receipt, agent: { ...receipt.agent, timed_out: true } },
      { ...receipt, scorerVersion: 'sb-7.2' },
    ])
      expect(rescoreEligibility(finished, candidate).ready).toBe(false);
    expect(rescoreEligibility(finished, { ...receipt, fixture_seed: 'short' }).reason).toBe(
      'The original scoring inputs are incomplete.'
    );
  });

  it('is held while the run is in progress, and never offered for a run that has no score', () => {
    expect(rescoreEligibility({ ...finished, outcome: 'running' }, receipt)).toEqual({
      ready: false,
      reason: 'This run is in progress — re-score it once it has ended.',
    });
    for (const outcome of ['did_not_finish', 'did_not_start'] as const)
      expect(rescoreEligibility({ ...finished, outcome }, receipt).ready).toBe(false);
  });

  it('re-grades a finished Forge rc row against its forge-1.0 receipt, never a Gauntlet one', () => {
    const forgeRow = { ...finished, scorerVersion: 'forge-1.0-rc' };
    const forgeReceipt = { ...receipt, scorerVersion: 'forge-1.0' };
    expect(rescoreEligibility(forgeRow, forgeReceipt)).toEqual({ ready: true });
    expect(rescoreEligibility(forgeRow, receipt).ready).toBe(false);
    // An rc Gauntlet identity is no rescorable tier.
    expect(rescoreEligibility({ ...finished, scorerVersion: 'sb-7.2-rc' }, receipt).reason).toBe(
      'Only Gauntlet 7.1, Gauntlet 7.2 and Forge 1.0 runs can be re-scored; this run is Gauntlet 7.2 rc.'
    );
  });
});

describe('publish runMeta — the same build, recognised on republish (both families)', () => {
  const meta = {
    startedAt: '2026-10-01T09:00:00.000Z',
    finishedAt: '2026-10-01T09:40:00.000Z',
    engineEvents: 12,
    repairRounds: 1,
  };
  for (const scorerVersion of ['sb-7.2', 'forge-1.0']) {
    it(`${scorerVersion}: carries buildId always and rescoredAt only for a re-scored result`, () => {
      const stored = { scorerVersion, runId: 'cloud-0b8d6f0e', runMeta: meta };
      expect(publishRunMeta(stored)).toEqual({ ...meta, buildId: 'cloud-0b8d6f0e' });
      const rescoredAt = '2026-10-03T10:00:00.000Z';
      expect(publishRunMeta({ ...stored, runMeta: { ...meta, rescoredAt } })).toEqual({
        ...meta,
        buildId: 'cloud-0b8d6f0e',
        rescoredAt,
      });
    });
  }

  it('is key-by-key: a stored stray never reaches the strict allowlist; no run id, no buildId', () => {
    expect(publishRunMeta({ runId: null, runMeta: { ...meta, workdir: '/Users/me/run' } })).toEqual(
      meta
    );
    expect(publishRunMeta({ runId: 'cloud-x' })).toBeNull();
  });
});

describe('a posted run is publishable again only once re-scored after the post', () => {
  const posted = { publishedAt: '2026-10-01T10:00:00.000Z' };
  it('decides from the post time and the re-score time', () => {
    expect(publishedIsCurrent(undefined, null)).toBe(false);
    expect(publishedIsCurrent(posted, null)).toBe(true);
    expect(publishedIsCurrent(posted, '2026-10-01T09:59:00.000Z')).toBe(true);
    expect(publishedIsCurrent(posted, '2026-10-03T10:00:00.000Z')).toBe(false);
    // A run matched on the board has no known post time: a re-score is newer than what it shows.
    expect(publishedIsCurrent({ publishedAt: null }, '2026-10-03T10:00:00.000Z')).toBe(false);
    expect(publishedIsCurrent({ publishedAt: null }, null)).toBe(true);
  });
});
