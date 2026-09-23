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
      link: { backend: 'jaccl', tbIp: '192.168.0.2', interface: 'en3', speed: '80 Gb/s' },
    },
  ],
  lastPreflight: FLASH_PREFLIGHT_OK,
  events: EVENTS,
  restarts: 1,
  config: FLASH_CONFIG,
};

export const FLASH_SERVING: MlxDistributedStatus = {
  ...FLASH_READY,
  state: 'serving',
  inflight: 2,
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
