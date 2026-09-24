import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MlxStateTile, type MlxStateTileProps } from './MlxStateTile';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { attributeServing, type MlxServingRow } from '../../utils/mlxServing';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  NO_RATES,
  advanceLastRates,
  advanceMountWatch,
  mountCostOf,
  mountFill,
  parseMlxLiveStatus,
  type LastRates,
  type MlxLiveStats,
  type TpsSample,
} from './mlxLiveStats';
import {
  DIST_READING_STATUS,
  DIST_WRITING_STATUS,
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from './mlxLiveStatus.fixtures';
import {
  FLASH_READY,
  FLASH_SERVING,
  HOSTING_RANK_1,
  STOPPED_WITH_CONFIG,
} from './mlxDistributed.fixtures';

const GIB = 1024 * 1024 * 1024;

const HISTORY: TpsSample[] = [
  { uptimeS: 1868.3, tps: 0 },
  { uptimeS: 1870.3, tps: 18.7 },
  { uptimeS: 1872.3, tps: 19.4 },
  { uptimeS: 1874.3, tps: 19.9 },
];

function statsOf(body: unknown): MlxLiveStats {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return read.stats;
}

/** What the tile carries after it watched the fixture's generating read. */
const LAST_AFTER_GENERATING: LastRates = advanceLastRates(NO_RATES, statsOf(GENERATING_STATUS));

const ROW_BASE = { startedAt: '2026-09-23T20:00:00Z', sessionError: null };

function tile(overrides: Partial<MlxStateTileProps>) {
  const props: MlxStateTileProps = {
    state: 'running',
    unreachable: false,
    live: null,
    history: [],
    last: NO_RATES,
    serving: null,
    mount: null,
    cost: null,
    failedError: null,
    action: null,
    modeLabel: 'Single · this Mac',
    distributed: null,
    ...overrides,
  };
  return render(
    <IntlTestWrapper>
      <MlxStateTile {...props} />
    </IntlTestWrapper>
  );
}

async function expectDesigned(container: HTMLElement) {
  assertStudioClean(container);
  // lucide's own marker classes (`lucide`, `lucide-play`) are not utilities.
  const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
  expect(await missingUtilities(utilities)).toEqual([]);
}

/** The sidecar's fit verdict in the tile's units (a 12.8 GB reserve, as before the one rule). */
const cost = (needGb: number, freeGb: number, verdict: string) =>
  mountCostOf({
    verdict,
    needBytes: needGb * GIB,
    availableBytes: freeGb * GIB,
    budgetBytes: (freeGb - 12.8) * GIB,
  });

describe('MlxStateTile RUNNING — the fill is what the engine is DOING', () => {
  it('writing: GREEN, the live writing rate big, the reading rate beside it, rows, lifetime facts', async () => {
    const { container } = tile({
      live: parseMlxLiveStatus(GENERATING_STATUS),
      history: HISTORY,
      last: LAST_AFTER_GENERATING,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-state', 'running');
    expect(t).toHaveAttribute('data-activity', 'generating');
    expect(t.className).toContain('bg-lz-phase-writing');
    expect(t.className).toContain('lg:w-[32rem]');
    expect(screen.getByTestId('mlx-activity')).toHaveTextContent('Writing');
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('19.9');
    expect(within(t).getByText('tok/s writing')).toBeInTheDocument();
    // 32,277 uncached prompt tokens over a 165 s time to first token.
    expect(screen.getByTestId('mlx-live-pps')).toHaveTextContent('196');
    expect(within(t).getByText('tok/s reading this prompt')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-tps-sparkline')).toBeInTheDocument();

    const rows = screen.getAllByTestId('mlx-live-request');
    // Running first (engine order), then the queue.
    expect(rows.map((r) => r.dataset.phase)).toEqual(['generation', 'prefill', 'queued']);
    expect(rows[0]).toHaveTextContent('Writing · 28,035 of 32,768 tokens');
    expect(rows[0]).toHaveTextContent('86%');
    expect(within(rows[0]).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '86');
    // The long silent pre-fill is visible, with the engine's own elapsed seconds and no fake bar.
    expect(rows[1]).toHaveTextContent('Reading prompt · 32.3K tokens');
    expect(rows[1]).toHaveTextContent('2m 45s');
    expect(within(rows[1]).queryByRole('progressbar')).toBeNull();
    expect(rows[2]).toHaveTextContent('Queued · 12K tokens');

    const facts = screen.getByTestId('mlx-live-facts');
    for (const [value, label] of [
      ['5', 'requests served'],
      ['91.7K', 'prompt tokens read'],
      ['672', 'tokens written'],
      ['45.1K', 'prompt tokens from cache'],
      ['78%', 'of cache lookups hit'],
      ['50.7 GB', 'GPU memory in use'],
      ['31m 14s', 'engine uptime'],
    ]) {
      expect(within(facts).getByText(value)).toBeInTheDocument();
      expect(within(facts).getByText(label)).toBeInTheDocument();
    }
    await expectDesigned(container);
  });

  it('reading a prompt: the ACCENT fill, the prompt size and its elapsed seconds as the hero', async () => {
    const { container } = tile({
      live: parseMlxLiveStatus(PREFILL_STATUS),
      last: LAST_AFTER_GENERATING,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-activity', 'prefill');
    expect(t.className).toContain('bg-lz-phase-reading');
    expect(screen.getByTestId('mlx-activity')).toHaveTextContent('Reading prompt');
    expect(screen.getByTestId('mlx-live-prompt')).toHaveTextContent('32.3K');
    expect(within(t).getByText('prompt tokens, reading for 2m 45s')).toBeInTheDocument();
    // No reading rate exists mid-prefill (the engine reports no progress): the last one, labelled so.
    expect(screen.getByTestId('mlx-live-pps')).toHaveTextContent('196');
    expect(within(t).getByText('tok/s reading, last prompt')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-live-tps')).toBeNull();
    await expectDesigned(container);
  });

  it('idle: a SOLID SLATE fill (not green) with the last rates as plain facts', async () => {
    const { container } = tile({
      live: parseMlxLiveStatus(IDLE_STATUS),
      history: [{ uptimeS: 1, tps: 19.9 }],
      last: LAST_AFTER_GENERATING,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-activity', 'idle');
    expect(t.className).toContain('bg-lz-phase-idle');
    expect(t.className).not.toContain('bg-lz-phase-writing');
    expect(screen.getByTestId('mlx-activity')).toHaveTextContent('Idle');
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('19.9');
    expect(within(t).getByText('tok/s writing, last run')).toBeInTheDocument();
    expect(within(t).getByText('tok/s reading, last prompt')).toBeInTheDocument();
    expect(screen.queryAllByTestId('mlx-live-request')).toHaveLength(0);
    expect(within(t).getByText('54.3 GB')).toBeInTheDocument();
    expect(within(t).getByText('20%')).toBeInTheDocument();
    await expectDesigned(container);
  });

  it('idle with nothing measured yet: dashes, never the engine aggregate (1,048,576 tok/s after a one-token request)', () => {
    tile({
      live: parseMlxLiveStatus({ status: 'idle', generation_tps: 1048576.0, requests: [] }),
    });
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('—');
    expect(screen.getByText('nothing written yet')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-live-pps')).toHaveTextContent('—');
    expect(screen.getByText('no prompt read yet')).toBeInTheDocument();
  });

  it('serving: a chat, an external /v1 client, and the unexplained rest COUNTED beside the live swarm run', async () => {
    const rows: MlxServingRow[] = [
      {
        ...ROW_BASE,
        id: 1,
        via: 'swarmRouter',
        sessionId: '20260923_7',
        provider: 'omlx',
        model: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
        nodeId: 'mihai-mlx',
        sessionName: 'Memory · verify recall',
        sessionType: 'user',
      },
      {
        ...ROW_BASE,
        id: 2,
        via: 'openaiApi',
        sessionId: '20260923_9',
        provider: 'omlx',
        model: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
        nodeId: null,
        sessionName: 'OpenAI-compatible request',
        sessionType: 'user',
      },
    ];
    const { container } = tile({
      live: parseMlxLiveStatus(GENERATING_STATUS),
      serving: attributeServing(rows, 3, ['bench-r9'], null),
    });
    const lines = screen.getAllByTestId('mlx-serving-row').map((r) => r.textContent);
    expect(lines).toEqual([
      'Chat · Memory · verify recall',
      'External client via /v1 · omlx/mihai-qwen3.8-27b-atlassian-q8-mlx',
      "1 request not from this app's chats or /v1",
      'Swarm run live: bench-r9',
    ]);
    await expectDesigned(container);
  });

  it('serving list unreadable: says so, never an empty "nobody"', () => {
    tile({
      live: parseMlxLiveStatus(GENERATING_STATUS),
      serving: attributeServing([], 3, [], 'goose backend returned 401'),
    });
    expect(screen.getByTestId('mlx-serving')).toHaveTextContent(
      'Who is using it could not be read: goose backend returned 401'
    );
    expect(screen.getByTestId('mlx-serving')).toHaveTextContent(
      "3 requests not from this app's chats or /v1"
    );
  });

  it('a failed read says "Live stats unavailable" with the reason — nothing invented', async () => {
    const { container } = tile({
      live: { ok: false, detail: 'unreachable: connect ECONNREFUSED 127.0.0.1:8090' },
    });
    expect(screen.getByTestId('mlx-live-unavailable')).toHaveTextContent(
      'Live stats unavailableunreachable: connect ECONNREFUSED 127.0.0.1:8090'
    );
    // Activity unknown: the loaded model's idle grey, not a colour that claims work.
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-idle');
    expect(screen.queryByTestId('mlx-live-tps')).toBeNull();
    expect(screen.queryByTestId('mlx-tps-sparkline')).toBeNull();
    await expectDesigned(container);
  });
});

describe('MlxStateTile MOUNTING — memory claimed toward the model size', () => {
  it('a measured mount draws the claimed GB, the fraction bar', async () => {
    const w = advanceMountWatch(advanceMountWatch(null, 'm', 90, 96.6), 'm', 80.6, null);
    const { container } = tile({ state: 'mounting', mount: mountFill(w, 80.6, 31 * GIB) });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-phase-loading');
    expect(screen.getByTestId('mlx-mount-fill')).toHaveAttribute('data-measured', 'true');
    expect(t).toHaveTextContent('16.0of 31.0 GB');
    expect(within(t).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '52');
    await expectDesigned(container);
  });

  it('opened mid-mount: the size and "Loading weights", no percent', () => {
    const w = advanceMountWatch(null, 'm', 70, null);
    tile({ state: 'mounting', mount: mountFill(w, 70, 31 * GIB) });
    const t = screen.getByTestId('mlx-state-badge');
    expect(screen.getByTestId('mlx-mount-fill')).toHaveAttribute('data-measured', 'false');
    expect(t).toHaveTextContent('31.0 GB');
    expect(t).toHaveTextContent('Loading weights');
    expect(within(t).getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
    expect(screen.getByTestId('mlx-load-indeterminate')).toBeInTheDocument();
  });
});

describe('MlxStateTile STOPPED — what mounting would cost, and Mount on the tile', () => {
  it('fits: the size, the meter against free memory, the verdict and the action', async () => {
    const { container } = tile({
      state: 'stopped',
      cost: cost(31, 68.6, 'allow'),
      action: <button type="button">Mount</button>,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-phase-unloaded');
    expect(screen.getByTestId('mlx-mount-cost')).toHaveAttribute('data-verdict', 'fits');
    expect(t).toHaveTextContent('31.0GB to mount');
    expect(t).toHaveTextContent('Fits, 24.8 GB to spare');
    expect(t).toHaveTextContent('68.6 GB available');
    expect(within(t).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '45');
    expect(within(t).getByRole('button', { name: 'Mount' })).toBeInTheDocument();
    await expectDesigned(container);
  });

  it('no-fit names the shortfall; no model picked asks for one', () => {
    const { unmount } = tile({ state: 'stopped', cost: cost(31, 40, 'block') });
    expect(screen.getByTestId('mlx-mount-cost')).toHaveAttribute('data-verdict', 'no-fit');
    expect(screen.getByTestId('mlx-state-badge')).toHaveTextContent(
      'Needs 3.8 GB more free memory'
    );
    unmount();
    tile({ state: 'stopped', cost: null });
    expect(screen.getByTestId('mlx-state-badge')).toHaveTextContent(
      'Pick a model to see what mounting it costs.'
    );
  });
});

describe('MlxStateTile FAILED — the error and Retry on the tile', () => {
  it('shows the engine error excerpt and the Retry action', async () => {
    const { container } = tile({
      state: 'failed',
      failedError: 'port 9600 never opened',
      action: <button type="button">Retry</button>,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-phase-failed');
    expect(screen.getByTestId('mlx-failed-excerpt')).toHaveTextContent('port 9600 never opened');
    expect(within(t).getByRole('button', { name: 'Retry' })).toBeInTheDocument();
    await expectDesigned(container);
  });
});

describe('MlxStateTile — the mode is always said, and a distributed run IS the tile', () => {
  it('single: the mode line says "Single · this Mac" under the state', () => {
    tile({ state: 'stopped', distributed: STOPPED_WITH_CONFIG });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'single');
    expect(screen.getByTestId('mlx-mode')).toHaveTextContent('Single · this Mac');
    expect(screen.queryByTestId('mlx-dist-tile')).toBeNull();
  });

  it('distributed READY: slate at rest, the mode, the model, in flight, each rank’s peak of its budget', async () => {
    const { container } = tile({
      state: 'stopped',
      modeLabel: 'Distributed · 2 nodes · JACCL',
      distributed: FLASH_READY,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'distributed');
    expect(t).toHaveAttribute('data-state', 'ready');
    expect(t.className).toContain('bg-lz-phase-idle');
    expect(within(t).getByRole('status')).toHaveTextContent('Ready');
    expect(screen.getByTestId('mlx-mode')).toHaveTextContent('Distributed · 2 nodes · JACCL');
    expect(t).toHaveTextContent('rapid-mlx/Qwen3.8-Flash-Next-4bit');
    expect(screen.getByTestId('mlx-dist-tile-inflight')).toHaveTextContent('0');
    expect(screen.getByTestId('mlx-dist-tile-load')).toHaveTextContent('slots 0 of 2 · 0 waiting');
    const nodes = screen.getAllByTestId('mlx-dist-tile-node');
    expect(nodes[0]).toHaveTextContent('MacBook Pro · L0–19');
    expect(nodes[0]).toHaveTextContent('61.0 of 83.4 GiB peak');
    expect(nodes[1]).toHaveTextContent('workhorse · L20–47');
    expect(nodes[1]).toHaveTextContent('42.5 of 55.4 GiB peak');
    // The stopped single engine's "what a mount costs" is not drawn while the run owns the Mac.
    expect(screen.queryByTestId('mlx-mount-cost')).toBeNull();
    await expectDesigned(container);
  });

  it('distributed SERVING is green with the requests in flight; a closed admission is warn', () => {
    const { unmount } = tile({ state: 'stopped', modeLabel: 'x', distributed: FLASH_SERVING });
    let t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-phase-writing');
    expect(screen.getByTestId('mlx-dist-tile-inflight')).toHaveTextContent('2');
    expect(t).toHaveTextContent('requests in flight');
    unmount();
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_SERVING, admissionOpen: false },
    });
    t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-phase-held');
    expect(t).toHaveTextContent('Admission closed: a node is low on memory');
  });

  it('distributed READING a long prompt: BLUE, the prompt size, the live read rate and how far in', async () => {
    const { container } = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_SERVING, inflight: 1, slotsInUse: 1 },
      live: parseMlxLiveStatus(DIST_READING_STATUS),
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'distributed');
    expect(t).toHaveAttribute('data-activity', 'prefill');
    expect(t.className).toContain('bg-lz-phase-reading');
    expect(screen.getByTestId('mlx-activity')).toHaveTextContent('Reading prompt');
    expect(screen.getByTestId('mlx-live-prompt')).toHaveTextContent('7K');
    // The split reports its prefill rate while it reads — the live figure, not the last prompt's.
    expect(screen.getByTestId('mlx-live-pps')).toHaveTextContent('152');
    expect(within(t).getByText('tok/s reading this prompt')).toBeInTheDocument();
    const [row] = screen.getAllByTestId('mlx-live-request');
    expect(row).toHaveTextContent('Reading prompt · 7K tokens');
    expect(row).toHaveTextContent('29% · 14s');
    expect(within(row).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '29');
    // The supervisor's bare count gives way to the live read; the ranks stay.
    expect(screen.queryByTestId('mlx-dist-tile-inflight')).toBeNull();
    expect(screen.getAllByTestId('mlx-dist-tile-node')).toHaveLength(2);
    await expectDesigned(container);
  });

  it('distributed WRITING: GREEN with the writing rate; the queue behind it rides the rows', () => {
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: FLASH_SERVING,
      live: parseMlxLiveStatus(DIST_WRITING_STATUS),
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-activity', 'generating');
    expect(t.className).toContain('bg-lz-phase-writing');
    expect(screen.getByTestId('mlx-activity')).toHaveTextContent('Writing');
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('171');
    // The engine's own prefill rate for the prompt it just read.
    expect(screen.getByTestId('mlx-live-pps')).toHaveTextContent('5,503');
    const rows = screen.getAllByTestId('mlx-live-request');
    expect(rows.map((r) => r.dataset.phase)).toEqual(['generation', 'queued']);
  });

  it('distributed with only QUEUED requests is ORANGE; a closed admission stays orange over any activity', () => {
    const queued = {
      ...DIST_WRITING_STATUS,
      requests: [DIST_WRITING_STATUS.requests[1]],
    };
    const { unmount } = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: FLASH_SERVING,
      live: parseMlxLiveStatus(queued),
    });
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-held');
    unmount();
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_SERVING, admissionOpen: false },
      live: parseMlxLiveStatus(DIST_WRITING_STATUS),
    });
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-held');
  });

  it('a failed rank 0 read on an up run says why, and the tile keeps the run colour', () => {
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: FLASH_SERVING,
      live: { ok: false, detail: 'unreachable: connect ECONNREFUSED 127.0.0.1:8091' },
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).not.toHaveAttribute('data-activity');
    expect(t.className).toContain('bg-lz-phase-writing');
    expect(screen.getByTestId('mlx-live-unavailable')).toHaveTextContent(
      'unreachable: connect ECONNREFUSED 127.0.0.1:8091'
    );
  });

  it('distributed slots: the pipeline says slots and the queue, tensor the queue only, a failed poll nothing', () => {
    const { unmount } = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_SERVING, waiting: 1 },
    });
    expect(screen.getByTestId('mlx-dist-tile-load')).toHaveTextContent('slots 2 of 2 · 1 waiting');
    unmount();
    const tensor = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: {
        ...FLASH_SERVING,
        runner: 'mlxLmTensor',
        waiting: 0,
        slots: undefined,
        slotsInUse: undefined,
        sequencesInFlight: undefined,
      },
    });
    expect(screen.getByTestId('mlx-dist-tile-load').textContent).toBe('0 waiting');
    tensor.unmount();
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: {
        ...FLASH_SERVING,
        waiting: undefined,
        slots: undefined,
        slotsInUse: undefined,
        sequencesInFlight: undefined,
        serverStatusError: '/v1/status did not answer',
      },
    });
    expect(screen.queryByTestId('mlx-dist-tile-load')).toBeNull();
  });
});

describe('MlxStateTile — this Mac serving a rank of another Mac over LeanZero Link', () => {
  it('the tile names the rank, whose run and the model; Mount is not offered', async () => {
    const { container } = tile({
      state: 'stopped',
      modeLabel:
        "Rank 1 of MacBook Pro's distributed engine · Qwen3.8-27B-Atlassian-Q8-mlx · JACCL",
      distributed: HOSTING_RANK_1,
      action: <button type="button">Mount</button>,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'hosting');
    expect(t).toHaveAttribute('data-state', 'stopped');
    const hosting = screen.getByTestId('mlx-hosting-tile');
    expect(hosting).toHaveTextContent('Rank 1 of 2');
    expect(hosting).toHaveTextContent("for MacBook Pro's distributed engine over LeanZero Link");
    expect(hosting).toHaveTextContent('Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx');
    expect(hosting).toHaveTextContent('rank pid 4242');
    expect(within(t).getByRole('status')).toHaveTextContent('Serving');
    expect(screen.queryByRole('button', { name: 'Mount' })).toBeNull();
    await expectDesigned(container);
  });
});

/**
 * The owner's live walkthrough of 3.0.27 (2026-09-24): chat was served by Work's Mac Studio at
 * 25.9 tok/s while this tile read "Stopped · Single · this Mac · 30.6 GB to mount · Fits". The tile
 * follows what serves chat: the route's engine, its state colour, its live rates, its Stop.
 */
describe('MlxStateTile — a remote single IS the tile while it serves this Mac’s chat', () => {
  const ROUTE = {
    state: 'ready',
    peer: 'worksmacstudio-lan-9c1e2a',
    peerHostname: 'WorksMacStudio.lan',
    peerComputerName: "Work's Mac Studio",
    baseUrl: 'http://127.0.0.1:61001/relay/cafe',
    modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  };

  it('ready and WRITING: green, the peer engine’s live rates and rows, whatever this Mac’s own engine says', async () => {
    const { container } = tile({
      state: 'stopped',
      cost: cost(30.6, 96.6, 'allow'),
      remote: ROUTE,
      modeLabel: "Serving from Work's Mac Studio",
      live: parseMlxLiveStatus(GENERATING_STATUS),
      last: LAST_AFTER_GENERATING,
      action: <button type="button">Stop</button>,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'remote');
    expect(t).toHaveAttribute('data-state', 'running');
    expect(t).toHaveAttribute('data-phase', 'writing');
    expect(t.className).toContain('bg-lz-phase-writing');
    expect(within(t).getByRole('status')).toHaveTextContent('Running');
    expect(screen.getByTestId('mlx-mode')).toHaveTextContent("Serving from Work's Mac Studio");
    expect(screen.getByTestId('mlx-remote-tile')).toHaveTextContent(ROUTE.modelId);
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('19.9');
    expect(screen.getAllByTestId('mlx-live-request')).toHaveLength(3);
    // This Mac's stopped engine does not speak over the route.
    expect(screen.queryByTestId('mlx-mount-cost')).toBeNull();
    expect(within(t).getByRole('button', { name: 'Stop' })).toBeInTheDocument();
    await expectDesigned(container);
  });

  it('ready and READING a prompt there is blue; IDLE is grey', () => {
    const { unmount } = tile({
      state: 'stopped',
      remote: ROUTE,
      live: parseMlxLiveStatus(PREFILL_STATUS),
    });
    expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-phase', 'reading');
    unmount();
    tile({ state: 'stopped', remote: ROUTE, live: parseMlxLiveStatus(IDLE_STATUS) });
    expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-phase', 'idle');
  });

  it('mounting there is amber with where it loads; failed is red with the peer’s own words', () => {
    const { unmount } = tile({ state: 'stopped', remote: { ...ROUTE, state: 'mounting' } });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-phase', 'loading');
    expect(t).toHaveAttribute('data-state', 'mounting');
    expect(screen.getByTestId('mlx-remote-tile')).toHaveTextContent(
      "Loading the model on Work's Mac Studio"
    );
    expect(screen.getByTestId('mlx-load-indeterminate')).toBeInTheDocument();
    unmount();

    tile({
      state: 'running',
      remote: { ...ROUTE, state: 'failed', lastError: 'the peer engine exited 137' },
    });
    const f = screen.getByTestId('mlx-state-badge');
    expect(f).toHaveAttribute('data-phase', 'failed');
    expect(screen.getByTestId('mlx-failed-excerpt')).toHaveTextContent(
      'the peer engine exited 137'
    );
  });

  it('a read over Link that timed out is a NAMED state — "Rates unavailable over LeanZero Link", no old number', () => {
    tile({
      state: 'stopped',
      remote: ROUTE,
      live: { ok: false, detail: 'timeout: no answer within 1500 ms' },
      last: LAST_AFTER_GENERATING,
    });
    const gone = screen.getByTestId('mlx-live-unavailable');
    expect(gone).toHaveAttribute('data-over-link', 'true');
    expect(gone).toHaveAttribute('data-reason', 'timeout');
    expect(gone).toHaveTextContent('Rates unavailable over LeanZero Link');
    expect(gone).toHaveTextContent('timeout: no answer within 1500 ms');
    expect(screen.queryByTestId('mlx-live-tps')).toBeNull();
    expect(screen.queryByTestId('mlx-live-pps')).toBeNull();
  });

  it('a route that is off claims nothing: this Mac’s own engine is the tile', () => {
    tile({ state: 'stopped', remote: { state: 'off' }, cost: cost(17, 96.6, 'allow') });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-mode', 'single');
    expect(screen.getByTestId('mlx-mount-cost')).toBeInTheDocument();
  });

  it('an older backend that did not say the computer name falls back to the hostname', () => {
    tile({
      state: 'stopped',
      remote: { ...ROUTE, state: 'failed', peerComputerName: undefined },
    });
    expect(screen.getByTestId('mlx-failed-excerpt')).toHaveTextContent(
      'The engine on WorksMacStudio.lan is not serving, and goose named no reason.'
    );
  });
});

/**
 * The owner's live test of 3.0.25 (2026-09-24): "grey should be for idle, some other color should
 * be for mounting, then another color for working" and "while it was mounting I don't see the other
 * mac as showing anything visual". Every state the tile can be in → its ONE palette colour.
 */
describe('MlxStateTile — the engine-phase palette, one colour per state', () => {
  const phaseOf = () => screen.getByTestId('mlx-state-badge').getAttribute('data-phase');

  it('stopped with no model: the dark neutral tile, OUTLINED', async () => {
    const { container } = tile({ state: 'stopped', cost: cost(17, 96.6, 'allow') });
    const t = screen.getByTestId('mlx-state-badge');
    expect(phaseOf()).toBe('unloaded');
    expect(t.className).toContain('bg-lz-phase-unloaded');
    expect(t.className).toContain('border-lz-phase-unloaded-line');
    await expectDesigned(container);
  });

  it('queued: ORANGE, with how many requests wait', async () => {
    const { container } = tile({
      live: parseMlxLiveStatus({
        status: 'idle',
        num_running: 0,
        num_waiting: 2,
        requests: [
          { request_id: 'a', status: 'waiting', phase: 'queued', prompt_tokens: 900 },
          { request_id: 'b', status: 'waiting', phase: 'queued', prompt_tokens: 400 },
        ],
      }),
    });
    expect(phaseOf()).toBe('held');
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-held');
    expect(screen.getByTestId('mlx-live-queued')).toHaveTextContent('2');
    expect(screen.getByTestId('mlx-state-badge')).toHaveTextContent('requests waiting');
    await expectDesigned(container);
  });

  it('a measured start: AMBER, the phase in words and resident ÷ on-disk bytes as the bar', async () => {
    const { container } = tile({
      state: 'mounting',
      load: { phase: 'loading', residentBytes: 15.75 * GIB, weightsBytes: 31.5 * GIB },
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(phaseOf()).toBe('loading');
    expect(t.className).toContain('bg-lz-phase-loading');
    expect(screen.getByTestId('mlx-mount-load')).toHaveAttribute('data-measured', 'true');
    expect(t).toHaveTextContent('Loading weights');
    expect(within(t).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByTestId('mlx-load-figure')).toHaveTextContent('15.8 of 31.5 GB');
    await expectDesigned(container);
  });

  it('making room before the engine flips to mounting is already AMBER, with no invented figure', async () => {
    const { container } = tile({
      state: 'stopped',
      load: { phase: 'makingRoom', residentBytes: null, weightsBytes: 31.5 * GIB },
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(phaseOf()).toBe('loading');
    expect(t).toHaveAttribute('data-state', 'mounting');
    expect(t).toHaveTextContent('Making room');
    expect(within(t).getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
    expect(screen.getByTestId('mlx-load-indeterminate')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-mount-cost')).toBeNull();
    await expectDesigned(container);
  });

  it('distributed STARTING: amber, and a strip with EACH Mac — its colour, layers and load', async () => {
    const starting = {
      ...FLASH_READY,
      state: 'starting',
      inflight: null,
      nodes: [
        // The backend's per-node load figure (binding point: nodeLoadProgress).
        {
          ...FLASH_READY.nodes[0],
          state: 'loading',
          // goose's figure: MLX's active memory on the rank against the weights planned on it.
          activeMemoryGb: 12,
          plannedWeightsGb: 48,
        },
        { ...FLASH_READY.nodes[1], state: 'loading' },
      ],
    } as MlxDistributedStatus;
    const { container, unmount } = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: starting,
    });
    expect(phaseOf()).toBe('loading');
    const rows = screen.getAllByTestId('mlx-dist-strip-node');
    expect(rows.map((r) => r.getAttribute('data-node'))).toEqual(['MacBook Pro', 'workhorse']);
    expect(rows.map((r) => r.getAttribute('data-phase'))).toEqual(['loading', 'loading']);
    expect(rows[0]).toHaveTextContent('L0–19');
    expect(rows[0]).toHaveTextContent('Loading');
    expect(within(rows[0]).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '25');
    expect(rows[0]).toHaveTextContent('12.0 of 48.0 GB');
    expect(rows[1]).toHaveTextContent('L20–47');
    // No figure reported for the workhorse: the indeterminate track, never a number.
    expect(within(rows[1]).getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
    await expectDesigned(container);
    unmount();

    // The workhorse joins first: its row turns grey while the MacBook still loads.
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: {
        ...starting,
        nodes: [starting.nodes[0], { ...starting.nodes[1], state: 'ready' }],
      },
    });
    const next = screen.getAllByTestId('mlx-dist-strip-node');
    expect(next.map((r) => r.getAttribute('data-phase'))).toEqual(['loading', 'idle']);
    expect(within(next[1]).queryByRole('progressbar')).toBeNull();
  });

  it('distributed PREFLIGHT lists no ranks yet: the configured Macs, each amber', () => {
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_READY, state: 'preflight', nodes: [] },
    });
    const rows = screen.getAllByTestId('mlx-dist-strip-node');
    expect(rows.map((r) => r.getAttribute('data-phase'))).toEqual(['loading', 'loading']);
    expect(rows[0]).toHaveTextContent('Preflight');
  });

  it('making room and warming up are said per Mac, amber, with no bar while memory is reclaimed', () => {
    const { unmount } = tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: {
        ...FLASH_READY,
        state: 'preflight',
        nodes: [],
        makingRoom: ['workhorse'],
      } as MlxDistributedStatus,
    });
    let rows = screen.getAllByTestId('mlx-dist-strip-node');
    expect(rows[0]).toHaveTextContent('Preflight');
    expect(rows[1]).toHaveTextContent('Making room');
    expect(rows.map((r) => r.getAttribute('data-phase'))).toEqual(['loading', 'loading']);
    unmount();
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: {
        ...FLASH_READY,
        state: 'starting',
        nodes: [
          {
            ...FLASH_READY.nodes[0],
            state: 'loading',
            loadPhase: 'warming',
            plannedWeightsGb: 58.4,
          },
          { ...FLASH_READY.nodes[1], state: 'loading', plannedWeightsGb: 80 },
        ],
        // the backend's new per-node fields, ahead of the regenerated SDK types
      } as unknown as MlxDistributedStatus,
    });
    rows = screen.getAllByTestId('mlx-dist-strip-node');
    expect(rows[0]).toHaveTextContent('Warming up');
    expect(within(rows[0]).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100');
    expect(rows[1]).toHaveTextContent('Loading');
    expect(within(rows[1]).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
  });

  it('distributed FAILED is red', () => {
    tile({ state: 'stopped', modeLabel: 'x', distributed: { ...FLASH_READY, state: 'failed' } });
    expect(phaseOf()).toBe('failed');
  });

  it('the peer’s own tile: "Loading rank 1 for MacBook Pro" in amber, then grey once joined', async () => {
    const loading = {
      ...HOSTING_RANK_1,
      hosting: {
        ...HOSTING_RANK_1.hosting!,
        state: 'loading',
        loadedBytes: 10 * GIB,
        plannedWeightBytes: 24 * GIB,
      },
    } as MlxDistributedStatus;
    const { container, unmount } = tile({ state: 'stopped', distributed: loading });
    const t = screen.getByTestId('mlx-state-badge');
    expect(phaseOf()).toBe('loading');
    expect(within(t).getByRole('status')).toHaveTextContent('Loading rank 1 for MacBook Pro');
    expect(within(t).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '42');
    expect(screen.getByTestId('mlx-load-figure')).toHaveTextContent('10.0 of 24.0 GB');
    await expectDesigned(container);
    unmount();
    tile({ state: 'stopped', distributed: HOSTING_RANK_1 });
    expect(phaseOf()).toBe('idle');
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-idle');
  });
});
