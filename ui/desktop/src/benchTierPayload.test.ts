import { describe, expect, it } from 'vitest';
import {
  BENCH_SPEC_FILE,
  BENCH_RENDER_PROBE,
  BENCH_RUN_FLAG,
  DEFAULT_BENCHMARK_NAME,
  defaultBenchmarkTier,
  defaultBenchmarkScorer,
} from './benchTierPayload';
import {
  ISOLATED_PAYMENTS_TIERS,
  TIERS,
  isolatedPaymentsTier,
} from './components/benchmark/baselines';

describe('every benchmark tier carries its OWN spec and probe', () => {
  /** THE DEFECT THIS EXISTS FOR. The mapping was a ternary with no sb-7 branch, and BENCH_SPEC overrides
   *  the --sb7 flag inside run_build.build_prompt. So selecting sb-7 ran the sb-5 spec: a 6,278-character
   *  "# Build `vendorsync`" instead of 54,146 characters of Meridian. The run looked healthy throughout.
   *  A ternary cannot be checked for completeness; a Record keyed by BenchTier can. */
  it('maps every tier — a missing one would silently run another tier’s spec', () => {
    for (const t of TIERS) {
      expect(BENCH_SPEC_FILE[t], `no spec for ${t}`).toBeTruthy();
      expect(BENCH_RENDER_PROBE[t], `no probe for ${t}`).toBeTruthy();
    }
  });

  it('gives each tier a DISTINCT spec and probe', () => {
    const specs = TIERS.map((t) => BENCH_SPEC_FILE[t]);
    const probes = TIERS.map((t) => BENCH_RENDER_PROBE[t]);
    expect(new Set(specs).size, `two tiers share a spec: ${specs.join(', ')}`).toBe(TIERS.length);
    expect(new Set(probes).size, `two tiers share a probe: ${probes.join(', ')}`).toBe(
      TIERS.length
    );
  });

  it('sb-7 gets the Meridian spec and the v3 probe, not VendorSync’s', () => {
    expect(BENCH_SPEC_FILE['sb-7']).toBe('spec-build-sb7.md');
    expect(BENCH_RENDER_PROBE['sb-7']).toBe('product_probe_v3.mjs');
  });

  it('launches stable SB7.2 for local and cloud entrants while retaining SB7.1 and the SB8 experiment payload', () => {
    expect(defaultBenchmarkTier()).toBe('sb-7.2');
    expect(BENCH_SPEC_FILE[defaultBenchmarkTier()]).toBe('spec-build-sb72.md');
    expect(BENCH_RENDER_PROBE[defaultBenchmarkTier()]).toBe('product_probe_sb72.mjs');
    expect(BENCH_RUN_FLAG[defaultBenchmarkTier()]).toBe('--sb72');
    expect(defaultBenchmarkScorer()).toBe('sb-7.2');
    // Visible name (owner 2026-10-03: the SB family reads "Gauntlet"); the ids above stay sb-*.
    expect(DEFAULT_BENCHMARK_NAME).toBe('Gauntlet 7.2 · payments');
    expect(TIERS).toContain('sb-7.1');
    expect(BENCH_SPEC_FILE['sb-7.1']).toBe('spec-build-sb71.md');
    expect(BENCH_RENDER_PROBE['sb-7.1']).toBe('product_probe_sb71.mjs');
    expect(TIERS).toContain('sb-8');
    expect(BENCH_SPEC_FILE['sb-8']).toBe('spec-build-sb8.md');
  });

  /** run_build.py regime flags: a tier missing from the old boolean chain launched with NO flag. */
  it('gives every tier a distinct run_build regime flag, sb-5 alone running flagless', () => {
    const flags = TIERS.map((t) => BENCH_RUN_FLAG[t]);
    expect(flags.filter((f) => f === null)).toHaveLength(1);
    expect(BENCH_RUN_FLAG['sb-5.3']).toBeNull();
    const named = flags.filter((f): f is string => f !== null);
    expect(new Set(named).size).toBe(named.length);
    for (const flag of named) expect(flag).toMatch(/^--(?:sb\d+|forge)$/);
    expect(BENCH_RUN_FLAG['forge-1.0']).toBe('--forge');
    expect(BENCH_RUN_FLAG['sb-7.1']).toBe('--sb71');
  });

  it('runs the stable tier on the isolated payments path SB7.1 introduced', () => {
    expect(ISOLATED_PAYMENTS_TIERS).toEqual(['sb-7.1', 'sb-7.2']);
    expect(isolatedPaymentsTier(defaultBenchmarkScorer())).toBe(defaultBenchmarkTier());
    expect(isolatedPaymentsTier('sb-7.1-rc')).toBe('sb-7.1');
    expect(isolatedPaymentsTier('sb-7.2')).toBe('sb-7.2');
    for (const other of ['sb-7.0-rc', 'sb-8.0-rc', 'sb-6.0', '', undefined, 'sb-7.10', 'sb-7.12'])
      expect(isolatedPaymentsTier(other)).toBeNull();
  });
});

import { benchmarkLaunchTier, benchmarkScorer } from './benchTierPayload';
it('uses stable SB7.2 identity and exact payload while retaining SB7.1 and legacy SB7 as history', () => {
  expect(benchmarkLaunchTier()).toBe('sb-7.2');
  expect(benchmarkLaunchTier({ tier: 'sb-7.2' })).toBe('sb-7.2');
  expect(benchmarkScorer(benchmarkLaunchTier({ tier: 'sb-7.2' }))).toBe('sb-7.2');
  expect(BENCH_SPEC_FILE[benchmarkLaunchTier({ tier: 'sb-7.2' })]).toBe('spec-build-sb72.md');
  expect(BENCH_RENDER_PROBE[benchmarkLaunchTier({ tier: 'sb-7.2' })]).toBe(
    'product_probe_sb72.mjs'
  );
  expect(benchmarkScorer('sb-7.1')).toBe('sb-7.1');
  expect(() => benchmarkLaunchTier({ tier: 'sb-7.1' })).toThrow('Only the latest stable');
  expect(() => benchmarkLaunchTier({ tier: 'sb-8' as 'sb-7' })).toThrow('Only the latest stable');
});

import { benchmarkLaunchProblem } from './benchTierPayload';
const stable = {
  scorerVersion: 'sb-7.2',
  title: 'SB7.2 payments',
  current: true,
  frozen: false,
  baselines: [],
};
it('refuses legacy, experimental and missing tier overrides at the launch boundary', () => {
  for (const tier of ['sb-7', 'sb-7.1', 'sb-8', 'sb-7.1-rc', 'sb-7.2-rc', '', undefined])
    expect(() => benchmarkLaunchTier({ tier } as never)).toThrow('Only the latest stable');
});
it('allows only one fresh available current stable release matching the bundled scorer', () => {
  expect(benchmarkLaunchProblem([stable])).toBeNull();
  expect(
    benchmarkLaunchProblem([stable, { ...stable, current: false, scorerVersion: 'sb-8.0-rc' }])
  ).toBeNull();
  for (const rows of [
    undefined,
    [],
    [stable, stable],
    [{ ...stable, frozen: true }],
    [{ ...stable, scorerVersion: 'sb-7.1-rc' }],
    [{ ...stable, scorerVersion: 'sb-7.2-rc' }],
    // The site still on SB7.1 while this app bundles SB7.2: refused with the update/bundle sentence.
    [{ ...stable, scorerVersion: 'sb-7.1' }],
    [{ ...stable, scorerVersion: 'sb-8.0' }],
  ])
    expect(benchmarkLaunchProblem(rows)).toBeTruthy();
  expect(benchmarkLaunchProblem([{ ...stable, frozen: undefined } as never])).toBeTruthy();
  expect(benchmarkLaunchProblem([stable], true)).toMatch(/Connect/);
  expect(benchmarkLaunchProblem([stable], false, 'sb-7.0-rc')).toMatch(/history only/);
  // An SB7.1 session cannot be re-scored once SB7.2 is the stable release.
  expect(benchmarkLaunchProblem([stable], false, 'sb-7.1')).toMatch(/history only/);
  expect(benchmarkLaunchProblem([{ ...stable, scorerVersion: 'sb-7.1' }])).toBe(
    'Update Goose to run the latest stable benchmark (Gauntlet 7.1). This app bundles Gauntlet 7.2.'
  );
});

import { BENCH_FAMILY_DEFAULT } from './benchTierPayload';
describe('the Forge family has its own bundled era and its own launch gate (forge/INTEGRATION.md)', () => {
  // The site's corrected shape (INTEGRATION.md 2026-10-03): `familyCurrent` per family, the legacy
  // `current` only on the SB entry so shipped apps still find exactly one sb- current.
  const forge = {
    scorerVersion: 'forge-1.0',
    title: 'Forge 1.0 — Scope Ledger',
    family: 'forge',
    familyCurrent: true,
    current: false,
    frozen: false,
    baselines: [],
  };
  it('launches forge-1.0 through run_build --forge with its own spec and probe', () => {
    expect(BENCH_FAMILY_DEFAULT).toEqual({ sb: 'sb-7.2', forge: 'forge-1.0' });
    expect(defaultBenchmarkTier('forge')).toBe('forge-1.0');
    expect(defaultBenchmarkScorer('forge')).toBe('forge-1.0');
    expect(BENCH_SPEC_FILE['forge-1.0']).toBe('forge/public/spec-build-forge.md');
    expect(BENCH_RENDER_PROBE['forge-1.0']).toBe('forge_probe.mjs');
    expect(benchmarkLaunchTier({ tier: 'forge-1.0' })).toBe('forge-1.0');
  });
  it('checks the current era OF THE SELECTED FAMILY against that family’s bundled default', () => {
    // Two families, one current each: both launch, neither disturbs the other.
    expect(benchmarkLaunchProblem([stable, forge])).toBeNull();
    expect(benchmarkLaunchProblem([stable, forge], false, undefined, 'forge')).toBeNull();
    // A site that has not opened Forge yet: SB unchanged, Forge refused in words.
    expect(benchmarkLaunchProblem([stable])).toBeNull();
    expect(benchmarkLaunchProblem([stable], false, undefined, 'forge')).toMatch(
      /no current Forge benchmark/
    );
    // A newer Forge era on the site: update, naming the family's bundle.
    expect(
      benchmarkLaunchProblem(
        [stable, { ...forge, scorerVersion: 'forge-1.1' }],
        false,
        undefined,
        'forge'
      )
    ).toBe(
      'Update Goose to run the latest stable benchmark (Forge 1.1). This app bundles Forge 1.0.'
    );
    // An rc identity is never a runnable era; a frozen one is closed; two currents are ambiguous.
    for (const rows of [
      [stable, { ...forge, scorerVersion: 'forge-1.0-rc' }],
      [stable, { ...forge, frozen: true }],
      [stable, forge, { ...forge, scorerVersion: 'forge-1.1' }],
    ])
      expect(benchmarkLaunchProblem(rows, false, undefined, 'forge')).toMatch(
        /no single available Forge/
      );
    expect(benchmarkLaunchProblem([stable, forge], true, undefined, 'forge')).toMatch(/Connect/);
  });
  it('keeps the SB rule when the catalog carries no family at all (apps ≤ 3.0.88 shape)', () => {
    const { family: _ignored, ...unlabelled } = forge;
    // No family + a forge scorer is still Forge — never a second SB current.
    expect(benchmarkLaunchProblem([stable, unlabelled])).toBeNull();
    expect(benchmarkLaunchProblem([stable, unlabelled], false, undefined, 'forge')).toBeNull();
    // An explicit sb family on an SB row reads exactly as an unlabelled one.
    expect(benchmarkLaunchProblem([{ ...stable, family: 'sb' }])).toBeNull();
    // The site's full shape for SB (family + familyCurrent beside the legacy current) reads the same.
    expect(
      benchmarkLaunchProblem([{ ...stable, family: 'sb', familyCurrent: true }, forge])
    ).toBeNull();
  });
  it('reads familyCurrent per family — never the legacy current for Forge', () => {
    // A Forge entry that only carries the legacy flag is not a Forge current era.
    const legacyOnly = { ...forge, familyCurrent: undefined, current: true };
    expect(benchmarkLaunchProblem([stable, legacyOnly], false, undefined, 'forge')).toMatch(
      /no current Forge benchmark/
    );
    // familyCurrent false wins over a stray current: true.
    expect(
      benchmarkLaunchProblem(
        [stable, { ...forge, familyCurrent: false, current: true }],
        false,
        undefined,
        'forge'
      )
    ).toMatch(/no current Forge benchmark/);
    // SB's familyCurrent decides for SB when the site states it.
    expect(
      benchmarkLaunchProblem([{ ...stable, family: 'sb', familyCurrent: false }, forge])
    ).toMatch(/no single available stable benchmark/);
  });
});

import { BENCH_FAMILY_NAME } from './benchTierPayload';
import { eraDisplayName, eraLabel } from './components/benchmark/baselines';
it('names eras for people — Gauntlet and Forge — while every id stays sb-* / forge-*', () => {
  expect(BENCH_FAMILY_NAME).toEqual({
    sb: 'Gauntlet 7.2 · payments',
    forge: 'Forge 1.0 · Scope Ledger',
  });
  expect(eraDisplayName('sb-7.2')).toBe('Gauntlet 7.2');
  expect(eraDisplayName('sb-7.0-rc')).toBe('Gauntlet 7.0 rc');
  expect(eraDisplayName('forge-1.0-rc')).toBe('Forge 1.0 rc');
  expect(eraDisplayName('custom-era')).toBe('custom-era');
  expect(eraLabel('sb-7.2', 'SB7.2 payments')).toBe('Gauntlet 7.2 · payments');
  expect(eraLabel('sb-6.0', 'VendorSync Pro')).toBe('Gauntlet 6.0 · VendorSync Pro');
  expect(eraLabel('forge-1.0', 'Forge 1.0 — Scope Ledger')).toBe('Forge 1.0 · Scope Ledger');
  expect(eraLabel('sb-5.3', 'sb-5.3')).toBe('Gauntlet 5.3');
});
