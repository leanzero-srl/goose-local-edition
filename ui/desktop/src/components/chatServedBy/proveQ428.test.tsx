import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { createIntl } from 'react-intl';
import type {
  NodeResidencyDto,
  NodeServedTurnDto,
  NodeServingOtherDto,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
  ResolvedNodeDef,
} from '@aaif/goose-sdk';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import type { MlxClient } from '../../utils/mlxServing';
import type { MountLookup } from '../noNodeNotice/mlxMount';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';
import { parseMlxLiveStatus } from '../leanzero-swarm/mlxLiveStats';
import { MEASURED_PENDING } from '../../utils/mlxMeasuredRuns';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { NODE_CLOUD, NODE_POOL, NODE_SPLIT, NODE_STUDIO } from '../nodes/nodeGlance.fixtures';
import { nodeRefusalOf, nodeWaitOf } from '../../utils/nodeSwap';
import { servedChipWords } from '../settings/models/bottom_bar/servedChip';
import {
  deriveChatServedBy,
  turnHeldBeforeModel,
  type ChatNodesFacts,
  type ChatServedInputs,
} from './chatServedBy';
import { loaderText } from './loaderText';
import { fellBackOf, fellBackText } from './turnLine';

import { ComposerReadinessStrip } from '../noNodeNotice/ComposerReadiness';

vi.mock('../leanzero-swarm/PeerHeldLine', () => ({ PeerHeldLine: () => null }));
vi.mock('../engineGlance/glanceStore', () => ({
  useGlanceNodes: () => ({ kind: 'unread' }),
  refreshGlanceNodes: vi.fn(),
}));

/**
 * The live prove of Q-428 on 3.0.74 (~/goose-builds/quality/PROVE-2026-09-29-q428/): chat Y on
 * "Studio chat, split for heavy work" — Chat: the Studio single, then deepseek (cloud) — while the
 * split "… · both Macs" answered chat X. The split is served through this Mac's engine, so the
 * node that FOLLOWS this Mac's engine names the same way: goosed named it first.
 */
const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });
const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const FOLLOWS: ResolvedNodeDef = {
  ...NODE_POOL,
  def: { ...NODE_POOL.def, id: 'this-macs-engine', name: 'This Mac’s engine' },
};
const X = 'European Printing Press History';
const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const POOL: MountLookup = {
  state: 'ready',
  devices: [{ id: 'mihai-mlx', model_id: ALIAS, weight: 2, enabled: true, engine: 'mlx-sidecar' }],
  settings: SETTINGS,
  intent: null,
};
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 63.9,
  totalMemoryGb: 128,
};
const SPLIT: MlxDistributedStatus = {
  ...FLASH_READY,
  modelId: HF,
  servedModelId: ALIAS,
  contextLimit: 65536,
};
const X_CHAT: MlxClient = {
  key: 'chat:x',
  kind: 'chat',
  work: null,
  sessionId: 'x',
  sessionName: X,
  count: 1,
};

/** The split writing X's answer, as main reads it. */
function busyWithX(): MlxEngineSnapshot {
  const read = parseMlxLiveStatus({
    status: 'generating',
    uptime_s: 90,
    num_running: 1,
    num_waiting: 0,
    requests: [
      {
        request_id: 'x-essay',
        status: 'running',
        phase: 'generation',
        elapsed_s: 71,
        prompt_tokens: 42000,
        completion_tokens: 300,
        max_tokens: 222148,
        tokens_per_second: 10.6,
        ttft_s: 20,
        cached_tokens: 0,
      },
    ],
  });
  if (!read.ok) throw new Error(read.detail);
  return {
    engine: 'distributed',
    mode: 'running',
    modelId: HF,
    modelDetail: null,
    baseUrl: null,
    stats: read.stats,
    statusDetail: null,
    measured: MEASURED_PENDING,
    serving: { clients: [X_CHAT], unattributed: 0, swarmRuns: [], error: null },
    startPhase: null,
    failedError: null,
    contact: null,
  };
}

type Setting = 'useNext' | 'wait' | 'takeOver';
const read = (ifServingOther: Setting): NodesReadResponse_unstable => ({
  config: {
    version: 1,
    defs: [FOLLOWS.def, NODE_SPLIT.def, NODE_STUDIO.def, NODE_CLOUD.def],
    strategies: [
      {
        id: 'studio-chat',
        name: 'Studio chat, split for heavy work',
        roles: {
          chat: {
            chain: [
              { node: NODE_STUDIO.def.id, weight: 1 },
              { node: NODE_CLOUD.def.id, weight: 1 },
            ],
            when: 'failover',
            ifNotLoaded: 'load',
            ifServingOther,
          },
          build: { chain: [{ node: NODE_SPLIT.def.id, weight: 1 }] },
        },
      },
    ],
  },
  nodes: [FOLLOWS, NODE_SPLIT, NODE_STUDIO, NODE_CLOUD],
  stored: true,
  lmStudioHidden: 0,
});
const MODEL = 'strategy:studio-chat';

/** goosed's facts as 3.0.74 sent them: the follows node's name first, now with the node ids. */
const OTHER: NodeServingOtherDto = {
  mac: 'Work’s Mac Studio',
  serving: FOLLOWS.def.name,
  servingNodes: [FOLLOWS.def.id, NODE_SPLIT.def.id],
  chats: [X],
  replies: 1,
};

function residency(studio: NodeResidencyDto['residency']): NodesResidencyResponse_unstable {
  return {
    nodes: [
      { node: FOLLOWS.def.id, residency: { kind: 'serving' } },
      { node: NODE_SPLIT.def.id, residency: { kind: 'serving' } },
      { node: NODE_STUDIO.def.id, residency: studio, load: { medianMs: 8000, count: 3 } },
      { node: NODE_CLOUD.def.id, residency: { kind: 'alwaysReady' } },
    ],
    serving: null,
    loaderInstalled: true,
  };
}
const NOT_RUNNING = residency({ kind: 'notRunning', otherWay: 'the split' });

/** Y's last turn went to deepseek: the Studio was left to the split serving X (Q-428 useNext). */
const FELL_TO_CLOUD: NodeServedTurnDto = {
  node: NODE_CLOUD.def.id,
  role: 'chat',
  rank: 2,
  reason: 'left to the split',
  tried: [{ node: NODE_STUDIO.def.id, reason: 'left to the split' }],
  atMs: 1,
  servingOther: OTHER,
};

const inputs = (over: Partial<ChatServedInputs>): ChatServedInputs => ({
  provider: 'swarm',
  lookup: POOL,
  single: STOPPED,
  distributed: SPLIT,
  remote: null,
  remoteReadError: null,
  main: null,
  sessionId: 'y',
  turnInFlight: false,
  thisMac: 'This Mac',
  engineLabel: 'LeanZero MLX',
  model: MODEL,
  ...over,
});

function strip(served: ReturnType<typeof deriveChatServedBy>, turnInFlight = false) {
  render(
    <IntlTestWrapper>
      <MemoryRouter>
        <ComposerReadinessStrip
          serving={{ served, single: STOPPED, armed: true, turnInFlight }}
          sessionId="y"
        />
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

afterEach(cleanup);

describe('Q-458: the busy bar follows the role’s ifServingOther, and never covers the turn line', () => {
  const busyFacts = (setting: Setting): ChatNodesFacts => ({
    read: read(setting),
    residency: NOT_RUNNING,
    servedNode: NODE_CLOUD.def.id,
    servedRecord: FELL_TO_CLOUD,
  });

  it('"Use the next node": the message goes to deepseek at once — nothing is stopped, nothing waits', () => {
    const served = deriveChatServedBy(inputs({ main: busyWithX(), nodes: busyFacts('useNext') }));
    expect(served.busyIn?.next).toEqual({
      kind: 'goesToNext',
      node: NODE_CLOUD.def.name,
      passed: NODE_STUDIO.def.name,
    });
    strip(served);
    const bars = screen.getAllByTestId('composer-readiness');
    const busy = bars.find((b) => b.dataset.readiness === 'busy-in')!;
    expect(within(busy).getByTestId('composer-readiness-detail')).toHaveTextContent(
      'A message sent now goes to Claude Sonnet · OpenRouter — 27B · Work’s Mac Studio is not loaded, so nothing that answer runs on is stopped.'
    );
    expect(busy.textContent).not.toContain('waits for that answer');
    // The turn line stays: where the last turn went, and the one way back.
    expect(bars.map((b) => b.dataset.readiness)).toEqual(['busy-in', 'fell-back']);
    expect(screen.getByTestId('composer-readiness-fell-back')).toHaveTextContent(
      'Chat is on Claude Sonnet · OpenRouter (2nd): Work’s Mac Studio is serving 27B Atlassian · both Macs for chat "European Printing Press History"'
    );
    expect(screen.getByTestId('composer-readiness-retry-primary')).toBeInTheDocument();
  });

  it('"Wait": the message waits until that chat is closed or moved — not only until it answers', () => {
    const served = deriveChatServedBy(inputs({ main: busyWithX(), nodes: busyFacts('wait') }));
    expect(served.busyIn?.next).toEqual({ kind: 'waitsForChat', node: NODE_STUDIO.def.name });
  });

  it('"Take it over": the message waits for that answer, then loads the Studio (unchanged)', () => {
    const served = deriveChatServedBy(inputs({ main: busyWithX(), nodes: busyFacts('takeOver') }));
    expect(served.busyIn?.next).toEqual({ kind: 'loadsAfter', node: NODE_STUDIO.def.name });
  });
});

describe('Q-459: the serving node is named from the node list, the split before the follows node', () => {
  const nodes = read('wait');

  it('the wait line and its take-over button name "… · both Macs", never "This Mac’s engine"', () => {
    const wait = nodeWaitOf(
      nodes,
      residency({ kind: 'waiting', reason: 'the loader’s words', servingOther: OTHER }),
      [NODE_STUDIO.def.id]
    )!;
    expect(wait.servingOther?.serving).toBe(NODE_SPLIT.def.name);
    const text = loaderText(intl, { kind: 'waiting', wait });
    expect(text).toContain(`Work’s Mac Studio serves ${NODE_SPLIT.def.name} for chat "${X}"`);
    expect(text).not.toContain(FOLLOWS.def.name);
  });

  it('the refusal and the turn line read the same one name', () => {
    const refusal = nodeRefusalOf(
      nodes,
      residency({
        kind: 'refusedLastTime',
        reason: 'goosed words',
        facts: { kind: 'servingOther', ...OTHER },
      }),
      NODE_STUDIO.def.id
    )!;
    expect(loaderText(intl, { kind: 'refused', refusal })).toBe(
      `${NODE_STUDIO.def.name} was not loaded: Work’s Mac Studio is serving ${NODE_SPLIT.def.name} for chat "${X}".`
    );
    const fell = fellBackOf(FELL_TO_CLOUD, nodes)!;
    expect(fellBackText(intl, fell)).toContain(`serving ${NODE_SPLIT.def.name}`);
  });

  it('an older goosed that sends no node ids keeps its own words', () => {
    const { servingNodes: _ids, ...older } = OTHER;
    const fell = fellBackOf({ ...FELL_TO_CLOUD, servingOther: older }, nodes)!;
    expect(fell.servingOther?.serving).toBe(FOLLOWS.def.name);
  });
});

describe('Q-460 + Q-461: while the turn waits for the Studio, the chip names the Studio and one status speaks', () => {
  const waitingFacts: ChatNodesFacts = {
    read: read('wait'),
    residency: residency({ kind: 'waiting', reason: 'the loader’s words', servingOther: OTHER }),
    servedNode: NODE_CLOUD.def.id,
    servedRecord: FELL_TO_CLOUD,
  };

  it('Q-460: the chip names the node the turn waits for, not deepseek it last ran on', () => {
    const served = deriveChatServedBy(inputs({ turnInFlight: true, nodes: waitingFacts }));
    expect(served.loader?.kind).toBe('waiting');
    const { chipLabel } = servedChipWords(intl, served, false);
    expect(chipLabel).toBe(`Studio chat, split for heavy work · ${NODE_STUDIO.def.name}`);
    expect(chipLabel).not.toContain(NODE_CLOUD.def.name);
    // Between turns the chip names where the last turn went, as before.
    const idle = deriveChatServedBy(
      inputs({ nodes: { ...waitingFacts, residency: NOT_RUNNING } })
    );
    expect(servedChipWords(intl, idle, false).chipLabel).toBe(
      `Studio chat, split for heavy work · ${NODE_CLOUD.def.name}`
    );
  });

  it('Q-461: the loader’s wait holds the turn — the working row’s "first words" line stays silent', () => {
    const served = deriveChatServedBy(inputs({ turnInFlight: true, nodes: waitingFacts }));
    expect(turnHeldBeforeModel(served)).toBe(true);
    const idle = deriveChatServedBy(
      inputs({ nodes: { ...waitingFacts, residency: NOT_RUNNING } })
    );
    expect(turnHeldBeforeModel(idle)).toBe(false);
  });
});

describe('Q-462: in the gap of a take-over the bar reads the switch, never "No model is mounted"', () => {
  // 44-Y-takeover-02-6s.png: the split stopped, no residency mark read yet for the Studio.
  const gapFacts: ChatNodesFacts = {
    read: read('wait'),
    residency: {
      ...NOT_RUNNING,
      nodes: NOT_RUNNING.nodes.map((r) =>
        r.node === FOLLOWS.def.id || r.node === NODE_SPLIT.def.id
          ? { ...r, residency: { kind: 'notRunning', otherWay: null } }
          : r
      ),
    },
    servedNode: NODE_CLOUD.def.id,
    servedRecord: FELL_TO_CLOUD,
  };

  it('says "Swapping to" the Studio, with its measured load, and the chip names the Studio', () => {
    const served = deriveChatServedBy(
      inputs({ turnInFlight: true, distributed: null, nodes: gapFacts })
    );
    expect(served.readiness.kind).toBe('loader');
    strip(served, true);
    expect(screen.getByTestId('composer-readiness')).toHaveAttribute('data-readiness', 'loader');
    expect(screen.getByTestId('composer-readiness-loader')).toHaveTextContent(
      `Swapping to ${NODE_STUDIO.def.name}`
    );
    expect(screen.getByTestId('composer-readiness-detail')).toHaveTextContent(
      'Loads in about 8s · median of 3 loads'
    );
    expect(document.body.textContent).not.toContain('No model is mounted');
    expect(servedChipWords(intl, served, false).chipLabel).toBe(
      `Studio chat, split for heavy work · ${NODE_STUDIO.def.name}`
    );
  });

  it('no turn in flight: nothing is switching, and the bar says what is true', () => {
    const served = deriveChatServedBy(inputs({ distributed: null, nodes: gapFacts }));
    expect(served.loader).toBeNull();
  });
});
