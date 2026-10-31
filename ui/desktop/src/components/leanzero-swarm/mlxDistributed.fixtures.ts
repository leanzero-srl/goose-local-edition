import type {
  MlxDistributedConfig,
  MlxDistributedPreflight,
  MlxDistributedStatus,
} from '../../acp/mlx-distributed';

/**
 * Realistic distributed-engine DTOs for tests, built from the recorded Flash JACCL run
 * (2026-09-24, crates/goose-sidecar/src/distributed/plan.rs): MacBook Pro 128 GiB with 92.7 GiB
 * available → budget 83.43 GiB, layers [0, 20), measured peak 61.0 GiB; workhorse 96 GiB with
 * 61.6 GiB available → budget 55.44 GiB, layers [20, 48), measured peak 42.5 GiB. Planned bytes are
 * the fork planner's 57.81 / 38.92 GiB, × the 1.10 runtime-overhead ratio.
 */

const GIB = 1024 * 1024 * 1024;
const bytes = (gib: number) => Math.round(gib * GIB);

export const FLASH_MODEL = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
export const RAN_AT_MS = Date.UTC(2026, 8, 24, 9, 14, 3);

export const FLASH_CONFIG: MlxDistributedConfig = {
  modelId: FLASH_MODEL,
  backend: 'jaccl',
  port: 8190,
  coordinatorPort: 32323,
  context: 8192,
  restartOnFailure: true,
  nodes: [
    {
      name: 'MacBook Pro',
      tbIp: '192.168.0.1',
      tbNetmask: '255.255.255.252',
      tbInterface: 'en3',
      tbService: 'EXO Thunderbolt 3',
      rdmaDevice: 'rdma_en3',
      python: '/Users/mihaiperdum/.goose/mlx-venv/bin/python',
      pipelinePython: '/Users/mihaiperdum/Projects/Rapid-MLX/.venv/bin/python',
      modelDir: `/Users/mihaiperdum/.goose/models/${FLASH_MODEL}`,
    },
    {
      name: 'workhorse',
      ssh: 'workhorse',
      tbIp: '192.168.0.2',
      tbNetmask: '255.255.255.252',
      tbInterface: 'en3',
      tbService: 'EXO Thunderbolt 2',
      rdmaDevice: 'rdma_en3',
      python: '/Users/workhorse/.goose/mlx-venv/bin/python',
      pipelinePython: '/Users/workhorse/Projects/Rapid-MLX-pipeline/.venv/bin/python',
      modelDir: `/Users/workhorse/.goose/models/${FLASH_MODEL}`,
    },
  ],
};

function plan(
  layerStart: number,
  layerEnd: number,
  weights: number,
  state: number,
  planned: number,
  budget: number
) {
  return {
    layerStart,
    layerEnd,
    weightsBytes: bytes(weights),
    stateBytes: bytes(state),
    workspaceBytes: bytes(0.36),
    promptCacheBytes: 0,
    plannedBytes: bytes(planned),
    withOverheadBytes: bytes(planned * 1.1),
    budgetBytes: bytes(budget),
    fits: planned * 1.1 <= budget,
  };
}

export const FLASH_PREFLIGHT_OK: MlxDistributedPreflight = {
  ok: true,
  ranAtMs: RAN_AT_MS,
  backend: 'jaccl',
  runner: 'pipelineQwen4',
  modelType: 'qwen4_exp',
  contextLimit: 8192,
  contextSource: 'requested',
  maxContextFits: 262144,
  checks: [
    {
      id: 'runner',
      verdict: 'pass',
      message: 'rapid_mlx.distributed.pipeline_qwen4 offers ["plan", "run", "serve"]',
    },
    {
      id: 'plan',
      verdict: 'pass',
      message:
        'rank 0 layers [0, 20) 63.59 GiB of 83.43 GiB; rank 1 layers [20, 48) 42.81 GiB of 55.44 GiB',
    },
  ],
  nodes: [
    {
      name: 'MacBook Pro',
      rank: 0,
      checks: [
        {
          id: 'memory',
          verdict: 'pass',
          message: '92.70 GiB available of 128.00 GiB, pressure normal',
        },
        { id: 'tbIpv4', verdict: 'pass', message: 'en3 carries 192.168.0.1' },
        { id: 'rdmaGid', verdict: 'pass', message: 'rdma_en3 GID[1] = ::ffff:192.168.0.1' },
      ],
      availableBytes: bytes(92.7),
      totalBytes: bytes(128),
      pressure: 'normal',
      plan: plan(0, 20, 57.33, 0.13, 57.81, 83.43),
      linkSpeed: '80 Gb/s',
      mlxVersion: 'mlx 0.32.2 · mlx_lm 0.31.3',
    },
    {
      name: 'workhorse',
      rank: 1,
      host: 'workhorse',
      checks: [
        { id: 'reachable', verdict: 'pass', message: 'ssh workhorse answered' },
        {
          id: 'memory',
          verdict: 'pass',
          message: '61.60 GiB available of 96.00 GiB, pressure normal',
        },
        { id: 'ping', verdict: 'pass', message: 'peers answer: 192.168.0.1' },
      ],
      availableBytes: bytes(61.6),
      totalBytes: bytes(96),
      pressure: 'normal',
      plan: plan(20, 48, 38.38, 0.18, 38.92, 55.44),
      linkSpeed: '80 Gb/s',
      mlxVersion: 'mlx 0.32.2 · mlx_lm 0.31.3',
    },
  ],
  repairs: [],
};

/** The workhorse with a browser eating its memory: the plan no longer fits and the check says by how much. */
export const FLASH_PREFLIGHT_REFUSED: MlxDistributedPreflight = {
  ...FLASH_PREFLIGHT_OK,
  ok: false,
  ranAtMs: RAN_AT_MS + 60_000,
  checks: [
    {
      id: 'runner',
      verdict: 'pass',
      message: 'rapid_mlx.distributed.pipeline_qwen4 offers ["plan", "run", "serve"]',
    },
    {
      id: 'plan',
      verdict: 'fail',
      message:
        'rank 1 workhorse needs 42.81 GiB with overhead; budget 35.10 GiB (39.00 GiB available × 0.90)',
    },
  ],
  nodes: [
    FLASH_PREFLIGHT_OK.nodes[0],
    {
      ...FLASH_PREFLIGHT_OK.nodes[1],
      checks: [
        { id: 'reachable', verdict: 'pass', message: 'ssh workhorse answered' },
        {
          id: 'memory',
          verdict: 'fail',
          message: '39.00 GiB available of 96.00 GiB, pressure warn',
        },
      ],
      availableBytes: bytes(39),
      pressure: 'warn',
      plan: plan(20, 48, 38.38, 0.18, 38.92, 35.1),
    },
  ],
};

const EVENTS = [
  { atMs: RAN_AT_MS, kind: 'preflight', message: 'preflight passed (2 nodes, jaccl)' },
  {
    atMs: RAN_AT_MS + 1_000,
    kind: 'linkRepaired',
    node: 'workhorse',
    message: 'EXO Thunderbolt 2 toggled; GID[1] ::ffff:192.168.0.2 restored',
  },
  { atMs: RAN_AT_MS + 2_000, kind: 'launched', message: 'rank 0 pid 81234, rank 1 pid 5521' },
  { atMs: RAN_AT_MS + 94_000, kind: 'ready', message: 'first completion streamed [DONE] in 3.1 s' },
  {
    atMs: RAN_AT_MS + 900_000,
    kind: 'hang',
    node: 'workhorse',
    message: 'no progress for 41.0 s (bound 10 × median 4.1 s)',
  },
  { atMs: RAN_AT_MS + 901_000, kind: 'restart', message: 'restart 1 of the breaker window' },
  {
    atMs: RAN_AT_MS + 990_000,
    kind: 'ready',
    message: 'first completion streamed [DONE] in 2.9 s',
  },
];

export const FLASH_READY: MlxDistributedStatus = {
  mode: 'distributed',
  state: 'ready',
  backend: 'jaccl',
  runner: 'pipelineQwen4',
  modelId: FLASH_MODEL,
  baseUrl: 'http://127.0.0.1:8190/v1',
  contextLimit: 8192,
  admissionOpen: true,
  inflight: 0,
  // Rank 0's /v1/status for the pipeline runner, idle: 2 slots planned, none reserved.
  waiting: 0,
  slots: 2,
  slotsInUse: 0,
  sequencesInFlight: 0,
  liveness: { samples: 12, medianMs: 410, boundMs: 4100, silentMs: 800 },
  nodes: [
    {
      name: 'MacBook Pro',
      rank: 0,
      role: 'coordinator',
      state: 'ready',
      pid: 81234,
      layerStart: 0,
      layerEnd: 20,
      availableMemoryGb: 31.7,
      totalMemoryGb: 128,
      pressure: 'normal',
      activeMemoryGb: 58.4,
      peakMemoryGb: 61.0,
      plannedMemoryGb: 57.81,
      memoryLimitGb: 96,
      wiredLimitGb: 76.8,
      cacheLimitGb: 8,
      // The plan's state + workspace for 2 full-context sequences (0.13 + 0.36 GiB).
      kvReservedGb: 0,
      kvBudgetGb: 0.49,
      link: { backend: 'jaccl', tbIp: '192.168.0.1', interface: 'en3', speed: '80 Gb/s' },
    },
    {
      name: 'workhorse',
      rank: 1,
      role: 'worker',
      host: 'workhorse',
      state: 'ready',
      pid: 5521,
      layerStart: 20,
      layerEnd: 48,
      availableMemoryGb: 19.1,
      totalMemoryGb: 96,
      pressure: 'normal',
      activeMemoryGb: 40.2,
      peakMemoryGb: 42.5,
      plannedMemoryGb: 38.92,
      memoryLimitGb: 72,
      wiredLimitGb: 57.6,
      cacheLimitGb: 8,
      kvReservedGb: 0,
      kvBudgetGb: 0.54,
      link: { backend: 'jaccl', tbIp: '192.168.0.2', interface: 'en3', speed: '80 Gb/s' },
    },
  ],
  lastPreflight: FLASH_PREFLIGHT_OK,
  events: EVENTS,
  restarts: 1,
  config: FLASH_CONFIG,
};

/** Two requests running: both slots held, every rank's KV budget reserved. */
export const FLASH_SERVING: MlxDistributedStatus = {
  ...FLASH_READY,
  state: 'serving',
  inflight: 2,
  slotsInUse: 2,
  sequencesInFlight: 2,
  nodes: FLASH_READY.nodes.map((n) => ({ ...n, kvReservedGb: n.kvBudgetGb })),
};

/** Nothing supervised: the persisted config is reported, the Mac belongs to the single engine. */
export const STOPPED_WITH_CONFIG: MlxDistributedStatus = {
  mode: 'single',
  state: 'stopped',
  admissionOpen: true,
  nodes: [],
  events: [],
  restarts: 0,
  config: FLASH_CONFIG,
};

/**
 * The workhorse serving rank 1 of the MacBook's 27B tensor run over LeanZero Link: its own engine is
 * stopped (the single engine is refused meanwhile) and `hosting` says whose run it serves.
 */
export const HOSTING_RANK_1: MlxDistributedStatus = {
  ...STOPPED_WITH_CONFIG,
  config: null,
  allowDistributedNode: true,
  hosting: {
    rank: 1,
    size: 2,
    requesterName: 'MacBook Pro',
    requesterNodeId: 'macbook-1a2b',
    requesterHostname: 'macbook',
    modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
    servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
    backend: 'jaccl',
    runner: 'mlxLmTensor',
    pid: 4242,
    state: 'serving',
    startedMs: 1790200000000,
    lastPollMs: 1790200002000,
  },
};

// ---------------------------------------------------------------------------
// MiMo (mimo_v2) on goose's own layer split, the mlx_lm pipeline runner
// ---------------------------------------------------------------------------

/**
 * The real MiMo plan at mimo_v2's own 5% reserve (goose-sidecar layer_split.rs, session 4,
 * 2026-10-26): rank 0 workhorse (96 GiB, 76.70 GB available) holds layers [29, 48), rank 1 mule
 * (128 GiB, 111.00 GB available) holds [0, 29). The derived context (60,672) fills each budget, so
 * spare is 0.00 / 0.23 GB by design; the margin is the reserve. Byte figures are the backend's.
 */
export const MIMO_MODEL = 'mlx-community/MiMo-V2-Flash-4bit';
export const MIMO_REASON =
  'split 19/29 (rank 0 = the last layers) leaves the most memory on the tightest Mac of all 47 contiguous splits: spare 0.00 GB / 0.23 GB at context 60672 with a 256-token prefill chunk, reserve 5.0% of RAM';

export const MIMO_CONFIG: MlxDistributedConfig = {
  modelId: MIMO_MODEL,
  backend: 'jaccl',
  port: 8190,
  coordinatorPort: 32323,
  restartOnFailure: true,
  nodes: [
    {
      name: 'workhorse',
      tbIp: '192.168.0.2',
      tbNetmask: '255.255.255.252',
      tbInterface: 'en3',
      tbService: 'EXO Thunderbolt 2',
      rdmaDevice: 'rdma_en3',
      python: '/Users/workhorse/.goose/mlx-venv/bin/python',
      modelDir: `/Users/workhorse/.goose/models/${MIMO_MODEL}`,
    },
    {
      name: 'mule',
      ssh: 'mule',
      tbIp: '192.168.0.1',
      tbNetmask: '255.255.255.252',
      tbInterface: 'en3',
      tbService: 'EXO Thunderbolt 3',
      rdmaDevice: 'rdma_en3',
      python: '/Users/mule/.goose/mlx-venv/bin/python',
      modelDir: `/Users/mule/.goose/models/${MIMO_MODEL}`,
    },
  ],
};

function mimoPlan(
  layerStart: number,
  layerEnd: number,
  weightsBytes: number,
  withOverheadBytes: number,
  budgetBytes: number
) {
  return {
    layerStart,
    layerEnd,
    weightsBytes,
    stateBytes: 0,
    workspaceBytes: withOverheadBytes - weightsBytes,
    promptCacheBytes: 0,
    plannedBytes: withOverheadBytes,
    withOverheadBytes,
    budgetBytes,
    fits: withOverheadBytes <= budgetBytes,
  };
}

const MIMO_NODES = [
  {
    name: 'workhorse',
    rank: 0,
    checks: [{ id: 'memory', verdict: 'pass', message: '76.70 GB available of 103.08 GB' }],
    availableBytes: 76_700_000_000,
    totalBytes: 103_079_215_104,
    pressure: 'normal',
    plan: mimoPlan(29, 48, 68_279_546_752, 71_545_402_240, 71_546_039_245),
    linkSpeed: '80 Gb/s',
  },
  {
    name: 'mule',
    rank: 1,
    host: 'mule',
    checks: [{ id: 'memory', verdict: 'pass', message: '111.00 GB available of 137.44 GB' }],
    availableBytes: 111_000_000_000,
    totalBytes: 137_438_953_472,
    pressure: 'normal',
    plan: mimoPlan(0, 29, 100_312_603_648, 103_900_896_256, 104_128_052_327),
    linkSpeed: '80 Gb/s',
  },
];

export const MIMO_PREFLIGHT_OK: MlxDistributedPreflight = {
  ok: true,
  ranAtMs: RAN_AT_MS,
  backend: 'jaccl',
  runner: 'mlxLmPipeline',
  modelType: 'mimo_v2',
  layerSplit: [19, 29],
  splitReason: MIMO_REASON,
  planReserveRatio: 0.05,
  planReserveSource: 'model',
  contextLimit: 60672,
  contextSource: 'derived',
  maxContextFits: 60672,
  checks: [
    {
      id: 'plan',
      verdict: 'pass',
      message:
        'rank 0 workhorse layers 29-47 71.55 GB of 71.55 GB; rank 1 mule layers 0-28 103.90 GB of 104.13 GB',
    },
  ],
  nodes: MIMO_NODES,
  repairs: [],
};

/**
 * The same pair with the reserve set to goose's shared 9.3% by hand: no split fits, and the planner
 * names the reserve that does (7.7%) — the sentence's fragments are the ones layer_split.rs's test
 * `the_shared_reserve_refuses_naming_the_numbers_and_the_reserve_that_fits` asserts.
 * The need, ceiling and spare figures between them are ILLUSTRATIVE (composed, not captured); the
 * view shows the sentence verbatim and never parses it.
 */
export const MIMO_REFUSAL =
  'no layer split fits at reserve 9.3% of RAM: the best (19/29 layers, rank 0 = the last) leaves workhorse layers 29-47 need 71.55 GB against budget 67.11 GB (available 76.70 GB − 9.3% of 103.08 GB RAM, GPU ceiling 77.31 GB), spare -4.44 GB; mule layers 0-28 need 103.90 GB against budget 98.22 GB (available 111.00 GB − 9.3% of 137.44 GB RAM, GPU ceiling 103.08 GB), spare -5.68 GB. a split fits at reserve 7.7% (19/29 layers, spare 0.21 GB / 0.48 GB): set plan_reserve_ratio between 0.05 and 0.077 to plan there';

export const MIMO_PREFLIGHT_REFUSED: MlxDistributedPreflight = {
  ...MIMO_PREFLIGHT_OK,
  ok: false,
  ranAtMs: RAN_AT_MS + 60_000,
  layerSplit: null,
  splitReason: null,
  planReserveRatio: 0.093,
  planReserveSource: 'user',
  fittingReserveRatio: 0.077,
  contextLimit: null,
  contextSource: null,
  maxContextFits: null,
  checks: [{ id: 'plan', verdict: 'fail', message: MIMO_REFUSAL }],
  nodes: MIMO_NODES.map((n) => ({ ...n, plan: null })),
};
