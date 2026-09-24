/**
 * Rapid-MLX `/v1/status` bodies for the state tile's tests.
 *
 * IDLE is VERBATIM from the running engine (Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx on
 * rapid-mlx v0.14.3-lz.1, `curl http://127.0.0.1:8090/v1/status`, 2026-09-23) — note the STICKY
 * generation_tps 19.91 on an idle engine. The in-flight bodies are that same body with `requests`
 * filled in the shape `scheduler.py get_running_requests_info` emits; the GENERATING request's
 * numbers are the owner's own capture from a live run (28,035 of 32,768 tokens, 19.9 tok/s); its
 * 165 s time to first token over a 32,277-token uncached prompt is the 195.6 tok/s prefill the
 * body's own sticky `prompt_tps` (195.5) says the engine measured for it.
 */
export const IDLE_STATUS = {
  status: 'idle',
  model: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  uptime_s: 874.3,
  steps_executed: 709,
  num_running: 0,
  num_waiting: 0,
  total_requests_processed: 5,
  total_prompt_tokens: 91743,
  total_completion_tokens: 672,
  generation_tps: 19.91,
  prompt_tps: 6463.49,
  adaptive_prefill: {
    chunk_size: 2048,
    protected_chunks: 28,
    reduced_chunks: 0,
  },
  mtp_prompt_lookup: {
    vendored_steps: 672,
    fallthrough_steps: 0,
    ft_batch_size: 0,
    ft_non_greedy: 0,
    ft_logits_processors: 0,
    ft_disabled: 0,
    gen_exhausted: 0,
    gen_raised: 0,
    invariant_violations: 0,
    prompt_lookup_proposals: 3.0,
    prompt_lookup_drafted_tokens: 6.0,
    prompt_lookup_matched_suffix_tokens: 25.0,
    prompt_lookup_accepted_tokens: 2.0,
    prompt_lookup_rejections: 3.0,
    prompt_lookup_cache_fallthroughs: 0.0,
    prompt_lookup_ev_declines: 0.0,
    prompt_lookup_mtp_sync_seconds: 0.01770737499828101,
    prompt_eval_seconds: 1.9082999642705545e-5,
    draft_seconds: 2.081213491012022,
    verify_sync_seconds: 27.093530885023938,
    verify_calls: 245.0,
  },
  idle_cache_clear: {
    enabled: false,
    seconds: 0.0,
    clear_count: 0,
    last_clear_at: null,
  },
  metal: {
    active_memory_gb: 54.28,
    peak_memory_gb: 55.07,
    cache_memory_gb: 0.02,
  },
  cache: {
    hits: 1,
    misses: 4,
    hit_rate: 0.2,
    evictions: 7,
    tokens_saved: 45056,
    current_memory_mb: 11389.44,
    max_memory_mb: 14718.75,
    current_memory_bytes: 11942690816,
    max_memory_bytes: 15433728000,
    memory_utilization: 0.7738,
    entry_count: 5,
    load_skipped: 0,
    save_drift_drops: 0,
    non_trimmable_skips: 0,
    non_trimmable_entries: 5,
    radix: {
      hits: 0,
      misses: 5,
      inserts: 21,
      removes: 7,
      deduped_prefix_bytes_saved: 951100,
      node_count: 46188,
      entry_count: 5,
      max_depth: 46011,
      lookup_p50_seconds: 2.792e-6,
      lookup_p99_seconds: 9.042e-6,
    },
  },
  requests: [],
};

export const GENERATING_STATUS = {
  ...IDLE_STATUS,
  status: 'generating',
  uptime_s: 1874.3,
  num_running: 2,
  num_waiting: 1,
  generation_tps: 19.9,
  prompt_tps: 195.5,
  metal: { active_memory_gb: 50.7, peak_memory_gb: 56.8, cache_memory_gb: 0.71 },
  cache: { ...IDLE_STATUS.cache, hit_rate: 0.78 },
  requests: [
    {
      request_id: 'req-waiting-3',
      status: 'waiting',
      phase: 'queued',
      elapsed_s: 4.2,
      prompt_tokens: 12004,
      completion_tokens: 0,
      max_tokens: 32768,
      progress: 0.0,
      tokens_per_second: null,
      ttft_s: null,
      cache_hit_type: null,
      cached_tokens: 0,
    },
    {
      request_id: 'req-gen-1',
      status: 'running',
      phase: 'generation',
      elapsed_s: 1574,
      prompt_tokens: 32277,
      completion_tokens: 28035,
      max_tokens: 32768,
      progress: 0.856,
      tokens_per_second: 19.9,
      ttft_s: 165,
      cache_hit_type: null,
      cached_tokens: 0,
    },
    {
      request_id: 'req-prefill-2',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 165.4,
      prompt_tokens: 32277,
      completion_tokens: 0,
      max_tokens: 32768,
      progress: 0.0,
      tokens_per_second: null,
      ttft_s: null,
      cache_hit_type: null,
      cached_tokens: 0,
    },
  ],
};

/** One request still reading its prompt, nothing generating yet — the long silent pre-fill. */
export const PREFILL_STATUS = {
  ...IDLE_STATUS,
  status: 'generating',
  uptime_s: 1880.1,
  num_running: 1,
  num_waiting: 0,
  requests: [GENERATING_STATUS.requests[2]],
};

/** A follow-up turn: most of its prompt came from the prefix cache, so the engine computed 2,400. */
export const CACHED_GENERATING_STATUS = {
  ...IDLE_STATUS,
  status: 'generating',
  uptime_s: 1900.5,
  num_running: 1,
  num_waiting: 0,
  requests: [
    {
      request_id: 'req-followup-4',
      status: 'running',
      phase: 'generation',
      elapsed_s: 30.0,
      prompt_tokens: 33000,
      completion_tokens: 240,
      max_tokens: 32768,
      progress: 0.007,
      tokens_per_second: 20.3,
      ttft_s: 12.0,
      cache_hit_type: 'prefix',
      cached_tokens: 30600,
    },
  ],
};

/**
 * The DISTRIBUTED engine's rank 0 `/v1/status` (rank_live.py over the pipeline fork): the runner's
 * own counters, slots and KV, plus the single engine's request table and the two fields Rapid-MLX
 * lacks — `prefilled_tokens` and `prompt_tokens_per_second`. No `uptime_s`, `metal` or `cache`: the
 * split reports none. READING is a 7,012-token prompt 2,048 tokens in; WRITING is the pipeline body
 * measured on 2026-09-24 (df7bdfff1): one request generating at 171.1 tok/s after a 5,503 tok/s
 * prefill, a second queued behind the KV admission.
 */
export const DIST_READING_STATUS = {
  num_running: 1,
  num_waiting: 0,
  slots: 2,
  slots_in_use: 1,
  sequences_in_flight: 1,
  status: 'generating',
  generation_tps: null,
  requests: [
    {
      request_id: 'dist-read-1',
      status: 'running',
      phase: 'prefill',
      elapsed_s: 14.2,
      prompt_tokens: 7012,
      prefilled_tokens: 2048,
      prompt_tokens_per_second: 152.4,
      completion_tokens: 0,
      max_tokens: 400,
      tokens_per_second: null,
      ttft_s: null,
      cached_tokens: null,
    },
  ],
};

export const DIST_WRITING_STATUS = {
  num_running: 1,
  num_waiting: 2,
  slots: 2,
  slots_in_use: 1,
  sequences_in_flight: 1,
  kv_reserved_bytes: [2061807632, 2114216960],
  kv_budget_bytes: [4123615264, 4254087168],
  status: 'generating',
  generation_tps: 171.1,
  requests: [
    {
      request_id: 'a52c969689464b6e88cbe1b5',
      status: 'running',
      phase: 'generation',
      elapsed_s: 1.209,
      prompt_tokens: 6546,
      prefilled_tokens: 6546,
      prompt_tokens_per_second: 5503.35,
      completion_tokens: 4,
      max_tokens: 80,
      tokens_per_second: 171.1,
      ttft_s: 1.19,
      cached_tokens: null,
    },
    {
      request_id: '58111b87b46a43b2982e9143',
      status: 'waiting',
      phase: 'queued',
      elapsed_s: 1.186,
      prompt_tokens: 6546,
      prefilled_tokens: 0,
      prompt_tokens_per_second: null,
      completion_tokens: 0,
      max_tokens: 80,
      tokens_per_second: null,
      ttft_s: null,
      cached_tokens: null,
    },
  ],
};
