import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { BenchmarkSection, benchRunHref, deriveEras } from './BenchmarkSection';
import type { BenchSession, CatalogBenchmark } from '../benchmark/bridge';

const nav = vi.hoisted(() => ({ navigate: vi.fn(), startChat: vi.fn() }));
vi.mock('react-router-dom', async (orig) => ({
  ...(await orig<typeof import('react-router-dom')>()),
  useNavigate: () => nav.navigate,
}));
vi.mock('./useStartChatAbout', () => ({ useStartChatAbout: () => nav.startChat }));

const CATALOG: CatalogBenchmark[] = [
  { scorerVersion: 'sb-7.1', title: 'SB7.1 payments', current: true, frozen: false, baselines: [] },
  { scorerVersion: 'sb-6.0', title: 'VendorSync Pro', current: false, frozen: true, baselines: [] },
];
const SESSIONS: BenchSession[] = [
  {
    runId: null,
    scorerVersion: 'sb-7.1',
    startedAt: '2026-08-30T10:00:00.000Z',
    outcome: 'running',
    publishable: false,
  },
  {
    runId: 's-fin',
    scorerVersion: 'sb-7.1',
    startedAt: '2026-08-29T10:00:00.000Z',
    outcome: 'finished',
    score: 0.0273,
    publishable: true,
  },
  {
    runId: 's-dnf',
    scorerVersion: 'sb-7.1',
    startedAt: '2026-08-28T10:00:00.000Z',
    outcome: 'did_not_finish',
    publishable: false,
  },
  {
    runId: 's-dns',
    scorerVersion: 'sb-7.1',
    startedAt: '2026-08-27T10:00:00.000Z',
    outcome: 'did_not_start',
    publishable: false,
  },
  {
    runId: 's-old',
    scorerVersion: 'sb-6.0',
    startedAt: '2026-08-19T10:00:00.000Z',
    outcome: 'finished',
    score: 0.8635,
    publishable: false,
  },
];

function mocks() {
  const m = {
    benchmarkSessions: vi.fn().mockResolvedValue({ sessions: SESSIONS }),
    benchmarkCatalog: vi.fn().mockResolvedValue({ ok: true, benchmarks: CATALOG }),
    benchmarkDeleteSession: vi.fn().mockResolvedValue({ ok: true }),
  };
  Object.assign(window.electron, m);
  return m;
}

const renderSection = () =>
  render(
    <MemoryRouter>
      <IntlProvider locale="en" messages={{}}>
        <BenchmarkSection />
      </IntlProvider>
    </MemoryRouter>
  );

beforeEach(() => vi.clearAllMocks());

describe('BenchmarkSection', () => {
  it('one row per benchmark era with its badge, runs nested newest first with honest outcomes', async () => {
    mocks();
    renderSection();
    const current = await screen.findByTestId('bench-era-sb-7.1');
    expect(within(current).getByText('CURRENT')).toBeInTheDocument();
    expect(within(screen.getByTestId('bench-era-sb-6.0')).getByText('FROZEN')).toBeInTheDocument();
    const runningChip = within(current).getByText('Running').closest('span')!;
    expect(runningChip.querySelector('.animate-lz-live')).not.toBeNull();
    expect(within(current).getByText('Finished')).toBeInTheDocument();
    expect(within(current).getByText(/· 2\.7%/)).toBeInTheDocument();
    expect(within(current).getByText('Did not finish')).toBeInTheDocument();
    expect(within(current).getByText('Did not start')).toBeInTheDocument();
    const runs = within(current).getAllByTestId(/^bench-run-/);
    expect(runs[0].getAttribute('data-testid')).toBe('bench-run-start-2026-08-30T10:00:00.000Z');
    expect(runs[1].getAttribute('data-testid')).toBe('bench-run-s-fin');
  });

  it('opening a run navigates the Benchmark view by URL; "+" opens the run setup', async () => {
    mocks();
    renderSection();
    fireEvent.click(await screen.findByTestId('bench-run-s-fin'));
    expect(nav.navigate).toHaveBeenCalledWith(benchRunHref('sb-7.1', 's-fin'));
    expect(benchRunHref('sb-7.1', 's-fin')).toBe('/benchmark?era=sb-7.1&run=s-fin');
    fireEvent.click(screen.getByLabelText('New benchmark run'));
    expect(nav.navigate).toHaveBeenCalledWith('/benchmark?new=1');
  });

  it('the run’s context menu: ask an AI, and delete after a confirm — never for the running one', async () => {
    const m = mocks();
    renderSection();
    fireEvent.contextMenu(await screen.findByTestId('bench-run-s-dnf'));
    const menu = await screen.findByTestId('bench-run-context-menu');
    fireEvent.click(within(menu).getByText('Start an AI session about this run'));
    expect(nav.startChat).toHaveBeenCalledWith(expect.stringContaining('s-dnf'));

    fireEvent.contextMenu(screen.getByTestId('bench-run-s-dnf'));
    fireEvent.click(await screen.findByText('Delete run'));
    expect(m.benchmarkDeleteSession).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('Confirm delete (removes its files)'));
    await waitFor(() => expect(m.benchmarkDeleteSession).toHaveBeenCalledWith('s-dnf'));

    fireEvent.contextMenu(screen.getByTestId('bench-run-start-2026-08-30T10:00:00.000Z'));
    const running = await screen.findByTestId('bench-run-context-menu');
    expect(within(running).getByText('Delete run').closest('button')).toBeDisabled();
  });

  it('deriveEras: the current era first, runs newest first, unknown eras from runs alone', () => {
    const eras = deriveEras(SESSIONS, CATALOG);
    expect(eras.map((e) => [e.scorerVersion, e.current, e.sessions.length])).toEqual([
      ['sb-7.1', true, 4],
      ['sb-6.0', false, 1],
    ]);
    const orphan = deriveEras([{ ...SESSIONS[4], scorerVersion: 'sb-5.3' }], null);
    expect(orphan[0]).toMatchObject({
      scorerVersion: 'sb-5.3',
      fromCatalog: false,
      title: 'sb-5.3',
    });
  });
});
