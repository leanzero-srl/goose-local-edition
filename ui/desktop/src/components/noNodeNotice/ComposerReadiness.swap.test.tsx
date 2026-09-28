import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import type { NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { ComposerReadinessStrip } from './ComposerReadiness';
import { useChatServedBy } from '../chatServedBy/useChatServedBy';
import { resetEngineGlanceForTests } from '../engineGlance/glanceStore';
import ModelsBottomBar from '../settings/models/bottom_bar/ModelsBottomBar';
import {
  J3_CHAT_NODE,
  J3_EXIT_143,
  J3_MODEL,
  J3_READ,
  J3_SERVING_SINGLE,
  J3_STRATEGY,
  J3_SWAP_TO_SINGLE,
  J3_SWAP_TO_SPLIT,
  J3_WAITING,
  REAL_FAILURE,
} from '../../utils/nodeSwap.fixtures';

/**
 * Q-254 / Q-255 on the composer — the readiness bar and the model chip, from the ONE derivation
 * (useChatServedBy) as ChatInput hands it to both. Each case is a state live J3 on 3.0.65 put on
 * screen (~/goose-builds/quality/LIVE-2026-09-28-3.0.65): the swap read as "No model is mounted —
 * mihai-mlx" with the stopped engine's "exit status: 143", and the chip named the chat
 * `strategy:new-strategy`. The negative control is a real failure, which still says Failed.
 */

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: vi.fn(),
  mlxEngineModelsList: async () => ({ models: [], diskAvailableBytes: 0, diskTotalBytes: 0 }),
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: vi.fn(),
  mlxEngineSettingsRead: async () => SETTINGS,
}));
let residency: NodesResidencyResponse_unstable = J3_SERVING_SINGLE;
const mockExtMethod = vi.fn(async (method: string) => {
  if (method.endsWith('/nodes/read')) return J3_READ;
  if (method.endsWith('/nodes/residency')) return residency;
  if (method.endsWith('/nodes/servedLast')) return { record: null };
  return { status: { state: 'off' } };
});
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));
let devices: SwarmDeviceRow[] = [];
vi.mock('../../acp/config', () => ({
  acpReadConfig: async () => ({ devices }),
}));
vi.mock('../../acp/mlx-serving-intent', () => ({
  mlxServingIntent: async () => ({ intent: null, error: null }),
}));
vi.mock('../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    currentModel: null,
    currentProvider: null,
    changeModel: vi.fn(),
  }),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ mlxEngine: true }),
}));
vi.mock('../settings/models/modelInterface', () => ({
  getProviderMetadata: async () => ({ display_name: 'Goose Swarm', default_model: 'swarm' }),
}));

const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const SETTINGS: MlxEngineSettings = {
  modelId: J3_MODEL,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
/** The pool's one MLX device, adopted as the node "Mihai Macbook engine". */
const MLX_DEVICE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const BASE: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
};
/** 19-j3-delegate-swap.png: the 27B single, SIGTERMed by the swap's stop. */
const STOPPED_BY_SWAP: MlxEngineStatus = {
  ...BASE,
  state: 'failed',
  modelId: J3_MODEL,
  lastError: J3_EXIT_143,
};
/** 17-j3-t1-02.png: the 27B single mounting for the chat's own turn. */
const MOUNTING: MlxEngineStatus = { ...BASE, state: 'mounting', modelId: J3_MODEL };
const RUNNING: MlxEngineStatus = {
  ...BASE,
  state: 'running',
  modelId: J3_MODEL,
  servedModelId: ALIAS,
};

function Composer({ model, turnInFlight }: { model: string; turnInFlight: boolean }) {
  const serving = useChatServedBy('swarm', 'chat-1', turnInFlight, model);
  return (
    <>
      <ComposerReadinessStrip serving={serving} />
      <ModelsBottomBar
        sessionId="chat-1"
        dropdownRef={{ current: document.createElement('div') }}
        setView={vi.fn()}
        sessionModel={model}
        sessionProvider="swarm"
        onModelChanged={vi.fn()}
        sessionLoaded
        served={serving.served}
      />
    </>
  );
}

function show(model: string, turnInFlight: boolean) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter>
        <Composer model={model} turnInFlight={turnInFlight} />
      </MemoryRouter>
    </IntlProvider>
  );
}

beforeEach(() => {
  mockExtMethod.mockClear();
  devices = [MLX_DEVICE];
});
afterEach(() => resetEngineGlanceForTests(null));

describe('Q-254: the composer during a strategy swap says the loader’s words', () => {
  it('J3 delegate swap (19): "Swapping to <split node>" — never "No model is mounted", never exit 143', async () => {
    residency = J3_SWAP_TO_SPLIT;
    mockStatus.mockResolvedValue(STOPPED_BY_SWAP);
    show(J3_STRATEGY, true);
    const bar = await screen.findByTestId('composer-readiness-loader');
    expect(bar.textContent).toBe('Swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs');
    const strip = screen.getByTestId('composer-readiness');
    expect(strip).toHaveAttribute('data-readiness', 'loader');
    expect(strip.className).toContain('bg-lz-phase-loading');
    expect(strip.textContent).not.toContain('No model is mounted');
    expect(strip.textContent).not.toContain('143');
    // The chip says the same line, amber — not "Failed".
    const phase = screen.getByTestId('model-chip-phase');
    expect(phase.textContent).toBe('Swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs');
    expect(phase.textContent).not.toContain('Failed');
  });

  it('J3 swap back (17-02): "Loading <node> for this chat: <phase>" while the weights go in', async () => {
    residency = J3_SWAP_TO_SINGLE;
    mockStatus.mockResolvedValue(MOUNTING);
    show(J3_STRATEGY, true);
    const bar = await screen.findByTestId('composer-readiness-loader');
    expect(bar.textContent).toBe(
      'Loading Qwen3.8-27B-Atlassian-Q8-mlx · this Mac for this chat: Loading weights'
    );
    expect(screen.getByTestId('composer-readiness').textContent).not.toContain(
      'No model is mounted'
    );
  });

  it('a displaced chat (its node stopped, no turn of its own) reads the swap, not a failure', async () => {
    residency = J3_SWAP_TO_SPLIT;
    mockStatus.mockResolvedValue(STOPPED_BY_SWAP);
    show(`node:${J3_CHAT_NODE.def.id}`, false);
    const bar = await screen.findByTestId('composer-readiness-loader');
    expect(bar.textContent).toBe('Swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs');
  });

  it('a turn queued in the loader says why, in the loader’s own words', async () => {
    residency = J3_WAITING;
    mockStatus.mockResolvedValue(RUNNING);
    show(J3_STRATEGY, true);
    const bar = await screen.findByTestId('composer-readiness-loader');
    expect(bar.textContent).toBe(
      'Qwen3.8-27B-Atlassian-Q8-mlx on Mihai Macbook is answering 1 reply; loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs when it finishes'
    );
    expect(screen.getByTestId('composer-readiness')).toHaveAttribute('data-loader', 'waiting');
  });

  it('negative control: a real failure with no swap still says it — by the node’s name (Q-255)', async () => {
    residency = {
      ...J3_SERVING_SINGLE,
      nodes: J3_SERVING_SINGLE.nodes.map((r) => ({
        ...r,
        residency: { kind: 'notRunning' as const },
      })),
      serving: null,
    };
    mockStatus.mockResolvedValue({ ...STOPPED_BY_SWAP, lastError: REAL_FAILURE });
    show('swarm', false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness').textContent).toContain(
        'No model is mounted — Mihai Macbook engine'
      )
    );
    const strip = screen.getByTestId('composer-readiness');
    expect(strip.textContent).toContain(`The last mount failed: ${REAL_FAILURE}`);
    expect(strip.textContent).not.toContain('mihai-mlx —');
    expect(screen.queryByTestId('composer-readiness-loader')).toBeNull();
  });
});

describe('Q-255: the chip names a node or strategy chat by the Nodes page’s names', () => {
  it('a strategy chat: "<strategy> · <its node that serves>", never strategy:<id>', async () => {
    residency = J3_SERVING_SINGLE;
    mockStatus.mockResolvedValue(RUNNING);
    show(J3_STRATEGY, false);
    await waitFor(() =>
      expect(screen.getByTestId('model-chip-served').textContent).toBe(
        'Quick · Qwen3.8-27B-Atlassian-Q8-mlx · this Mac'
      )
    );
    expect(document.body.textContent).not.toContain('strategy:new-strategy');
  });

  it('36-j3-w2-demand-split: a pool the renderer cannot see into named the chat by id — now by name', async () => {
    // An LM Studio device beside the MLX one: no one engine can be named, so the chip fell back to
    // the session's model id — "strategy:new-strategy" on the J3 screenshots.
    devices = [MLX_DEVICE, { id: 'studio-lm', model_id: 'qwen', weight: 1, enabled: true }];
    residency = J3_SERVING_SINGLE;
    mockStatus.mockResolvedValue(RUNNING);
    show(J3_STRATEGY, false);
    await waitFor(() =>
      expect(screen.getByTestId('model-chip-served').textContent).toBe(
        'Quick · Qwen3.8-27B-Atlassian-Q8-mlx · this Mac'
      )
    );
    expect(document.body.textContent).not.toContain('strategy:new-strategy');
  });

  it('a node chat: the node’s name, never node:<id>', async () => {
    residency = J3_SERVING_SINGLE;
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${J3_CHAT_NODE.def.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('model-chip-served').textContent).toBe(
        'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac'
      )
    );
    expect(document.body.textContent).not.toContain(`node:${J3_CHAT_NODE.def.id}`);
  });
});
