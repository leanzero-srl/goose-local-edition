/** Preserve recorded SB7-family composition inputs; no legacy formula substitutes for absence. */
export function publicScoreDetails(verdict: Record<string, unknown>): Record<string, unknown> {
  const excellence = verdict.excellence as Record<string, unknown> | undefined;
  const critical = verdict.critical as Record<string, unknown> | undefined;
  const candidates = {
    scoreInner: verdict.inner,
    excellenceFraction: excellence?.fraction,
    excellenceEMean: excellence?.e_mean,
    criticalMultiplier: critical?.multiplier,
    criticalFloor: critical?.floor,
    preSeverityScore: critical?.pre_severity_score,
  };
  const result = Object.fromEntries(Object.entries(candidates).filter(([, value]) => value !== undefined));
  if (Array.isArray(excellence?.conditions)) result.gateConditions = excellence.conditions.map(({ name, ok, value }) => ({ name, ok, value }));
  if (Array.isArray(critical?.rows)) result.criticalRows = critical.rows.map(({ check, score, factor, why, suppressed, severity_input }) => ({ check, score, factor, why, ...(suppressed !== undefined ? { suppressed } : {}), ...(severity_input !== undefined ? { severity_input } : {}) }));
  return result;
}
