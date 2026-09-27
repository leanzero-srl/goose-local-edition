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
    baseUrl: 'http://127.0.0.1:8091',
    stats: liveStats(requests),
    statusDetail: null,
    measured: { kind: 'pending' },
    serving: {
      clients: [
        {
          key: 'chat:20260926_19',
          kind: 'chat',
          sessionId: '20260926_19',
          sessionName: 'Jira Migration Kickoff Notes',
          count: 1,
        },
      ],
      unattributed: 0,
      swarmRuns: [],
      error: null,
    },
    failedError: null,
    contact: null,
  };
}
