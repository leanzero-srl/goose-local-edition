# goose distributed tensor rank: what a prefill step and a batch may hold, from the plan's figures
# (spec["prefill"], plan.rs TensorPrefill). Pure stdlib; concatenated after rank_budget.py, before
# rank_batch.py (which reads mlx_lm's batch for it) and rank_wrapper.py (which installs both).
#
# Q-104 (measured 2026-09-26 on 2 localhost ranks of the owner's 27B): a prefill step's peak is
# the batch's padded KV plus the attention scores of the full-attention layer it is computing —
# MLX 0.32.2 has no fused prefill kernel for head_dim 256, so rows × query heads × chunk × width
# scores exist at once (chunk 2,048 at width 135,168: 6.6 GB for ONE row) — and mlx_lm pads every
# row of a batch to the longest (E2E #2's four summaries rode an agent turn's ~50k-token width).
# Neither was in the plan: a batched prefill beside a 50k-token turn raised a rank 35 → 44 GB in
# one second (Q-79's repro). Two rules keep a rank inside its plan:
# - every step's chunk is sized so its scores fit the plan's workspace (`prefill_chunk`): the plan
#   charged one row's chunk at the full context, so wider batches take smaller chunks;
# - rank 0 admits a request only while the batch it would join keeps its padded KV — times what
#   mlx_lm's batch operations transiently hold of it — inside the plan's KV charge
#   (`batch_kv_charge`, `admits`); the prompt cache yields to it.


def prefill_chunk(prefill, rows, width, kv_step):
    """The tokens one prefill step of `rows` rows reaching `width` tokens may take: the plan's
    workspace over what one chunk token costs them, a whole number of KV steps when at least one
    fits, at most the plan's step. Never below one token (see `chunk_overruns`)."""
    per_token = rows * width * prefill["pair_bytes"]
    chunk = min(prefill["step"], prefill["workspace_bytes"] // max(per_token, 1))
    if chunk >= kv_step:
        chunk -= chunk % kv_step
    return max(1, chunk)


def chunk_overruns(prefill, rows, width, chunk):
    """The bytes a step of `chunk` tokens would materialize past the workspace (0 when inside)."""
    return max(0, rows * width * chunk * prefill["pair_bytes"] - prefill["workspace_bytes"])


def batch_kv_charge(prefill, rows, width):
    """What a batch of `rows` rows padded to `width` tokens holds at its peak: its KV (every row at
    the longest row's width, and each row's recurrent state), times what mlx_lm's merge / extend /
    split transiently hold of it once there are two rows or more. A lone row holds its KV once."""
    padded = rows * (width * prefill["kv_bytes_per_token"] + prefill["sequence_state_bytes"])
    return padded if rows <= 1 else int(padded * prefill["batch_transient_ratio"])


def admits(prefill, limit_bytes, rows, width, prompt_tokens):
    """Whether a request of `prompt_tokens` may join a live batch of `rows` rows at `width`: an idle
    engine always takes it (the plan holds one row up to the window), a busy one only while the
    batch it would join stays inside `limit_bytes` (the plan's whole KV charge, live + cached)."""
    if rows == 0:
        return True
    charge = batch_kv_charge(prefill, rows + 1, max(width, prompt_tokens))
    return charge <= limit_bytes
