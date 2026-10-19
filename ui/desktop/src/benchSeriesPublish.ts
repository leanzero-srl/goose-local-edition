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

export interface PairPublishDeps {
  /** This machine's published record per run key (its url), as main keeps it. */
  published: Record<string, { url?: string | null } | undefined>;
  /** Whether the record for `key` shows exactly this result (published and not re-scored since). */
  isCurrent: (key: string, rescoredAt: string | undefined) => boolean;
  /** Prepare a build without uploading or posting anything: its coupling facts, or why it cannot be published. */
  preflight: (key: string) => Promise<Record<string, unknown>>;
  /** Post a build (uploads, POST, recorded as published). */
  publish: (key: string) => Promise<Record<string, unknown>>;
}

/**
 * Publish both builds of a finished series in ONE action. Both are prepared first; a build that is already live stays
 * live (never re-posted, never blocking); a partner that can never be published does not strand the requested build
 * (it goes out on its own, said on the card); a pair that would not land as one entry is refused with
 * `couplingRefused`, so the card can offer publishing the builds separately as the user's explicit choice. Then the
 * builds post in build order; a refusal after one build is live is reported per build (`partial`), and publishing
 * again posts only the missing one. `url` is the REQUESTED build's page.
 */
export async function publishSeriesPair(
  keys: [string, string],
  requestedKey: string,
  deps: PairPublishDeps
): Promise<Record<string, unknown>> {
  const prepared: Array<{
    index: 1 | 2;
    key: string;
    facts: PairCouplingFacts | null;
    error: string | null;
    posted: boolean;
  }> = [];
  for (const [i, key] of keys.entries()) {
    const check = (await deps.preflight(key)) as {
      ok?: boolean;
      error?: string;
      facts?: PairCouplingFacts;
      rescoredAt?: string;
    };
    const ready = check.ok && check.facts ? check.facts : null;
    prepared.push({
      index: (i + 1) as 1 | 2,
      key,
      facts: ready,
      error: ready ? null : (check.error ?? 'could not be prepared'),
      posted: ready ? deps.isCurrent(key, check.rescoredAt) : deps.published[key] != null,
    });
  }
  const requested = prepared.find((p) => p.key === requestedKey) ?? prepared[0];
  if (requested.error && !requested.posted)
    return {
      ok: false,
      error: `Build ${requested.index}: ${requested.error}. Neither build was published.`,
    };
  const lone = prepared.find((p) => p.error && !p.posted);
  let alone: string | null = null;
  if (lone) {
    alone = `Build ${lone.index} cannot be published (${lone.error}), so build ${requested.index} was published on its own.`;
    prepared.splice(prepared.indexOf(lone), 1);
  } else if (prepared[0].facts && prepared[1].facts) {
    const coupling = pairCouplingProblem(prepared[0].facts, prepared[1].facts);
    if (coupling)
      return {
        ok: false,
        couplingRefused: true,
        error: `The two builds would not land as one entry on leanzero.net: ${coupling}. Neither build was published.`,
      };
  }
  if (prepared.every((p) => p.posted))
    return {
      ok: false,
      error:
        prepared.length === 2
          ? 'both builds of this series are already live on leanzero.net'
          : `this build is already live on leanzero.net. ${alone ?? ''}`.trim(),
    };
  const builds: Array<Record<string, unknown>> = [];
  for (const p of prepared) {
    if (p.posted) {
      builds.push({
        index: p.index,
        ok: true,
        alreadyLive: true,
        url: deps.published[p.key]?.url ?? null,
      });
      continue;
    }
    const result = await deps.publish(p.key);
    builds.push({ index: p.index, ...result });
    if (!result.ok) {
      const live = builds.filter((b) => b.ok).map((b) => `build ${b.index}`);
      return {
        ok: false,
        partial: live.length > 0,
        builds,
        error: `Build ${p.index} was refused: ${String(result.error ?? 'no reason given')}${
          live.length
            ? ` ${live.join(' and ')} is live on its own until build ${p.index} is published: publish again to post it.`
            : ''
        }`,
      };
    }
  }
  const last = builds[builds.length - 1];
  const apart =
    alone ??
    (prepared.length === 2 && prepared[0].facts && prepared[1].facts
      ? pairCouplingWarning(prepared[0].facts, prepared[1].facts)
      : null);
  const own = builds.find((b) => b.index === requested.index);
  return {
    ...last,
    ok: true,
    pair: !alone,
    ...(apart ? { pairWarning: apart } : {}),
    url:
      (typeof own?.url === 'string' ? own.url : null) ??
      builds.find((b) => typeof b.url === 'string')?.url ??
      null,
    builds,
  };
}
