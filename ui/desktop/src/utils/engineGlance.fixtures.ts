import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import { attributeServing, type MlxServingRow } from './mlxServing';
import { parseMlxLiveStatus, type MlxLiveStats } from '../components/leanzero-swarm/mlxLiveStats';
import type { MeasuredRunsAnswer, MlxMeasuredRead, SpeedFigure } from './mlxMeasuredRuns';
import {
  DEFAULT_GLANCE_PREFS,
  NO_SESSIONS,
  buildEngineGlance,
  type EngineGlanceOptions,
  type GlancePrefs,
  type GlancePush,
  type GlanceSessions,
} from './engineGlance';

/** Fixtures for the engine glance: main's snapshot in each state, and a push built from it. */

export const GLANCE_MODEL = 'mlx-community/Qwen3.6-27B-4bit';

export function statsOf(body: unknown): MlxLiveStats {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return read.stats;
}

export function runningSnapshot(
  body: unknown,
  over: Partial<MlxEngineSnapshot> = {}
): MlxEngineSnapshot {
  return {
    ...INITIAL_SNAPSHOT,
    mode: 'running',
    modelId: GLANCE_MODEL,
    baseUrl: 'http://127.0.0.1:8090',
    stats: statsOf(body),
    serving: attributeServing([], 0, [], null),
    ...over,
  };
}

export const figure = (value: number, low: number, high: number, runs: number): SpeedFigure => ({
  estimate: { value, low, high },
  measured: true,
  runs,
});

export function measuredRead(answer: Partial<MeasuredRunsAnswer>): MlxMeasuredRead {
  return {
    kind: 'read',
    answer: {
      way: null,
      wayError: null,
      recorded: 0,
      writing: null,
      writingBasis: null,
      reading: null,
      readingByBucket: [],
      storeErrors: [],
      ...answer,
    },
  };
}

/** The owner's own screenshot: a split reading an 80.3K prompt for 3m 11s at 237 tok/s. */
export const SPLIT_READING_BODY = {
  num_running: 1,
  num_waiting: 0,
  slots: 2,
  slots_in_use: 1,
  status: 'generating',
  generation_tps: null,
  requests: [
    {
      request_id: 'split-read-1',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 191,
      prompt_tokens: 80300,
      prefilled_tokens: 45200,
      prompt_tokens_per_second: 237,
      completion_tokens: 0,
      max_tokens: 4096,
      tokens_per_second: null,
      ttft_s: null,
      cached_tokens: null,
    },
  ],
};

export const CHAT_ROW: MlxServingRow = {
  id: 1,
  via: 'swarmRouter',
  sessionId: '20260927_12',
  provider: 'omlx',
  model: GLANCE_MODEL,
  nodeId: null,
  startedAt: '2026-09-27T10:00:00Z',
  sessionName: 'Refactor the auth flow',
  sessionType: 'user',
  sessionError: null,
};

export function glancePush(
  snapshot: MlxEngineSnapshot,
  options: Partial<EngineGlanceOptions> = {},
  sessions: GlanceSessions = NO_SESSIONS,
  prefs: Partial<GlancePrefs> = {}
): GlancePush {
  return {
    engine: buildEngineGlance(snapshot, { distributed: null, remote: null, served: [], ...options }),
    sessions,
    prefs: { ...DEFAULT_GLANCE_PREFS, ...prefs },
    desktopDismissed: false,
  };
}

/**
 * The owner's screenshot 26 (Q-218): the split reading the chat's 77k prompt, 1% in, while goose's
 * own 174-token side call (a tool label) is read beside it — the older, longer-read request. The
 * card said "Reading prompt · 174 prompt tokens, reading for 12s · 3.9 tok/s".
 */
export const TURN_BESIDE_SIDE_CALL_BODY = {
  num_running: 2,
  num_waiting: 0,
  status: 'generating',
  generation_tps: null,
  requests: [
    {
      request_id: 'side-label-1',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 12,
      prompt_tokens: 174,
      prefilled_tokens: 64,
      prompt_tokens_per_second: 3.9,
      completion_tokens: 0,
      max_tokens: 256,
      tokens_per_second: null,
      ttft_s: null,
      cached_tokens: null,
    },
    {
      request_id: 'turn-77k',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 9,
      prompt_tokens: 77000,
      prefilled_tokens: 770,
      prompt_tokens_per_second: 85.6,
      completion_tokens: 0,
      max_tokens: 32768,
      tokens_per_second: null,
      ttft_s: null,
      cached_tokens: null,
    },
  ],
};

/** The same chat's turn WAITING for a slot while goose's side call is the one being read. */
export const TURN_WAITING_BEHIND_SIDE_CALL_BODY = {
  ...TURN_BESIDE_SIDE_CALL_BODY,
  num_running: 1,
  num_waiting: 1,
  requests: [
    TURN_BESIDE_SIDE_CALL_BODY.requests[0],
    {
      ...TURN_BESIDE_SIDE_CALL_BODY.requests[1],
      status: 'waiting',
      phase: 'queued',
      prefilled_tokens: null,
      prompt_tokens_per_second: null,
    },
  ],
};

/** goose's tool-label call for the same chat (Q-185 `work`), running beside its turn. */
export const TOOL_LABEL_ROW: MlxServingRow = {
  ...CHAT_ROW,
  id: 2,
  work: 'toolLabel',
};
