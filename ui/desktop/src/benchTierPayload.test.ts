import { describe, expect, it } from 'vitest';
import { BENCH_SPEC_FILE, BENCH_RENDER_PROBE, tierOf } from './benchTierPayload';
import { TIERS } from './components/benchmark/baselines';

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
    expect(new Set(probes).size, `two tiers share a probe: ${probes.join(', ')}`).toBe(TIERS.length);
  });

  it('sb-7 gets the Meridian spec and the v3 probe, not VendorSync’s', () => {
    expect(BENCH_SPEC_FILE['sb-7']).toBe('spec-build-sb7.md');
    expect(BENCH_RENDER_PROBE['sb-7']).toBe('product_probe_v3.mjs');
  });

  it('resolves the tier argument, defaulting to the comparability tier', () => {
    expect(tierOf('sb-7')).toBe('sb-7');
    expect(tierOf('sb-6')).toBe('sb-6');
    expect(tierOf(undefined)).toBe('sb-5.3');
    expect(tierOf('nonsense')).toBe('sb-5.3');
  });
});
