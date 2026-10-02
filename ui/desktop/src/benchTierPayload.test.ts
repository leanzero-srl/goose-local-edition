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
    expect(DEFAULT_BENCHMARK_NAME).toBe('SB7.2 payments');
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
    for (const flag of named) expect(flag).toMatch(/^--sb\d+$/);
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
    'Update Goose to run the latest stable benchmark (sb-7.1). This app bundles sb-7.2.'
  );
});
