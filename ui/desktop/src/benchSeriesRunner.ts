import fs from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { benchIdentityHash, canonicalBenchJson } from './benchSeriesComparison';
import path from 'node:path';
import type { BenchSeriesBuild, BenchSeriesSnapshot } from './benchSeries';
import { benchSeriesSummary, parseBenchSeries } from './benchSeries';

const seriesWrites = new Map<string, Promise<void>>();

export async function writeBenchSeries(root: string, series: BenchSeriesSnapshot): Promise<void> {
  if (!/^forge-series-[a-f0-9-]+$/.test(series.id))
    throw new Error('Invalid benchmark series identity.');
  const file = path.join(root, `${series.id}.json`);
  const bytes = JSON.stringify(series, null, 2);
  const previous = seriesWrites.get(file);
  const write = async () => {
    await fs.mkdir(root, { recursive: true });
    const pending = `${file}.pending-${randomUUID()}`;
    try {
      await fs.writeFile(pending, bytes);
      await fs.rename(pending, file);
    } finally {
      await fs.rm(pending, { force: true });
    }
  };
  // A failed earlier write must not prevent a later explicit recovery save.
  const operation = previous ? previous.then(write, write) : write();
  seriesWrites.set(file, operation);
  try {
    await operation;
  } finally {
    if (seriesWrites.get(file) === operation) seriesWrites.delete(file);
  }
}

export async function readBenchSeries(
  root: string,
  id?: string
): Promise<BenchSeriesSnapshot | null> {
  if (id && !/^forge-series-[a-f0-9-]+$/.test(id)) return null;
  let names: string[];
  try {
    names = id
      ? [`${id}.json`]
      : (await fs.readdir(root)).filter((name) => /^forge-series-[a-f0-9-]+\.json$/.test(name));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return null;
    throw error;
  }
  const rows = await Promise.all(
    names.map(async (name) => {
      try {
        return parseBenchSeries(JSON.parse(await fs.readFile(path.join(root, name), 'utf8')));
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === 'ENOENT' || error instanceof SyntaxError)
          return null;
        throw error;
      }
    })
  );
  const saved =
    rows
      .filter((row): row is BenchSeriesSnapshot => row !== null)
      .sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0] ?? null;
  if (!saved) return null;
  try {
    const cancellation = JSON.parse(
      await fs.readFile(path.join(root, `${saved.id}.cancel.json`), 'utf8')
    );
    if (cancellation?.id === saved.id && cancellation.cancelled === true) saved.cancelled = true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
  }
  return saved;
}

export interface BenchSeriesResult {
  runId: unknown;
  score: unknown;
  modelId: unknown;
  provider?: unknown;
  scorerVersion: unknown;
  modelFields?: unknown;
  seriesId?: unknown;
  buildIndex?: unknown;
  [key: string]: unknown;
}

export function benchSeriesResultProblem(
  series: BenchSeriesSnapshot,
  build: BenchSeriesBuild,
  row: BenchSeriesResult
): string | null {
  if (row.runId !== build.runId || row.seriesId !== series.id || row.buildIndex !== build.index)
    return 'The scored result does not belong to this independent build.';
  if (
    row.provider !== series.provider ||
    row.modelId !== series.model ||
    row.scorerVersion !== series.scorerVersion ||
    canonicalBenchJson(row.modelFields ?? null) !== canonicalBenchJson(series.modelFields)
  )
    return 'The scored build does not match the selected model, treatment or benchmark.';
  if (
    typeof row.score !== 'number' ||
    !Number.isFinite(row.score) ||
    row.score < 0 ||
    row.score > 1
  )
    return 'This build has no valid canonical score.';
  if (row.nativeBuildId !== build.runId)
    return 'The canonical verdict names a different independent build.';
  const comparison = row.seriesComparison as
    | { identity?: Record<string, unknown>; problem?: string }
    | undefined;
  if (comparison?.problem) return comparison.problem;
  if (!comparison?.identity) return 'The raw canonical comparison receipt is missing.';
  return null;
}

/** The owner is Electron main; view lifetime and renderer events never decide whether build two runs. */
export async function runBenchSeries(
  series: BenchSeriesSnapshot,
  deps: {
    save: (series: BenchSeriesSnapshot) => Promise<void>;
    inputHash: () => Promise<string>;
    run: (build: BenchSeriesBuild) => Promise<BenchSeriesResult>;
    changed: (series: BenchSeriesSnapshot, row?: BenchSeriesResult) => void;
  }
): Promise<BenchSeriesResult | null> {
  let last: BenchSeriesResult | null = null;
  const save = async () => {
    series.summary = benchSeriesSummary(series.builds);
    await deps.save(series);
  };
  try {
    await save();
    deps.changed(series);
    for (const build of series.builds) {
      if (series.cancelled) break;
      if ((await deps.inputHash()) !== series.inputHash)
        throw new Error('The benchmark inputs changed; the queued build was not started.');
      if (series.cancelled) break;
      series.currentBuild = build.index;
      series.stage = 'running';
      build.outcome = 'running';
      build.startedAt = new Date().toISOString();
      await save();
      if (series.cancelled) break;
      deps.changed(series);
      const row = await deps.run(build);
      const problem = benchSeriesResultProblem(series, build, row);
      build.outcome = 'scored';
      build.runKey = build.runId;
      build.score = row.score as number;
      build.endedAt = new Date().toISOString();
      last = row;
      const comparison = row.seriesComparison as
        | { identity?: Record<string, unknown>; problem?: string }
        | undefined;
      build.comparisonHash = comparison?.identity
        ? benchIdentityHash(comparison.identity)
        : undefined;
      build.comparisonProblem = problem ?? comparison?.problem;
      if (build.index === 2 && series.builds[0].comparisonHash !== build.comparisonHash)
        build.comparisonProblem =
          'The two native builds used different task, kit, scorer, treatment or fixtures.';
      if (build.comparisonProblem) throw new Error(build.comparisonProblem);
      series.stage = build.index === 1 ? 'between-builds' : 'finished';
      await save();
      deps.changed(series, row);
    }
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    series.error = detail;
    const active = series.builds.find((build) => build.outcome === 'running');
    if (active) {
      active.outcome = series.cancelled ? 'cancelled' : 'failed';
      active.error = detail;
      active.endedAt = new Date().toISOString();
    }
    series.stage = series.cancelled ? 'cancelled' : 'failed';
  }
  if (series.cancelled) {
    series.stage = 'cancelled';
    for (const build of series.builds) if (build.outcome === 'running') build.outcome = 'cancelled';
  }
  for (const build of series.builds) if (build.outcome === 'queued') build.outcome = 'skipped';
  series.running = false;
  series.endedAt = new Date().toISOString();
  await save();
  deps.changed(series, last ?? undefined);
  return last;
}
