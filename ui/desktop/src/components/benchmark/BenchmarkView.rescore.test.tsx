import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * RE-SCORE A FINISHED RUN (owner 2026-10-03: "fix the scorer, leave the benchmark, then take all runs
 * and rescore them and republish"). A finished run with a completed-build receipt offers a re-grade of
 * its saved build with this app's scorer; success replaces the shown result and reopens Publish even
 * for a run posted before; failure keeps the previous result and says so.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});
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

import BenchmarkView from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const RUN_ID = 'cloud-0b8d6f0e-1c1a-4c43-9d1e-5a0f2f9a7d11';
const RUN_META = {
  startedAt: '2026-10-01T09:00:00.000Z',
  finishedAt: '2026-10-01T09:40:00.000Z',
  engineEvents: 0,
  repairRounds: 0,
};
const MINE = {
  label: 'openai/gpt-6-luna · single agent',
  score: 0.42,
  tiers: { A: 0.5, B: 0.4, C: 0.3, D: 0.2 },
  nodes: 1,
  provider: 'openrouter',
  mine: true,
  scorerVersion: 'sb-7.2',
  runMeta: RUN_META,
  workdir: '/tmp/sb-run',
  modelId: 'openai/gpt-6-luna',
  runId: RUN_ID,
};
const PUBLISHED = {
  url: '/agentic-benchmarks/run/brun-1',
  title: 'Luna first pass',
  score: 0.42,
  publishedAt: '2026-10-01T10:00:00.000Z',
  source: 'app' as const,
};
const SESSION = {
  runId: RUN_ID,
  scorerVersion: 'sb-7.2',
  startedAt: RUN_META.startedAt,
  endedAt: RUN_META.finishedAt,
  outcome: 'finished',
  score: 0.42,
  tiers: MINE.tiers,
  nodes: 1,
  publishable: false,
  published: PUBLISHED,
  retryScoring: { ready: false, reason: 'This session is not awaiting a scoring retry.' },
  rescore: { ready: true },
};
const SB_CURRENT = {
  scorerVersion: 'sb-7.2',
  title: 'SB7.2 payments',
  family: 'sb',
  familyCurrent: true,
  current: true,
  frozen: false,
  baselines: [],
};
const FORGE_CURRENT = {
  scorerVersion: 'forge-1.0',
  title: 'Forge 1.0 — Scope Ledger',
  family: 'forge',
  familyCurrent: true,
  current: false,
  frozen: false,
  baselines: [],
};

function mockElectron(sessions: () => unknown[], mine: unknown = MINE) {
  const e = electron();
  e.benchmarkRuntimeStatus = vi.fn(async () => ({ state: 'ready', downloadBytes: 0 }));
  e.benchmarkForgeKitStatus = vi.fn(async () => ({
    state: 'ready',
    missing: [],
    callBudget: 150,
    reasoningEffort: 'medium',
  }));
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  e.benchmarkRead = vi.fn(async () => mine);
  e.benchmarkShots = vi.fn(async () => []);
  e.benchmarkRunResult = undefined;
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog = vi.fn(async () => ({
    ok: true,
    stale: false,
    benchmarks: [SB_CURRENT, FORGE_CURRENT],
  }));
  e.benchmarkSessions = vi.fn(async () => ({ sessions: sessions() }));
  e.benchmarkMedia = vi.fn(async () => ({ videos: [] }));
}

const mount = () =>
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );

/** The Score stat's value: the h1 figure right above its "Score" label. */
const scoreStat = () =>
  screen
    .getAllByText('Score')
    .map((label) => label.previousElementSibling?.textContent ?? null)
    .find((value) => value != null && /%|missing/.test(value)) ?? null;

describe('Benchmark view — Re-score a finished run', () => {
  afterEach(() => {
    cleanup();
    window.location.hash = '';
  });

  it('a successful re-score shows the new score and reopens Publish for a run posted before', async () => {
    window.location.hash = `#/benchmark?era=sb-7.2&run=${RUN_ID}`;
    let current: unknown[] = [SESSION];
    mockElectron(() => current);
    let finish: (row: unknown) => void = () => {};
    const rescore = vi.fn(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    const retry = vi.fn();
    const publish = vi.fn(async () => ({ ok: true, url: '/agentic-benchmarks/run/brun-1' }));
    electron().benchmarkRescore = rescore;
    electron().benchmarkRetryScoring = retry;
    electron().benchmarkPublish = publish;
    mount();

    expect(await screen.findByTestId('published-live')).toHaveTextContent('Luna first pass');
    const panel = screen.getByTestId('rescore');
    expect(panel).toHaveTextContent('No model calls');
    expect(panel).toHaveTextContent('replaces the stored one only if scoring succeeds');
    const button = within(panel).getByRole('button', { name: 'Re-score saved build' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(rescore).toHaveBeenCalledWith(RUN_ID);
    expect(retry).not.toHaveBeenCalled();
    // A re-score in progress is a run in progress: the action is held, with the reason.
    await waitFor(() =>
      expect(
        within(screen.getByTestId('rescore')).getByRole('button', { name: 'Re-score saved build' })
      ).toBeDisabled()
    );
    expect(screen.getByTestId('rescore')).toHaveTextContent(
      'Waiting for the run in progress to finish.'
    );

    const rescoredAt = '2026-10-03T10:00:00.000Z';
    current = [
      {
        ...SESSION,
        score: 0.61,
        publishable: true,
        rescoredAt,
      },
    ];
    finish({ ...MINE, score: 0.61, runMeta: { ...RUN_META, rescoredAt } });

    await waitFor(() => expect(scoreStat()).toBe('61.0%'));
    expect(await screen.findByText('Re-score complete.')).toBeInTheDocument();
    expect(screen.getByTestId('rescored-at')).toHaveTextContent('This result was re-scored');
    // Posted before, re-scored since: the board entry is offered for replacement, not shown as live.
    expect(screen.queryByTestId('published-live')).toBeNull();
    expect(screen.getByTestId('republish-note')).toHaveTextContent(
      'Re-scored since it was posted (“Luna first pass” · 42.0%). Publishing replaces that board entry with this result.'
    );
    fireEvent.change(screen.getByRole('textbox', { name: /Title/ }), {
      target: { value: 'Luna, re-scored' },
    });
    const publishButton = screen.getByRole('button', { name: 'Publish' });
    await waitFor(() => expect(publishButton).toBeEnabled());
    fireEvent.click(publishButton);
    await waitFor(() =>
      expect(publish).toHaveBeenCalledWith({ title: 'Luna, re-scored', runKey: RUN_ID })
    );
  });

  it('a failed re-score keeps the previous result and says so', async () => {
    window.location.hash = `#/benchmark?era=sb-7.2&run=${RUN_ID}`;
    let current: unknown[] = [SESSION];
    mockElectron(() => current);
    electron().benchmarkRescore = vi.fn(async () => {
      current = [{ ...SESSION, scoringError: 'Scoring failed (exit 1). probe refused' }];
      throw new Error('Re-score did not finish. The previous result is kept.');
    });
    mount();
    const button = await screen.findByRole('button', { name: 'Re-score saved build' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(
      await screen.findByText('Re-score did not finish — the previous result is kept.')
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId('rescore')).toHaveTextContent(
        'The last re-score did not finish — the result shown is the previous one, unchanged.'
      )
    );
    expect(scoreStat()).toBe('42.0%');
    expect(screen.getByTestId('published-live')).toHaveTextContent('Luna first pass');
  });

  it('a finished run without a completed-build receipt says why there is no Re-score', async () => {
    window.location.hash = `#/benchmark?era=sb-7.2&run=${RUN_ID}`;
    mockElectron(() => [
      {
        ...SESSION,
        rescore: { ready: false, reason: 'No completed-build receipt was recorded for this run.' },
      },
    ]);
    mount();
    expect(await screen.findByTestId('rescore')).toHaveTextContent(
      'Re-score is not available: No completed-build receipt was recorded for this run.'
    );
    expect(screen.queryByRole('button', { name: 'Re-score saved build' })).toBeNull();
  });

  it('offers the same Re-score on a finished Forge run, through the same door', async () => {
    window.location.hash = `#/benchmark?era=forge-1.0-rc&run=cloud-forge-1`;
    const forgeSession = {
      ...SESSION,
      runId: 'cloud-forge-1',
      scorerVersion: 'forge-1.0-rc',
      published: undefined,
      publishable: true,
    };
    mockElectron(() => [forgeSession], {
      ...MINE,
      runId: 'cloud-forge-1',
      scorerVersion: 'forge-1.0-rc',
      tiers: {},
    });
    const rescore = vi.fn(() => new Promise(() => {}));
    electron().benchmarkRescore = rescore;
    mount();
    const button = await screen.findByRole('button', { name: 'Re-score saved build' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(rescore).toHaveBeenCalledWith('cloud-forge-1');
  });
});
