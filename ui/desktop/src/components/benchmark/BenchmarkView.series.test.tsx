import { act, render, screen, waitFor, fireEvent } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { BenchSeriesSnapshot } from '../../benchSeries';
import { seriesFixture, completedSeriesFixture, sessionsFixture } from './series.fixture';

vi.mock('../swarm/SwarmRunPanel', () => ({
  SwarmRunPanel: () => <div data-testid="swarm-panel-stub" />,
}));
vi.mock('../swarm/useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => () => {} }));
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: vi.fn(async () => [
    {
      name: 'openrouter',
      is_configured: true,
      metadata: { display_name: 'OpenRouter', known_models: [] },
    },
  ]),
}));
vi.mock('../../acp/modelFields', () => ({
  acpListModelFields: vi.fn(async () => ({ source: 'provider_metadata', fields: [], values: {} })),
  acpSaveModelFields: vi.fn(),
}));
import BenchmarkView from './BenchmarkView';

const handlers = new Map<string, (event: unknown, payload: unknown) => void>();
const electron = () => window.electron as unknown as Record<string, unknown>;
let series: BenchSeriesSnapshot;
let workdir: string | undefined;
function emit(channel: string, payload: unknown) {
  act(() => handlers.get(channel)?.(null, payload));
}
function mount() {
  return render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
}

beforeEach(() => {
  handlers.clear();
  window.location.hash = '#/benchmark?family=forge';
  series = seriesFixture();
  workdir = '/tmp/first-native';
  Object.assign(electron(), {
    benchmarkRuntimeStatus: vi.fn(async () => ({ state: 'ready', downloadBytes: 0 })),
    benchmarkForgeKitStatus: vi.fn(async () => ({
      state: 'ready',
      missing: [],
      callBudget: 300,
      reasoningEffort: 'medium',
    })),
    benchmarkStatus: vi.fn(async () => ({
      running: series.running,
      series,
      workdir,
      provider: series.provider,
      startedAt: series.startedAt,
      phase: 'build',
    })),
    benchmarkSeries: vi.fn(async () => series),
    benchmarkSessions: vi.fn(async () => ({ sessions: sessionsFixture(series) })),
    benchmarkRead: vi.fn(async () => null),
    benchmarkRunResult: vi.fn(async () => null),
    benchmarkShots: vi.fn(async () => []),
    benchmarkMedia: vi.fn(async () => ({ videos: [] })),
    benchmarkCatalog: vi.fn(async () => ({
      ok: true,
      benchmarks: [
        {
          scorerVersion: 'forge-2.0',
          title: 'Forge 2.0',
          family: 'forge',
          familyCurrent: true,
          current: false,
          frozen: false,
          baselines: [],
        },
      ],
    })),
    readSwarmRun: vi.fn(async () => null),
    fleetStatus: vi.fn(async () => ({})),
    on: vi.fn((channel: string, callback: (event: unknown, payload: unknown) => void) =>
      handlers.set(channel, callback)
    ),
    off: vi.fn(),
    benchmarkCancel: vi.fn(async () => ({ ok: true })),
  });
});

afterEach(() => vi.restoreAllMocks());

describe('Forge series in the full Benchmark view', () => {
  it('keeps the app busy after build one, then rejects late first-build finish/log events', async () => {
    const first = series;
    mount();
    await screen.findByRole('button', { name: 'Cancel run' });
    series = {
      ...first,
      stage: 'between-builds',
      builds: [{ ...first.builds[0], outcome: 'scored', score: 0.6 }, first.builds[1]],
      summary: { count: 1, provisional: true },
    };
    emit('benchmark-finished', { series, row: { runId: first.builds[0].runId, score: 0.6 } });
    expect(screen.getByRole('button', { name: 'Cancel run' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Run 2 builds' })).toBeNull();
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent('One scored build');
    expect(screen.queryByTestId('swarm-panel-stub')).toBeNull();
    series = {
      ...series,
      currentBuild: 2,
      stage: 'running',
      builds: [series.builds[0], { ...series.builds[1], outcome: 'running' }],
    };
    workdir = '/tmp/second-native';
    emit('benchmark-started', {
      series,
      workdir,
      provider: series.provider,
      startedAt: series.startedAt,
    });
    emit('benchmark-log', {
      series: first,
      line: 'STALE FIRST BUILD LINE',
      phase: 'done',
      scored: true,
    });
    emit('benchmark-finished', {
      series: { ...first, running: false, stage: 'failed' },
      error: 'STALE FIRST FAILURE',
    });
    expect(screen.getByRole('button', { name: 'Cancel run' })).toBeEnabled();
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('Build 2 of 2 is running');
    expect(screen.queryByText('STALE FIRST FAILURE')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Console details' }));
    expect(screen.queryByText('STALE FIRST BUILD LINE')).toBeNull();
    series = completedSeriesFixture();
    emit('benchmark-finished', { series, row: { runId: series.builds[1].runId, score: 0.8 } });
    expect(await screen.findByRole('button', { name: 'Run 2 builds' })).toBeInTheDocument();
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent('Local median 0.7000');
  });
  it('reattaches between builds without treating absence of a live child as completion', async () => {
    const base = series;
    series = {
      ...base,
      stage: 'between-builds',
      builds: [{ ...base.builds[0], outcome: 'scored', score: 0.6 }, base.builds[1]],
      summary: { count: 1, provisional: true },
    };
    workdir = undefined;
    mount();
    expect(await screen.findByRole('button', { name: 'Cancel run' })).toBeEnabled();
    expect(await screen.findByText('Build 1 is complete. Preparing build 2.')).toBeVisible();
    expect(screen.queryByTestId('swarm-panel-stub')).toBeNull();
  });
  it('cancellation is still available in the gap and preserves the first real score', async () => {
    const base = series;
    series = {
      ...base,
      stage: 'between-builds',
      builds: [{ ...base.builds[0], outcome: 'scored', score: 0.6 }, base.builds[1]],
      summary: { count: 1, provisional: true },
    };
    workdir = undefined;
    mount();
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel run' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel the run' }));
    await waitFor(() => expect(electron().benchmarkCancel).toHaveBeenCalledTimes(1));
    series = {
      ...series,
      running: false,
      cancelled: true,
      stage: 'cancelled',
      builds: [series.builds[0], { ...series.builds[1], outcome: 'skipped' }],
    };
    emit('benchmark-series', series);
    expect(screen.getByTestId('forge-series-build-1')).toHaveTextContent('0.6000');
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('Did not start');
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent(
      'One scored build is provisional'
    );
  });
  it('a delayed build-one status response cannot end build two or revive its console state', async () => {
    const first = series;
    const interval = globalThis.setInterval;
    let poll: (() => void) | undefined;
    vi.spyOn(globalThis, 'setInterval').mockImplementation((callback, delay, ...args) => {
      if (delay === 1000 && typeof callback === 'function') poll = callback as () => void;
      return interval(callback, delay, ...args);
    });
    mount();
    await screen.findByRole('button', { name: 'Cancel run' });
    let answer: ((status: unknown) => void) | undefined;
    const pending = new Promise((resolve) => {
      answer = resolve;
    });
    vi.mocked(window.electron.benchmarkStatus).mockImplementationOnce(
      () => pending as ReturnType<typeof window.electron.benchmarkStatus>
    );
    act(() => poll?.());
    series = {
      ...first,
      currentBuild: 2,
      builds: [
        { ...first.builds[0], outcome: 'scored', score: 0.6 },
        { ...first.builds[1], outcome: 'running' },
      ],
      summary: { count: 1, provisional: true },
    };
    workdir = '/tmp/second-native';
    emit('benchmark-started', { series, workdir, provider: series.provider, phase: 'build' });
    await act(async () =>
      answer?.({
        running: false,
        series: { ...first, running: false, stage: 'finished' },
        phase: 'done',
        scored: true,
      })
    );
    expect(screen.getByRole('button', { name: 'Cancel run' })).toBeEnabled();
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('Build 2 of 2 is running');
    expect(screen.queryByText('Two builds complete.')).toBeNull();
  });
  it('retains a real first score but never reports completion when the second child fails', async () => {
    mount();
    await screen.findByRole('button', { name: 'Cancel run' });
    series = {
      ...series,
      currentBuild: 2,
      running: false,
      stage: 'failed',
      error: 'Native second build failed',
      builds: [
        { ...series.builds[0], outcome: 'scored', score: 0.6 },
        { ...series.builds[1], outcome: 'failed', error: 'Native second build failed' },
      ],
      summary: { count: 1, provisional: true },
    };
    emit('benchmark-finished', { series, row: { runId: series.builds[0].runId, score: 0.6 } });
    expect(await screen.findByRole('button', { name: 'Run 2 builds' })).toBeInTheDocument();
    expect(screen.getByText('Two-build sequence did not finish.')).toBeVisible();
    expect(screen.queryByText('Run complete.')).toBeNull();
    expect(screen.getByTestId('forge-series-build-1')).toHaveTextContent('0.6000');
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent(
      'One scored build is provisional'
    );
  });
  it('states a saved-series read failure without inventing a pair score', async () => {
    series = completedSeriesFixture();
    Object.assign(electron(), {
      benchmarkSeries: vi.fn(async () => {
        throw new Error('Series custody unavailable');
      }),
    });
    window.location.hash = `#/benchmark?era=forge-2.0&series=${series.id}`;
    mount();
    expect(
      await screen.findByText(
        /Saved two-build evidence could not be read.*Series custody unavailable/
      )
    ).toBeVisible();
    expect(screen.queryByText('Local median 0.7000')).toBeNull();
  });
  it('one Run 2 builds action from new setup owns the sequence and leaves both completed scores visible', async () => {
    series = completedSeriesFixture();
    window.location.hash = '#/benchmark?family=forge&new=1';
    let finish: ((result: unknown) => void) | undefined;
    const result = new Promise((resolve) => {
      finish = resolve;
    });
    const run = vi.fn(() => result);
    Object.assign(electron(), { benchmarkRunCloud: run });
    mount();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Model provider' })).toBeEnabled()
    );
    fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'OpenRouter' }));
    const model = await screen.findByRole('textbox', { name: 'Model ID' });
    fireEvent.change(model, { target: { value: series.model } });
    const button = screen.getByRole('button', { name: 'Run 2 builds' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(run).toHaveBeenCalledTimes(1);
    expect(window.location.hash).toBe('#/benchmark?family=forge');
    series = seriesFixture({ id: 'forge-series-33333333-3333-3333-3333-333333333333' });
    emit('benchmark-series', series);
    expect(await screen.findByTestId('forge-build-series')).toHaveAttribute(
      'data-series-id',
      series.id
    );
    const completed = completedSeriesFixture();
    series = { ...completed, id: series.id };
    emit('benchmark-finished', { series, row: { runId: series.builds[1].runId, score: 0.8 } });
    await act(async () => finish?.({ series, runId: series.builds[1].runId, score: 0.8 }));
    expect(screen.getByTestId('forge-series-build-1')).toHaveTextContent('0.6000');
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('0.8000');
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent('Local median 0.7000');
    expect(run).toHaveBeenCalledTimes(1);
  });
});
