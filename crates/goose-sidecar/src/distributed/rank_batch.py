# goose distributed tensor rank: reading mlx_lm 0.31.3's batch (models/cache.py, generate.py) for
# the prefill plan (rank_prefill.py). Concatenated after rank_prefill.py, before rank_wrapper.py,
# which installs it; importable on its own beside a real mlx_lm (launch.rs's tests run it there).
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
