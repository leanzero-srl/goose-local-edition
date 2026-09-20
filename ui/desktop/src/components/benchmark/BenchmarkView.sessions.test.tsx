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
      scorerVersion: 'sb-7.0-rc',
      title: 'Meridian Payments Console',
      current: true,
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
      await screen.findByText('No completed-build receipt was recorded for this run.')
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
    fireEvent.click(screen.getByRole('button', { name: 'SB7 · legacy' }));
    fireEvent.click(screen.getByRole('button', { name: 'Run benchmark' }));
    await waitFor(() =>
      expect(cloud).toHaveBeenLastCalledWith('google', 'gemini-3.8-flash', 'sb-7')
    );
  });

  it('renders all four outcomes honestly — running pulses, finished carries its score, the dead ones say so', async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // Every state names itself: the four chips of the current (expanded) era.
    const runningChip = (await screen.findByText('Running')).closest('span')!;
    // The live mark scales on the motion token (DESIGN.md) — never the fading pulse.
    expect(runningChip.querySelector('.animate-lz-live')).not.toBeNull();
    expect(runningChip.querySelector('.animate-pulse')).toBeNull();
    expect(screen.getByText('Finished')).toBeInTheDocument();
    expect(screen.getByText(/· 2\.7%/)).toBeInTheDocument();
    expect(screen.getByText('Did not finish')).toBeInTheDocument();
    expect(screen.getByText('Did not start')).toBeInTheDocument();

    // The era badges come from the catalog: current is runnable, frozen only viewable.
    expect(screen.getByText('CURRENT')).toBeInTheDocument();
    expect(screen.getByText('FROZEN')).toBeInTheDocument();
    expect(screen.getByText('Meridian Payments Console')).toBeInTheDocument();

    // The running session is selected by default and its detail promises, never invents:
    expect(screen.getByText(/result lands here when the run finishes/i)).toBeInTheDocument();
  });

  it("a finished session's detail compares against the catalog's retrieved baselines for ITS era", async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    fireEvent.click((await screen.findByText('Finished')).closest('button')!);
    // The comparison row is RETRIEVED (catalog), and it is the era's own board — never sb-6's.
    // The Board table and the Overall bars both carry the row — one comparison, two registers.
    await screen.findAllByText('Claude Opus 5');
    expect(screen.queryByText('GPT-5.6 Sol')).toBeNull();
    // The session's own row and score render from the stored session, not a baked table
    // (the stat tile and the chart's own bar both carry it).
    expect(screen.getAllByText('2.7%').length).toBeGreaterThanOrEqual(1);
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
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // Newest-first in the era: s-fin (Aug 29) renders before s-fin-old (Aug 26). The NEWER
    // finished session must not borrow the stored result's runMeta tiles…
    const finishedChips = await screen.findAllByText('Finished');
    fireEvent.click(finishedChips[0].closest('button')!);
    // The Board table and the Overall bars both carry the row — one comparison, two registers.
    await screen.findAllByText('Claude Opus 5');
    expect(screen.queryByText('Repair rounds')).toBeNull();

    // …while the session the row NAMES carries them.
    fireEvent.click(screen.getAllByText('Finished')[1].closest('button')!);
    await screen.findByText('Repair rounds');
    expect(screen.getByText('Engine events')).toBeInTheDocument();
    expect(screen.getByText('424')).toBeInTheDocument();
  });

  it('deletes a session through the custom confirm dialog, never a native confirm', async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The running session cannot be deleted (and its runId is still null — the label falls back
    // to the startedAt stamp); a dead one can.
    const liveDelete = await screen.findByLabelText('Delete session 2026-08-30T10:00:00.000Z');
    expect(liveDelete).toBeDisabled();

    fireEvent.click(screen.getByLabelText('Delete session s-dnf'));
    await screen.findByText('Delete this benchmark session?');
    fireEvent.click(screen.getByRole('button', { name: 'Delete session' }));

    await waitFor(() =>
      expect((electron().benchmarkDeleteSession as ReturnType<typeof vi.fn>).mock.calls).toEqual([
        ['s-dnf'],
      ])
    );
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
    const notice = await screen.findByText(/Runs use the bundled scorer/);
    expect(notice.textContent).toContain('sb-8.0');
    expect(notice.textContent).toContain('sb-7.0-rc');

    // A later launch with NO mismatch clears the notice — the event stream updates the claim.
    handlers.get('benchmark-started')?.(null, {
      workdir: '/tmp/bench',
      startedAt: '2026-08-30T11:00:00.000Z',
      sampling: {},
    });
    await waitFor(() => expect(screen.queryByText(/Runs use the bundled scorer/)).toBeNull());
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
