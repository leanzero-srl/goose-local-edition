import type { BenchSeriesSnapshot } from '../../benchSeries';
import type { BenchSession } from './bridge';

export function seriesFixture(overrides: Partial<BenchSeriesSnapshot> = {}): BenchSeriesSnapshot {
  return {
    schemaVersion: 1,
    id: 'forge-series-11111111-1111-1111-1111-111111111111',
    totalBuilds: 2,
    provider: 'openrouter',
    model: 'fixture/model',
    scorerVersion: 'forge-2.0',
    inputHash: 'a'.repeat(64),
    modelFields: {
      sent: {},
      pinnedByTier: ['effort'],
    },
    startedAt: '2026-10-12T10:00:00.000Z',
    running: true,
    cancelled: false,
    stage: 'running',
    currentBuild: 1,
    builds: [
      { index: 1, runId: 'cloud-11111111-1111-1111-1111-111111111111', outcome: 'running' },
      { index: 2, runId: 'cloud-22222222-2222-2222-2222-222222222222', outcome: 'queued' },
    ],
    summary: { count: 0, provisional: true },
    ...overrides,
  };
}
export function completedSeriesFixture(): BenchSeriesSnapshot {
  const base = seriesFixture();
  return {
    ...base,
    running: false,
    stage: 'finished',
    currentBuild: 2,
    builds: [
      { ...base.builds[0], outcome: 'scored', score: 0.6, comparisonHash: 'b'.repeat(64) },
      { ...base.builds[1], outcome: 'scored', score: 0.8, comparisonHash: 'b'.repeat(64) },
    ],
    summary: { count: 2, provisional: false, median: 0.7, min: 0.6, max: 0.8 },
  };
}
export function sessionsFixture(series: BenchSeriesSnapshot): BenchSession[] {
  return series.builds
    .filter((build) => build.outcome !== 'queued')
    .map((build) => ({
      runId: build.runId,
      seriesId: series.id,
      buildIndex: build.index,
      series,
      scorerVersion: series.scorerVersion,
      startedAt: series.startedAt,
      outcome:
        build.outcome === 'scored'
          ? 'finished'
          : build.outcome === 'running'
            ? 'running'
            : 'did_not_finish',
      score: build.score,
      publishable: build.outcome === 'scored',
    }));
}
