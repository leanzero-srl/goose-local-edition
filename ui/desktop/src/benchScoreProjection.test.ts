import { describe, expect, it } from 'vitest';
import { projectBenchScore } from './benchScoreProjection';
import sb8 from './components/benchmark/sb8-failed.fixture.json';

describe('persisted benchmark score projection', () => {
  it('preserves SB8 scorer identity, numeric E tier and all composition inputs across JSON storage', () => {
    const stored = JSON.parse(JSON.stringify(projectBenchScore(sb8)));
    expect(stored.scorerVersion).toBe(sb8.scorerVersion);
    expect(stored.tiers).toEqual(sb8.tiers);
    expect(stored.verdict).toMatchObject({
      scorerVersion: sb8.scorerVersion,
      scoreInner: sb8.scoreInner,
      criticalMultiplier: sb8.criticalMultiplier,
      calibrated: false,
    });
    expect(stored.verdict.checks).toEqual(sb8.checks);
  });

  it('preserves legacy means and composition without manufacturing missing tiers', () => {
    const stored = projectBenchScore({
      scorer_version: 'sb-5.3',
      tiers: { A: { mean: 0.8 } },
      core: 0.5,
      hard: 0.2,
    });
    expect(stored.tiers).toEqual({ A: 0.8 });
    expect(stored.verdict).toMatchObject({ scorerVersion: 'sb-5.3', core: 0.5, hard: 0.2 });
    expect(stored.verdict).not.toHaveProperty('criticalMultiplier');
  });
});

import { recoverStoredSb8Score } from './benchScoreProjection';
import truncated from './components/benchmark/sb8-desktop-truncated.fixture.json';
import canonical from './components/benchmark/sb8-desktop-canonical.fixture.json';

describe('read-path recovery from canonical run evidence', () => {
  it('recovers the actual truncated desktop result and preserves its repair story without mutation', () => {
    const stored = globalThis.structuredClone(truncated);
    const original = globalThis.structuredClone(stored);
    const recovered = recoverStoredSb8Score(stored, canonical) as typeof stored & {
      verdict: { scoreInner: number; criticalMultiplier: number; scorerVersion: string };
    };
    expect(recovered.tiers).toEqual(canonical.tiers);
    expect(recovered.verdict).toMatchObject({
      scorerVersion: canonical.scorerVersion,
      scoreInner: canonical.scoreInner,
      criticalMultiplier: canonical.criticalMultiplier,
    });
    expect(recovered.verdict.findingsHeld).toEqual(stored.verdict.findingsHeld);
    expect(recovered.verdict.repairRounds).toEqual(stored.verdict.repairRounds);
    expect(stored).toEqual(original);
  });

  it.each([
    { scorerVersion: 'sb-7.0-rc' },
    { scorer_version: 'sb-7.0-rc' },
    { score: 0.123 },
    { model: 'another-model' },
    { model: undefined },
    { scoreInner: undefined },
    { criticalMultiplier: undefined },
    { tiers: { A: 0, B: 0, C: 0, D: 0 } },
  ])(
    'keeps the stored evidence unchanged for mismatched or incomplete canonical input %j',
    (patch) => {
      expect(recoverStoredSb8Score(truncated, { ...canonical, ...patch })).toBe(truncated);
    }
  );

  it('carries canonical provenance when available', () => {
    const provenance = {
      scorer_sha256: 'canonical-scorer-hash',
      spec_sha256: 'canonical-spec-hash',
    };
    expect(recoverStoredSb8Score(truncated, { ...canonical, provenance })).toMatchObject({
      verdict: { provenance },
    });
  });
});

it('retains the declared composition and requires its F tier during recovery', () => {
  const planning = {
    ...canonical,
    tiers: { ...canonical.tiers, F: 0 },
    weights: { A: 0.08, B: 0.17, C: 0.3, D: 0.15, E: 0.05, F: 0.25 },
    core_tiers: ['A', 'B', 'C', 'D', 'F'],
  };
  const projected = projectBenchScore(planning);
  expect(projected.verdict).toMatchObject({
    weights: planning.weights,
    core_tiers: planning.core_tiers,
  });
  expect(recoverStoredSb8Score(truncated, planning)).toMatchObject({
    tiers: { F: 0 },
    verdict: { weights: planning.weights, core_tiers: planning.core_tiers },
  });
  expect(recoverStoredSb8Score(truncated, { ...planning, tiers: canonical.tiers })).toBe(truncated);
});

import planningFixture from './components/benchmark/sb8-planning-failed.fixture.json';

it('preserves metadata from the actual planning scorer output across disk serialization', () => {
  const stored = JSON.parse(JSON.stringify(projectBenchScore(planningFixture)));
  expect(stored.tiers.F).toBe(0);
  expect(stored.verdict).toMatchObject({
    weights: planningFixture.weights,
    core_tiers: planningFixture.core_tiers,
    scorer_files_sha256: planningFixture.scorer_files_sha256,
  });
  expect(stored.verdict.scoreInner).toBe(0.75);
});
