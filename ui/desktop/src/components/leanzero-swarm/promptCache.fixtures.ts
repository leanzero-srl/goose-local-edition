/**
 * `/v1/status` bodies for a prompt read split by the prefix cache (Q-337). The prompt and cached
 * figures are measured: E2E #3p's turn 7 (27B tensor split, calls.tsv) read 113,824 of 114,948
 * tokens from the cache; E2E #3o (Q-294) read turn 6 with 40,244 of 102,210 cached and turn 5 cold
 * (0 of 109,655). The position inside the new part and the elapsed seconds are the fixture's, at the
 * 27B split's measured cold reading rate (412 tok/s, goose-mlx-inference skill). `prefilled_tokens`
 * is the prompt POSITION, the restored prefix included (rank_live.py; Q-338 made the tensor split
 * report it that way too).
 */
const splitRow = (over: Record<string, unknown>) => ({
  request_id: 'turn-read',
  status: 'running',
  phase: 'prefill',
  elapsed_s: 4.1,
  prompt_tokens: 114948,
  prefilled_tokens: 114400,
  prompt_tokens_per_second: 412,
  completion_tokens: 0,
  max_tokens: 32768,
  tokens_per_second: null,
  ttft_s: null,
  cached_tokens: 113824,
  client: '127.0.0.1:50110',
  held_for_room: false,
  stopped: null,
  stopped_after_s: null,
  leaving: false,
  ...over,
});

const splitBody = (row: Record<string, unknown>) => ({
  num_running: 1,
  num_waiting: 0,
  status: 'generating',
  generation_tps: null,
  requests: [row],
});

/** #3p turn 7: 114,948 tokens, 113,824 from the cache, 576 of the 1,124 new ones read. */
export const SPLIT_WARM_READ = splitBody(splitRow({}));

/** #3o turn 6: 102,210 tokens, 40,244 from the cache, 22,528 of the 61,966 new ones read. */
export const SPLIT_PARTLY_CACHED_READ = splitBody(
  splitRow({
    elapsed_s: 54.7,
    prompt_tokens: 102210,
    cached_tokens: 40244,
    prefilled_tokens: 62772,
  })
);

/** #3o turn 5: nothing cached — 45,056 of 109,655 tokens read. */
export const SPLIT_COLD_READ = splitBody(
  splitRow({
    elapsed_s: 109.4,
    prompt_tokens: 109655,
    cached_tokens: 0,
    prefilled_tokens: 45056,
  })
);

/**
 * Q-498, E2E #3x (rank 0's log, 2026-09-29 15:17:00 local): #3x's next call, 200,456 tokens, read
 * cold — 91,689 read, 653 s after it arrived — because the second chat's 84-token side call joined
 * that chat's 77,683-token row at 15:10:16 and the cache was trimmed below #3x's 199,798-token
 * prefix while it kept the second chat's. rank 0 names it (`evicted_prefix`, rank_boundary.py
 * `EvictionLog.lost_prefix`: 42.93 s before the lookup, a batch of 2 rows at 77,683).
 */
export const SPLIT_EVICTED_READ = splitBody(
  splitRow({
    elapsed_s: 653.0,
    prompt_tokens: 200456,
    cached_tokens: 0,
    prefilled_tokens: 91689,
    prompt_tokens_per_second: 254.1,
    evicted_prefix: {
      tokens: 199798,
      while_keeping: 'another_conversation',
      rows: 2,
      width: 77683,
      requests: ['req-424', 'req-425'],
      ago_s: 42.93,
    },
  })
);

/** The same miss where the cache kept no other chat's prefix (no conversation kept at all). */
export const SPLIT_EVICTED_FOR_ROOM_READ = splitBody(
  splitRow({
    elapsed_s: 653.0,
    prompt_tokens: 200456,
    cached_tokens: 0,
    prefilled_tokens: 91689,
    prompt_tokens_per_second: 254.1,
    evicted_prefix: {
      tokens: 199798,
      while_keeping: null,
      rows: 1,
      width: 150,
      requests: ['req-430'],
      ago_s: 12.5,
    },
  })
);

/** The same read before rank 0 has looked the prompt up: nothing is known of the cache. */
export const SPLIT_UNKNOWN_READ = splitBody(
  splitRow({ cached_tokens: null, prefilled_tokens: 0, prompt_tokens_per_second: null })
);

/**
 * Rapid-MLX's single engine reading the #3p turn: its row carries `cached_tokens` once the prefix
 * cache was looked up (`cache_hit_type` set) — and no position and no prefill rate, so its split
 * is known and its progress is not.
 */
export const SINGLE_WARM_READ = {
  status: 'generating',
  model: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  uptime_s: 2210.4,
  num_running: 1,
  num_waiting: 0,
  requests: [
    {
      request_id: 'single-turn-read',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 3.2,
      prompt_tokens: 114948,
      completion_tokens: 0,
      max_tokens: 32768,
      progress: 0.0,
      tokens_per_second: null,
      ttft_s: null,
      cache_hit_type: 'prefix',
      cached_tokens: 113824,
    },
  ],
};

/** The single engine's row before its lookup: `cache_hit_type` null, `cached_tokens` its default 0. */
export const SINGLE_UNLOOKED_READ = {
  ...SINGLE_WARM_READ,
  requests: [{ ...SINGLE_WARM_READ.requests[0], cache_hit_type: null, cached_tokens: 0 }],
};
