import { describe, expect, it, vi } from 'vitest';
import { completedSeriesFixture, seriesFixture } from './components/benchmark/series.fixture';
import {
  pairCouplingProblem,
  pairCouplingWarning,
  publishSeriesPair,
  seriesPublishPlan,
  type PairCouplingFacts,
  type SeriesPublishRow,
} from './benchSeriesPublish';
import { droppableAfterRefusal } from './benchPublishBilling';
import type { BenchSeriesSnapshot } from './benchSeries';

const key = (row: SeriesPublishRow) => row.runId ?? `start-${row.startedAt}`;
const rowsOf = (series: BenchSeriesSnapshot): SeriesPublishRow[] =>
  series.builds.map((build) => ({
    runId: build.runId,
    startedAt: series.startedAt,
    seriesId: series.id,
    buildIndex: build.index,
    outcome: 'finished',
  }));

describe('a Forge 2.0 series publishes as one action', () => {
  it('publishing EITHER build of a finished two-build series publishes both, in build order', () => {
    const series = completedSeriesFixture();
    const rows = rowsOf(series);
    for (const row of rows)
      expect(seriesPublishPlan({ row, series, rows, runKeyOf: key })).toEqual({
        kind: 'pair',
        seriesId: series.id,
        keys: [series.builds[0].runId, series.builds[1].runId],
      });
  });
  it('build 1 alone waits while build 2 is still to come', () => {
    const series = seriesFixture({ running: false, stage: 'between-builds' });
    const rows = rowsOf(series);
    expect(seriesPublishPlan({ row: rows[0], series, rows, runKeyOf: key })).toMatchObject({
      kind: 'wait',
    });
  });
  it('a series that ended with one scored build publishes that build on its own', () => {
    const base = completedSeriesFixture();
    const failed: BenchSeriesSnapshot = {
      ...base,
      stage: 'failed',
      builds: [base.builds[0], { ...base.builds[1], outcome: 'failed', score: undefined }],
      summary: { count: 1, provisional: true },
    };
    const rows = rowsOf(failed);
    expect(seriesPublishPlan({ row: rows[0], series: failed, rows, runKeyOf: key })).toEqual({
      kind: 'single',
    });
  });
  it('a run outside any series publishes on its own', () => {
    const row = { runId: 'cloud-x', startedAt: 't' };
    expect(seriesPublishPlan({ row, series: null, rows: [row], runKeyOf: key })).toEqual({
      kind: 'single',
    });
  });
  it('refuses when a build vanished from the session list rather than posting half the pair', () => {
    const series = completedSeriesFixture();
    const rows = rowsOf(series).slice(0, 1);
    expect(seriesPublishPlan({ row: rows[0], series, rows, runKeyOf: key })).toMatchObject({
      kind: 'wait',
    });
  });
});

describe('the coupling key both builds must share before either is posted', () => {
  const facts: PairCouplingFacts = {
    model: 'deepseek/deepseek-v4.1-flash-1017',
    provider: 'openrouter',
    scorerVersion: 'forge-2.0',
    taskFingerprint: 'e'.repeat(64),
    comparisonCohortSha: 'f'.repeat(64),
    title: 'DeepSeek V4.1 Flash 1017, single model via OpenRouter',
    operator: true,
  };
  it('two builds of one setup couple', () => {
    expect(pairCouplingProblem(facts, { ...facts })).toBeNull();
  });
  it.each([
    ['model', { model: 'other/model' }, /different models/],
    ['provider', { provider: 'google' }, /different providers/],
    ['task', { taskFingerprint: 'a'.repeat(64) }, /different Forge 2.0 tasks/],
    ['title', { title: 'Other' }, /different titles/],
    ['operator', { operator: false }, /operator key/],
  ])('refuses a pair that differs in %s', (_name, change, problem) => {
    expect(pairCouplingProblem(facts, { ...facts, ...change })).toMatch(problem);
  });
});

describe('a pair the board will list apart is still published, and says so', () => {
  const facts: PairCouplingFacts = {
    model: 'm/x',
    provider: 'openrouter',
    scorerVersion: 'forge-2.0',
    taskFingerprint: 'e'.repeat(64),
    comparisonCohortSha: 'f'.repeat(64),
    title: 'X, single model via OpenRouter',
    operator: false,
  };
  it('a build without its receipt or under other conditions is a warning, never a refusal', () => {
    expect(pairCouplingProblem(facts, { ...facts, comparisonCohortSha: null })).toBeNull();
    expect(pairCouplingWarning(facts, { ...facts, comparisonCohortSha: null })).toMatch(
      /no comparison receipt/
    );
    expect(pairCouplingWarning(facts, { ...facts, comparisonCohortSha: 'a'.repeat(64) })).toMatch(
      /different conditions/
    );
    expect(pairCouplingWarning(facts, { ...facts })).toBeNull();
  });
});

describe('an older site that does not know tokenUsage never costs the publish', () => {
  it('drops the field on "Unknown field(s)" or a tokenUsage refusal, and only then', () => {
    expect(
      droppableAfterRefusal(400, 'Unknown field(s): tokenUsage, comparisonCohort', [
        'tokenUsage',
        'comparisonCohort',
        'score',
      ])
    ).toEqual(['tokenUsage', 'comparisonCohort']);
    expect(
      droppableAfterRefusal(
        422,
        'tokenUsage: totalTokens must equal promptTokens + completionTokens',
        ['tokenUsage']
      )
    ).toEqual(['tokenUsage']);
    expect(droppableAfterRefusal(422, 'score disagrees with the checks', ['tokenUsage'])).toEqual(
      []
    );
  });
});

describe('the card says what happened to the pair and to each build', () => {
  it('pair note, the apart warning, then each posted build’s own notes; already-live builds add nothing', async () => {
    const { publishResultNotes } = await import('./benchPublishNotes');
    expect(
      publishResultNotes({
        pair: true,
        pairWarning:
          'A build carries no comparison receipt, so leanzero.net lists the two builds separately.',
        builds: [
          { index: 1, ok: true, alreadyLive: true, baseline: true },
          {
            index: 2,
            ok: true,
            baseline: true,
            tokenUsageLine: 'Tokens: 1.0M in · 10.0k out (exact, OpenRouter records)',
          },
        ],
      })
    ).toEqual([
      'Both builds of this Forge 2.0 series were published together, as one entry.',
      'A build carries no comparison receipt, so leanzero.net lists the two builds separately.',
      'Build 2: Listed as a LeanZero cloud baseline.',
      'Build 2: Tokens: 1.0M in · 10.0k out (exact, OpenRouter records)',
    ]);
  });
});

describe('publishSeriesPair: both builds in one action (main.ts publishBenchPair)', () => {
  const facts: PairCouplingFacts = {
    model: 'm/x',
    provider: 'openrouter',
    scorerVersion: 'forge-2.0',
    taskFingerprint: 'e'.repeat(64),
    comparisonCohortSha: 'f'.repeat(64),
    title: 'X, single model via OpenRouter',
    operator: true,
  };
  const ok = (key: string) => ({ ok: true, facts, rescoredAt: undefined, key });
  function harness(
    over: {
      preflight?: Record<string, Record<string, unknown>>;
      publish?: Record<string, Record<string, unknown>>;
      published?: Record<string, { url: string }>;
    } = {}
  ) {
    const published: Record<string, { url?: string | null }> = { ...(over.published ?? {}) };
    const posted: string[] = [];
    const deps = {
      published,
      isCurrent: (key: string) => key in published,
      preflight: vi.fn(async (key: string) => over.preflight?.[key] ?? ok(key)),
      publish: vi.fn(async (key: string) => {
        posted.push(key);
        const r = over.publish?.[key] ?? { ok: true, url: `/run/${key}`, baseline: true };
        if (r.ok) published[key] = { url: r.url as string };
        return r;
      }),
    };
    return { deps, posted, published };
  }
  it('posts both in build order; the url is the REQUESTED build’s page', async () => {
    const h = harness();
    const r = await publishSeriesPair(['b1', 'b2'], 'b2', h.deps);
    expect(h.posted).toEqual(['b1', 'b2']);
    expect(r).toMatchObject({ ok: true, pair: true, url: '/run/b2' });
  });
  it('a partner that cannot be prepared: the requested build goes out alone and the card says why', async () => {
    const h = harness({ preflight: { b2: { ok: false, error: 'the scorer held this build' } } });
    const r = await publishSeriesPair(['b1', 'b2'], 'b1', h.deps);
    expect(h.posted).toEqual(['b1']);
    expect(r).toMatchObject({ ok: true, pair: false });
    expect(r.pairWarning).toMatch(/Build 2 cannot be published \(the scorer held this build\)/);
  });
  it('the requested build cannot be prepared: nothing is posted', async () => {
    const h = harness({ preflight: { b1: { ok: false, error: 'verdict gone' } } });
    const r = await publishSeriesPair(['b1', 'b2'], 'b1', h.deps);
    expect(h.posted).toEqual([]);
    expect(r).toMatchObject({ ok: false });
    expect(r.error).toMatch(/Build 1: verdict gone. Neither build was published/);
  });
  it('partial failure: build 1 live, build 2 refused; publishing again posts only build 2', async () => {
    const h = harness({ publish: { b2: { ok: false, error: 'HTTP 422: tokenUsage …' } } });
    const first = await publishSeriesPair(['b1', 'b2'], 'b1', h.deps);
    expect(first).toMatchObject({ ok: false, partial: true });
    expect(first.error).toMatch(
      /Build 2 was refused: HTTP 422.*build 1 is live on its own until build 2 is published: publish again/
    );
    expect(
      (first.builds as Array<{ index: number; ok: boolean }>).map((b) => [b.index, b.ok])
    ).toEqual([
      [1, true],
      [2, false],
    ]);
    h.deps.publish.mockImplementation(async (key: string) => {
      h.posted.push(key);
      return { ok: true, url: `/run/${key}` };
    });
    const again = await publishSeriesPair(['b1', 'b2'], 'b2', h.deps);
    expect(h.posted).toEqual(['b1', 'b2', 'b2']);
    expect(again).toMatchObject({ ok: true, pair: true, url: '/run/b2' });
    expect((again.builds as Array<{ alreadyLive?: boolean }>)[0].alreadyLive).toBe(true);
  });
  it('an already-live build that no longer prepares is not re-posted and does not block its partner', async () => {
    const h = harness({
      published: { b1: { url: '/run/b1' } },
      preflight: { b1: { ok: false, error: 'folder moved' } },
    });
    const r = await publishSeriesPair(['b1', 'b2'], 'b2', h.deps);
    expect(h.posted).toEqual(['b2']);
    expect(r).toMatchObject({ ok: true });
  });
  it('builds that would not land as one entry are refused with couplingRefused, and nothing is posted', async () => {
    const h = harness({ preflight: { b2: { ok: true, facts: { ...facts, title: 'Other' } } } });
    const r = await publishSeriesPair(['b1', 'b2'], 'b1', h.deps);
    expect(h.posted).toEqual([]);
    expect(r).toMatchObject({ ok: false, couplingRefused: true });
  });
});
