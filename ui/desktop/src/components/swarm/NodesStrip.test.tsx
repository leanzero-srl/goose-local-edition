import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import NodesStrip from './NodesStrip';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { MlxEngineStatus } from '../../acp/mlx-engine';

/**
 * Pass E — the blank-session "Nodes" strip: rows come from the CONFIGURED swarm devices (the same
 * swarm-config read the LeanZero Swarm nodes tab uses), never from LM Studio discovery, and
 * occupancy renders only where a live signal exists (the local mlx-sidecar node via the MLX engine
 * status). No fake states: cloud rows get a chip and nothing else.
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
    // Provider chips: LeanZero MLX for the sidecar row, the cloud chip for the zai row.
    expect(screen.getByText('LEANZERO MLX')).toBeInTheDocument();
    expect(screen.getByText('Z.AI')).toBeInTheDocument();
  });

  it('shows IDLE for the mlx node when the engine is stopped', async () => {
    mlxStatus = { state: 'stopped', restartRequired: false, availableMemoryGb: 32, totalMemoryGb: 64 };
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('nodes-strip-occupancy-workhorse-mlx')).toHaveTextContent('idle')
    );
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
  });

  it('shows MOUNTING while the engine mounts', async () => {
    mlxStatus = { state: 'mounting', restartRequired: false, availableMemoryGb: 32, totalMemoryGb: 64 };
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('nodes-strip-occupancy-workhorse-mlx')).toHaveTextContent(
        'mounting'
      )
    );
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
});
