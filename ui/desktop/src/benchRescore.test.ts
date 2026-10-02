import { expect, it } from 'vitest';
import { retryScoringEligibility } from './benchRescore';
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
      `Only Gauntlet 7.1 and Gauntlet 7.2 runs can be rescored; this run is ${shown}.`
    );
});
it('keeps legacy missing completion evidence explicit rather than using usage or transcript prose', () => {
  expect(retryScoringEligibility(row, null).reason).toContain('No completed-build receipt');
  expect(retryScoringEligibility({ ...row, scorerVersion: 'sb-7.1-rc' }, receipt).ready).toBe(
    false
  );
});
