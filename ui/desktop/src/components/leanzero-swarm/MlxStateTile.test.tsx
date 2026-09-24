import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MlxStateTile, type MlxStateTileProps } from './MlxStateTile';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { attributeServing, type MlxServingRow } from '../../utils/mlxServing';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  NO_RATES,
  advanceLastRates,
  advanceMountWatch,
  mountCost,
  mountFill,
  parseMlxLiveStatus,
  type LastRates,
  type MlxLiveStats,
  type TpsSample,
} from './mlxLiveStats';
import { GENERATING_STATUS, IDLE_STATUS, PREFILL_STATUS } from './mlxLiveStatus.fixtures';
import { FLASH_READY, FLASH_SERVING, STOPPED_WITH_CONFIG } from './mlxDistributed.fixtures';

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
    expect(t.className).toContain('bg-lz-ok-solid');
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
    expect(t.className).toContain('bg-lz-accent');
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
    expect(t.className).toContain('bg-lz-stopped-solid');
    expect(t.className).not.toContain('bg-lz-ok-solid');
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
    // Activity unknown: the neutral slate, not a colour that claims work.
    expect(screen.getByTestId('mlx-state-badge').className).toContain('bg-lz-stopped-solid');
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
    expect(t.className).toContain('bg-lz-accent');
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
    expect(within(t).queryByRole('progressbar')).toBeNull();
  });
});

describe('MlxStateTile STOPPED — what mounting would cost, and Mount on the tile', () => {
  it('fits: the size, the meter against free memory, the verdict and the action', async () => {
    const { container } = tile({
      state: 'stopped',
      cost: mountCost(31 * GIB, 68.6, 128),
      action: <button type="button">Mount</button>,
    });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-stopped-solid');
    expect(screen.getByTestId('mlx-mount-cost')).toHaveAttribute('data-verdict', 'fits');
    expect(t).toHaveTextContent('31.0GB to mount');
    expect(t).toHaveTextContent('Fits, 24.8 GB to spare');
    expect(t).toHaveTextContent('68.6 GB available');
    expect(within(t).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '45');
    expect(within(t).getByRole('button', { name: 'Mount' })).toBeInTheDocument();
    await expectDesigned(container);
  });

  it('no-fit names the shortfall; no model picked asks for one', () => {
    const { unmount } = tile({ state: 'stopped', cost: mountCost(31 * GIB, 40, 128) });
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
    expect(t.className).toContain('bg-lz-err-solid');
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
    expect(t.className).toContain('bg-lz-stopped-solid');
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
    expect(t.className).toContain('bg-lz-ok-solid');
    expect(screen.getByTestId('mlx-dist-tile-inflight')).toHaveTextContent('2');
    expect(t).toHaveTextContent('requests in flight');
    unmount();
    tile({
      state: 'stopped',
      modeLabel: 'x',
      distributed: { ...FLASH_SERVING, admissionOpen: false },
    });
    t = screen.getByTestId('mlx-state-badge');
    expect(t.className).toContain('bg-lz-warn-solid');
    expect(t).toHaveTextContent('Admission closed: a node is low on memory');
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
