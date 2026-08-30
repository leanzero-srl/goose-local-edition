import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * FINDING 17 of the frontend truth review: `scored` was BenchmarkView component state set by a
 * renderer-side regex over scorer stdout, so a window recreated mid-run re-initialized it false and
 * the pipeline strip regressed a finished 'done' to a spinning 'score' — an active-work claim for
 * completed work, with no log line left to re-derive the truth from. main owns the fact now and the
 * re-attach path restores both `scored` and `lastLine` from benchmark-status; live updates carry
 * `scored` on every benchmark-log payload instead of a second regex.
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

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: '2026-08-17T13:54:13.000000+00:00' },
  { event: 'phase', phase: 'open' },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const LAST_LINE = 'rep0 (swarm-3node) score=61.0';

describe('BenchmarkView — the scored fact survives a window recreation', () => {
  beforeEach(() => {
    const e = electron();
    e.benchmarkRead = vi.fn(async () => null);
    e.benchmarkIdentity = vi.fn(async () => ({ handle: 'mihai' }));
    e.benchmarkShots = vi.fn(async () => []);
    e.readSwarmRun = vi.fn(async (dir: string) => ({
      runId: 'swarm-bench',
      dir,
      events: EVENTS,
      activity: {},
      activityMtimes: {},
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    e.fleetStatus = vi.fn(async () => ({}));
    e.on = vi.fn();
    e.off = vi.fn();
  });

  it("restores scored + lastLine from benchmark-status, so the strip re-mounts at 'done', not a spinning 'score'", async () => {
    electron().benchmarkStatus = vi.fn(async () => ({
      running: true,
      workdir: '/tmp/bench',
      startedAt: '2026-08-17T13:54:13.000Z',
      sampling: {},
      scored: true,
      lastLine: LAST_LINE,
    }));

    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    // The restored last output line renders without a single fresh benchmark-log event.
    expect(await screen.findByText(LAST_LINE)).toBeInTheDocument();
    // 'Done' is the ACTIVE chip (it carries the spinner glyph); 'Scoring' is settled — no spinner.
    const done = (await screen.findByText('Done')).closest('span')!;
    const scoring = screen.getByText('Scoring').closest('span')!;
    await waitFor(() => expect(done.querySelector('.animate-spin')).not.toBeNull());
    expect(scoring.querySelector('.animate-spin')).toBeNull();
  });

  it('a run that has not scored yet re-attaches onto the live phase, not onto done', async () => {
    electron().benchmarkStatus = vi.fn(async () => ({
      running: true,
      workdir: '/tmp/bench',
      startedAt: '2026-08-17T13:54:13.000Z',
      sampling: {},
      scored: false,
      lastLine: 'building…',
    }));

    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );

    expect(await screen.findByText('building…')).toBeInTheDocument();
    const done = screen.getByText('Done').closest('span')!;
    expect(done.querySelector('.animate-spin')).toBeNull();
  });

  it('flips to done off the scored flag riding a benchmark-log payload — no renderer regex', async () => {
    electron().benchmarkStatus = vi.fn(async () => ({
      running: true,
      workdir: '/tmp/bench',
      startedAt: '2026-08-17T13:54:13.000Z',
      sampling: {},
      scored: false,
      lastLine: null,
    }));
    const handlers = new Map<string, (e: unknown, payload: unknown) => void>();
    electron().on = vi.fn((channel: string, cb: (e: unknown, payload: unknown) => void) => {
      handlers.set(channel, cb);
    });

    render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await screen.findByText('Scoring');
    // A line that MATCHES the old renderer regex but carries scored:false must not flip the strip —
    // main is the only matcher left.
    handlers.get('benchmark-log')?.(null, { line: 'echo rep0 (not the verdict)', stream: 'stdout', scored: false });
    expect(screen.getByText('Done').closest('span')!.querySelector('.animate-spin')).toBeNull();

    handlers.get('benchmark-log')?.(null, { line: LAST_LINE, stream: 'stdout', scored: true });
    await waitFor(() =>
      expect(screen.getByText('Done').closest('span')!.querySelector('.animate-spin')).not.toBeNull()
    );
  });
});
