import { describe, expect, it } from 'vitest';
import { completedSeriesFixture, seriesFixture } from './components/benchmark/series.fixture';
import {
  pairCouplingProblem,
  pairCouplingWarning,
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
