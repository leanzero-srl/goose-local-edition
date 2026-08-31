import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SwarmNodesSection from './SwarmNodesSection';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';

// ---------------------------------------------------------------------------
// The Swarm Settings tab, post-amendment: NODES ONLY. These tests pin
//  - the simplification itself (no tunable beyond weight renders),
//  - the WRITE PATHS (config upsert for local rows, engine CLI for cloud — the invariant),
//  - the MLX machine cap (discovered machines minus already-added),
//  - the remote-machine row (host + the amber awaiting-routing chip).
// ---------------------------------------------------------------------------

const mockRead = vi.fn();
const mockUpsert = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead, upsert: mockUpsert }),
}));

const fleetState = {
  lanes: [] as unknown[],
  models: ['gabee-qwen3.8-27b'],
  online: true,
  loading: false,
  endpoint: 'http://localhost:1234',
};
vi.mock('../swarm/useFleet', () => ({
  useFleet: () => fleetState,
  deviceFromModelId: (id: string) => {
    const bare = id.split('/').pop() || id;
    const dash = bare.indexOf('-');
    return dash > 0 ? bare.slice(0, dash) : bare;
  },
}));

const mockMlxModelsList = vi.fn();
const mockMlxSettingsRead = vi.fn();
const mockMlxSettingsUpdate = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineModelsList: (...a: unknown[]) => mockMlxModelsList(...a),
  mlxEngineSettingsRead: (...a: unknown[]) => mockMlxSettingsRead(...a),
  mlxEngineSettingsUpdate: (...a: unknown[]) => mockMlxSettingsUpdate(...a),
}));

const mockSwarmCloud = vi.fn();
const mockFleetMachines = vi.fn();

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const render = () => rtlRender(<SwarmNodesSection />, { wrapper: IntlTestWrapper });

const HF = 'mlx-community/Qwen3.5-9B-MLX-4bit';

/** The live config shape: one mlx-sidecar node + unknown fields that MUST round-trip. */
const BASE_CFG: SwarmConfig = {
  endpoint: 'http://localhost:1234',
  devices: [
    {
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 2,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
      // an unknown per-row field a future engine may add — the panel must pass it through
      future_row_field: 'keep-me',
    } as unknown as SwarmDeviceRow,
    {
      id: 'zai-glm',
      model_id: 'glm-5.3-flash',
      weight: 2,
      enabled: true,
      provider: 'zai',
      host: 'zai',
    },
  ],
  planner_model: 'workhorse-qwen3.5-9b-4bit-mlx',
  worker_extensions: ['developer'], // unknown top-level field — must survive every write
};

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  fleetState.models = ['gabee-qwen3.8-27b'];
  mockRead.mockResolvedValue(BASE_CFG);
  mockUpsert.mockResolvedValue(undefined);
  mockMlxModelsList.mockResolvedValue({
    models: [{ id: HF, sizeBytes: 5e9, complete: true, missingFiles: 0 }],
    diskAvailableBytes: 0,
    diskTotalBytes: 0,
  });
  mockMlxSettingsRead.mockResolvedValue({
    modelId: HF,
    modelsDir: '/x',
    port: 8090,
    spawnCommand: ['uvx', 'rapid-mlx', 'serve'],
    servedModelName: 'workhorse-qwen3.5-9b-4bit-mlx',
    modelProfiles: {},
  });
  mockMlxSettingsUpdate.mockImplementation(async (s: unknown) => s);
  mockSwarmCloud.mockResolvedValue({ ok: true, stdout: '{}', stderr: '', error: null });
  mockFleetMachines.mockResolvedValue([
    { machine: 'workhorse', local: true },
    { machine: 'mihai', local: false },
  ]);
  // the shared setup's window.electron is writable-but-not-configurable — extend it in place
  Object.assign(window.electron as unknown as Record<string, unknown>, {
    swarmCloud: mockSwarmCloud,
    fleetMachines: mockFleetMachines,
  });
});

/** The last call's payload — the write that would actually land in config.yaml. */
const lastUpsertPayload = (): SwarmConfig => {
  const calls = mockUpsert.mock.calls;
  return calls[calls.length - 1][1] as SwarmConfig;
};

afterEach(() => cleanup());

describe('the simplified Nodes tab', () => {
  it('lists configured rows (mlx violet chip, cloud chip) AND the discovered LM Studio node', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-workhorse-mlx')).toBeInTheDocument();
    });
    expect(
      within(screen.getByTestId('swarm-node-workhorse-mlx')).getByText('LEANZERO MLX')
    ).toBeInTheDocument();
    expect(within(screen.getByTestId('swarm-node-zai-glm')).getByText('Z.AI')).toBeInTheDocument();
    const discovered = screen.getByTestId('swarm-node-gabee-qwen3.8-27b');
    expect(within(discovered).getByText('LM STUDIO')).toBeInTheDocument();
    expect(within(discovered).getByText('auto')).toBeInTheDocument();
  });

  it('renders NO tunable beyond weight: no switches, no golden formula, no planner, no timeouts', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-workhorse-mlx')).toBeInTheDocument();
    });
    // the amendment: label + provider + weight + remove, and NOTHING else
    expect(screen.queryAllByRole('switch')).toHaveLength(0);
    for (const leverText of [
      'Golden formula',
      'Worker max turns',
      'Planner model',
      'Run panel detail',
      'Ask when uncertain',
      'Sampling defaults',
      'Smartest',
    ]) {
      expect(screen.queryByText(leverText)).not.toBeInTheDocument();
    }
    // weight steppers exist for every row
    expect(screen.getAllByRole('button', { name: /More work/ }).length).toBeGreaterThanOrEqual(3);
  });

  it('a REMOTE mlx row (host set) wears the solid amber awaiting-routing chip', async () => {
    mockRead.mockResolvedValue({
      ...BASE_CFG,
      devices: [
        ...(BASE_CFG.devices as SwarmDeviceRow[]),
        {
          id: 'mihai-mlx',
          model_id: 'mihai-qwen3.5-9b-4bit-mlx',
          weight: 2,
          enabled: true,
          instances: 1,
          engine: 'mlx-sidecar',
          host: 'mihai',
        },
      ],
    });
    render();
    await waitFor(() => {
      expect(screen.getByTestId('awaiting-routing-mihai-mlx')).toBeInTheDocument();
    });
    // the LOCAL mlx node never wears it
    expect(screen.queryByTestId('awaiting-routing-workhorse-mlx')).not.toBeInTheDocument();
  });

  it('a weight edit on a configured row rewrites SwarmDevice.weight in place, unknown fields intact', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-workhorse-mlx')).toBeInTheDocument();
    });
    await userEvent.click(
      within(screen.getByTestId('swarm-node-workhorse-mlx')).getByRole('button', {
        name: 'More work (workhorse-mlx)',
      })
    );
    await waitFor(() => {
      expect(mockUpsert).toHaveBeenCalled();
    });
    const payload = lastUpsertPayload();
    const row = payload.devices?.find((d) => d.id === 'workhorse-mlx');
    expect(row).toMatchObject({ weight: 3, engine: 'mlx-sidecar', instances: 1 });
    expect((row as unknown as Record<string, unknown>).future_row_field).toBe('keep-me');
    expect(payload.worker_extensions).toEqual(['developer']);
    // speed_weights is NOT the write target any more (the amendment)
    expect(payload.speed_weights).toBeUndefined();
  });

  it('a weight edit on a DISCOVERED row materializes a device row for it', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-gabee-qwen3.8-27b')).toBeInTheDocument();
    });
    await userEvent.click(
      screen.getByRole('button', { name: 'More work (gabee-qwen3.8-27b)' })
    );
    await waitFor(() => {
      expect(mockUpsert).toHaveBeenCalled();
    });
    const payload = lastUpsertPayload();
    expect(payload.devices?.find((d) => d.id === 'gabee')).toMatchObject({
      model_id: 'gabee-qwen3.8-27b',
      weight: 2,
      enabled: true,
    });
  });

  it('a weight edit on a CLOUD row rides the CLI (rm then add --weight) — never an upsert', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-zai-glm')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'More work (zai-glm)' }));
    await waitFor(() => {
      expect(mockSwarmCloud).toHaveBeenCalledWith('zai', ['rm', 'glm-5.3-flash']);
      expect(mockSwarmCloud).toHaveBeenCalledWith('zai', [
        'add',
        'glm-5.3-flash',
        '--weight',
        '3',
      ]);
    });
    expect(mockUpsert).not.toHaveBeenCalled();
  });

  it('removing a CLOUD node drives the CLI (rm) and never a device upsert', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-zai-glm')).toBeInTheDocument();
    });
    await userEvent.click(
      within(screen.getByTestId('swarm-node-zai-glm')).getByRole('button', {
        name: 'Remove node: zai-glm',
      })
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(mockSwarmCloud).toHaveBeenCalledWith('zai', ['rm', 'glm-5.3-flash']);
    });
    expect(mockUpsert).not.toHaveBeenCalled();
  });

  it('removing a LOCAL node splices the device row via the config write', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-workhorse-mlx')).toBeInTheDocument();
    });
    await userEvent.click(
      within(screen.getByTestId('swarm-node-workhorse-mlx')).getByRole('button', {
        name: 'Remove node: workhorse-mlx',
      })
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(mockUpsert).toHaveBeenCalled();
    });
    const payload = lastUpsertPayload();
    expect(payload.devices?.some((d) => d.id === 'workhorse-mlx')).toBe(false);
    expect(payload.devices?.some((d) => d.id === 'zai-glm')).toBe(true);
  });
});

describe('Add node — MLX machine cap', () => {
  it('offers exactly the discovered machines minus those already added, tagged local/remote', async () => {
    render();
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /LeanZero MLX/ }));
    await waitFor(() => {
      expect(screen.getByTestId('add-node-mlx-pane')).toBeInTheDocument();
    });
    // machines: workhorse (local, ALREADY ADDED as workhorse-mlx) + mihai (remote) + gabee
    // (HTTP-discovered) => addable = mihai + gabee
    const pane = within(screen.getByTestId('add-node-mlx-pane'));
    await userEvent.click(pane.getAllByRole('combobox')[0]);
    const opts = await screen.findAllByRole('option');
    const names = opts.map((o) => o.textContent);
    expect(names.some((n) => n?.includes('mihai'))).toBe(true);
    expect(names.some((n) => n?.includes('gabee'))).toBe(true);
    expect(names.some((n) => n?.includes('workhorse'))).toBe(false);
  });

  it('adding a REMOTE machine writes host + engine and NEVER touches the local engine settings', async () => {
    render();
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /LeanZero MLX/ }));
    await waitFor(() => {
      expect(screen.getByTestId('add-node-mlx-pane')).toBeInTheDocument();
    });
    const pane = within(screen.getByTestId('add-node-mlx-pane'));
    await userEvent.click(pane.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /mihai/ }));
    // the awaiting-routing reality is stated in the dialog
    expect(screen.getByText(/AWAITS FLEET ROUTING/)).toBeInTheDocument();
    await userEvent.click(pane.getAllByRole('combobox')[1]);
    await userEvent.click(await screen.findByRole('option', { name: /Qwen3\.5-9B-MLX-4bit/ }));
    await userEvent.click(screen.getByTestId('add-node-mlx-submit'));
    await waitFor(() => {
      expect(mockUpsert).toHaveBeenCalled();
    });
    const payload = lastUpsertPayload();
    const devices = payload.devices ?? [];
    expect(devices[devices.length - 1]).toEqual({
      id: 'mihai-mlx',
      model_id: 'mihai-qwen3.5-9b-4bit-mlx',
      weight: 2,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
      host: 'mihai',
    });
    // remote adds must never re-point the LOCAL engine
    expect(mockMlxSettingsUpdate).not.toHaveBeenCalled();
  });

  it('adding the LOCAL machine aligns mlx_engine (model_id + served alias) AND writes the row', async () => {
    // free the local machine: config without the workhorse node
    mockRead.mockResolvedValue({ ...BASE_CFG, devices: [] });
    render();
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /LeanZero MLX/ }));
    await waitFor(() => {
      expect(screen.getByTestId('add-node-mlx-pane')).toBeInTheDocument();
    });
    const pane = within(screen.getByTestId('add-node-mlx-pane'));
    await userEvent.click(pane.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /workhorse/ }));
    await userEvent.click(pane.getAllByRole('combobox')[1]);
    await userEvent.click(await screen.findByRole('option', { name: /Qwen3\.5-9B-MLX-4bit/ }));
    await userEvent.click(screen.getByTestId('add-node-mlx-submit'));
    await waitFor(() => {
      expect(mockMlxSettingsUpdate).toHaveBeenCalled();
    });
    expect(mockMlxSettingsUpdate.mock.calls[0][0]).toMatchObject({
      modelId: HF,
      servedModelName: 'workhorse-qwen3.5-9b-4bit-mlx',
    });
    await waitFor(() => {
      expect(mockUpsert).toHaveBeenCalled();
    });
    const payload = lastUpsertPayload();
    const devices = payload.devices ?? [];
    expect(devices[devices.length - 1]).toEqual({
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 2,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
    });
  });
});

describe('Add node — cloud path (the invariant)', () => {
  it('a cloud add drives the CLI IPC with the chosen weight and NEVER a config upsert', async () => {
    mockSwarmCloud.mockImplementation(async (_provider: string, args: string[]) => {
      if (args[0] === 'models') {
        return {
          ok: true,
          stdout: JSON.stringify({ models: ['glm-5.3-turbo'] }),
          stderr: '',
          error: null,
        };
      }
      return { ok: true, stdout: '', stderr: '', error: null };
    });
    render();
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /Z\.ai/ }));
    await waitFor(() => {
      expect(mockSwarmCloud).toHaveBeenCalledWith('zai', ['models', '--json']);
    });
    await userEvent.click(screen.getByRole('button', { name: 'More work (weight)' }));
    await userEvent.click(await screen.findByRole('button', { name: '+ Add' }));
    await waitFor(() => {
      expect(mockSwarmCloud).toHaveBeenCalledWith('zai', [
        'add',
        'glm-5.3-turbo',
        '--weight',
        '3',
      ]);
    });
    expect(mockUpsert).not.toHaveBeenCalled();
    expect(mockRead).toHaveBeenCalled();
  });
});
