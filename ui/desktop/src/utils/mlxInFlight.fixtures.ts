import type { MlxEngineSnapshot } from './mlxEngineMonitor';
import type { MlxLiveRequest, MlxLiveStats } from '../components/leanzero-swarm/mlxLiveStats';

/**
 * The live round of 2026-09-26 (ROUND-2026-09-26-live-1.md): the 27B tensor split over Mihai
 * Macbook + Work's Mac Studio writing session 20260926_19 "Jira Migration Kickoff Notes" —
 * /v1/status at 23:13: 24,228 tokens written, 2,355 s elapsed, 11 tok/s, prompt 39,996.
 */
export const LIVE_WRITING: MlxLiveRequest = {
  id: 'r1',
  status: 'running',
  phase: 'generation',
  elapsedS: 2355,
  promptTokens: 39996,
  completionTokens: 24228,
  maxTokens: 222148,
  tokensPerSecond: 11,
  ttftS: 133,
  cachedTokens: 0,
  prefilledTokens: null,
  promptTps: null,
  client: null,
  heldForRoom: null,
  stopped: null,
  stoppedAfterS: null,
  leaving: false,
};

export function liveStats(requests: MlxLiveRequest[]): MlxLiveStats {
  return {
    engineStatus: requests.length ? 'generating' : 'idle',
    uptimeS: 3000,
    generationTps: 11,
    numRunning: requests.length,
    numWaiting: 0,
    activeMemoryGb: null,
    cacheHitRate: null,
    cacheTokensSaved: null,
    totalRequests: 1,
    totalPromptTokens: null,
    totalCompletionTokens: null,
    requests,
    samplingDefaults: null,
  };
}

export function liveSplitSnapshot(
  requests: MlxLiveRequest[] = [LIVE_WRITING],
  engine: MlxEngineSnapshot['engine'] = 'distributed'
): MlxEngineSnapshot {
  return {
    engine,
    mode: 'running',
    modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
    modelDetail: null,
    baseUrl: 'http://127.0.0.1:8091',
    stats: liveStats(requests),
    statusDetail: null,
    measured: { kind: 'pending' },
    serving: {
      clients: [
        {
          key: 'chat:20260926_19',
          kind: 'chat',
          work: null,
          sessionId: '20260926_19',
          sessionName: 'Jira Migration Kickoff Notes',
          count: 1,
        },
      ],
      unattributed: 0,
      swarmRuns: [],
      error: null,
    },
    startPhase: null,
    failedError: null,
    contact: null,
  };
}

/**
 * E2E #3i (3.0.60, 2026-09-27 17:23:25Z, Q-185): the reply was done and goose's end-of-turn fact
 * checker read its 1,094-token prompt on the split, 3 s in — for session 20260927_5, which the
 * sidebar lists as "Jira Migration Kickoff Notes · 5" beside an older "… · 4".
 */
export const FACT_CHECK_READING: MlxLiveRequest = {
  ...LIVE_WRITING,
  id: 'r-check',
  phase: 'prefill',
  elapsedS: 3,
  promptTokens: 1094,
  completionTokens: 0,
  tokensPerSecond: null,
  ttftS: null,
};

export function factCheckSnapshot(): MlxEngineSnapshot {
  const snapshot = liveSplitSnapshot([FACT_CHECK_READING]);
  return {
    ...snapshot,
    serving: {
      clients: [
        {
          key: 'chat:20260927_5:factCheck',
          kind: 'chat',
          work: 'factCheck',
          sessionId: '20260927_5',
          sessionName: 'Jira Migration Kickoff Notes',
          count: 1,
        },
      ],
      unattributed: 0,
      swarmRuns: [],
      error: null,
    },
  };
}
