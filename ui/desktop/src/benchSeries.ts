import { validBenchModelFields, type BenchModelFieldsRecord } from './benchModelFields';

export type BenchBuildIndex = 1 | 2;
export type BenchSeriesStage =
  | 'preparing'
  | 'running'
  | 'between-builds'
  | 'finished'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

export interface BenchSeriesBuild {
  index: BenchBuildIndex;
  runId: string;
  runKey?: string;
  outcome: 'queued' | 'running' | 'scored' | 'failed' | 'cancelled' | 'skipped';
  startedAt?: string;
  endedAt?: string;
  score?: number;
  comparisonHash?: string;
  comparisonProblem?: string;
  error?: string;
}

export interface BenchSeriesSummary {
  count: 0 | 1 | 2;
  median?: number;
  min?: number;
  max?: number;
  provisional: boolean;
  reason?: string;
}

export interface BenchSeriesSnapshot {
  schemaVersion: 1;
  id: string;
  totalBuilds: 2;
  provider: string;
  model: string;
  scorerVersion: string;
  /** Local source and treatment binding; never presented as a public comparison cohort. */
  inputHash: string;
  modelFields: BenchModelFieldsRecord;
  startedAt: string;
  endedAt?: string;
  running: boolean;
  cancelled: boolean;
  stage: BenchSeriesStage;
  currentBuild: BenchBuildIndex;
  builds: [BenchSeriesBuild, BenchSeriesBuild];
  summary: BenchSeriesSummary;
  error?: string;
}

export interface BenchSeriesMembership {
  seriesId: string;
  buildIndex: BenchBuildIndex;
}

export function benchSeriesSummary(builds: readonly BenchSeriesBuild[]): BenchSeriesSummary {
  const scored = builds.filter(
    (build) =>
      build.outcome === 'scored' &&
      !build.comparisonProblem &&
      typeof build.comparisonHash === 'string' &&
      /^[a-f0-9]{64}$/.test(build.comparisonHash) &&
      typeof build.score === 'number' &&
      Number.isFinite(build.score) &&
      build.score >= 0 &&
      build.score <= 1
  );
  if (scored.length === 2 && scored[0].comparisonHash !== scored[1].comparisonHash)
    return {
      count: 0,
      provisional: true,
      reason: 'The two builds used different benchmark inputs or fixtures.',
    };
  if (scored.length === 2 && scored[0].runId === scored[1].runId)
    return { count: 0, provisional: true, reason: 'Both results name the same build.' };
  const scores = scored.map((build) => build.score!).sort((a, b) => a - b);
  const problem = builds.find((build) => build.comparisonProblem)?.comparisonProblem;
  if (!scores.length)
    return { count: 0, provisional: true, ...(problem ? { reason: problem } : {}) };
  const min = scores[0];
  const max = scores[scores.length - 1];
  return {
    count: scores.length as 1 | 2,
    median: scores.length === 2 ? (min + max) / 2 : min,
    min,
    max,
    provisional: scores.length !== 2,
    ...(problem ? { reason: problem } : {}),
  };
}

/** Saved active work is interrupted on process restart; reading it never starts paid work. */
export function interruptedBenchSeries(series: BenchSeriesSnapshot): BenchSeriesSnapshot {
  if (!series.running) return series;
  const builds = series.builds.map((build) =>
    build.outcome === 'running' || build.outcome === 'queued'
      ? {
          ...build,
          outcome: build.outcome === 'queued' ? 'skipped' : 'failed',
          error: 'The app ended before this two-build sequence finished.',
        }
      : { ...build }
  ) as BenchSeriesSnapshot['builds'];
  return {
    ...series,
    running: false,
    stage: 'interrupted',
    builds,
    summary: benchSeriesSummary(builds),
    error: 'The app ended before this two-build sequence finished. Saved builds are preserved.',
  };
}

export function parseBenchSeries(value: unknown): BenchSeriesSnapshot | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const series = value as Partial<BenchSeriesSnapshot>;
  if (
    series.schemaVersion !== 1 ||
    series.totalBuilds !== 2 ||
    typeof series.id !== 'string' ||
    !/^forge-series-[a-f0-9-]+$/.test(series.id) ||
    typeof series.provider !== 'string' ||
    !series.provider ||
    typeof series.model !== 'string' ||
    !series.model ||
    typeof series.scorerVersion !== 'string' ||
    !series.scorerVersion.startsWith('forge-') ||
    typeof series.inputHash !== 'string' ||
    (!/^[a-f0-9]{64}$/.test(series.inputHash) && series.inputHash !== '') ||
    typeof series.startedAt !== 'string' ||
    !Number.isFinite(Date.parse(series.startedAt)) ||
    typeof series.running !== 'boolean' ||
    typeof series.cancelled !== 'boolean' ||
    ![
      'preparing',
      'running',
      'between-builds',
      'finished',
      'failed',
      'cancelled',
      'interrupted',
    ].includes(series.stage ?? '') ||
    ![1, 2].includes(series.currentBuild ?? 0) ||
    !Array.isArray(series.builds) ||
    series.builds.length !== 2 ||
    !series.modelFields ||
    !validBenchModelFields(series.modelFields.sent) ||
    !Array.isArray(series.modelFields.pinnedByTier) ||
    !series.modelFields.pinnedByTier.every((field) => typeof field === 'string')
  )
    return null;
  for (const [i, build] of series.builds.entries()) {
    if (
      !build ||
      build.index !== i + 1 ||
      typeof build.runId !== 'string' ||
      !/^cloud-[a-f0-9-]+$/.test(build.runId) ||
      !['queued', 'running', 'scored', 'failed', 'cancelled', 'skipped'].includes(build.outcome) ||
      (build.score !== undefined &&
        (typeof build.score !== 'number' ||
          !Number.isFinite(build.score) ||
          build.score < 0 ||
          build.score > 1))
    )
      return null;
  }
  if (
    series.builds[0].runId === series.builds[1].runId ||
    (series.inputHash === '' &&
      series.builds.some((build) => build.outcome === 'running' || build.outcome === 'scored'))
  )
    return null;
  const parsed = series as BenchSeriesSnapshot;
  return { ...parsed, summary: benchSeriesSummary(parsed.builds) };
}
