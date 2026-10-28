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
# - the tool parser for a template that writes `<tool_call><function=` (MiMo): qwen3_coder, where
#   the model's tokenizer_config.json names json_tools.
# - MiMo's agent turns think (rank_thinking.py `TOOL_TURNS_THINK`): the card's default, Xiaomi's API
#   and the cloud baseline all think on tool-bearing requests; the single engine's
#   thinking-off-on-tools rule made a local run a different, non-thinking model.
# - one rank-invariant prompt-cache byte count (`pipeline_cache_weights`): every rank runs its own
#   prompt cache over the same requests and must evict alike (a rank that reuses a different
#   prefix runs a different number of prefill steps, and the hidden states it sends no longer
#   pair up), but a pipeline rank holds only ITS layers' KV, so the raw byte counts differ by rank
#   and by layer kind. Each layer cache's `nbytes` is scaled by its kind's weight on this rank
#   (full attention: lcm(full layers per rank) / this rank's; sliding window likewise), so every
#   rank's sum is the same integer; the spec's limits and prefill figures are in the same unit.
#   The weights are >= 1, so the scaled count bounds the real bytes from above.



def evict_file_cache(path):
    """Drops `path`'s pages from the macOS file cache: msync(MS_INVALIDATE) over a shared
    read-only mapping of the whole file (no root, no write). Measured 2026-10-27 on a 4.66 GB
    MiMo shard: file-backed pages 28.94 -> 24.28 GB. Returns the bytes asked to drop (0 when
    the file could not be mapped; the load goes on, only slower to free its cache)."""
    import ctypes
    import os

    libc = ctypes.CDLL(None, use_errno=True)
    libc.mmap.restype = ctypes.c_void_p
    libc.mmap.argtypes = [
        ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_long,
    ]
    libc.msync.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
    libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    fd = os.open(path, os.O_RDONLY)
    try:
        size = os.fstat(fd).st_size
        if size == 0:
            return 0
        prot_read, map_shared, ms_invalidate = 0x1, 0x1, 0x2
        addr = libc.mmap(None, size, prot_read, map_shared, fd, 0)
        if addr in (None, ctypes.c_void_p(-1).value):
            return 0
        try:
            return size if libc.msync(ctypes.c_void_p(addr), size, ms_invalidate) == 0 else 0
        finally:
            libc.munmap(ctypes.c_void_p(addr), size)
    finally:
        os.close(fd)


def shard_groups(names, weight_map):
    """The parameter names grouped by the checkpoint file that holds them, in file order; names
    the index does not list come last as one group (evaluated, never evicted)."""
    groups = {}
    for name in names:
        groups.setdefault(weight_map.get(name), []).append(name)
    files = sorted(f for f in groups if f is not None)
    return [(f, groups[f]) for f in files] + ([(None, groups[None])] if None in groups else [])


def checkpoint_weight_map(weight_map):
    """The checkpoint's weight index, also keyed by the names the vendored model's parameters carry.
    An mlx-vlm-layout omni build (EigenLabs MiMo-V2.6-Flash-MOPD) indexes `language_model.model.*`
    and `language_model.lm_head.*`, which the fork's sanitize strips on load; mlx_lm's sharded_load
    looks every parameter up by its stripped name and refused the build ("Pipeline loading is only
    supported for MLX converted models", 2026-10-28), and the shard-by-shard eviction found no file.
    Names the index already carries keep their entry."""
    mapped = dict(weight_map)
    for name, file in weight_map.items():
        if name.startswith("language_model."):
            mapped.setdefault(name[len("language_model."):], file)
    return mapped


class CheckpointIndexJson:
    """mlx_lm.utils' `json` while sharded_load runs: `load` hands back the index with
    `checkpoint_weight_map` applied; everything else is the json module."""

    def load(self, fid, *args, **kwargs):
        data = json.load(fid, *args, **kwargs)
        if isinstance(data, dict) and isinstance(data.get("weight_map"), dict):
            data["weight_map"] = checkpoint_weight_map(data["weight_map"])
        return data

    def __getattr__(self, name):
        return getattr(json, name)


def pipeline_load_turn(rank, size):
    """(barriers before, barriers after) this rank's load: the ranks load one at a time, the last
    rank first and rank 0 last. Every rank joins `size - 1` CPU barriers around its load (plus
    mlx_lm's own one at the end of `sharded_load`), so barrier k completes when rank `size-1-k`
    has loaded and lets rank `size-2-k` start. Why (2026-10-27): rank 0's Mac (the workhorse)
    also serves the model's files to the others over NFS, and a loaded MLX array is not wired
    until the GPU uses it: rank 0's weights sitting idle while the mule's reads filled the
    workhorse's file cache were compressed (12:44, rank 0 first: wired 9 GB, compressor 55 GB,
    swap +3.2 GB). Rank 0 now loads last, after dropping the folder's cached pages
    (`evict_file_cache`), and serves at once."""
    return size - 1 - rank, rank


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
    # MiMo-V2.6's chat template writes qwen3_coder's XML (`<tool_call><function=NAME><parameter=..`)
    # but its tokenizer_config.json names `"tool_parser_type": "json_tools"` (and mlx_lm 0.31.3's
    # own inference would miss the no-newline form too), so every tool call the model made was
    # unparsed (2026-10-27: GOOSE_RANK_TOOL_CALL_UNPARSED "JSONDecodeError: Expecting value: line
    # 1 column 1"; goose's coding prompt got four empty responses in a row). Both parsers share
    # `<tool_call>`/`</tool_call>`, so only the parse function moves; the template decides, so a
    # model whose template lacks that XML is untouched.
    import mlx_lm.utils as pipeline_utils  # noqa: E402
    from mlx_lm.tool_parsers import qwen3_coder as pipeline_xml_tools  # noqa: E402

    original_load_tokenizer = pipeline_utils.load_tokenizer

    def load_tokenizer_reading_template(*args, **kwargs):
        tokenizer = original_load_tokenizer(*args, **kwargs)
        template = getattr(tokenizer, "chat_template", None)
        configured = getattr(tokenizer.tool_parser, "__module__", None)
        if (
            isinstance(template, str)
            and "<tool_call><function=" in template
            and tokenizer.tool_parser is not pipeline_xml_tools.parse_tool_call
        ):
            tokenizer._tool_parser = pipeline_xml_tools.parse_tool_call
            emit(
                "RANK_PIPELINE_TOOL_PARSER",
                {"rank": spec["rank"], "configured": configured, "used": "qwen3_coder"},
            )
        return tokenizer

    pipeline_utils.load_tokenizer = load_tokenizer_reading_template

    TOOL_TURNS_THINK = True

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

    from pathlib import Path  # noqa: E402

    import mlx.core as mx  # noqa: E402
    import mlx_lm.server as pipeline_server  # noqa: E402

    original_sharded_load = pipeline_server.sharded_load

    def sharded_load_with_barrier(*args, **kwargs):
        before, after = pipeline_load_turn(spec["rank"], spec["size"])
        waited = time.monotonic()
        for _ in range(before):
            mx.eval(mx.distributed.all_sum(mx.array(1.0), stream=mx.cpu))
        emit(
            "RANK_PIPELINE_LOAD_TURN",
            {"rank": spec["rank"], "waited_seconds": round(time.monotonic() - waited, 1)},
        )
        started = time.monotonic()
        model_dir = Path(args[0]) if args and Path(str(args[0])).is_dir() else None
        weight_map = (
            checkpoint_weight_map(
                json.loads((model_dir / "model.safetensors.index.json").read_text())["weight_map"]
            )
            if model_dir is not None and (model_dir / "model.safetensors.index.json").exists()
            else None
        )
        upstream_eval = mx.eval
        evicted = {"files": 0, "bytes": 0}

        def eval_shard_by_shard(*arrays):
            # mlx_lm's sharded_load ends with ONE mx.eval(model.parameters()): every shard's
            # pages sit in the file cache beside their copies until it returns, and on the
            # workhorse (96 GB, a 68 GB stage) the kernel compressed the fresh weights instead
            # (compressor 53 GB, swap +2.3 GB, 2026-10-27 12:28 with rank 0 loading alone).
            # Each shard is evaluated, wired, and then dropped from the file cache.
            if weight_map is None or len(arrays) != 1 or not isinstance(arrays[0], dict):
                return upstream_eval(*arrays)
            from mlx.utils import tree_flatten

            flat = dict(tree_flatten(arrays[0]))
            for name, names in shard_groups(list(flat), weight_map):
                upstream_eval([flat[n] for n in names])
                # A loaded array is not wired until a GPU command buffer uses it (measured: a
                # 4.8 GB shard left wired memory at 10.2 GB, a one-element GPU sum per array
                # took it to 14.7): until then the kernel may compress it. Touch each now.
                upstream_eval([mx.sum(flat[n].reshape(-1)[:1]) for n in names])
                if name is not None:
                    dropped = evict_file_cache(str(model_dir / name))
                    evicted["files"] += 1 if dropped else 0
                    evicted["bytes"] += dropped
            return None

        if model_dir is not None:
            # The others' reads of this folder (rank 0's Mac serves it over NFS) left it in the
            # file cache: drop it before this rank's weights need the room.
            for shard in sorted(model_dir.glob("*.safetensors")):
                dropped = evict_file_cache(str(shard))
                evicted["before_bytes"] = evicted.get("before_bytes", 0) + dropped
        mx.eval = eval_shard_by_shard
        utils_json = pipeline_utils.json
        pipeline_utils.json = CheckpointIndexJson()
        try:
            loaded = original_sharded_load(*args, **kwargs)
        finally:
            mx.eval = upstream_eval
            pipeline_utils.json = utils_json
        emit("RANK_PIPELINE_FILE_CACHE", {"rank": spec["rank"], **evicted})
        for _ in range(after):
            mx.eval(mx.distributed.all_sum(mx.array(1.0), stream=mx.cpu))
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

    # Diagnostic (2026-10-28, the 2-row JACCL desync): with /tmp/goose-pipe-trace present on a
    # node, each collective this rank BUILDS is appended to /tmp/goose-pipe-trace-rank<r>.log
    # (sequence, time, op, shape, dtype, thread), so two ranks' sequences can be diffed.
    if os.path.exists("/tmp/goose-pipe-trace"):
        pipe_trace = open(f"/tmp/goose-pipe-trace-rank{spec['rank']}.log", "a", buffering=1)
        pipe_trace_seq = [0]

        def pipe_traced(name, fn):
            def traced(x, *args, **kwargs):
                pipe_trace_seq[0] += 1
                # mlx_lm also passes plain Python numbers (`all_sum(0)`, `all_sum(data.size)`).
                pipe_trace.write(
                    f"{pipe_trace_seq[0]} {time.time():.3f} {name} "
                    f"{tuple(getattr(x, 'shape', ()))} {getattr(x, 'dtype', type(x).__name__)} "
                    f"{threading.current_thread().name}\n"
                )
                return fn(x, *args, **kwargs)

            return traced

        for pipe_op in ("send", "recv_like", "all_gather", "all_sum"):
            setattr(mx.distributed, pipe_op, pipe_traced(pipe_op, getattr(mx.distributed, pipe_op)))

    full_weight, swa_weight = int(weights["full"]), int(weights["swa"])
    for name in ("KVCache", "BatchKVCache"):
        weigh(getattr(pipeline_cache, name), full_weight)
    for name in ("RotatingKVCache", "BatchRotatingKVCache"):
        weigh(getattr(pipeline_cache, name), swa_weight)
    emit(
        "RANK_PIPELINE_SPLIT",
        {
            "rank": spec["rank"],
            "split": pipeline_split,
            "cache_weights": weights,
            "tool_turns_think": TOOL_TURNS_THINK,
        },
    )
