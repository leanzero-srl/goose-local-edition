import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  benchSeriesSummary,
  interruptedBenchSeries,
  parseBenchSeries,
  type BenchSeriesSnapshot,
} from './benchSeries';
import {
  benchSeriesResultProblem,
  readBenchSeries,
  runBenchSeries,
  writeBenchSeries,
  type BenchSeriesResult,
} from './benchSeriesRunner';
import {
  benchIdentityHash,
  canonicalBenchJson,
  forgeSeriesComparison,
} from './benchSeriesComparison';

const hash = 'a'.repeat(64);
const makeSeries = (): BenchSeriesSnapshot => ({
  schemaVersion: 1,
  id: 'forge-series-11111111-1111-1111-1111-111111111111',
  totalBuilds: 2,
  provider: 'openrouter',
  model: 'provider/model',
  scorerVersion: 'forge-2.0',
  inputHash: hash,
  modelFields: { sent: {}, pinnedByTier: ['effort'] },
  startedAt: '2026-10-12T10:00:00.000Z',
  running: true,
  cancelled: false,
  stage: 'preparing',
  currentBuild: 1,
  builds: [
    { index: 1, runId: 'cloud-11111111-1111-1111-1111-111111111111', outcome: 'queued' },
    { index: 2, runId: 'cloud-22222222-2222-2222-2222-222222222222', outcome: 'queued' },
  ],
  summary: { count: 0, provisional: true },
});
const result = (series: BenchSeriesSnapshot, index: number, score: number): BenchSeriesResult => ({
  runId: series.builds[index].runId,
  nativeBuildId: series.builds[index].runId,
  modelId: series.model,
  provider: series.provider,
  scorerVersion: series.scorerVersion,
  modelFields: series.modelFields,
  seriesId: series.id,
  buildIndex: index + 1,
  score,
  seriesComparison: { identity: { fixture_seeds: ['a', 'b', 'c'], taskFingerprint: hash } },
});
const directories: string[] = [];
afterEach(async () => {
  await Promise.all(
    directories.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true }))
  );
});

describe('native two-build owner', () => {
  it('dispatches two fresh identities serially, preserves both rows and reports build variation once', async () => {
    const series = makeSeries();
    const calls: string[] = [];
    const snapshots: BenchSeriesSnapshot[] = [];
    let childActive = false;
    const last = await runBenchSeries(series, {
      save: async () => {},
      inputHash: async () => hash,
      changed: (state) => snapshots.push(globalThis.structuredClone(state)),
      run: async (build) => {
        expect(childActive).toBe(false);
        childActive = true;
        calls.push(build.runId);
        await Promise.resolve();
        childActive = false;
        return result(series, build.index - 1, build.index === 1 ? 0.7294 : 0.6196);
      },
    });
    expect(new Set(calls).size).toBe(2);
    expect(last?.runId).toBe(series.builds[1].runId);
    expect(series.summary).toEqual({
      count: 2,
      median: expect.closeTo(0.6745, 12),
      min: 0.6196,
      max: 0.7294,
      provisional: false,
    });
    expect(snapshots.find((state) => state.stage === 'between-builds')).toMatchObject({
      running: true,
      summary: { count: 1, provisional: true },
    });
    expect(snapshots[snapshots.length - 1]).toMatchObject({ running: false, stage: 'finished' });
  });

  it('suppresses the queued second child on cancellation between native builds', async () => {
    const series = makeSeries();
    const run = vi.fn(async () => result(series, 0, 0.5));
    await runBenchSeries(series, {
      save: async () => {},
      inputHash: async () => hash,
      run,
      changed: (state) => {
        if (state.stage === 'between-builds') state.cancelled = true;
      },
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(series.builds[1].outcome).toBe('skipped');
    expect(series).toMatchObject({
      running: false,
      stage: 'cancelled',
      summary: { count: 1, median: 0.5, provisional: true },
    });
  });

  it('checks cancellation again after a pending source/provider binding read', async () => {
    const series = makeSeries();
    const run = vi.fn();
    await runBenchSeries(series, {
      save: async () => {},
      changed: () => {},
      run,
      inputHash: async () => {
        series.cancelled = true;
        return hash;
      },
    });
    expect(run).not.toHaveBeenCalled();
    expect(series.builds.every((build) => build.outcome === 'skipped')).toBe(true);
  });

  it('preserves a genuine first score when native build two fails', async () => {
    const series = makeSeries();
    await runBenchSeries(series, {
      save: async () => {},
      inputHash: async () => hash,
      changed: () => {},
      run: async (build) => {
        if (build.index === 2) throw new Error('Native runner exited without a verdict.');
        return result(series, 0, 0.73);
      },
    });
    expect(series).toMatchObject({
      stage: 'failed',
      summary: { count: 1, median: 0.73, provisional: true },
    });
    expect(series.builds[1]).toMatchObject({
      outcome: 'failed',
      error: 'Native runner exited without a verdict.',
    });
  });

  it('retains native scores but refuses a mismatched second fixture identity', async () => {
    const series = makeSeries();
    await runBenchSeries(series, {
      save: async () => {},
      inputHash: async () => hash,
      changed: () => {},
      run: async (build) => ({
        ...result(series, build.index - 1, build.index / 3),
        seriesComparison: {
          identity: { fixture_seeds: build.index === 1 ? ['a', 'b', 'c'] : ['d', 'e', 'f'] },
        },
      }),
    });
    expect(series.builds.map((build) => build.score)).toEqual([1 / 3, 2 / 3]);
    expect(series.stage).toBe('failed');
    expect(series.summary.count).toBe(1);
    expect(series.summary.reason).toContain('different task');
  });

  it('does not start build two after selected credentials, runtime or treatment change', async () => {
    const series = makeSeries();
    let reads = 0;
    const run = vi.fn(async () => result(series, 0, 0.6));
    await runBenchSeries(series, {
      save: async () => {},
      changed: () => {},
      run,
      inputHash: async () => (++reads === 1 ? hash : 'b'.repeat(64)),
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(series.builds[1].outcome).toBe('skipped');
    expect(series.summary.count).toBe(1);
    expect(series.error).toContain('inputs changed');
  });

  it('makes no native dispatch after initial durable ownership fails', async () => {
    const series = makeSeries();
    const run = vi.fn();
    await expect(
      runBenchSeries(series, {
        save: async () => {
          throw new Error('disk full');
        },
        inputHash: async () => hash,
        changed: () => {},
        run,
      })
    ).rejects.toThrow('disk full');
    expect(run).not.toHaveBeenCalled();
  });

  it.each([
    'runId',
    'nativeBuildId',
    'modelId',
    'provider',
    'scorerVersion',
    'seriesId',
    'buildIndex',
  ])('refuses the wrong %s rather than borrowing another result', (key) => {
    const series = makeSeries();
    expect(
      benchSeriesResultProblem(series, series.builds[0], {
        ...result(series, 0, 0.5),
        [key]: 'wrong',
      })
    ).not.toBeNull();
  });
});

describe('durable two-build history', () => {
  it('survives view replacement and never auto-replays work after app restart', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-series-test-'));
    directories.push(root);
    const series = makeSeries();
    series.builds[0] = { ...series.builds[0], outcome: 'scored', score: 0.4, comparisonHash: hash };
    series.stage = 'between-builds';
    await writeBenchSeries(root, series);
    const loaded = await readBenchSeries(root);
    expect(loaded?.builds[0].score).toBe(0.4);
    const recovered = interruptedBenchSeries(loaded!);
    expect(recovered).toMatchObject({
      running: false,
      stage: 'interrupted',
      summary: { count: 1, median: 0.4 },
    });
    expect(recovered.builds[1].outcome).toBe('skipped');
  });

  it('keeps a synchronous quit/cancel marker even if an older pending save lands afterward', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-series-test-'));
    directories.push(root);
    const series = makeSeries();
    await writeBenchSeries(root, series);
    await fs.writeFile(
      path.join(root, `${series.id}.cancel.json`),
      JSON.stringify({ id: series.id, cancelled: true })
    );
    await writeBenchSeries(root, series);
    expect((await readBenchSeries(root, series.id))?.cancelled).toBe(true);
  });

  it('serializes concurrent history saves and retains the last requested snapshot', async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-series-test-'));
    directories.push(root);
    const series = makeSeries();
    const first = writeBenchSeries(root, series);
    series.stage = 'between-builds';
    const second = writeBenchSeries(root, series);
    series.stage = 'finished';
    series.running = false;
    const third = writeBenchSeries(root, series);
    await Promise.all([first, second, third]);
    expect((await readBenchSeries(root, series.id))?.stage).toBe('finished');
    expect(await fs.readdir(root)).toEqual([`${series.id}.json`]);
  });

  it.each([
    { sent: null, pinnedByTier: [] },
    { sent: { effort: { enabled: true } }, pinnedByTier: [] },
    { sent: {}, pinnedByTier: [1] },
  ])('refuses a malformed saved treatment without substituting defaults', (modelFields) => {
    expect(parseBenchSeries({ ...makeSeries(), modelFields })).toBeNull();
  });

  it('rejects duplicate run identity and malformed scores without manufacturing a score', () => {
    const series = makeSeries();
    series.builds[1].runId = series.builds[0].runId;
    expect(parseBenchSeries(series)).toBeNull();
    expect(
      benchSeriesSummary([{ ...series.builds[0], outcome: 'scored', score: Number.NaN }])
    ).toEqual({ count: 0, provisional: true });
  });

  it('ignores an injected stored median and reconstructs arithmetic from eligible children', () => {
    const series = makeSeries();
    series.builds = series.builds.map((build, index) => ({
      ...build,
      outcome: 'scored',
      score: 0.2 + index * 0.4,
      comparisonHash: hash,
    })) as BenchSeriesSnapshot['builds'];
    series.summary = { count: 2, median: 1, provisional: false };
    expect(parseBenchSeries(series)?.summary.median).toBeCloseTo(0.4);
  });
});

const rawVerdict = () => ({
  scorerVersion: 'forge-2.0',
  status: 'scored',
  publishable: true,
  taskFingerprint: hash,
  buildId: 'cloud-first',
  scorer_files_sha256: { scorer: hash },
  spec_sha256: hash,
  kit_lock_sha256: hash,
  wrapper_sha256: hash,
  kit_code_sha256: hash,
  dev_seed: 'seed',
  input_manifest: { 'config.json': hash, 'model-limits.json': 'provider limits' },
  reasoning_effort: 'medium',
  fixture_seeds: ['alpha', 'beta', 'gamma'],
  seed_scores: { alpha: 0.3, beta: 0.4, gamma: 0.5 },
  checks: [
    { check: 'first', score: 0.3 },
    { check: 'second', score: 0.4 },
  ],
  agent: { budget: { stopped_by: 'model_finished' } },
});
describe('raw canonical comparison identity', () => {
  it('counts fixture sites as one build and excludes only native model limits', () => {
    const first = forgeSeriesComparison(rawVerdict());
    const second = forgeSeriesComparison({
      ...rawVerdict(),
      buildId: 'cloud-second',
      input_manifest: { 'config.json': hash, 'model-limits.json': 'different limits' },
    });
    expect(first.problem).toBeUndefined();
    expect(benchIdentityHash(first.identity)).toBe(benchIdentityHash(second.identity));
    expect(first.identity?.input_manifest).toEqual({ 'config.json': hash });
  });
  it.each([
    'fixture_seeds',
    'seed_scores',
    'input_manifest',
    'taskFingerprint',
    'scorer_files_sha256',
    'reasoning_effort',
  ])('refuses missing native %s', (field) => {
    expect(forgeSeriesComparison({ ...rawVerdict(), [field]: undefined }).problem).toBeDefined();
  });
  it('refuses unavailable, resource-interrupted and duplicated-check canonical results', () => {
    expect(forgeSeriesComparison({ ...rawVerdict(), publishable: false }).problem).toBeDefined();
    expect(
      forgeSeriesComparison({ ...rawVerdict(), agent: { budget: { stopped_by: 'wallet_guard' } } })
        .problem
    ).toBeDefined();
    expect(
      forgeSeriesComparison({ ...rawVerdict(), checks: [{ check: 'dup' }, { check: 'dup' }] })
        .problem
    ).toBeDefined();
  });
  it('compares object identity independently of insertion order and rejects non-finite fields', () => {
    expect(canonicalBenchJson({ b: { x: 1 }, a: 2 })).toBe(
      canonicalBenchJson({ a: 2, b: { x: 1 } })
    );
    expect(() => canonicalBenchJson({ score: Infinity })).toThrow();
  });
});
