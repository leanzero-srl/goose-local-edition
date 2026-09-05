import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import NodesStrip from './NodesStrip';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * Pass E — the blank-session "Nodes" strip: rows come from the CONFIGURED swarm devices (the same
 * swarm-config read the Goose Swarm nodes tab uses), never from LM Studio discovery, and
 * occupancy renders only where a live signal exists (the local mlx-sidecar node via the MLX engine
 * status). No fake states: cloud rows get a chip and nothing else.
 *
 * Studio remake (surface C): a Panel whose header counts the rows it shows; each row is a node dot
 * (identity hue by configured order) + name, the engine as a chip (LeanZero MLX in the secondary
 * tone, everything else quiet), the model id as quiet meta, and occupancy as a status-tone chip.
 */

const readMock = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: readMock }),
}));

let mlxStatus: MlxEngineStatus | null = null;
vi.mock('../leanzero-swarm/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: () => ({ status: mlxStatus, error: null }),
}));

const DEVICES = [
  {
    id: 'workhorse-mlx',
    model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
    weight: 2,
    enabled: true,
    engine: 'mlx-sidecar',
  },
  { id: 'zai-node', model_id: 'glm-4.7', weight: 2, enabled: true, provider: 'zai' },
];

const mount = () =>
  render(
    <IntlTestWrapper>
      <NodesStrip />
    </IntlTestWrapper>
  );

const dotIn = (row: HTMLElement) => row.querySelector('[data-testid="lz-status-dot"]');
const chipsIn = (row: HTMLElement) =>
  Array.from(row.querySelectorAll<HTMLElement>('[data-testid="lz-chip"]'));

beforeEach(() => {
  mlxStatus = null;
  readMock.mockReset();
  readMock.mockResolvedValue({ devices: DEVICES });
});

describe('NodesStrip', () => {
  it('renders exactly the configured devices — no LM Studio-discovered rows', async () => {
    mount();
    await waitFor(() => expect(screen.getByTestId('nodes-strip')).toBeInTheDocument());
    expect(screen.getByTestId('nodes-strip-row-workhorse-mlx')).toBeInTheDocument();
    expect(screen.getByTestId('nodes-strip-row-zai-node')).toBeInTheDocument();
    expect(screen.getAllByTestId(/^nodes-strip-row-/)).toHaveLength(DEVICES.length);
    // Engine chips: LeanZero MLX for the sidecar row, the cloud provider for the zai row — normal
    // case (uppercase belongs to the zone header alone).
    expect(screen.getByText('LeanZero MLX')).toBeInTheDocument();
    expect(screen.getByText('Z.ai')).toBeInTheDocument();
  });

  it('shows IDLE for the mlx node when the engine is stopped', async () => {
    mlxStatus = { state: 'stopped', restartRequired: false, availableMemoryGb: 32, totalMemoryGb: 64 };
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('nodes-strip-occupancy-workhorse-mlx')).toHaveTextContent('idle')
    );
    const occ = screen.getByTestId('nodes-strip-occupancy-workhorse-mlx');
    expect(chipsIn(occ)[0]?.getAttribute('data-tone')).toBe('stopped');
    expect(dotIn(screen.getByTestId('nodes-strip-row-workhorse-mlx'))?.getAttribute('data-live')).toBeNull();
  });

  it('shows SERVING with the served model when the engine runs', async () => {
    mlxStatus = {
      state: 'running',
      modelId: 'mlx-community/Qwen3.5-9B-4bit',
      servedModelId: 'workhorse-qwen3.5-9b-4bit-mlx',
      restartRequired: false,
      availableMemoryGb: 32,
      totalMemoryGb: 64,
    };
    mount();
    const occ = await screen.findByTestId('nodes-strip-occupancy-workhorse-mlx');
    expect(occ).toHaveTextContent('serving');
    expect(occ).toHaveTextContent('workhorse-qwen3.5-9b-4bit-mlx');
    expect(chipsIn(occ)[0]?.getAttribute('data-tone')).toBe('ok');
    // A serving node is the one live thing on a blank session: its identity dot pulses (by scale).
    expect(dotIn(screen.getByTestId('nodes-strip-row-workhorse-mlx'))?.getAttribute('data-live')).toBe(
      'true'
    );
  });

  it('shows MOUNTING while the engine mounts', async () => {
    mlxStatus = { state: 'mounting', restartRequired: false, availableMemoryGb: 32, totalMemoryGb: 64 };
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('nodes-strip-occupancy-workhorse-mlx')).toHaveTextContent(
        'mounting'
      )
    );
    const occ = screen.getByTestId('nodes-strip-occupancy-workhorse-mlx');
    expect(chipsIn(occ)[0]?.getAttribute('data-tone')).toBe('warn');
  });

  it('never invents occupancy: cloud rows have no state, and no signal means no chip', async () => {
    mlxStatus = null;
    mount();
    await waitFor(() => expect(screen.getByTestId('nodes-strip')).toBeInTheDocument());
    expect(screen.queryByTestId('nodes-strip-occupancy-zai-node')).toBeNull();
    expect(screen.queryByTestId('nodes-strip-occupancy-workhorse-mlx')).toBeNull();
  });

  it('renders nothing at all when no devices are configured', async () => {
    readMock.mockResolvedValue({ devices: [] });
    mount();
    await waitFor(() => expect(readMock).toHaveBeenCalled());
    expect(screen.queryByTestId('nodes-strip')).toBeNull();
  });

  it('is a Studio Panel: the header counts the rows, each row is a node dot + name, the engine is a chip', async () => {
    mlxStatus = { state: 'stopped', restartRequired: false, availableMemoryGb: 32, totalMemoryGb: 64 };
    const { container } = mount();
    await waitFor(() => expect(screen.getByTestId('lz-panel')).toBeInTheDocument());
    expect(screen.getByTestId('lz-section-count').textContent).toBe(String(DEVICES.length));
    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('Nodes');

    const mlxRow = screen.getByTestId('nodes-strip-row-workhorse-mlx');
    const zaiRow = screen.getByTestId('nodes-strip-row-zai-node');
    // Identity hue follows the configured order: row 1 → node-1, row 2 → node-2.
    expect(dotIn(mlxRow)?.className).toContain('bg-lz-node-1');
    expect(dotIn(zaiRow)?.className).toContain('bg-lz-node-2');
    expect(dotIn(mlxRow)?.getAttribute('aria-label')).toBe('workhorse-mlx');
    // The engine chip: LeanZero MLX keeps its violet through the secondary tone; a cloud provider
    // is quiet metadata (outline, no fill).
    const mlxChip = chipsIn(mlxRow).find((c) => c.textContent === 'LeanZero MLX');
    const zaiChip = chipsIn(zaiRow).find((c) => c.textContent === 'Z.ai');
    expect(mlxChip?.getAttribute('data-tone')).toBe('secondary');
    expect(zaiChip?.getAttribute('data-tone')).toBeNull();
    expect(zaiChip?.className).toContain('border-lz-border-strong');
    // The model id is quiet meta on the row.
    expect(mlxRow).toHaveTextContent('workhorse-qwen3.5-9b-4bit-mlx');
    expect(zaiRow).toHaveTextContent('glm-4.7');
    // Rows are the dense 32px register.
    expect(mlxRow.className).toContain('h-lz-row-dense');
    // Uppercase belongs to the zone header alone.
    for (const el of Array.from(container.querySelectorAll<HTMLElement>('[class*="uppercase"]'))) {
      expect(el.tagName).toBe('H2');
    }
    // No hand-written colour anywhere — every hue is a token utility.
    for (const el of Array.from(container.querySelectorAll<HTMLElement>('[style]'))) {
      expect(el.getAttribute('style') ?? '').not.toMatch(/#[0-9a-f]{3,6}|rgb/i);
    }
    assertStudioClean(container);
  });

  it('every class it emits compiles to a real rule against main.css', async () => {
    mlxStatus = {
      state: 'running',
      modelId: 'mlx-community/Qwen3.5-9B-4bit',
      servedModelId: 'workhorse-qwen3.5-9b-4bit-mlx',
      restartRequired: false,
      availableMemoryGb: 32,
      totalMemoryGb: 64,
    };
    const { container } = mount();
    await screen.findByTestId('nodes-strip-occupancy-workhorse-mlx');
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('NodesStrip — a clipped model id is a door to the whole id', () => {
  it('titles the clipped id with the full model id and reveals it with the node as a chip', async () => {
    Object.defineProperty(HTMLElement.prototype, 'scrollWidth', { configurable: true, get: () => 1000 });
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 100 });
    try {
      mount();
      const models = await screen.findAllByTestId('nodes-strip-model');
      const mlx = models.find((m) => m.getAttribute('title') === 'workhorse-qwen3.5-9b-4bit-mlx');
      expect(mlx).toBeDefined();
      expect(mlx).toHaveAttribute('data-clipped', 'true');
      fireEvent.click(mlx as HTMLElement);
      const dialog = screen.getByRole('dialog');
      expect(screen.getByTestId('reveal-body')).toHaveTextContent('workhorse-qwen3.5-9b-4bit-mlx');
      expect(dialog).toHaveTextContent('workhorse-mlx');
      assertStudioClean(dialog);
      expect(await missingUtilities(allClasses(dialog).filter((c) => !c.startsWith('lucide')))).toEqual([]);
      fireEvent.click(screen.getByTestId('reveal-close'));
      expect(screen.queryByRole('dialog')).toBeNull();
    } finally {
      delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
      delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
    }
  });
});
