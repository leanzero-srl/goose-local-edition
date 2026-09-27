import type { NodeLoadGroupDto, NodesConfig, NodesServingWayDto } from '@aaif/goose-sdk';
import type { PlacementCandidate, PlacementPlan } from '../../acp/mlx-placement';
import type { MlxLocalModel } from '../../acp/mlx-engine';
import type { ProviderDetails } from '../../types/providers';
import type { EngineGlance } from '../../utils/engineGlance';
import type { Mac } from '../leanzero-swarm/macs';
import type { PlacementPlansRead } from '../leanzero-swarm/usePlacementPlans';
import type { NodeFacts } from './nodeGlance';
import type { ResolvedNodeDef } from './model';

/**
 * Facts for every node state of DESIGN-NODES-AND-STRATEGIES.md §4.3, in the shapes goosed and main
 * send them. The two Macs and the 27B split are the owner's (the 3.0.60/3.0.61 walk); every other
 * number is illustrative. The Q-128 case is here on purpose: the split serves under its HF id
 * while the engine answers to an alias, and goosed's residency (which matches through
 * `node_names_model`) is what says Serving — the card never re-derives it.
 */

export const MODEL_27B = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
export const MODEL_FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
export const STUDIO_KEY = 'n-studio';
export const STUDIO = `link:${STUDIO_KEY}`;

const GIB = 1024 * 1024 * 1024;

export const SELF_MAC: Mac = {
  key: 'self',
  isSelf: true,
  nodeId: 'n-macbook',
  name: 'Mihai Macbook',
  hostname: 'mihai-macbook',
  meshIp: '100.64.0.1',
  online: true,
  sessionsActive: 1,
  allows: { manage_models: true, answer_chat: true, run_split: true },
  pollError: null,
};

export const STUDIO_MAC: Mac = {
  key: STUDIO_KEY,
  isSelf: false,
  nodeId: STUDIO_KEY,
  name: 'Work’s Mac Studio',
  hostname: 'works-mac-studio',
  meshIp: '100.64.0.2',
  online: true,
  sessionsActive: 0,
  allows: { manage_models: true, answer_chat: true, run_split: true },
  pollError: null,
};

export const TWO_MACS: readonly Mac[] = [SELF_MAC, STUDIO_MAC];
export const ONE_MAC: readonly Mac[] = [SELF_MAC];

interface CandidateSpec {
  kind: 'single' | 'tensor' | 'pipeline';
  nodes: string[];
  names: string[];
  needGb: number[];
  budgetGb: number[];
  status?: PlacementCandidate['fit']['status'];
  shortGb?: number;
  shortNode?: string;
  action?: PlacementCandidate['action'];
  outcome?: PlacementCandidate['outcome'];
  supported?: boolean;
  decode?: number;
  measuredRuns?: number;
  context?: number;
}

export function candidate(spec: CandidateSpec): PlacementCandidate {
  const link = spec.kind === 'single' ? null : 'jaccl';
  const runs = spec.measuredRuns ?? 0;
  const decode = spec.decode ?? 20;
  return {
    id: `${spec.kind}:${link ? `${link}:` : ''}${spec.nodes.join('+')}`,
    key: link
      ? { kind: spec.kind, nodes: spec.nodes, link }
      : { kind: spec.kind, nodes: spec.nodes },
    nodeNames: spec.names,
    chips: spec.names.map(() => null),
    backend: 'rapid-mlx',
    supported: spec.supported ?? true,
    fit: {
      status: spec.status ?? 'fits',
      context: spec.context ?? 262144,
      shortBytes: spec.shortGb != null ? spec.shortGb * GIB : null,
      shortNode: spec.shortNode ?? null,
      nodes: spec.names.map((name, i) => ({
        name,
        needBytes: spec.needGb[i] * GIB,
        budgetBytes: spec.budgetGb[i] * GIB,
      })),
      detail: `${spec.names.join(' + ')}: the planner's arithmetic for this way`,
    },
    speed: {
      decode: {
        estimate: { value: decode, low: decode * 0.95, high: decode * 1.05 },
        measured: runs > 0,
        runs,
      },
      concurrency: 8,
    },
    action:
      spec.action ??
      (spec.kind === 'single'
        ? spec.nodes[0] === 'local'
          ? { kind: 'mountHere' }
          : { kind: 'remoteSingle' }
        : { kind: 'startSplit', setupMatches: true }),
    outcome: spec.outcome ?? { code: 'best' },
  };
}

export const SPLIT_27B = candidate({
  kind: 'pipeline',
  nodes: ['local', STUDIO],
  names: ['Mihai Macbook', 'Work’s Mac Studio'],
  needGb: [38.6, 38.6],
  budgetGb: [61.8, 66.2],
  decode: 11.2,
  measuredRuns: 3,
});

export const STUDIO_27B = candidate({
  kind: 'single',
  nodes: [STUDIO],
  names: ['Work’s Mac Studio'],
  needGb: [46.5],
  budgetGb: [66.2],
  decode: 21.9,
  outcome: { code: 'slower', mine: 21.9, best: 22.4 },
});

export const LOCAL_27B_SHORT = candidate({
  kind: 'single',
  nodes: ['local'],
  names: ['Mihai Macbook'],
  needGb: [63.4],
  budgetGb: [61.8],
  status: 'short',
  shortGb: 1.6,
  shortNode: 'Mihai Macbook',
  outcome: { code: 'doesNotFit' },
});

export const LOCAL_FLASH = candidate({
  kind: 'single',
  nodes: ['local'],
  names: ['Mihai Macbook'],
  needGb: [18.2],
  budgetGb: [61.8],
  decode: 41,
  measuredRuns: 4,
  context: 131072,
});

export const PLANS: PlacementPlan[] = [
  {
    modelId: MODEL_27B,
    goal: 'chat',
    candidates: [SPLIT_27B, STUDIO_27B, LOCAL_27B_SHORT],
    best: SPLIT_27B.id,
    bestAvailable: SPLIT_27B.id,
    badge: { kind: 'needsBothMacs', needs: null },
    notes: [],
  },
  {
    modelId: MODEL_FLASH,
    goal: 'chat',
    candidates: [LOCAL_FLASH],
    best: LOCAL_FLASH.id,
    bestAvailable: LOCAL_FLASH.id,
    badge: { kind: 'fitsThisMac' },
    notes: [],
  },
];

export const PLANS_READ: PlacementPlansRead = {
  kind: 'read',
  plans: new Map(PLANS.map((p) => [p.modelId, p])),
  storeErrors: [],
};

function mlxNode(
  id: string,
  name: string,
  model: string,
  placement: NonNullable<ResolvedNodeDef['def']['placement']>
): ResolvedNodeDef {
  return {
    def: { id, name, kind: 'mlx', model, placement, goal: 'chat', origin: 'user' },
    model,
    modelFrom: { kind: 'own' },
  };
}

export const NODE_SPLIT = mlxNode('27b-both', '27B Atlassian · both Macs', MODEL_27B, {
  kind: 'pipeline',
  macs: ['local', STUDIO],
  link: 'jaccl',
});
export const NODE_STUDIO = mlxNode('27b-studio', '27B · Work’s Mac Studio', MODEL_27B, {
  kind: 'single',
  macs: [STUDIO],
});
export const NODE_LOCAL_27B = mlxNode('27b-here', '27B · this Mac', MODEL_27B, {
  kind: 'single',
  macs: ['local'],
});
export const NODE_FLASH = mlxNode('flash-here', 'Flash · this Mac', MODEL_FLASH, {
  kind: 'single',
  macs: ['local'],
});

export const NODE_POOL: ResolvedNodeDef = {
  def: {
    id: 'mlx-local',
    name: 'Mihai Macbook engine',
    kind: 'mlx',
    placement: { kind: 'follows' },
    poolDevice: 'mlx-local',
    origin: 'pool',
  },
  model: 'qwen3.8-27b',
  modelFrom: { kind: 'pool' },
};

export const NODE_POOL_LEFT: ResolvedNodeDef = {
  ...NODE_POOL,
  def: { ...NODE_POOL.def, id: 'mlx-gone', name: 'Old engine', poolDevice: 'mlx-gone' },
  model: null,
  modelFrom: { kind: 'leftPool' },
};

export const NODE_CLOUD: ResolvedNodeDef = {
  def: {
    id: 'sonnet',
    name: 'Claude Sonnet · OpenRouter',
    kind: 'cloud',
    model: 'anthropic/claude-sonnet-4.5',
    provider: 'openrouter',
    origin: 'user',
  },
  model: 'anthropic/claude-sonnet-4.5',
  provider: 'openrouter',
  modelFrom: { kind: 'own' },
};

export const NODE_ENDPOINT: ResolvedNodeDef = {
  def: {
    id: 'desk-vllm',
    name: 'Qwen · Desk vLLM',
    kind: 'endpoint',
    model: 'qwen3-coder',
    provider: 'custom_desk_vllm',
    origin: 'user',
  },
  model: 'qwen3-coder',
  provider: 'custom_desk_vllm',
  modelFrom: { kind: 'own' },
};

export function provider(
  name: string,
  displayName: string,
  over: Partial<ProviderDetails> = {}
): ProviderDetails {
  return {
    name,
    provider_type: 'Preferred',
    is_configured: true,
    credentials_saved: true,
    connection_checked: true,
    connection_error: null,
    metadata: {
      name,
      display_name: displayName,
      description: '',
      default_model: '',
      model_doc_link: '',
      config_keys: [],
      known_models: [
        { name: 'anthropic/claude-sonnet-4.5', context_limit: 200000 },
        { name: 'openai/gpt-5', context_limit: 400000 },
      ],
    },
    ...over,
  };
}

export const OPENROUTER = provider('openrouter', 'OpenRouter');

/** The 27B split as goosed's residency reads it: its HF id, served under an alias (Q-128). */
export const WAY_SPLIT: NodesServingWayDto = {
  kind: 'split',
  macs: [],
  link: 'jaccl',
  modelId: MODEL_27B,
  servedModelId: 'qwen3.8-27b',
  macNames: ['Mihai Macbook', 'Work’s Mac Studio'],
};

/** Flash on the Studio as a remote single, while this Mac's own engine is idle. */
export const WAY_REMOTE: NodesServingWayDto = {
  kind: 'remoteSingle',
  macs: [STUDIO],
  modelId: MODEL_FLASH,
  servedModelId: 'Qwen3.8-Flash-Next-4bit',
  macNames: ['Work’s Mac Studio'],
};

export const WAY_LOADING: NodesServingWayDto = {
  kind: 'single',
  macs: ['local'],
  modelId: MODEL_FLASH,
  servedModelId: 'Qwen3.8-Flash-Next-4bit',
  macNames: ['Mihai Macbook'],
  loadPhase: 'loading',
};

export function engineGlance(over: Partial<EngineGlance> = {}): EngineGlance {
  return {
    present: true,
    busy: true,
    phase: 'writing',
    stage: 'generating',
    engine: { mode: 'remote', peerName: 'Work’s Mac Studio' },
    modelId: MODEL_27B,
    hero: { kind: 'writing', tps: 11.2 },
    second: { kind: 'readingMedian', median: 154, runs: 3 },
    progress: null,
    waiting: 0,
    inflight: null,
    chat: { sessionId: 's1', name: 'Kickoff notes', work: null },
    side: [],
    otherClients: 0,
    ranges: { writing: { low: 10.4, high: 11.6 }, reading: { low: 81, high: 227 } },
    nodes: [
      { name: 'Mihai Macbook', phase: 'writing', peakGb: 38.6, budgetGb: 61.8, load: null },
      { name: 'Work’s Mac Studio', phase: 'writing', peakGb: 38.6, budgetGb: 66.2, load: null },
    ],
    detail: null,
    servedBy: null,
    ...over,
  };
}

export const LOADS_FLASH: NodeLoadGroupDto[] = [
  {
    placement: { kind: 'single', nodes: ['local'] },
    medianTotalMs: 48000,
    count: 3,
    records: [],
  },
];

const complete = (id: string): MlxLocalModel => ({
  id,
  sizeBytes: 31 * GIB,
  complete: true,
  missingFiles: 0,
});

/** Both models on both Macs, as useMacs reads the folders. */
export const MODELS_EVERYWHERE = (): MlxLocalModel[] => [
  complete(MODEL_27B),
  complete(MODEL_FLASH),
];

export function facts(over: Partial<NodeFacts> = {}): NodeFacts {
  return {
    residency: { kind: 'read', value: { kind: 'notRunning', otherWay: null } },
    serving: null,
    servingNodeName: null,
    glance: null,
    plans: PLANS_READ,
    macs: TWO_MACS,
    modelsOn: MODELS_EVERYWHERE,
    loads: { kind: 'read', value: [] },
    buildHolder: null,
    provider: { kind: 'read', value: OPENROUTER },
    ...over,
  };
}

/** A config with one strategy that names the split 1st for Chat and Build and the cloud 2nd. */
export const CONFIG: NodesConfig = {
  version: 1,
  defs: [NODE_SPLIT.def, NODE_FLASH.def, NODE_CLOUD.def],
  strategies: [
    {
      id: 'everyday',
      name: 'Everyday',
      roles: {
        chat: {
          chain: [
            { node: NODE_SPLIT.def.id, weight: 1 },
            { node: NODE_CLOUD.def.id, weight: 1 },
          ],
          when: 'failover',
        },
        build: {
          chain: [
            { node: NODE_SPLIT.def.id, weight: 2 },
            { node: NODE_CLOUD.def.id, weight: 1 },
          ],
          when: 'share',
        },
      },
    },
  ],
  declined: [],
  forNewChats: { kind: 'strategy', id: 'everyday' },
  forBuilds: { kind: 'pool' },
};
