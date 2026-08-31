import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SwarmNodesSection from './SwarmNodesSection';
import { deriveProviderOptions, SHOW_LMSTUDIO_PROVIDER } from './AddNodeDialog';
import { CLOUD_PROVIDERS } from './cloud';
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
// The mock honors the `enabled` gate exactly like the real hook: disabled discovery reads offline.
vi.mock('../swarm/useFleet', () => ({
  useFleet: (_pollMs?: number, _endpoint?: string, enabled = true) =>
    enabled
      ? fleetState
      : { lanes: [], models: [], online: false, loading: false, endpoint: '' },
  deviceFromModelId: (id: string) => {
    const bare = id.split('/').pop() || id;
    const dash = bare.indexOf('-');
    return dash > 0 ? bare.slice(0, dash) : bare;
  },
}));

// Pass E follow-up: LM Studio-DISCOVERED rows ride the showLmStudioFleet setting (default OFF).
let lmStudioVisible = false;
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({
  useLmStudioFleetVisible: () => lmStudioVisible,
}));

// The configured-provider join for the add dialog: registry ids acpListProviderDetails reports
// as configured on this machine (mutable per test).
let providerDetails: Array<{ name: string; is_configured: boolean }> = [];
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: async () => providerDetails,
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
  lmStudioVisible = false;
  // zai holds a key on the fixture machine; the other engine families do not.
  providerDetails = [
    { name: 'zai', is_configured: true },
    { name: 'aws_bedrock', is_configured: false },
    { name: 'google', is_configured: false },
    { name: 'anthropic', is_configured: true }, // non-swarm provider: never becomes a node option
  ];
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
  it('lists ONLY configured rows by default — LM Studio-discovered rows stay hidden (setting off)', async () => {
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-workhorse-mlx')).toBeInTheDocument();
    });
    expect(
      within(screen.getByTestId('swarm-node-workhorse-mlx')).getByText('LEANZERO MLX')
    ).toBeInTheDocument();
    expect(within(screen.getByTestId('swarm-node-zai-glm')).getByText('Z.AI')).toBeInTheDocument();
    expect(screen.queryByTestId('swarm-node-gabee-qwen3.8-27b')).toBeNull();
    expect(screen.queryByText('LM STUDIO')).toBeNull();
  });

  it('discovered LM Studio rows RETURN when showLmStudioFleet is on (the visible twin)', async () => {
    lmStudioVisible = true;
    render();
    await waitFor(() => {
      expect(screen.getByTestId('swarm-node-gabee-qwen3.8-27b')).toBeInTheDocument();
    });
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
    // weight steppers exist for every rendered (configured) row
    expect(screen.getAllByRole('button', { name: /More work/ }).length).toBeGreaterThanOrEqual(2);
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
    lmStudioVisible = true;
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

describe('Add node — DERIVED provider list (pass E follow-up, owner)', () => {
  it('derives [mlx, ...every CLOUD_PROVIDERS entry] from the ONE mirror — never a hardcoded list', () => {
    const opts = deriveProviderOptions(new Set(['zai']), false);
    // The cloud slice IS the mirror, 1:1 and in order: a provider added to CLOUD_PROVIDERS
    // appears here with no other change.
    expect(opts.map((o) => o.value)).toEqual(['mlx', ...CLOUD_PROVIDERS.map((c) => c.cli)]);
    expect(opts.slice(1).map((o) => o.label)).toEqual(CLOUD_PROVIDERS.map((c) => c.label));
  });

  it('the mirror carries the engine CLOUD_DEFS registry ids (the join keys)', () => {
    expect(CLOUD_PROVIDERS.map((c) => c.registry)).toEqual([
      'aws_bedrock',
      'zai',
      'google',
      'custom_deepseek',
    ]);
  });

  it('configured -> selectable; unconfigured -> the no-key state; unknown read -> selectable', () => {
    const opts = deriveProviderOptions(new Set(['zai', 'google']), false);
    const byValue = Object.fromEntries(opts.map((o) => [o.value, o.configured]));
    expect(byValue).toMatchObject({ mlx: true, zai: true, google: true, bedrock: false, deepseek: false });
    // A failed provider-details read must not dead-end the dialog: everything stays selectable
    // and the engine-side CloudPane check governs.
    expect(deriveProviderOptions(null, false).every((o) => o.configured)).toBe(true);
  });

  it('lmstudio is absent by default (code gate off) and present only when BOTH gates open', () => {
    expect(SHOW_LMSTUDIO_PROVIDER).toBe(false);
    expect(deriveProviderOptions(new Set(), false).some((o) => o.value === 'lmstudio')).toBe(false);
    const withLm = deriveProviderOptions(new Set(), true);
    expect(withLm.some((o) => o.value === 'lmstudio')).toBe(true);
    expect(withLm[0].value).toBe('mlx'); // MLX stays first
  });

  it('renders true configured state: zai plain, the key-less families badged, no LM Studio', async () => {
    render();
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    const opts = await screen.findAllByRole('option');
    const names = opts.map((o) => o.textContent ?? '');
    expect(names.some((n) => n.includes('LeanZero MLX'))).toBe(true);
    expect(names.some((n) => /LM Studio/i.test(n))).toBe(false);
    expect(screen.queryByTestId('provider-no-key-zai')).toBeNull();
    expect(screen.getByTestId('provider-no-key-bedrock')).toBeInTheDocument();
    expect(screen.getByTestId('provider-no-key-google')).toBeInTheDocument();
    expect(screen.getByTestId('provider-no-key-deepseek')).toBeInTheDocument();
  });

  it('picking a key-less provider shows the no-key state whose action deep-links to Cloud Providers', async () => {
    const openCloud = vi.fn();
    rtlRender(<SwarmNodesSection onOpenCloudProviders={openCloud} />, {
      wrapper: IntlTestWrapper,
    });
    await userEvent.click(await screen.findByTestId('swarm-add-node'));
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /Amazon Bedrock/ }));
    await waitFor(() => expect(screen.getByTestId('add-node-no-key-pane')).toBeInTheDocument());
    // the add pane must NOT render for a key-less family
    expect(mockSwarmCloud).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId('add-node-configure-cloud'));
    expect(openCloud).toHaveBeenCalledTimes(1);
  });
});

describe('Add node — MLX machine cap', () => {
  it('offers exactly the discovered machines minus those already added, tagged local/remote', async () => {
    // gabee is HTTP-discovered via fleetModels, so this pin runs with discovery visible
    lmStudioVisible = true;
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
