import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MlxStateTile, type MlxStateTileProps } from './MlxStateTile';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  advanceMountWatch,
  mountCost,
  mountFill,
  parseMlxLiveStatus,
  type TpsSample,
} from './mlxLiveStats';
import { GENERATING_STATUS, IDLE_STATUS, PREFILL_STATUS } from './mlxLiveStatus.fixtures';

const GIB = 1024 * 1024 * 1024;

const HISTORY: TpsSample[] = [
  { uptimeS: 1868.3, tps: 0 },
  { uptimeS: 1870.3, tps: 18.7 },
  { uptimeS: 1872.3, tps: 19.4 },
  { uptimeS: 1874.3, tps: 19.9 },
];

function tile(overrides: Partial<MlxStateTileProps>) {
  const props: MlxStateTileProps = {
    state: 'running',
    unreachable: false,
    live: null,
    history: [],
    mount: null,
    cost: null,
    failedError: null,
    action: null,
    ...overrides,
  };
  return render(<MlxStateTile {...props} />);
}

async function expectDesigned(container: HTMLElement) {
  assertStudioClean(container);
  // lucide's own marker classes (`lucide`, `lucide-play`) are not utilities.
  const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
  expect(await missingUtilities(utilities)).toEqual([]);
}

describe('MlxStateTile RUNNING — the live instrument', () => {
  it('generating: the decode rate big, the sparkline, every request with its phase, the facts', async () => {
    const { container } = tile({ live: parseMlxLiveStatus(GENERATING_STATUS), history: HISTORY });
    const t = screen.getByTestId('mlx-state-badge');
    expect(t).toHaveAttribute('data-state', 'running');
    expect(t.className).toContain('bg-lz-ok-solid');
    expect(t.className).toContain('lg:w-[30rem]');
    expect(within(t).getByText('Generating')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('19.9');
    expect(within(t).getByText('tokens per second')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-tps-sparkline')).toBeInTheDocument();

    const rows = screen.getAllByTestId('mlx-live-request');
    // Running first (engine order), then the queue.
    expect(rows.map((r) => r.dataset.phase)).toEqual(['generation', 'prefill', 'queued']);
    expect(rows[0]).toHaveTextContent('Writing · 28,035 of 32,768 tokens');
    expect(rows[0]).toHaveTextContent('86%');
    expect(within(rows[0]).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '86');
    // The long silent pre-fill is visible, with the engine's own elapsed seconds and no fake bar.
    expect(rows[1]).toHaveTextContent('Reading prompt · 32k tokens');
    expect(rows[1]).toHaveTextContent('2m 45s');
    expect(within(rows[1]).queryByRole('progressbar')).toBeNull();
    expect(rows[2]).toHaveTextContent('Queued · 12k tokens');

    expect(within(t).getByText('50.7 GB')).toBeInTheDocument();
    expect(within(t).getByText('GPU memory in use')).toBeInTheDocument();
    expect(within(t).getByText('78%')).toBeInTheDocument();
    expect(within(t).getByText('Prompt cache hits')).toBeInTheDocument();
    expect(within(t).getByText('Waiting')).toBeInTheDocument();
    await expectDesigned(container);
  });

  it('prefill only: "Reading prompt" is the activity, and the rate is labelled the LAST run', () => {
    tile({ live: parseMlxLiveStatus(PREFILL_STATUS) });
    const t = screen.getByTestId('mlx-state-badge');
    expect(screen.getByTestId('mlx-live')).toHaveAttribute('data-activity', 'prefill');
    expect(screen.getByTestId('mlx-live-tps')).toHaveTextContent('19.9');
    expect(within(t).getByText('tokens per second, last run')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-live-request')).toHaveTextContent('Reading prompt · 32k tokens');
  });

  it('idle (the verbatim engine body): Idle, the sticky rate as last run, no request rows', () => {
    tile({ live: parseMlxLiveStatus(IDLE_STATUS) });
    const t = screen.getByTestId('mlx-state-badge');
    expect(within(t).getByText('Idle')).toBeInTheDocument();
    expect(within(t).getByText('tokens per second, last run')).toBeInTheDocument();
    expect(screen.queryAllByTestId('mlx-live-request')).toHaveLength(0);
    expect(within(t).getByText('54.3 GB')).toBeInTheDocument();
    expect(within(t).getByText('20%')).toBeInTheDocument();
  });

  it('a failed read says "Live stats unavailable" with the reason — nothing invented', async () => {
    const { container } = tile({
      live: { ok: false, detail: 'unreachable: connect ECONNREFUSED 127.0.0.1:8090' },
    });
    expect(screen.getByTestId('mlx-live-unavailable')).toHaveTextContent(
      'Live stats unavailableunreachable: connect ECONNREFUSED 127.0.0.1:8090'
    );
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
    expect(t).toHaveTextContent('68.6 GB free');
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
