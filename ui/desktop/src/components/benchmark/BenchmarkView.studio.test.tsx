import { render, screen, waitFor, within } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/** lucide stamps `lucide lucide-<name>` identifiers on its svgs — names, not utilities. */
const utilitiesOf = (classes: string[]) => classes.filter((c) => !c.startsWith('lucide'));

/**
 * LeanZero Studio remake of the benchmark view — the claims a screenshot cannot make on its own:
 * the header counts what the board shows, the live run is marked by the live dot and the status
 * bands carry the tone their words mean, the locked node strip keeps its selection readable, and
 * every class the view emits compiles (a no-op utility is invisible in the running app). The board
 * rows are the era's RETRIEVED catalog baselines plus the session's own row — nothing baked. The
 * SwarmRunPanel and the sampling knobs are other surfaces: stubbed, and excluded from the class
 * sweep (SamplingKnobs still carries the host's dead `font-bold`).
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

const MINE = {
  label: 'Your fleet · 3 nodes',
  score: 0.61,
  mine: true,
  nodes: 3,
  scorerVersion: 'sb-5.3',
  runId: 'brun-mine',
  tiers: { A: 0.7, B: 0.5, C: 0.6, D: 0.6 },
  wallSecs: 3600,
  runMeta: {
    startedAt: '2026-08-28T09:00:00.000Z',
    finishedAt: '2026-08-28T12:00:00.000Z',
    engineEvents: 1200,
    repairRounds: 1,
  },
  workdir: '/tmp/bench',
  modelId: 'qwen3.6-27b-mtp',
  verdict: {
    checks: [
      { check: 'modules_present', tier: 'A', score: 1, detail: '5/5 named files' },
      { check: 'sync_completeness', tier: 'B', score: 0.5, detail: '123/247', consequence: 'no sync' },
    ],
    tiers: { A: { mean: 1, checks: 1, weight: 0.25 }, B: { mean: 0.5, checks: 1, weight: 0.3 } },
    core: 0.4,
    repairRounds: [{ round: 0, findings: 1 }],
  },
};

/** The era's published board, RETRIEVED — the rows the table compares the session against. */
const CATALOG = {
  ok: true,
  fetchedAt: '2026-08-30T08:00:00.000Z',
  stale: false,
  benchmarks: [
    {
      scorerVersion: 'sb-5.3',
      title: 'VendorSync',
      current: true,
      frozen: false,
      baselines: [
        { label: 'Claude Opus 5', score: 0.9142, model: 'claude-opus-5' },
        { label: 'Claude Sonnet 5', score: 0.4971, model: 'claude-sonnet-5' },
        { label: 'Claude Haiku 4.5', score: 0.4615, model: 'claude-haiku-4-5' },
      ],
    },
  ],
};

/** The one finished session that IS the stored result (joined on runId). */
const SESSION = {
  runId: 'brun-mine',
  scorerVersion: 'sb-5.3',
  startedAt: '2026-08-28T09:00:00.000Z',
  endedAt: '2026-08-28T12:00:00.000Z',
  outcome: 'finished',
  score: 0.61,
  tiers: MINE.tiers,
  nodes: 3,
  publishable: true,
};

const SHOTS = [
  { name: 'loaded-before', caption: 'Before repairs', b64: 'AA==' },
  { name: 'loaded-after', caption: 'After repairs', b64: 'AA==' },
];

/** Classes from the subtrees this view owns: the header, every panel, the bands, the strips. */
function ownedClasses(container: HTMLElement): string[] {
  const roots = container.querySelectorAll<HTMLElement>(
    '[data-testid="lz-page-header"], [data-testid="lz-panel"], [data-testid="tone-band"], [role="group"], footer'
  );
  const set = new Set<string>();
  for (const root of Array.from(roots)) for (const c of allClasses(root)) set.add(c);
  return [...set].sort();
}

function mockElectron(opts: {
  running: boolean;
  mine?: typeof MINE | null;
  shots?: typeof SHOTS;
  scored?: boolean;
  lastLine?: string | null;
  catalog?: unknown;
  sessions?: unknown[];
}) {
  const e = electron();
  e.benchmarkStatus = vi.fn(async () =>
    opts.running
      ? {
          running: true,
          workdir: '/tmp/bench',
          startedAt: '2026-08-29T09:00:00.000Z',
          sampling: {},
          scored: opts.scored ?? false,
          lastLine: opts.lastLine ?? null,
        }
      : { running: false }
  );
  e.benchmarkRead = vi.fn(async () => (opts.mine === undefined ? MINE : opts.mine));
  e.benchmarkShots = vi.fn(async () => opts.shots ?? []);
  e.benchmarkCatalog = 'catalog' in opts ? opts.catalog : vi.fn(async () => CATALOG);
  e.benchmarkSessions = vi.fn(async () => ({ sessions: opts.sessions ?? [SESSION] }));
  e.benchmarkDeleteSession = vi.fn(async () => ({ ok: true }));
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  const handlers = new Map<string, (e: unknown, payload: unknown) => void>();
  e.on = vi.fn((channel: string, cb: (e: unknown, payload: unknown) => void) => {
    handlers.set(channel, cb);
  });
  e.off = vi.fn();
  return handlers;
}

const renderView = () =>
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );

describe('BenchmarkView — LeanZero Studio', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('is a PageHeader over Panels: the board is a table whose header counts its rows, and the primary is the launch', async () => {
    mockElectron({ running: false, shots: SHOTS });
    renderView();
    const header = screen.getByTestId('lz-page-header');
    expect(within(header).getByRole('heading', { level: 1 }).textContent).toBe('Benchmark');
    const launch = within(header).getByRole('button', { name: 'Run benchmark' });
    expect(launch.getAttribute('data-variant')).toBe('primary');

    // The board: the era's retrieved baselines + the session's own row, one <tr> each, keyed by identity.
    const expected = CATALOG.benchmarks[0].baselines.length + 1;
    const table = await screen.findByRole('table', { name: 'Benchmark board' });
    await waitFor(() => expect(within(table).getAllByTestId('lz-row')).toHaveLength(expected));
    const board = table.closest<HTMLElement>('[data-testid="lz-panel"]')!;
    expect(within(board).getByTestId('lz-section-count').textContent).toBe(String(expected));
    const keys = within(table)
      .getAllByTestId('lz-row')
      .map((r) => r.getAttribute('data-key'));
    expect(new Set(keys).size).toBe(expected);
    expect(keys).toContain('mine');
    // The row's mark is a StatusDot whose label is its meaning.
    expect(within(table).getByRole('img', { name: 'your fleet' })).toBeTruthy();
    expect(within(table).getAllByRole('img', { name: 'baseline' })).toHaveLength(expected - 1);

    // The shots panel counts the figures it shows.
    const shots = await screen.findByText('What it built');
    const shotsPanel = shots.closest<HTMLElement>('[data-testid="lz-panel"]')!;
    expect(within(shotsPanel).getByTestId('lz-section-count').textContent).toBe('2');
    expect(shotsPanel.querySelectorAll('figure')).toHaveLength(2);

    // The publish form: Studio inputs and its own primary, inside the publish panel.
    const model = (await screen.findByLabelText(/^Model/)) as HTMLInputElement;
    expect(model.className).toContain('rounded-lz-control');
    const publish = screen.getByRole('button', { name: /Publish/ });
    expect(publish.getAttribute('data-variant')).toBe('primary');
    expect(publish.closest('[data-testid="lz-panel"]')).not.toBeNull();
  });

  it('marks the live run with the live dot, keeps the locked selection readable, and never makes Cancel the primary', async () => {
    mockElectron({ running: true, mine: null, sessions: [], lastLine: 'rep0 (swarm-3node) score=61.0' });
    renderView();
    const pipeline = (await screen.findByText('Benchmark pipeline')).closest<HTMLElement>(
      '[data-testid="lz-panel"]'
    )!;
    const live = within(pipeline).getByRole('img', { name: 'run in progress' });
    expect(live.getAttribute('data-live')).toBe('true');
    expect(live.className).toContain('bg-lz-accent');
    expect(within(pipeline).getByText('rep0 (swarm-3node) score=61.0')).toBeTruthy();
    expect(within(pipeline).getByText('/tmp/bench')).toBeTruthy();

    // The node strip is the locked Segmented: the selection stays the accent fill — a solid, never
    // an opacity. (There is no benchmark chooser: the catalog's current era is the only one open.)
    const nodes = screen.getByRole('group', { name: 'Nodes' });
    const three = within(nodes).getByRole('button', { name: '3' });
    await waitFor(() => expect(three).toBeDisabled());
    expect(three.className).toContain('bg-lz-accent');
    expect(three.className).not.toMatch(/opacity/);
    const other = within(nodes).getByRole('button', { name: '1' });
    expect(other.className).not.toContain('bg-lz-accent');
    expect(screen.queryByRole('button', { name: 'sb-5.3' })).toBeNull();

    const cancel = screen.getByRole('button', { name: 'Cancel run' });
    expect(cancel.getAttribute('data-variant')).toBe('secondary');
    expect(screen.queryByRole('button', { name: 'Run benchmark' })).toBeNull();
  });

  it('status bands carry the tone their words mean: failure err, cancellation stopped', async () => {
    const handlers = mockElectron({ running: true, mine: null, sessions: [] });
    renderView();
    await screen.findByText('Benchmark pipeline');

    handlers.get('benchmark-finished')?.(null, { error: 'vendor sim never bound its port' });
    const err = await screen.findByTestId('tone-band');
    expect(err.getAttribute('data-tone')).toBe('err');
    expect(err.className).toContain('bg-lz-err-solid');
    expect(err.textContent).toContain('The run failed: vendor sim never bound its port');

    handlers.get('benchmark-finished')?.(null, { cancelled: true });
    await waitFor(() =>
      expect(screen.getByTestId('tone-band').getAttribute('data-tone')).toBe('stopped')
    );
    expect(screen.getByTestId('tone-band').className).toContain('bg-lz-stopped-solid');
  });

  it('an unreachable catalog is a solid err band, and the board keeps only the session — no invented baseline', async () => {
    mockElectron({ running: false, catalog: undefined });
    renderView();
    const bands = await screen.findAllByTestId('tone-band');
    const absent = bands.find((b) => /Catalog unreachable/.test(b.textContent ?? ''))!;
    expect(absent.getAttribute('data-tone')).toBe('err');
    expect(absent.className).toContain('bg-lz-err-solid');
    const table = await screen.findByRole('table', { name: 'Benchmark board' });
    await waitFor(() => expect(within(table).getByRole('img', { name: 'your fleet' })).toBeTruthy());
    expect(within(table).queryAllByRole('img', { name: 'baseline' })).toHaveLength(0);
    expect(screen.queryByText('Claude Opus 5')).toBeNull();
  });

  it('emits no banned pattern, no node hue, and only utilities the pipeline compiles', async () => {
    mockElectron({ running: false, shots: SHOTS });
    const { container } = renderView();
    await screen.findByText('What it built');
    await screen.findByText('Repair progression');
    assertStudioClean(container);
    expect(container.innerHTML).not.toMatch(/color-node-|color-block-teal|#[0-9a-f]{6}/i);
    const missing = await missingUtilities(utilitiesOf(ownedClasses(container)));
    expect(missing).toEqual([]);
  }, 30_000);
});
