# goose distributed rank: what rank 0 answers `/v1/status` with while requests run, in Rapid-MLX's
# own `/v1/status` shape (rapid_mlx/routes/health.py: `status`, `generation_tps`, `requests[]` with
# `request_id`, `status`, `phase` = queued | prefill | generation, `elapsed_s`, `prompt_tokens`,
# `completion_tokens`, `max_tokens`, `tokens_per_second`, `ttft_s`, `cached_tokens`), so the desktop
# reads a split engine with the parser it reads the single engine with. Two fields Rapid-MLX lacks:
# `prefilled_tokens` (how far into the prompt the prefill is) and `prompt_tokens_per_second` (the
# prefill's own rate) — the split is slow enough at reading that "reading" and "writing" must be
# told apart. Pure stdlib; concatenated after rank_env.py, before the rank program.


def prefill_position(ends, chunks, start=0, pad=0):
    """How far into one row's prompt the prefill is after `chunks` of the batch's prefill ranges.
    `ends` are the ranges' ends in the padded batch (the fork's `prefill_chunks`), `start` where
    the prefill began (a restored prefix-cache entry's length, else 0), `pad` the row's own left
    padding, which is no part of its prompt."""
    done = ends[min(chunks, len(ends)) - 1] if chunks and ends else start
    return max(0, done - pad)


def live_request(
    request_id,
    arrived,
    now,
    prompt_tokens=None,
    cached_tokens=None,
    max_tokens=None,
    prefill_started=None,
    prefilled=0,
    first_token=None,
    last_token=None,
    completion=0,
):
    """One in-flight request from its measured instants (monotonic seconds). A rate is None until
    it has a span to divide by: the prefill's from its first processed token, decode's from its
    second token (the first token's own time belongs to the prefill). `prefilled` counts the
    prompt position, a restored prefix included; the prefill rate counts only what was read."""
    if first_token is not None:
        phase = "generation"
    elif prefill_started is not None:
        phase = "prefill"
    else:
        phase = "queued"
    prefill_end = first_token if first_token is not None else now
    prompt_rate = None
    read = prefilled - (cached_tokens or 0)
    if prefill_started is not None and read > 0 and prefill_end > prefill_started:
        prompt_rate = read / (prefill_end - prefill_started)
    decode_rate = None
    if first_token is not None and last_token is not None and completion > 1:
        span = last_token - first_token
        if span > 0:
            decode_rate = (completion - 1) / span
    return {
        "request_id": request_id,
        "status": "waiting" if phase == "queued" else "running",
        "phase": phase,
        "elapsed_s": round(now - arrived, 3),
        "prompt_tokens": prompt_tokens,
        "prefilled_tokens": prefilled,
        "prompt_tokens_per_second": None if prompt_rate is None else round(prompt_rate, 2),
        "completion_tokens": completion,
        "max_tokens": max_tokens,
        "tokens_per_second": None if decode_rate is None else round(decode_rate, 2),
        "ttft_s": None if first_token is None else round(first_token - arrived, 3),
        "cached_tokens": cached_tokens,
    }


def live_status(base, requests):
    """The runner's own `/v1/status` fields (counters, slots, KV) plus the request table.
    `status` is Rapid-MLX's word: "generating" while any request is in flight (queued ones
    included — the counters count them too), else "idle". `generation_tps` sums the decode
    rates of the requests generating now — None when none is, never a stale figure."""
    rates = [
        r["tokens_per_second"]
        for r in requests
        if r["phase"] == "generation" and r["tokens_per_second"] is not None
    ]
    body = dict(base)
    body["status"] = "generating" if requests else "idle"
    body["generation_tps"] = round(sum(rates), 2) if rates else None
    body["requests"] = requests
    return body
