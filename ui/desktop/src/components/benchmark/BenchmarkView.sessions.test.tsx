import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';
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
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  e.benchmarkRead = vi.fn(async () => null);
  e.benchmarkShots = vi.fn(async () => []);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog =
    'catalog' in opts ? opts.catalog : vi.fn(async () => CATALOG);
  e.benchmarkSessions = vi.fn(async () => ({ sessions: opts.sessions ?? SESSIONS }));
  e.benchmarkDeleteSession = vi.fn(async () => ({ ok: true }));
}

describe('the benchmark sections and their sessions', () => {
  afterEach(() => cleanup());

  it('renders all four outcomes honestly — running pulses, finished carries its score, the dead ones say so', async () => {
    mockElectron();
    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // Every state names itself: the four chips of the current (expanded) era.
    const runningChip = (await screen.findByText('Running')).closest('span')!;
    expect(runningChip.querySelector('.animate-pulse')).not.toBeNull();
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
    await screen.findByText('Claude Opus 5');
    expect(screen.queryByText('GPT-5.6 Sol')).toBeNull();
    // The session's own row and score render from the stored session, not a baked table
    // (the stat tile and the chart's own bar both carry it).
    expect(screen.getAllByText('2.7%').length).toBeGreaterThanOrEqual(1);
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
      expect(
        (electron().benchmarkDeleteSession as ReturnType<typeof vi.fn>).mock.calls
      ).toEqual([['s-dnf']])
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
    const notice = await screen.findByText(/needs an app update/);
    expect(notice.textContent).toContain('sb-8.0');
    expect(notice.textContent).toContain('sb-7.0-rc');

    // A later launch with NO mismatch clears the notice — the event stream updates the claim.
    handlers.get('benchmark-started')?.(null, {
      workdir: '/tmp/bench',
      startedAt: '2026-08-30T11:00:00.000Z',
      sampling: {},
    });
    await waitFor(() => expect(screen.queryByText(/needs an app update/)).toBeNull());
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
