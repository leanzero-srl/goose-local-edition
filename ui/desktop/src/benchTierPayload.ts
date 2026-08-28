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
import type { BenchTier } from './components/benchmark/baselines';

export const BENCH_SPEC_FILE: Record<BenchTier, string> = {
  'sb-5.3': 'spec-build-v2.md',
  'sb-6': 'spec-build-v3.md',
  'sb-7': 'spec-build-sb7.md',
};

export const BENCH_RENDER_PROBE: Record<BenchTier, string> = {
  'sb-5.3': 'product_probe.mjs',
  'sb-6': 'product_probe_v2.mjs',
  'sb-7': 'product_probe_v3.mjs',
};

/** The tier a `benchmark-run` argument names, defaulting to the comparability tier when absent. */
export function tierOf(raw: string | undefined): BenchTier {
  return raw === 'sb-7' ? 'sb-7' : raw === 'sb-6' ? 'sb-6' : 'sb-5.3';
}
