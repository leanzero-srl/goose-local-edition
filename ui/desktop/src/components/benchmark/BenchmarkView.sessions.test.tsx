import { act, render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE SESSION LIST TELLS THE TRUTH. Each benchmark era is an expand/collapse section retrieved
 * from the site's catalog; under it, every session states its REAL outcome — a run that died
 * before scoring says "Did not finish", one that never launched says "Did not start" — never a
 * clean pass by omission. Deleting goes through the app's own confirm dialog (no native confirm),
 * and when the catalog is unreachable the view says so and renders NO comparison rows: the baked
 * baseline boards this replaced are gone, so there is nothing left to silently substitute.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});

vi.mock('../swarm/useSamplingDefaults', () => ({
  useSaveSamplingDefaults: () => () => {},
}));

vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: vi.fn(async () => [
    {
      name: 'google',
      is_configured: true,
      metadata: { display_name: 'Google Gemini', known_models: [] },
    },
  ]),
}));

import BenchmarkView from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const CATALOG = {
  ok: true,
  fetchedAt: '2026-08-30T08:00:00.000Z',
  stale: false,
  benchmarks: [
    {
      scorerVersion: 'sb-7.1',
      title: 'SB7.1 payments',
      current: true,
      frozen: false,
      baselines: [],
    },
    {
      scorerVersion: 'sb-7.0-rc',
      title: 'Meridian Payments Console',
      current: false,
      frozen: false,
      baselines: [{ label: 'Claude Opus 5', score: 0.9142, model: 'claude-opus-5' }],
    },
    {
      scorerVersion: 'sb-6.0',
      title: 'VendorSync Pro',
      current: false,
      frozen: true,
      baselines: [{ label: 'GPT-5.6 Sol', score: 0.9956, model: 'gpt-5.6-sol' }],
    },
  ],
};

const SESSIONS = [
  {
    // The just-launched shape: runId is null until .swarm/current-run.json reconciles (~2s in).
    // The row must still key stably (by startedAt) and must refuse deletion.
    runId: null,
    scorerVersion: 'sb-7.0-rc',
    startedAt: '2026-08-30T10:00:00.000Z',
    outcome: 'running',
    publishable: false,
  },
  {
    runId: 's-fin',
    scorerVersion: 'sb-7.0-rc',
    startedAt: '2026-08-29T10:00:00.000Z',
    endedAt: '2026-08-29T12:30:00.000Z',
    outcome: 'finished',
    score: 0.0273,
    tiers: { A: 0.4625, B: 0.2247, C: 0.0, D: 0.5571 },
    publishable: true,
  },
  {
    runId: 's-dnf',
    scorerVersion: 'sb-7.0-rc',
    startedAt: '2026-08-28T10:00:00.000Z',
    endedAt: '2026-08-28T11:00:00.000Z',
    outcome: 'did_not_finish',
    publishable: false,
  },
  {
    runId: 's-dns',
    scorerVersion: 'sb-7.0-rc',
    startedAt: '2026-08-27T10:00:00.000Z',
    outcome: 'did_not_start',
    publishable: false,
  },
  {
    runId: 's-old',
    scorerVersion: 'sb-6.0',
    startedAt: '2026-08-19T10:00:00.000Z',
    endedAt: '2026-08-19T11:00:00.000Z',
    outcome: 'finished',
    score: 0.8635,
    tiers: { A: 1.0, B: 1.0, C: 1.0, D: 0.86 },
    publishable: false,
  },
];

function mockElectron(opts: { catalog?: unknown; sessions?: unknown[] } = {}) {
  const e = electron();
  e.benchmarkRuntimeStatus = vi.fn(async () => ({ state: 'ready', downloadBytes: 0 }));
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  e.benchmarkRead = vi.fn(async () => null);
  e.benchmarkShots = vi.fn(async () => []);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog = 'catalog' in opts ? opts.catalog : vi.fn(async () => CATALOG);
  e.benchmarkSessions = vi.fn(async () => ({ sessions: opts.sessions ?? SESSIONS }));
  e.benchmarkDeleteSession = vi.fn(async () => ({ ok: true }));
}

describe('the benchmark sections and their sessions', () => {
  afterEach(() => cleanup());

  it('retries only scoring for a receipt-backed build and keeps technical failures expandable', async () => {
    const session = {
      runId: 'cloud-retry',
      scorerVersion: 'sb-7.1',
      startedAt: '2026-09-20T10:00:00Z',
      outcome: 'did_not_finish',
      publishable: false,
      retryScoring: { ready: true },
      scoringError: 'Traceback /private/example/threading.py: Event object is not callable',
    };
    mockElectron({ sessions: [session] });
    const retry = vi.fn(() => new Promise(() => {}));
    const cloud = vi.fn();
    const swarm = vi.fn();
    electron().benchmarkRetryScoring = retry;
    electron().benchmarkRunCloud = cloud;
    electron().benchmarkRun = swarm;
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    expect(await screen.findByText('Model build completed. Scoring did not finish.')).toBeVisible();
    expect(screen.queryByText(session.scoringError)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Technical details' }));
    expect(screen.getByText(session.scoringError)).toHaveClass('break-all');
    fireEvent.click(screen.getByRole('button', { name: 'Retry scoring' }));
    expect(retry).toHaveBeenCalledWith('cloud-retry');
    expect(cloud).not.toHaveBeenCalled();
    expect(swarm).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Retry scoring' })).toBeDisabled();
  });

  it('does not offer retry or claim build completion for legacy usage-only evidence', async () => {
    mockElectron({
      sessions: [
        {
          runId: 'legacy',
          scorerVersion: 'sb-7.1',
          startedAt: '2026-09-20T10:00:00Z',
          outcome: 'did_not_finish',
          publishable: false,
          retryScoring: {
            ready: false,
            reason: 'No completed-build receipt was recorded for this run.',
          },
        },
      ],
    });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    expect(
      await screen.findByText(
        'Retry scoring is not available: No completed-build receipt was recorded for this run.'
      )
    ).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Retry scoring' })).toBeNull();
    expect(screen.queryByText(/Model build completed/)).toBeNull();
  });

  it('launches the exact Gemini model as one cloud entrant, without a swarm launch', async () => {
    mockElectron({ sessions: [] });
    const cloud = vi.fn(async () => null);
    const swarm = vi.fn(async () => null);
    electron().benchmarkRunCloud = cloud;
    electron().benchmarkRun = swarm;
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    fireEvent.click(screen.getByRole('button', { name: 'Single model' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Model provider' })).toBeEnabled()
    );
    fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Google Gemini' }));
    expect(screen.getByRole('button', { name: 'Run benchmark' })).toBeDisabled();
    fireEvent.change(screen.getByRole('textbox', { name: 'Model ID' }), {
      target: { value: 'gemini-3.8-flash' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Run benchmark' }));
    await waitFor(() => expect(cloud).toHaveBeenCalledWith('google', 'gemini-3.8-flash', 'sb-7.1'));
    expect(swarm).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'SB7 · legacy' })).toBeNull();
    expect(cloud).toHaveBeenCalledTimes(1);
  });

  it('the selected run (the running one by default) renders under its era with its badges; the run list itself lives in the sidebar', async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The running session is selected by default: ONE chip for the selected run, the era's title
    // and badge beside it, and a detail that promises, never invents.
    const runningChip = (await screen.findByText('Running')).closest('span')!;
    // The live mark scales on the motion token (DESIGN.md) — never the fading pulse.
    expect(runningChip.querySelector('.animate-lz-live')).not.toBeNull();
    expect(runningChip.querySelector('.animate-pulse')).toBeNull();
    expect(screen.getByText('Meridian Payments Console')).toBeInTheDocument();
    expect(screen.getByText(/result lands here when the run finishes/i)).toBeInTheDocument();
    // No second list of runs in the main view (one list, in the sidebar).
    expect(screen.queryByText('Did not finish')).toBeNull();
    expect(screen.queryByText('Did not start')).toBeNull();
    // The benchmark is a dropdown at run setup, its value the current benchmark.
    const chooser = screen.getByRole('combobox', { name: 'Benchmark' });
    expect(chooser.textContent).toContain('sb-7.1');
    fireEvent.click(chooser);
    const options = screen.getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual([
      'sb-7.1 — SB7.1 payments',
      'sb-7.0-rc — Meridian Payments Console (history)',
      'sb-6.0 — VendorSync Pro (frozen)',
    ]);
    expect(options[1].getAttribute('aria-disabled')).toBe('true');
    expect(options[2].getAttribute('aria-disabled')).toBe('true');
  });

  it("a finished session's detail compares against the catalog's retrieved baselines for ITS era", async () => {
    mockElectron();
    window.location.hash = '#/benchmark?era=sb-7.0-rc&run=s-fin';
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await screen.findByText('Finished');
    // The comparison row is RETRIEVED (catalog), and it is the era's own board — never sb-6's.
    // The Board table and the Overall bars both carry the row — one comparison, two registers.
    await screen.findAllByText('Claude Opus 5');
    expect(screen.queryByText('GPT-5.6 Sol')).toBeNull();
    // The session's own row and score render from the stored session, not a baked table
    // (the stat tile and the chart's own bar both carry it).
    expect(screen.getAllByText('2.7%').length).toBeGreaterThanOrEqual(1);
    window.location.hash = '';
  });

  it('a stored result carrying runId joins its OWN session exactly — never the newer finished sibling', async () => {
    // Two finished sb-7 sessions; mine/result.json NAMES the older one by runId. The exact join
    // must win over the newest-finished-of-era heuristic (kept only for pre-runId rows).
    const olderFinished = {
      runId: 's-fin-old',
      scorerVersion: 'sb-7.0-rc',
      startedAt: '2026-08-26T09:00:00.000Z',
      endedAt: '2026-08-26T10:30:00.000Z',
      outcome: 'finished',
      score: 0.51,
      tiers: { A: 0.5, B: 0.5, C: 0.5, D: 0.5 },
      publishable: false,
    };
    mockElectron({ sessions: [...SESSIONS, olderFinished] });
    electron().benchmarkRead = vi.fn(async () => ({
      label: 'Your fleet · 3 nodes',
      score: 0.51,
      tiers: { A: 0.5, B: 0.5, C: 0.5, D: 0.5 },
      nodes: 3,
      mine: true,
      scorerVersion: 'sb-7.0-rc',
      runId: 's-fin-old',
      runMeta: {
        startedAt: '2026-08-26T09:00:00.000Z',
        finishedAt: '2026-08-26T10:30:00.000Z',
        engineEvents: 424,
        repairRounds: 2,
      },
    }));
    window.location.hash = '#/benchmark?era=sb-7.0-rc&run=s-fin';
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The NEWER finished session (s-fin, Aug 29) must not borrow the stored result's runMeta tiles…
    await screen.findAllByText('Claude Opus 5');
    expect(screen.queryByText('Repair rounds')).toBeNull();

    // …while the session the row NAMES carries them (the sidebar selects it by URL).
    window.location.hash = '#/benchmark?era=sb-7.0-rc&run=s-fin-old';
    window.dispatchEvent(new Event('hashchange'));
    await screen.findByText('Repair rounds');
    expect(screen.getByText('Engine events')).toBeInTheDocument();
    expect(screen.getByText('424')).toBeInTheDocument();
    window.location.hash = '';
  });

  it('deletes the selected session through the custom confirm dialog, never a native confirm', async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The running session (selected by default) cannot be deleted; its runId is still null so the
    // label falls back to the startedAt stamp.
    const liveDelete = await screen.findByLabelText('Delete session 2026-08-30T10:00:00.000Z');
    expect(liveDelete).toBeDisabled();

    window.location.hash = '#/benchmark?era=sb-7.0-rc&run=s-dnf';
    window.dispatchEvent(new Event('hashchange'));
    fireEvent.click(await screen.findByLabelText('Delete session s-dnf'));
    await screen.findByText('Delete this benchmark session?');
    fireEvent.click(screen.getByRole('button', { name: 'Delete session' }));

    await waitFor(() =>
      expect((electron().benchmarkDeleteSession as ReturnType<typeof vi.fn>).mock.calls).toEqual([
        ['s-dnf'],
      ])
    );
    window.location.hash = '';
  });

  it('renders the catalog-mismatch notice from benchmark-started — the site moved on, the app has not', async () => {
    mockElectron();
    const handlers = new Map<string, (e: unknown, payload: unknown) => void>();
    electron().on = vi.fn((channel: string, cb: (e: unknown, payload: unknown) => void) => {
      handlers.set(channel, cb);
    });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await screen.findByText('Meridian Payments Console');

    handlers.get('benchmark-started')?.(null, {
      workdir: '/tmp/bench',
      startedAt: '2026-08-30T10:00:00.000Z',
      sampling: {},
      tier: 'sb-7',
      scorerVersion: 'sb-7.0-rc',
      catalogMismatch: { siteCurrent: 'sb-8.0', bundled: 'sb-7.0-rc' },
    });
    const notice = await screen.findByText(/Update Goose before starting another run/);
    expect(notice.textContent).toContain('sb-8.0');
    expect(notice.textContent).toContain('sb-7.0-rc');

    // A later launch with NO mismatch clears the notice — the event stream updates the claim.
    handlers.get('benchmark-started')?.(null, {
      workdir: '/tmp/bench',
      startedAt: '2026-08-30T11:00:00.000Z',
      sampling: {},
    });
    await waitFor(() =>
      expect(screen.queryByText(/Update Goose before starting another run/)).toBeNull()
    );
  });

  it('states the catalog absence LOUDLY and renders no comparison rows — never invented bars', async () => {
    mockElectron({ catalog: undefined, sessions: [SESSIONS[1]] });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The named absence, top-level AND where the comparison chart would have been.
    const notices = await screen.findAllByText(/Catalog unreachable — no comparison rows/);
    expect(notices.length).toBeGreaterThanOrEqual(2);
    // No baked board sneaks back in as a stand-in.
    expect(screen.queryByText('Claude Opus 5')).toBeNull();
    expect(screen.queryByText('GPT-5.6 Sol')).toBeNull();
    // The session itself still renders with its honest state.
    expect(screen.getByText('Finished')).toBeInTheDocument();
  });
});

it('joins the cloud result label and SB8 details to the exact session', async () => {
  const verdict = (await import('./sb8-failed.fixture.json')).default;
  mockElectron({
    sessions: [
      {
        runId: 'cloud-fixture',
        scorerVersion: verdict.scorerVersion,
        startedAt: '2026-09-20T10:00:00Z',
        endedAt: '2026-09-20T10:04:00Z',
        outcome: 'finished',
        score: verdict.score,
        tiers: { A: 1, B: 0.97, C: 1, D: 1 },
        publishable: false,
      },
    ],
  });
  electron().benchmarkRead = vi.fn(async () => ({
    label: 'gemini-3.8-flash · single agent',
    modelId: 'gemini-3.8-flash',
    runId: 'cloud-fixture',
    scorerVersion: verdict.scorerVersion,
    score: verdict.score,
    tiers: verdict.tiers,
    verdict,
  }));
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  expect((await screen.findAllByText('gemini-3.8-flash · single agent')).length).toBeGreaterThan(0);
  expect(screen.queryByText('Your fleet')).toBeNull();
  expect(screen.queryByText(/60% core build/)).toBeNull();
  expect(await screen.findByText('Swept collision')).toBeInTheDocument();
  expect(screen.getByText('E 100%')).toBeInTheDocument();
  cleanup();
});

it('can cancel a cloud run while its launch IPC promise remains pending', async () => {
  mockElectron({ sessions: [] });
  let finish!: () => void;
  let settled = false;
  const pending = new Promise<null>((resolve) => {
    finish = () => {
      settled = true;
      resolve(null);
    };
  });
  electron().benchmarkRunCloud = vi.fn(() => pending);
  const cancel = vi.fn(async () => ({ ok: true }));
  electron().benchmarkCancel = cancel;
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  fireEvent.click(screen.getByRole('button', { name: 'Single model' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Model provider' })).toBeEnabled());
  fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
  fireEvent.click(await screen.findByRole('menuitem', { name: 'Google Gemini' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'Model ID' }), {
    target: { value: 'gemini-3.8-flash' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Run benchmark' }));
  const stop = await screen.findByRole('button', { name: 'Cancel run' });
  expect(settled).toBe(false);
  expect(stop).toBeEnabled();
  expect(stop).not.toHaveAttribute('aria-busy', 'true');
  fireEvent.click(stop);
  fireEvent.click(await screen.findByRole('button', { name: 'Cancel the run' }));
  await waitFor(() => expect(cancel).toHaveBeenCalledOnce());
  expect(settled).toBe(false);
  expect(screen.getByRole('button', { name: 'Cancelling…' })).toBeDisabled();
  await act(async () => finish());
  expect(await screen.findByRole('button', { name: 'Run benchmark' })).toBeEnabled();
  cleanup();
});

it('restores the cloud scoring stage without a swarm event log', async () => {
  mockElectron({ sessions: [] });
  electron().benchmarkStatus = vi.fn(async () => ({
    running: true,
    workdir: '/cloud/run',
    provider: 'google',
    phase: 'score',
    startedAt: '2026-09-20T10:00:00Z',
  }));
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  await waitFor(() =>
    expect(screen.getByText('Scoring').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'accent'
    )
  );
  expect(screen.getByText('Model build').closest('[data-tone]')).toHaveAttribute('data-tone', 'ok');
  expect(screen.getByRole('button', { name: 'Cancel run' })).toBeEnabled();
  cleanup();
});

it('shows measured cloud build and scoring independently without invented swarm counts', async () => {
  const session = {
    runId: 'cloud-measured',
    scorerVersion: 'sb-7.1',
    startedAt: '2026-09-20T10:00:00Z',
    endedAt: '2026-09-20T10:13:00Z',
    outcome: 'finished',
    score: 0.699,
    publishable: false,
  };
  mockElectron({ sessions: [session] });
  electron().benchmarkRead = vi.fn(async () => ({
    ...session,
    label: 'model · single agent',
    provider: 'google',
    wallSecs: 554,
    scoringSecs: 226.3,
    runMeta: {
      startedAt: session.startedAt,
      finishedAt: session.endedAt,
      engineEvents: 0,
      repairRounds: 0,
    },
  }));
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  expect((await screen.findAllByText('9m 14s')).length).toBeGreaterThan(0);
  expect(screen.getByText('3m 46s')).toBeInTheDocument();
  expect(screen.queryByText('Engine events')).toBeNull();
  expect(screen.queryByText('Repair rounds')).toBeNull();
  cleanup();
});

it('updates cloud pipeline stages from harness events, not model prose', async () => {
  mockElectron({ sessions: [] });
  const handlers = new Map<string, (event: unknown, payload: unknown) => void>();
  electron().on = vi.fn((channel: string, cb: (event: unknown, payload: unknown) => void) =>
    handlers.set(channel, cb)
  );
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  await act(async () =>
    handlers.get('benchmark-started')?.(null, {
      workdir: '/cloud/run',
      provider: 'google',
      phase: 'boot',
    })
  );
  expect(screen.getByText('Prepare').closest('[data-tone]')).toHaveAttribute('data-tone', 'accent');
  await act(async () =>
    handlers.get('benchmark-log')?.(null, { line: 'model says done', phase: 'build' })
  );
  expect(screen.getByText('Model build').closest('[data-tone]')).toHaveAttribute(
    'data-tone',
    'accent'
  );
  await act(async () =>
    handlers.get('benchmark-log')?.(null, { line: 'harness scoring', phase: 'score' })
  );
  expect(screen.getByText('Scoring').closest('[data-tone]')).toHaveAttribute('data-tone', 'accent');
  cleanup();
});

it('refuses Run until the user explicitly installs missing benchmark tools', async () => {
  mockElectron({ sessions: [] });
  const start = vi.fn(async () => null);
  const install = vi.fn(async () => {});
  electron().benchmarkRun = start;
  electron().benchmarkRuntimeInstall = install;
  electron().benchmarkRuntimeStatus = vi
    .fn()
    .mockResolvedValueOnce({ state: 'missing', downloadBytes: 49092883 })
    .mockResolvedValueOnce({ state: 'ready', downloadBytes: 49092883 });
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  await screen.findByText(/46.8 MiB download/);
  const run = screen.getByRole('button', { name: 'Run benchmark' });
  expect(run).toBeDisabled();
  expect(install).not.toHaveBeenCalled();
  fireEvent.click(run);
  expect(start).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Install benchmark tools' }));
  await waitFor(() => expect(run).toBeEnabled());
  expect(install).toHaveBeenCalledOnce();
  fireEvent.click(run);
  await waitFor(() => expect(start).toHaveBeenCalled());
  cleanup();
});

it.each([
  [
    'newer stable release',
    { ...CATALOG, benchmarks: [{ ...CATALOG.benchmarks[0], scorerVersion: 'sb-8.0' }] },
    /Update Goose to run/,
  ],
  ['cached catalog', { ...CATALOG, stale: true }, /Connect to leanzero.net/],
  [
    'experimental current release',
    { ...CATALOG, benchmarks: [{ ...CATALOG.benchmarks[0], scorerVersion: 'sb-8.0-rc' }] },
    /no single available stable benchmark/,
  ],
] as const)('blocks launching against %s', async (_name, catalog, message) => {
  mockElectron({ catalog: vi.fn(async () => catalog), sessions: [] });
  const start = vi.fn();
  electron().benchmarkRun = start;
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );
  expect(await screen.findByText(message)).toBeVisible();
  const run = screen.getByRole('button', { name: 'Run benchmark' });
  expect(run).toBeDisabled();
  fireEvent.click(run);
  expect(start).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Refresh benchmark' })).toBeEnabled();
  expect(screen.getByText(/Bundled benchmark/)).toBeInTheDocument();
  expect(screen.queryByText(/Latest stable benchmark/)).not.toBeInTheDocument();
  cleanup();
});

describe('the benchmark view says what it shows (UX audit B1)', () => {
  afterEach(() => {
    window.location.hash = '';
    cleanup();
  });

  const RC_DNF = {
    runId: 'rc-dnf',
    scorerVersion: 'sb-7.0-rc',
    startedAt: '2026-09-20T19:54:00.000Z',
    endedAt: '2026-09-20T20:30:00.000Z',
    outcome: 'did_not_finish',
    publishable: false,
    retryScoring: {
      ready: false,
      reason: 'Only SB7.1 runs can be rescored; this run is sb-7.0-rc.',
    },
  };

  it("an earlier era's run is labelled history and the page states the current era has no runs", async () => {
    mockElectron({ sessions: [RC_DNF] });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    expect(await screen.findByText('Meridian Payments Console')).toBeInTheDocument();
    expect(screen.getByText('history')).toBeInTheDocument();
    expect(screen.queryByText('CURRENT')).toBeNull();
    expect(screen.getByTestId('era-note')).toHaveTextContent(
      'This run is from an earlier benchmark (sb-7.0-rc). The current benchmark, SB7.1 payments (sb-7.1), has no runs on this machine yet.'
    );
  });

  it('an old did-not-finish is the header chip plus one line — no full-width band; the retry sentence says why it is absent', async () => {
    mockElectron({ sessions: [RC_DNF] });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const line = await screen.findByTestId('dnf-line');
    expect(line).toHaveTextContent('this run ended without a score.');
    expect(screen.queryByTestId('tone-band')).toBeNull();
    expect(
      screen.getByText(
        'Retry scoring is not available: Only SB7.1 runs can be rescored; this run is sb-7.0-rc.'
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Retry scoring' })).toBeNull();
  });

  it('a run this view watched fail keeps the full-width band', async () => {
    const live = { ...RC_DNF, outcome: 'running', endedAt: undefined, retryScoring: undefined };
    let current: unknown[] = [live];
    mockElectron({ sessions: current });
    electron().benchmarkSessions = vi.fn(async () => ({ sessions: current }));
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await screen.findByText(/result lands here when the run finishes/i);
    // The run ends: main's terminal event makes the view re-read its sessions.
    current = [RC_DNF];
    const handlers = (window.electron.on as ReturnType<typeof vi.fn>).mock.calls.filter(
      ([name]) => name === 'benchmark-finished'
    );
    const onFinished = handlers[handlers.length - 1][1] as (e: unknown, p: unknown) => void;
    await act(async () => {
      onFinished(null, {});
    });
    await waitFor(() =>
      expect(
        screen
          .getAllByTestId('tone-band')
          .some((b) => b.textContent?.includes('this run ended without a score.'))
      ).toBe(true)
    );
    expect(screen.queryByTestId('dnf-line')).toBeNull();
  });

  it('?new=1 lands on the run setup: ringed, focused, and no old run headlines the page', async () => {
    window.location.hash = '#/benchmark?new=1';
    mockElectron({ sessions: [RC_DNF] });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const setup = await screen.findByRole('region', { name: 'Run setup' });
    await waitFor(() => expect(setup).toHaveFocus());
    expect(setup).toHaveAttribute('data-new-run', 'true');
    expect(setup.className).toContain('ring-lz-accent');
    expect(screen.getByRole('heading', { name: 'New run' })).toBeInTheDocument();
    await waitFor(() => expect(electron().benchmarkSessions).toHaveBeenCalled());
    expect(screen.queryByTestId('dnf-line')).toBeNull();
    expect(screen.queryByText('Meridian Payments Console')).toBeNull();
  });

  it("?era= without a run opens that era's newest run", async () => {
    window.location.hash = '#/benchmark?era=sb-6.0';
    // No live run (a live run wins the page while it runs).
    mockElectron({ sessions: SESSIONS.filter((s) => s.outcome !== 'running') });
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    expect(await screen.findByText('VendorSync Pro')).toBeInTheDocument();
  });
});
