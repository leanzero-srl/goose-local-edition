import type { BenchSeriesSnapshot } from './benchSeries';

/**
 * A Forge 2.0 run in the app is a SERIES of two independent builds, and on leanzero.net it is ONE entry: "2 independent
 * builds", their median and range. Publishing either build therefore publishes both, in one action, under one title,
 * with one operator decision — never build 1 alone while build 2 waits (a lone build is a provisional single row: green
 * for a community poster, a one-build violet row for LeanZero's operator key). A series that ended with only one
 * scored build (build 2 cancelled or failed) still publishes that build on its own; that is the truth of the series.
 */
export type SeriesPublishPlan =
  | { kind: 'single' }
  | { kind: 'pair'; seriesId: string; keys: [string, string] }
  | { kind: 'wait'; reason: string };

export interface SeriesPublishRow {
  runId: string | null;
  startedAt: string;
  seriesId?: string;
  buildIndex?: 1 | 2;
  outcome?: string;
}

export function seriesPublishPlan(input: {
  row: SeriesPublishRow | null;
  series: BenchSeriesSnapshot | null;
  rows: readonly SeriesPublishRow[];
  runKeyOf: (row: SeriesPublishRow) => string;
}): SeriesPublishPlan {
  const { row, series } = input;
  if (!row?.seriesId || !row.buildIndex) return { kind: 'single' };
  if (!series || series.id !== row.seriesId) return { kind: 'single' };
  if (series.running || ['preparing', 'running', 'between-builds'].includes(series.stage))
    return {
      kind: 'wait',
      reason:
        'This build is one of two in a Forge 2.0 series: both builds publish together, as one entry, once build 2 is scored.',
    };
  const scored = series.builds.filter((build) => build.outcome === 'scored');
  if (series.summary.count !== 2 || series.summary.provisional || scored.length !== 2)
    return { kind: 'single' };
  const keys: string[] = [];
  for (const index of [1, 2] as const) {
    const build = series.builds.find((b) => b.index === index);
    const child = input.rows.find(
      (r) => r.seriesId === series.id && r.buildIndex === index && build && r.runId === build.runId
    );
    if (!child)
      return {
        kind: 'wait',
        reason: `Build ${index} of this Forge 2.0 series is no longer in the session list, so the two builds cannot be published together.`,
      };
    keys.push(input.runKeyOf(child));
  }
  return { kind: 'pair', seriesId: series.id, keys: [keys[0], keys[1]] };
}

/** What makes two builds ONE board entry (website lib/forge2Baselines.ts: model + task + cohort for a baseline; plus
 *  install, class and provider for a community pair) and one presentation (title, operator decision). */
export interface PairCouplingFacts {
  model: string;
  provider: string | null;
  scorerVersion: string;
  taskFingerprint: string | null;
  comparisonCohortSha: string | null;
  title: string;
  operator: boolean;
}

/** Why two prepared builds must NOT be published as one series action; null = go. A difference the series itself
 *  guarantees cannot happen (model, provider, task, title, operator decision) means something is wrong: refused. */
export function pairCouplingProblem(a: PairCouplingFacts, b: PairCouplingFacts): string | null {
  if (a.scorerVersion !== 'forge-2.0' || b.scorerVersion !== 'forge-2.0')
    return 'only Forge 2.0 builds publish as a two-build entry';
  if (a.model !== b.model) return `the builds name different models (${a.model}, ${b.model})`;
  if (a.provider !== b.provider) return 'the builds ran on different providers';
  if (!a.taskFingerprint || a.taskFingerprint !== b.taskFingerprint)
    return 'the builds ran different Forge 2.0 tasks';
  if (a.title !== b.title) return 'the builds would be published under different titles';
  if (a.operator !== b.operator) return 'only one of the builds would carry the operator key';
  return null;
}

/** Why the board will list the two builds apart although both are published (said on the card, never hidden): a
 *  build without its comparison receipt (an older build), or receipts naming different conditions (an app update
 *  between the builds changed the engine). Refusing these would leave the series unpublishable from the app. */
export function pairCouplingWarning(a: PairCouplingFacts, b: PairCouplingFacts): string | null {
  if (!a.comparisonCohortSha || !b.comparisonCohortSha)
    return 'A build carries no comparison receipt, so leanzero.net lists the two builds separately.';
  if (a.comparisonCohortSha !== b.comparisonCohortSha)
    return 'The builds ran under different conditions (engine, kit or settings), so leanzero.net lists them separately.';
  return null;
}
