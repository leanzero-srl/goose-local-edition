/**
 * Which spec and which render probe each benchmark tier runs.
 *
 * WHY THIS IS A MODULE AND NOT A TERNARY IN main.ts. `benchmark-run` sets `BENCH_SPEC`, and
 * `run_build.build_prompt` reads it BEFORE the regime's own default:
 *
 *     spec_file = os.environ.get("BENCH_AMEND_SPEC","") or os.environ.get("BENCH_SPEC","")
 *
 * So this mapping OVERRIDES the `--sb7` / `--sb6` flag rather than agreeing with it, and a tier missing
 * from it does not fall back to its own spec — it silently runs someone else's.
 *
 * MEASURED 2026-08-28: the mapping was `sb6 ? 'spec-build-v3.md' : 'spec-build-v2.md'`, with no sb-7
 * branch. Selecting sb-7 in the UI passed `--sb7`, run_build set BENCH_SB7, `_regime()` correctly chose
 * spec-build-sb7.md — and build_prompt threw it away for the sb-5 spec. The run received 6,278 characters
 * beginning "# Build `vendorsync`" instead of 54,146 characters of Meridian Payments Console, and looked
 * entirely healthy doing it: nine balanced slices, coverage reporting nothing missing, clean judge
 * verdicts. The render probe had the identical gap and would have graded a Meridian console by
 * VendorSync's rules.
 *
 * A ternary cannot be checked for completeness. A record keyed by BenchTier can, and the test beside this
 * file asserts every tier in TIERS has a distinct spec and probe — so adding sb-8 and forgetting this
 * file fails the build instead of quietly running sb-7's spec.
 */
import { TIER_SCORER } from './components/benchmark/baselines';
import type { BenchCatalogBenchmark } from './benchSessions';
import type { BenchTier } from './components/benchmark/baselines';

export const BENCH_SPEC_FILE: Record<BenchTier, string> = {
  'sb-5.3': 'spec-build-v2.md',
  'sb-6': 'spec-build-v3.md',
  'sb-7': 'spec-build-sb7.md',
  'sb-7.1': 'spec-build-sb71.md',
  'sb-8': 'spec-build-sb8.md',
};

export const BENCH_RENDER_PROBE: Record<BenchTier, string> = {
  'sb-5.3': 'product_probe.mjs',
  'sb-6': 'product_probe_v2.mjs',
  'sb-7': 'product_probe_v3.mjs',
  'sb-7.1': 'product_probe_sb71.mjs',
  'sb-8': 'product_probe_v4.mjs',
};

/** The latest stable benchmark shipped by this app; bundled experiments remain available for historical reads. */
export const DEFAULT_BENCHMARK_TIER = 'sb-7.1' satisfies BenchTier;

export function defaultBenchmarkTier(): BenchTier {
  return DEFAULT_BENCHMARK_TIER;
}

export function defaultBenchmarkScorer(): string {
  return TIER_SCORER[DEFAULT_BENCHMARK_TIER];
}

export type CloudBenchmarkTier = 'sb-7' | 'sb-7.1';
export function benchmarkLaunchTier(cloud?: { tier: CloudBenchmarkTier }): BenchTier {
  if (cloud && cloud.tier !== DEFAULT_BENCHMARK_TIER)
    throw new Error(
      'Only the latest stable benchmark can be run. Older benchmarks are history only.'
    );
  return DEFAULT_BENCHMARK_TIER;
}

export function benchmarkLaunchProblem(
  benchmarks: BenchCatalogBenchmark[] | null | undefined,
  stale = false,
  scorer = defaultBenchmarkScorer()
): string | null {
  if (!benchmarks || stale)
    return 'Connect to leanzero.net to verify the latest stable benchmark before running.';
  const current = benchmarks.filter((entry) => entry?.current === true);
  if (
    current.length !== 1 ||
    !/^sb-\d+(?:\.\d+)*$/.test(current[0].scorerVersion) ||
    current[0].frozen !== false
  )
    return 'The catalog has no single available stable benchmark. Refresh before running.';
  if (current[0].scorerVersion !== defaultBenchmarkScorer())
    return `Update Goose to run the latest stable benchmark (${current[0].scorerVersion}). This app bundles ${defaultBenchmarkScorer()}.`;
  if (scorer !== current[0].scorerVersion)
    return 'This benchmark is history only. Only the latest stable benchmark can be run or re-scored.';
  return null;
}
export function benchmarkScorer(tier: BenchTier): string {
  return TIER_SCORER[tier];
}
