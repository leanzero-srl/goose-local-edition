import type React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import type { NodeServedTurnDto, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { ComposerReadinessStrip } from './ComposerReadiness';
import { useChatServedBy } from '../chatServedBy/useChatServedBy';
import { resetEngineGlanceForTests } from '../engineGlance/glanceStore';
import ModelsBottomBar from '../settings/models/bottom_bar/ModelsBottomBar';
import {
  J3_BUILD_NODE,
  J3_CHAT_NODE,
  J3_MODEL,
  J3_POOL_NODE,
  J3_READ,
  J3_SERVING_SINGLE,
  J3_SERVING_SPLIT,
  J3_STRATEGY,
} from '../../utils/nodeSwap.fixtures';

/**
 * Q-271 / Q-272 / Q-273 / Q-274 on the composer and the chip — through the ONE derivation
 * (useChatServedBy) as ChatInput hands it to both, over the facts goosed's loader now carries
 * (nodes/residency: a starting split's phase, a wait's replies, a refusal's facts, the measured
 * load, the displaced notice; nodes/servedLast: the fallback). Each line is §8.7's, word for word.
 */

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: vi.fn(),
  mlxEngineModelsList: async () => ({ models: [], diskAvailableBytes: 0, diskTotalBytes: 0 }),
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: vi.fn(),
  mlxEngineSettingsRead: async () => settings,
}));
let residency: NodesResidencyResponse_unstable = J3_SERVING_SINGLE;
let servedRecord: NodeServedTurnDto | null = null;
const mockExtMethod = vi.fn(async (method: string, _params?: unknown) => {
  if (method.endsWith('/nodes/read')) return J3_READ;
  if (method.endsWith('/nodes/residency')) return residency;
  if (method.endsWith('/nodes/servedLast')) return { record: servedRecord };
  if (method.endsWith('/nodes/ensureServing'))
    return { answer: { kind: 'wait', reason: 'loading' } };
  if (method.endsWith('/nodes/write')) return { written: true, refusals: [] };
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
const changeModel = vi.fn(async () => true);
vi.mock('../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({ currentModel: null, currentProvider: null, changeModel }),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ mlxEngine: true }),
}));
vi.mock('../settings/models/modelInterface', () => ({
  getProviderMetadata: async () => ({ display_name: 'Goose Swarm', default_model: 'swarm' }),
}));
// The chip's menu, always open: its rows are plain elements.
vi.mock('../ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuLabel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuSeparator: () => <hr />,
  DropdownMenuItem: ({
    children,
    onClick,
    ...rest
  }: {
    children: React.ReactNode;
    onClick?: () => void;
    [key: string]: unknown;
  }) => (
    <div role="menuitem" {...rest} onClick={onClick}>
      {children}
    </div>
  ),
}));

const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const FLASH_ALIAS = 'mihai-flash-qwen3.8-flash-next-4bit-mlx';
const SAVED_27B: MlxEngineSettings = {
  modelId: J3_MODEL,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
/** After a swap to Flash: the saved single model is Flash; the pool device still wants the 27B. */
const SAVED_FLASH: MlxEngineSettings = {
  ...SAVED_27B,
  modelId: 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
  servedModelName: FLASH_ALIAS,
};
let settings: MlxEngineSettings = SAVED_27B;
const MLX_DEVICE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
};
const RUNNING: MlxEngineStatus = {
  ...STOPPED,
  state: 'running',
  modelId: J3_MODEL,
  servedModelId: ALIAS,
};

const CHAT = J3_CHAT_NODE.def;
const SPLIT = J3_BUILD_NODE.def;

function Composer({ model, turnInFlight }: { model: string; turnInFlight: boolean }) {
  const serving = useChatServedBy('swarm', 'chat-1', turnInFlight, model);
  return (
    <>
      <ComposerReadinessStrip serving={serving} sessionId="chat-1" onModelChanged={vi.fn()} />
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

/** The J3 nodes with `rows` replacing their residency (others keep J3_SERVING_SINGLE's). */
function withRows(
  base: NodesResidencyResponse_unstable,
  rows: Partial<Record<string, NodesResidencyResponse_unstable['nodes'][number]>>,
  extra: Partial<NodesResidencyResponse_unstable> = {}
): NodesResidencyResponse_unstable {
  return {
    ...base,
    nodes: base.nodes.map((r) => rows[r.node] ?? r),
    ...extra,
  };
}

const LOAD_98S = { medianMs: 98_000, count: 3 };

beforeEach(() => {
  mockExtMethod.mockClear();
  changeModel.mockClear();
  devices = [MLX_DEVICE];
  settings = SAVED_27B;
  servedRecord = null;
});
afterEach(() => resetEngineGlanceForTests(null));

describe('Q-271: a split that is starting is loading — the loader’s line, never its own Starting bar', () => {
  it('a node chat on the split, its turn waiting on the start: §8.7 turnLoading + turnFirstLoad', async () => {
    // goosed now: the split's owner record exists from the start, the supervisor says Starting and
    // no rank has a phase → the split node reads loading{starting} (residency.rs, Q-271).
    residency = withRows(J3_SERVING_SINGLE, {
      [CHAT.id]: { node: CHAT.id, residency: { kind: 'notRunning' } },
      [SPLIT.id]: { node: SPLIT.id, residency: { kind: 'loading', phase: 'starting' } },
    });
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${SPLIT.id}`, true);
    const line = await screen.findByTestId('composer-readiness-loader');
    expect(line.textContent).toBe(
      'Loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs for this chat: Starting the engine'
    );
    expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
      'First load of Qwen3.8-27B-Atlassian-Q8-mlx · both Macs, not measured yet'
    );
  });

  it('a measured split says how long it takes, from the load store’s median', async () => {
    residency = withRows(J3_SERVING_SINGLE, {
      [CHAT.id]: { node: CHAT.id, residency: { kind: 'notRunning' } },
      [SPLIT.id]: {
        node: SPLIT.id,
        residency: { kind: 'loading', phase: 'warming' },
        load: LOAD_98S,
      },
    });
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${SPLIT.id}`, true);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
        'Loads in about 1m 38s · median of 3 loads'
      )
    );
    expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
      'Loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs for this chat: Warming up'
    );
  });
});

describe('Q-272: the five §8.7 lines, from the loader’s facts', () => {
  it('nodes.turnWaiting: the way waited on by its node’s name, the replies ahead, the duration', async () => {
    residency = withRows(J3_SERVING_SINGLE, {
      [SPLIT.id]: {
        node: SPLIT.id,
        residency: {
          kind: 'waiting',
          reason: "this Mac's engine is answering 2 replies; loading … when they finish",
          replies: { way: "this Mac's engine", wayNodes: [CHAT.id], count: 2 },
        },
        load: LOAD_98S,
      },
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, true);
    const words =
      'Waiting for Qwen3.8-27B-Atlassian-Q8-mlx · this Mac to finish 2 replies, then loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs (about 1m 38s)';
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(words)
    );
    expect(screen.getByTestId('composer-readiness')).toHaveAttribute('data-loader', 'waiting');
    // The chip says the same line.
    expect(screen.getByTestId('model-chip-phase').textContent).toBe(words);
  });

  it('nodes.turnWaiting with no measured load says so — never an estimate', async () => {
    residency = withRows(J3_SERVING_SINGLE, {
      [SPLIT.id]: {
        node: SPLIT.id,
        residency: {
          kind: 'waiting',
          reason: 'x',
          replies: { way: "this Mac's engine", wayNodes: [CHAT.id], count: 1 },
        },
      },
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, true);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        'Waiting for Qwen3.8-27B-Atlassian-Q8-mlx · this Mac to finish 1 reply, then loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs (first load not measured yet)'
      )
    );
  });

  const refused = (facts: NonNullable<unknown>) =>
    withRows(J3_SERVING_SINGLE, {
      [SPLIT.id]: {
        node: SPLIT.id,
        residency: {
          kind: 'refusedLastTime',
          reason: 'the loader’s own words',
          facts: facts as never,
        },
      },
    });

  it('nodes.refusedKept', async () => {
    residency = refused({
      kind: 'keptLoaded',
      keptNode: CHAT.id,
      kept: CHAT.name,
      mac: 'this Mac',
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        "Can't load Qwen3.8-27B-Atlassian-Q8-mlx · both Macs: Qwen3.8-27B-Atlassian-Q8-mlx · this Mac is kept loaded on this Mac."
      )
    );
    const bar = screen.getByTestId('composer-readiness');
    expect(bar).toHaveAttribute('data-loader', 'refused');
    expect(bar.className).toContain('bg-lz-phase-failed');
  });

  it('nodes.refusedBuild', async () => {
    residency = refused({ kind: 'heldByBuild', way: "this Mac's engine" });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        "Can't load Qwen3.8-27B-Atlassian-Q8-mlx · both Macs: a swarm build is using this Mac's engine. It frees when the build ends."
      )
    );
  });

  it('nodes.refusedFit, with Make room', async () => {
    residency = refused({
      kind: 'fit',
      mac: 'your Macs',
      verdict: 'short 1.6 GB on Work’s Mac Studio',
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        "Can't load Qwen3.8-27B-Atlassian-Q8-mlx · both Macs on your Macs: short 1.6 GB on Work’s Mac Studio"
      )
    );
    expect(screen.getByTestId('composer-readiness-make-room').textContent).toBe('Make room');
  });

  it('nodes.loadFailed', async () => {
    residency = refused({ kind: 'loadFailed', words: 'rank 1 exited: exit status 1' });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs failed to load: rank 1 exited: exit status 1'
      )
    );
  });

  it('a refusal with no facts of §8.7’s is said in the loader’s own words', async () => {
    residency = withRows(J3_SERVING_SINGLE, {
      [SPLIT.id]: {
        node: SPLIT.id,
        residency: { kind: 'refusedLastTime', reason: 'goose cannot run this as a split yet' },
      },
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-loader').textContent).toBe(
        'Goose cannot run this as a split yet'
      )
    );
  });

  const DISPLACED = withRows(
    J3_SERVING_SPLIT,
    {
      [CHAT.id]: {
        node: CHAT.id,
        residency: { kind: 'notRunning' },
        load: { medianMs: 16_800, count: 2 },
      },
    },
    {
      displaced: [
        {
          node: CHAT.id,
          forNode: SPLIT.id,
          bySession: 'kickoff',
          byChat: 'Kickoff notes',
          atMs: 1,
        },
      ],
    }
  );

  /** The displaced chat's last turn ran on the node that was stopped (the router's record). */
  const RAN_ON_CHAT_NODE: NodeServedTurnDto = {
    node: CHAT.id,
    role: 'chat',
    rank: 1,
    tried: [],
    atMs: 0,
  };

  it('nodes.displacedNotice: the displaced chat is told, with its two actions', async () => {
    residency = DISPLACED;
    servedRecord = RAN_ON_CHAT_NODE;
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${CHAT.id}`, false);
    const notice = await screen.findByTestId('composer-readiness-displaced');
    expect(notice.textContent).toBe(
      'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac was stopped for Qwen3.8-27B-Atlassian-Q8-mlx · both Macs in chat "Kickoff notes". Your next message loads it back (about 17s).'
    );
    expect(screen.getByTestId('composer-readiness-keep-loaded').textContent).toBe(
      'Keep Qwen3.8-27B-Atlassian-Q8-mlx · this Mac loaded'
    );
    const use = screen.getByTestId('composer-readiness-use-instead');
    expect(use.textContent).toBe('Use Qwen3.8-27B-Atlassian-Q8-mlx · both Macs instead');
    fireEvent.click(use);
    await waitFor(() =>
      expect(changeModel).toHaveBeenCalledWith(
        'chat-1',
        expect.objectContaining({ name: `node:${SPLIT.id}`, provider: 'swarm' })
      )
    );
  });

  it('Keep {node} loaded: the node’s Keep loaded is written, then it loads back', async () => {
    residency = DISPLACED;
    servedRecord = RAN_ON_CHAT_NODE;
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${CHAT.id}`, false);
    fireEvent.click(await screen.findByTestId('composer-readiness-keep-loaded'));
    await waitFor(() =>
      expect(mockExtMethod).toHaveBeenCalledWith('_goose/unstable/nodes/ensureServing', {
        node: CHAT.id,
      })
    );
    const write = mockExtMethod.mock.calls.find(([m]) => m === '_goose/unstable/nodes/write');
    const written = (write?.[1] as { config: typeof J3_READ.config }).config.defs?.find(
      (d) => d.id === CHAT.id
    );
    expect(written?.keepLoaded).toBe(true);
  });

  it('Q-435: a chat that never ran on the stopped node is not told it was stopped', async () => {
    // Shot 40: a brand-new chat set to the node another chat displaced read "… was stopped for
    // … Your next message loads it back" — nothing was stopped for it.
    residency = DISPLACED;
    servedRecord = null;
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${CHAT.id}`, false);
    await waitFor(() => expect(screen.getByTestId('model-chip-served')).toBeTruthy());
    expect(screen.queryByTestId('composer-readiness-displaced')).toBeNull();
    cleanup();
    // Its last turn ran on another node: the stopped one was not serving it either.
    servedRecord = { ...RAN_ON_CHAT_NODE, node: SPLIT.id };
    show(`node:${CHAT.id}`, false);
    await waitFor(() => expect(screen.getByTestId('model-chip-served')).toBeTruthy());
    expect(screen.queryByTestId('composer-readiness-displaced')).toBeNull();
  });

  it('the chat that asked for the other node is not "displaced" — the swap was its own', async () => {
    residency = {
      ...DISPLACED,
      displaced: DISPLACED.displaced?.map((d) => ({ ...d, bySession: 'chat-1' })),
    };
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${CHAT.id}`, false);
    await waitFor(() => expect(screen.getByTestId('model-chip-served')).toBeTruthy());
    expect(screen.queryByTestId('composer-readiness-displaced')).toBeNull();
  });

  it('nodes.displacedFailed: the other node failed to load — said, and nothing to move to', async () => {
    residency = {
      ...DISPLACED,
      displaced: DISPLACED.displaced?.map((d) => ({ ...d, failed: 'rank 1 exited' })),
    };
    servedRecord = RAN_ON_CHAT_NODE;
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${CHAT.id}`, false);
    const notice = await screen.findByTestId('composer-readiness-displaced');
    expect(notice.textContent).toBe(
      'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac was stopped for Qwen3.8-27B-Atlassian-Q8-mlx · both Macs, which failed to load: rank 1 exited. Your next message loads Qwen3.8-27B-Atlassian-Q8-mlx · this Mac back.'
    );
    expect(screen.queryByTestId('composer-readiness-use-instead')).toBeNull();
    expect(screen.queryByTestId('composer-readiness-keep-loaded')).toBeNull();
  });

  it('nodes.fellBack: the role, the node, its rank, the 1st and why — with Retry', async () => {
    residency = J3_SERVING_SPLIT;
    servedRecord = {
      node: SPLIT.id,
      role: 'chat',
      rank: 2,
      reason: 'failed to load: short 1.6 GB',
      tried: [{ node: CHAT.id, reason: 'failed to load: short 1.6 GB' }],
      atMs: 1,
    };
    mockStatus.mockResolvedValue(STOPPED);
    show(J3_STRATEGY, false);
    const line = await screen.findByTestId('composer-readiness-fell-back');
    expect(line.textContent).toBe(
      "Chat is on Qwen3.8-27B-Atlassian-Q8-mlx · both Macs (2nd): Qwen3.8-27B-Atlassian-Q8-mlx · this Mac can't run: failed to load: short 1.6 GB"
    );
    const retry = screen.getByTestId('composer-readiness-retry-primary');
    expect(retry.textContent).toBe('Retry Qwen3.8-27B-Atlassian-Q8-mlx · this Mac');
    fireEvent.click(retry);
    await waitFor(() =>
      expect(mockExtMethod).toHaveBeenCalledWith('_goose/unstable/nodes/ensureServing', {
        node: CHAT.id,
      })
    );
    // The chip's menu names it too.
    expect(screen.getByTestId('model-menu-fell-back').textContent).toBe(line.textContent);
  });
});

describe('Q-273: a pool device’s mismatch is said only where it decides this chat’s turn', () => {
  const NOTHING_SERVES = {
    ...J3_SERVING_SINGLE,
    nodes: J3_SERVING_SINGLE.nodes.map((r) => ({
      ...r,
      residency: { kind: 'notRunning' as const },
    })),
    serving: null,
  };

  it('an Auto chat over two MLX devices: no "wants" line for a device its turn need not go to', async () => {
    settings = SAVED_FLASH;
    devices = [MLX_DEVICE, { ...MLX_DEVICE, id: 'studio-mlx', model_id: 'studio-alias' }];
    residency = NOTHING_SERVES;
    mockStatus.mockResolvedValue(STOPPED);
    show('swarm', false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness').textContent).toContain('No model is mounted')
    );
    expect(screen.getByTestId('composer-readiness').textContent).not.toContain('wants');
  });

  it('an Auto chat whose pool is that one device: said, in the node’s name', async () => {
    settings = SAVED_FLASH;
    residency = NOTHING_SERVES;
    mockStatus.mockResolvedValue(STOPPED);
    show('swarm', false);
    await waitFor(() =>
      expect(screen.getByTestId('composer-readiness-detail').textContent).toBe(
        `The saved MLX model serves ${FLASH_ALIAS}; ${J3_POOL_NODE.def.name} wants ${ALIAS}.`
      )
    );
  });

  it('a node chat on another node: the pool device’s mismatch is not its turn’s', async () => {
    settings = SAVED_FLASH;
    residency = NOTHING_SERVES;
    mockStatus.mockResolvedValue(STOPPED);
    show(`node:${SPLIT.id}`, false);
    await waitFor(() => expect(screen.getByTestId('model-chip-served')).toBeTruthy());
    expect(document.body.textContent).not.toContain('wants');
  });
});

describe('Q-274: the chip’s menu — §8.5’s node list, the strategy, use this node for this chat', () => {
  it('lists the strategies and nodes with their states, and picking one sets THIS chat’s model', async () => {
    residency = withRows(J3_SERVING_SINGLE, {
      [SPLIT.id]: { ...J3_SERVING_SINGLE.nodes[2], load: { medianMs: 48_000, count: 3 } },
    });
    mockStatus.mockResolvedValue(RUNNING);
    show(J3_STRATEGY, false);
    const menu = await screen.findByTestId('nodes-chip-menu');
    expect(menu.textContent).toContain('Run this chat on');
    const quick = screen.getByTestId('nodes-chip-strategy-new-strategy');
    expect(quick).toHaveAttribute('data-current', 'yes');
    expect(quick.textContent).toContain('Chat → Qwen3.8-27B-Atlassian-Q8-mlx · this Mac');
    expect(quick.textContent).toContain('Serving');
    const split = screen.getByTestId(`nodes-chip-node-${SPLIT.id}`);
    expect(split.textContent).toContain('starts in about 48s');
    expect(split.textContent).toContain('Not loaded');
    expect(screen.getByTestId(`nodes-chip-node-${CHAT.id}`).textContent).toContain('Serving');
    expect(screen.getByTestId('nodes-chip-auto').textContent).toBe('Any node (Auto)');
    fireEvent.click(split);
    await waitFor(() =>
      expect(changeModel).toHaveBeenCalledWith(
        'chat-1',
        expect.objectContaining({ name: `node:${SPLIT.id}`, provider: 'swarm' })
      )
    );
    fireEvent.click(screen.getByTestId('nodes-chip-auto'));
    await waitFor(() =>
      expect(changeModel).toHaveBeenCalledWith(
        'chat-1',
        expect.objectContaining({ name: 'swarm', provider: 'swarm' })
      )
    );
    expect(screen.getByTestId('nodes-chip-manage').textContent).toBe('Manage nodes…');
    expect(screen.getByTestId('nodes-chip-other').textContent).toContain(
      'Other models and providers…'
    );
  });
});
