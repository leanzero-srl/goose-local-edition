import { describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxEngineSnapshot } from '../../utils/mlxEngineMonitor';
import type { MlxClient } from '../../utils/mlxServing';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import type { MountLookup } from '../noNodeNotice/mlxMount';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';
import { parseMlxLiveStatus } from '../leanzero-swarm/mlxLiveStats';
import { MEASURED_PENDING } from '../../utils/mlxMeasuredRuns';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ComposerReadinessStrip } from '../noNodeNotice/ComposerReadiness';
import type { NodesReadResponse_unstable, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import {
  deriveChatServedBy,
  servedReady,
  type ChatNodesFacts,
  type ChatServedInputs,
} from './chatServedBy';
import { NODE_CLOUD, NODE_SPLIT, NODE_STUDIO } from '../nodes/nodeGlance.fixtures';
import {
  publishListedNames,
  resetListedNamesForTests,
} from '../sessionActivity/sessionActivityStore';

vi.mock('../leanzero-swarm/PeerHeldLine', () => ({ PeerHeldLine: () => null }));

/**
 * Q-152, round live-1 (3.0.52): an older session's composer said "Serving other work" while the
 * 27B split wrote session 20260926_19's answer — 2,355 s since it arrived — and its Send stayed
 * enabled with no word that a message would share the engine with that answer.
 */
const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const MLX_NODE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const POOL: MountLookup = { state: 'ready', devices: [MLX_NODE], settings: SETTINGS, intent: null };
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 63.9,
  totalMemoryGb: 128,
};
const SPLIT: MlxDistributedStatus = {
  ...FLASH_READY,
  nodes: FLASH_READY.nodes.map((n, i) => ({
    ...n,
    name: i === 0 ? 'Mihai Macbook' : 'Work’s Mac Studio',
  })),
  modelId: HF,
  servedModelId: ALIAS,
  contextLimit: 65536,
};

const request = (over: Record<string, unknown> = {}) => ({
  request_id: 'chatcmpl-live1',
  status: 'running',
  phase: 'generation',
  elapsed_s: 2355,
  prompt_tokens: 39996,
  completion_tokens: 24228,
  max_tokens: 222148,
  tokens_per_second: 11.0,
  ttft_s: 133,
  cached_tokens: 0,
  ...over,
});
const body = (requests: unknown[]) => ({
  status: 'generating',
  uptime_s: 2400,
  num_running: requests.length,
  num_waiting: 0,
  requests,
});

const LIVE_CHAT: MlxClient = {
  key: 'chat:20260926_19',
  kind: 'chat',
  work: null,
  sessionId: '20260926_19',
  sessionName: 'Jira Migration Kickoff Notes',
  count: 1,
};
const TITLE_CALL: MlxClient = {
  key: 'session:row-9',
  kind: 'session',
  work: null,
  sessionId: null,
  sessionName: null,
  sessionType: null,
  count: 1,
};

function main(requests: unknown[], clients: MlxClient[], unattributed = 0): MlxEngineSnapshot {
  const read = parseMlxLiveStatus(body(requests));
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
    serving: { clients, unattributed, swarmRuns: [], error: null },
    startPhase: null,
    failedError: null,
    contact: null,
  };
}

const inputs = (over: Partial<ChatServedInputs>): ChatServedInputs => ({
  provider: 'swarm',
  lookup: POOL,
  single: STOPPED,
  distributed: SPLIT,
  remote: null,
  remoteReadError: null,
  main: null,
  sessionId: '20260926_8',
  turnInFlight: false,
  thisMac: 'This Mac',
  engineLabel: 'LeanZero MLX',
  ...over,
});

describe('Q-152: an idle chat names the chat the engine is busy in', () => {
  it('the round’s moment: one other chat, its name, the engine’s own age of its request', () => {
    const served = deriveChatServedBy(inputs({ main: main([request()], [LIVE_CHAT]) }));
    expect(served.work).toBe('others');
    expect(servedReady(served)).toBe(true);
    expect(served.busyIn).toEqual({
      sessionId: '20260926_19',
      sessionName: 'Jira Migration Kickoff Notes',
      work: null,
      elapsedS: 2355,
      waits: false,
    });
  });

  it('goose’s own small call beside it neither hides the chat nor lends it its age', () => {
    const title = request({ request_id: 'chatcmpl-title', elapsed_s: 1.2, prompt_tokens: 140 });
    const served = deriveChatServedBy(
      inputs({ main: main([title, request()], [LIVE_CHAT, TITLE_CALL]) })
    );
    expect(served.busyIn).toMatchObject({ sessionId: '20260926_19', elapsedS: 2355 });
  });

  it('a request the engine holds WAITING means a send waits too', () => {
    const waiting = request({ request_id: 'q', status: 'waiting', phase: 'queued', elapsed_s: 9 });
    const served = deriveChatServedBy(inputs({ main: main([request(), waiting], [LIVE_CHAT]) }));
    expect(served.busyIn?.waits).toBe(true);
  });

  it('names no one it cannot prove: two chats, an external client, an unattributed request, or this chat’s own turn', () => {
    const second: MlxClient = { ...LIVE_CHAT, key: 'chat:x', sessionId: 'x', sessionName: 'X' };
    const ext: MlxClient = { key: 'e', kind: 'external', model: ALIAS, count: 1 };
    const derive = (over: Partial<ChatServedInputs>) => deriveChatServedBy(inputs(over)).busyIn;
    expect(derive({ main: main([request()], [LIVE_CHAT, second]) })).toBeNull();
    expect(derive({ main: main([request()], [LIVE_CHAT, ext]) })).toBeNull();
    expect(derive({ main: main([request()], [LIVE_CHAT], 1) })).toBeNull();
    expect(
      derive({ sessionId: '20260926_19', turnInFlight: true, main: main([request()], [LIVE_CHAT]) })
    ).toBeNull();
  });
});

function Where() {
  return <span data-testid="where">{useLocation().search}</span>;
}

describe('Q-152: the composer says where the engine is busy, what a send does, and links there', () => {
  const strip = (m: MlxEngineSnapshot) => {
    const served = deriveChatServedBy(inputs({ main: m }));
    render(
      <IntlTestWrapper>
        <MemoryRouter initialEntries={['/pair?resumeSessionId=20260926_8']}>
          <Routes>
            <Route
              path="/pair"
              element={
                <>
                  <ComposerReadinessStrip
                    serving={{ served, single: STOPPED, armed: true, turnInFlight: false }}
                  />
                  <Where />
                </>
              }
            />
          </Routes>
        </MemoryRouter>
      </IntlTestWrapper>
    );
  };

  it('headline, the send’s fate, and the way to that chat', () => {
    strip(main([request()], [LIVE_CHAT]));
    expect(screen.getByTestId('composer-readiness')).toHaveAttribute('data-readiness', 'busy-in');
    expect(screen.getByTestId('composer-readiness-busy-in')).toHaveTextContent(
      'Busy in ‘Jira Migration Kickoff Notes’ · 39m 15s'
    );
    expect(screen.getByTestId('composer-readiness-detail')).toHaveTextContent(
      'A message sent now shares the engine with that answer — it runs slower, or waits if there is no room.'
    );
    fireEvent.click(screen.getByTestId('composer-readiness-open-busy-chat'));
    expect(screen.getByTestId('where')).toHaveTextContent('?resumeSessionId=20260926_19');
  });

  it('with a request held waiting, it says a send waits', () => {
    const waiting = request({ request_id: 'q', status: 'waiting', phase: 'queued', elapsed_s: 9 });
    strip(main([request(), waiting], [LIVE_CHAT]));
    expect(screen.getByTestId('composer-readiness-detail')).toHaveTextContent(
      'A message sent now waits until the engine has room for it.'
    );
  });

  // Q-185, E2E #3i: the other chat's reply was done — goose was checking it. The composer names
  // the check and the listed name, and says the truth about a send: the check steps aside.
  it('another chat’s fact check: named as that, and a send goes first', () => {
    publishListedNames([
      {
        id: '20260926_19',
        base: 'Jira Migration Kickoff Notes',
        label: 'Jira Migration Kickoff Notes · 5',
      },
    ]);
    const check: MlxClient = { ...LIVE_CHAT, key: 'chat:20260926_19:factCheck', work: 'factCheck' };
    strip(main([request({ elapsed_s: 3, prompt_tokens: 1094, phase: 'prefill' })], [check]));
    expect(screen.getByTestId('composer-readiness-busy-in')).toHaveTextContent(
      'Busy for ‘Jira Migration Kickoff Notes · 5’: Checking the reply · 3s'
    );
    expect(screen.getByTestId('composer-readiness-detail')).toHaveTextContent(
      'A message sent now goes first: goose sets this check aside and runs it again after.'
    );
    resetListedNamesForTests();
  });
});

/**
 * Q-431 (owner demo, shot sd-B3-03): a chat on "Studio chat, split for heavy work" — Chat on the
 * Studio single, then the cloud, "Use the next meanwhile" — while the split answered another chat.
 * The bar said a message "shares the engine with that answer"; its turn went to the cloud.
 */
describe('Q-431: the busy bar says where THIS chat’s next message goes, by its own chain', () => {
  const read = (ifNotLoaded: 'load' | 'useNext'): NodesReadResponse_unstable => ({
    config: {
      version: 1,
      defs: [NODE_SPLIT.def, NODE_STUDIO.def, NODE_CLOUD.def],
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
              ifNotLoaded,
            },
            build: { chain: [{ node: NODE_SPLIT.def.id, weight: 1 }] },
          },
        },
      ],
    },
    nodes: [NODE_SPLIT, NODE_STUDIO, NODE_CLOUD],
    stored: true,
    lmStudioHidden: 0,
  });
  const residency: NodesResidencyResponse_unstable = {
    nodes: [
      { node: NODE_SPLIT.def.id, residency: { kind: 'serving' } },
      { node: NODE_STUDIO.def.id, residency: { kind: 'notRunning', otherWay: 'the split' } },
      { node: NODE_CLOUD.def.id, residency: { kind: 'alwaysReady' } },
    ],
    serving: null,
    loaderInstalled: true,
  };
  const detail = (model: string, nodes: ChatNodesFacts) => {
    const served = deriveChatServedBy(
      inputs({ main: main([request()], [LIVE_CHAT]), model, nodes })
    );
    render(
      <IntlTestWrapper>
        <MemoryRouter>
          <ComposerReadinessStrip
            serving={{ served, single: STOPPED, armed: true, turnInFlight: false }}
          />
        </MemoryRouter>
      </IntlTestWrapper>
    );
    const text = screen.getByTestId('composer-readiness-detail').textContent;
    cleanup();
    return text;
  };

  it('Use the next meanwhile: it names the node the message goes to, and that it does not wait', () => {
    expect(
      detail('strategy:studio-chat', { read: read('useNext'), residency, servedNode: null })
    ).toBe(
      'A message sent now goes to Claude Sonnet · OpenRouter — it does not wait for that answer.'
    );
  });

  it('Load it and wait: the message waits for that answer, then loads its node', () => {
    const loads =
      'A message sent now waits for that answer to finish, then loads 27B · Work’s Mac Studio.';
    expect(
      detail('strategy:studio-chat', { read: read('load'), residency, servedNode: null })
    ).toBe(loads);
    // A chat on one node is that node's chain of one: it loads after the answer.
    expect(
      detail(`node:${NODE_STUDIO.def.id}`, { read: read('load'), residency, servedNode: null })
    ).toBe(loads);
  });

  it('a chat whose next turn goes to the busy way still shares the engine', () => {
    expect(
      detail(`node:${NODE_SPLIT.def.id}`, { read: read('load'), residency, servedNode: null })
    ).toBe(
      'A message sent now shares the engine with that answer — it runs slower, or waits if there is no room.'
    );
  });
});
