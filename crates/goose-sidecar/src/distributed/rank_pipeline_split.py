# goose distributed rank: the mlx_lm PIPELINE split (layer ranges, `mlx_lm.server --pipeline`).
# Concatenated after rank_admission.py, before rank_wrapper.py (launch.rs TENSOR_PROGRAM); a spec
# without `pipeline_split` (every tensor launch) runs none of it. What it does, and why:
# - `import rapid_mlx.mlx_lm_models`: the Rapid-MLX fork's vendored architectures (mimo_v2 for
#   mlx-lm 0.31.3, which ships none); after it `mlx_lm.models.mimo_v2` resolves. A rank whose
#   interpreter cannot import it ends here, in words, before any load.
# - the split goose's preflight planned (layer_split.rs; s[r] = layers on rank r, rank 0 = the
#   LAST layers): mlx_lm's sharded_load calls `model.model.pipeline(group)` with no split, so the
#   vendored `MiMoV2Model.pipeline(group, split=None)` is given the planned one by default. The
#   shared PipelineMixin is NOT patched: the vendored class defines its own `pipeline`, which a
#   PipelineMixin patch never reaches. A split that does not sum to the model's layers is refused.
# - a logged CPU-stream barrier after the load (GOOSE_RANK_PIPELINE_LOADED / _BARRIER): the ranks
#   finish loading minutes apart (the mule reads ~100 GB over NFS); mlx_lm 0.31.3's sharded_load
#   already ends with one, and this one makes a stall at that point readable in the rank log.
# - a timed local GPU round trip and a second CPU barrier before the sampling seed (the first
#   GPU-stream collective): rank 1's Metal watchdog killed that seed on three loads in a row
#   (2026-10-27), GOOSE_RANK_PIPELINE_WARM says how long each GPU took to answer.
# - one rank-invariant prompt-cache byte count (`pipeline_cache_weights`): every rank runs its own
#   prompt cache over the same requests and must evict alike (a rank that reuses a different
#   prefix runs a different number of prefill steps, and the hidden states it sends no longer
#   pair up), but a pipeline rank holds only ITS layers' KV, so the raw byte counts differ by rank
#   and by layer kind. Each layer cache's `nbytes` is scaled by its kind's weight on this rank
#   (full attention: lcm(full layers per rank) / this rank's; sliding window likewise), so every
#   rank's sum is the same integer; the spec's limits and prefill figures are in the same unit.
#   The weights are >= 1, so the scaled count bounds the real bytes from above.

if spec.get("pipeline_split") is not None:
    import inspect  # noqa: E402

    pipeline_split = [int(n) for n in spec["pipeline_split"]]
    if len(pipeline_split) != spec["size"] or any(n <= 0 for n in pipeline_split):
        raise SystemExit(
            f"goose rank pipeline: split {pipeline_split} must give each of the {spec['size']} "
            "ranks at least one layer"
        )
    try:
        import rapid_mlx.mlx_lm_models  # noqa: E402,F401
        import mlx_lm.models.mimo_v2 as pipeline_arch  # noqa: E402
    except Exception as failure:
        raise SystemExit(
            f"goose rank pipeline: this interpreter ({sys.executable}) cannot import the fork's "
            f"vendored mlx_lm architectures (rapid_mlx.mlx_lm_models → mlx_lm.models.mimo_v2): "
            f"{type(failure).__name__}: {failure}. Install the Rapid-MLX pipeline env goose pins."
        ) from failure
    pipeline_class = pipeline_arch.MiMoV2Model
    if "split" not in inspect.signature(pipeline_class.pipeline).parameters:
        raise SystemExit(
            f"goose rank pipeline: {pipeline_class.__module__}.MiMoV2Model.pipeline takes no split "
            "(not the fork's vendored module): the planned split cannot be loaded"
        )
    original_pipeline = pipeline_class.pipeline
    pipeline_applied = []

    def pipeline_with_split(self, group, split=None):
        if sum(pipeline_split) != len(self.layers):
            raise SystemExit(
                f"goose rank pipeline: split {pipeline_split} covers {sum(pipeline_split)} layers, "
                f"the model has {len(self.layers)}"
            )
        original_pipeline(self, group, split if split is not None else pipeline_split)
        pipeline_applied.append((self.start_idx, self.end_idx))

    pipeline_class.pipeline = pipeline_with_split

    import mlx.core as mx  # noqa: E402
    import mlx_lm.server as pipeline_server  # noqa: E402

    original_sharded_load = pipeline_server.sharded_load

    def sharded_load_with_barrier(*args, **kwargs):
        started = time.monotonic()
        loaded = original_sharded_load(*args, **kwargs)
        if not pipeline_applied:
            raise SystemExit(
                "goose rank pipeline: the load never split the model's layers (no pipeline group, "
                "or not the mimo_v2 architecture): the planned split was not applied"
            )
        start, end = pipeline_applied[-1]
        emit(
            "RANK_PIPELINE_LOADED",
            {
                "rank": spec["rank"],
                "layers": [start, end],
                "split": pipeline_split,
                "seconds": round(time.monotonic() - started, 1),
            },
        )
        mx.eval(mx.distributed.all_sum(mx.array(1.0), stream=mx.cpu))
        emit("RANK_PIPELINE_BARRIER", {"rank": spec["rank"]})
        # Each rank's own GPU, alone, before the first GPU-stream collective (the sampling seed):
        # measured 2026-10-27, a seed entered 20 ms after this barrier on both ranks still left
        # rank 1's Metal command buffer waiting past the GPU watchdog ("GPU Timeout Error"), on
        # the GPU stream and with the all_sum itself on the CPU stream alike. The warm-up times a
        # local GPU round trip on the seed's own input, then a second CPU barrier waits for the
        # slower rank, so the seed starts only when both GPUs answer.
        warm_started = time.monotonic()
        mx.eval(mx.random.state[0] + 0)
        mx.synchronize()
        warm_seconds = round(time.monotonic() - warm_started, 2)
        mx.eval(mx.distributed.all_sum(mx.array(1.0), stream=mx.cpu))
        emit(
            "RANK_PIPELINE_WARM",
            {
                "rank": spec["rank"],
                "gpu_seconds": warm_seconds,
                "barrier_seconds": round(time.monotonic() - warm_started - warm_seconds, 2),
            },
        )
        return loaded


    pipeline_server.sharded_load = sharded_load_with_barrier

    weights = spec.get("pipeline_cache_weights")
    if weights is None:
        raise SystemExit(
            "goose rank pipeline: the spec carries a split but no pipeline_cache_weights: the ranks' "
            "prompt caches could not evict alike"
        )
    from mlx_lm.models import cache as pipeline_cache  # noqa: E402

    def weigh(cls, weight):
        raw = cls.nbytes

        def nbytes(self):
            return raw.fget(self) * weight

        cls.nbytes = property(nbytes)

    full_weight, swa_weight = int(weights["full"]), int(weights["swa"])
    for name in ("KVCache", "BatchKVCache"):
        weigh(getattr(pipeline_cache, name), full_weight)
    for name in ("RotatingKVCache", "BatchRotatingKVCache"):
        weigh(getattr(pipeline_cache, name), swa_weight)
    emit(
        "RANK_PIPELINE_SPLIT",
        {"rank": spec["rank"], "split": pipeline_split, "cache_weights": weights},
    )
