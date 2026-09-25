import type { MlxDistributedStatus } from '../../acp/mlx-distributed';

/**
 * E2E #2 (2026-09-25, installed 3.0.41): the 27B split across both Macs over JACCL served chat for
 * twenty minutes; then Work's Mac Studio's memory ran low, its rank ended, four answers were cut and
 * the supervisor stopped the pair — the goosed log's own events, times and words (UTC ms).
 */
export const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
export const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
export const STUDIO = 'Work’s Mac Studio';
export const MACBOOK = 'Mihai Macbook';

export const LAUNCHED_MS = Date.UTC(2026, 8, 25, 19, 23, 16, 394);
export const READY_MS = Date.UTC(2026, 8, 25, 19, 23, 23, 411);
export const WARN_MS = Date.UTC(2026, 8, 25, 19, 43, 44, 365);
export const DIED_MS = Date.UTC(2026, 8, 25, 19, 43, 46, 366);
export const STOPPED_MS = Date.UTC(2026, 8, 25, 19, 43, 46, 477);

export const RANK_DIED_MESSAGE =
  'rank 1 ended on its Link node (its LeanZero Link session closed) (exit status: 0); the pair cannot serve. Last output:\nGOOSE_RANK_GROUP {"rank": 1, "size": 2, "mlx": "0.32.2"}';

export const SPLIT_STOPPED_E2E2: MlxDistributedStatus = {
  mode: 'single',
  state: 'failed',
  backend: 'jaccl',
  runner: 'mlxLmTensor',
  modelId: HF,
  servedModelId: ALIAS,
  contextLimit: 262144,
  admissionOpen: false,
  nodes: [
    {
      name: MACBOOK,
      rank: 0,
      role: 'coordinator',
      state: 'stopped',
      availableMemoryGb: 61.2,
      totalMemoryGb: 128,
      pressure: 'normal',
      link: { backend: 'jaccl', tbIp: '192.168.0.1', interface: 'en3' },
    },
    {
      name: STUDIO,
      rank: 1,
      role: 'worker',
      state: 'failed',
      availableMemoryGb: 3.9,
      totalMemoryGb: 96,
      pressure: 'normal',
      link: { backend: 'jaccl', tbIp: '192.168.0.2', interface: 'en3' },
    },
  ],
  events: [
    { atMs: LAUNCHED_MS - 2_000, kind: 'preflight', message: 'preflight before launch' },
    { atMs: LAUNCHED_MS, kind: 'launched', message: '2 ranks over jaccl (262144 context)' },
    { atMs: READY_MS, kind: 'ready', message: 'the readiness completion ended with [DONE]' },
    {
      atMs: WARN_MS,
      kind: 'watchdogWarn',
      message: `${STUDIO}: kernel pressure normal, available 3.9 GiB of 96.0 GiB (WARN below 4.8 GiB = 0.050 × RAM, CRITICAL below 1.9 GiB = 0.020 × RAM)`,
    },
    {
      atMs: WARN_MS,
      kind: 'admissionClosed',
      message: 'new requests are answered 503 until memory recovers',
    },
    { atMs: DIED_MS, kind: 'rankDied', node: STUDIO, message: RANK_DIED_MESSAGE },
    {
      atMs: DIED_MS,
      kind: 'streamWithoutDone',
      node: STUDIO,
      message:
        '4 in-flight request(s) cut by the rankDied: their streams end without `data: [DONE]` (an HTTP 200 already sent is not a completion)',
    },
    {
      atMs: STOPPED_MS,
      kind: 'stopped',
      message: 'after rankDied: verified; rank 0 (Mihai Macbook) pid 21399: SIGTERM → exited',
    },
  ],
  restarts: 0,
  lastError: RANK_DIED_MESSAGE,
};
