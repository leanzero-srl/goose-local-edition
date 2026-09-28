# goose distributed tensor rank: reading mlx_lm 0.31.3's batch (models/cache.py, generate.py) for
# the prefill plan (rank_prefill.py). Concatenated after rank_prefill.py, before rank_wrapper.py,
# which installs it; importable on its own beside a real mlx_lm (launch.rs's tests run it there).
import inspect  # noqa: E402

import mlx.core as mx  # noqa: E402
from mlx_lm.models.cache import ArraysCache, BatchKVCache, KVCache  # noqa: E402


def cache_width(caches):
    """The tokens a layer cache list holds (a batch's: the longest row's, padding included)."""
    for cache in caches:
        if isinstance(cache, (KVCache, BatchKVCache)):
            return cache.size()
    return 0


def batch_shape(batch):
    """(rows, width) of a BatchGenerator: every row it holds — queued, prefilling, generating —
    and the width its KV reaches once every queued prompt is read (the prefilling batch at its
    padded width plus the longest prompt left; a queued row at its cached prefix plus its
    prompt). mlx_lm pads every row of a batch to the longest, so this is what each row costs."""
    rows = len(batch._generation_batch) + len(batch._prompt_batch)
    rows += len(batch._unprocessed_sequences)
    width = cache_width(batch._generation_batch.prompt_cache)
    left = max((sum(map(len, row[0])) for row in batch._currently_processing), default=0)
    width = max(width, cache_width(batch._prompt_batch.prompt_cache) + left)
    for row in batch._unprocessed_sequences:
        width = max(width, cache_width(row[3]) + sum(map(len, row[1])))
    return rows, width


# Q-447: mlx_lm 0.31.3's BatchKVCache.extend pads a side that holds no KV yet with
# `mx.array([])` — FLOAT32 — and concatenates it with the other side's bfloat16 KV, so the whole
# batch's KV becomes float32. That happens whenever a request whose prompt is read cold joins a
# prompt batch another row has already stepped (or a row with KV joins one that has not). From
# there the attention output, the residual stream and every later layer run in float32, every
# prompt-cache entry the row leaves is float32 (twice the bytes per token), and every request
# restored from one inherits it (BatchKVCache.merge keeps the first row's dtype) — through the
# chat's conversation prefix and its kept stable head, for as long as the launch lives. The 27B
# tensor split's 22:57 launch of 2026-09-28 (rank0 log ...1790625429786): the first chat request
# (42,642 tokens, cold) joined a 206-token helper after the helper's first step; its stable head
# measured 2,816,448,512 B = 41,780 x 65,536 + 78,354,432 (3.0.70's head: 1.40 GB = 40,421 x
# 32,768 + 76,972,032) and every chat turn decoded 4.05-4.19 tok/s, until a two-row merge led by
# a bfloat16 row cast the chat back (20:29:54Z): 10.35-12.26 tok/s on the same chat, its new
# entries back at 32,768 B per token.
def extend_in_its_dtype(cache, other, upstream_extend):
    """BatchKVCache.extend with a side that holds no KV padded in the other side's dtype: the
    same shapes, padding and values as upstream, never a float32 promotion."""
    filled = [c for c in (cache, other) if c.keys is not None]
    if len(filled) == 1:
        keys, values = filled[0].keys, filled[0].values
        for empty in (c for c in (cache, other) if c.keys is None):
            rows = empty.offset.shape[0]
            empty.keys = mx.zeros((rows, keys.shape[1], 0, keys.shape[3]), dtype=keys.dtype)
            empty.values = mx.zeros((rows, values.shape[1], 0, values.shape[3]), dtype=values.dtype)
    return upstream_extend(cache, other)


def kv_dtypes(caches):
    """The distinct dtypes of the KV the given layer caches hold (names, sorted)."""
    return sorted({str(c.keys.dtype) for c in caches if getattr(c, "keys", None) is not None})


def settle_left_padding(cache):
    """BatchKVCache.filter's shift without its gather: drop the left padding every row shares."""
    shared = cache.left_padding.min().item()
    if shared > 0:
        if cache.keys is not None:
            cache.keys = cache.keys[..., shared:, :]
            cache.values = cache.values[..., shared:, :]
        cache._idx -= shared
        cache.left_padding -= shared


def moving_split(batch, indices, upstream_split):
    """PromptProcessingBatch.split, moving the caches when every row leaves. mlx_lm deep-copies
    the whole batch's caches and filters both copies; a lone request reaching generation (or a
    batch finishing its prompts together) then holds its KV three times over for a moment —
    measured on the 27B's rank geometry, a 16,384-token row: 1.23 GB peak for 0.61 GB of KV. The
    moved batch is the one upstream returns (its caches filtered to every row, in order), the
    caches not copied. A partial split, or a cache kind this does not know, is upstream's."""
    everyone = list(range(len(batch.uids)))
    if sorted(indices) != everyone or not all(
        isinstance(c, (BatchKVCache, ArraysCache)) for c in batch.prompt_cache
    ):
        return upstream_split(batch, indices)
    caches = batch.prompt_cache
    moved = batch.__class__.__new__(batch.__class__)
    moved.__dict__.update(batch.__dict__)
    for name in ("uids", "tokens", "samplers", "logits_processors", "state_machines", "max_tokens"):
        setattr(moved, name, list(getattr(batch, name)))
    moved.prompt_cache = []
    moved.filter(everyone)
    for cache in caches:
        if isinstance(cache, BatchKVCache):
            settle_left_padding(cache)
        else:
            cache.filter(everyone)
    moved.prompt_cache = caches
    batch.prompt_cache = []
    batch.filter([])
    return moved


# Q-161: each generating row runs ITS OWN logits processors. mlx_lm 0.31.3's GenerationBatch.filter
# filters `logits_processors` only `if any(self.logits_processors)`: when every row left carries
# none (a tool-less request's `[]`, or the `None` PromptProcessingBatch.extend fills in for one),
# the departed rows' entries stay. The next request to join is appended AFTER them, so its row
# reads a departed row's list, and its own processors run only on the token its own
# GenerationBatch sampled on arrival. E2E #3f turn 0 (2026-09-27, 3.0.57): goose's title request
# (474 tokens, no tools, POST 11:52:24) generated and left; the tool request (40,537 tokens, POST
# 11:52:27) joined, read the title's `[]`, and the XML skeleton guard never ran again — the answer
# left its call as `</tool_call>` + `!` into 3,251 chars of `!\n</parameter>\n</function>\n…`.
# A departed `None` beside a row that carries processors is iterated instead:
# `TypeError: 'NoneType' object is not iterable` in the generation thread.
def row_processors(processors, uids):
    """One list per row: `[]` for a row that has none."""
    if not processors:
        return [[] for _ in uids]
    return [row or [] for row in processors]


def generation_init(upstream_init):
    """GenerationBatch.__init__, handed `row_processors` of its logits processors."""
    signature = inspect.signature(upstream_init)
    if not {"logits_processors", "uids"} <= signature.parameters.keys():
        raise SystemExit(
            f"goose rank wrapper: mlx_lm's GenerationBatch.__init__{signature} takes no "
            "logits_processors/uids; the per-row processors were written against mlx_lm 0.31.3"
        )

    def __init__(self, *args, **kwargs):
        bound = signature.bind(self, *args, **kwargs)
        bound.arguments["logits_processors"] = row_processors(
            bound.arguments.get("logits_processors"), bound.arguments["uids"]
        )
        upstream_init(*bound.args, **bound.kwargs)

    return __init__


def generation_filter(upstream_filter):
    """GenerationBatch.filter, keeping exactly the kept rows' processors."""

    def filter(self, keep):
        kept = [self.logits_processors[i] for i in keep]
        upstream_filter(self, keep)
        self.logits_processors = kept

    return filter
