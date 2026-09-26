# goose distributed tensor rank: one tensor-parallel rank of mlx_lm.server. Runs after
# rank_env.py (the shared prelude: `spec`, `emit`, the backend env, `mx`, `report_memory`).
#
# What it adds around mlx_lm.server, and why:
# - in-process memory caps at THIS node's GPU ceiling (max_recommended_working_set_size — the
#   budget preflight planned against is at most that), applied after mlx_lm.server's own
#   set_wired_limit, so an over-allocation raises in MLX instead of the kernel wiring past the
#   ceiling (exo panicked a 96 GB M3 Ultra that way); MLX's free-buffer cache at the plan's
#   transient allowance (Q-79), not at the ceiling's remainder;
# - every prompt-cache entry owns exactly its bytes (Q-79, `compact`): mlx_lm's extracted
#   entries kept whole padded batch buffers alive while counting one row, so the cache's byte
#   bound was enforced on a figure several times smaller than what it held;
# - a generation thread that dies ends the rank with RANK_FATAL and a non-zero exit (Q-79: the
#   Studio rank's Metal OOM exited 0);
# - rank 0's HTTP surface: /v1/models serves ONLY the goose model's names — its id, then every
#   other name goose's identity gives the same model (mlx_lm lists the HF cache, and a request
#   naming any of those would make every rank try to load it), the goose id maps to each rank's
#   OWN --model path (paths differ per node) and an alias is answered as the id, /goose/progress exposes the
#   generation loop's step counter (the supervisor's liveness measure), /goose/admission lets the
#   memory watchdog stop admitting new requests;
# - the generation budget (rank_budget.py): a request with no max_tokens generates until the model
#   stops or the launch's context window is full, never to mlx_lm's 512 default; an explicit one is
#   held inside the window. Rank 0 settles it BEFORE the request is shared, so every rank receives
#   the same integer — a peer running an older wrapper (its own goosed embeds its program) still
#   reads an int, never an absence it would crash on;
# - the doorbell (Q-66): mlx_lm's generation thread polls its request queue every 0.1 s while
#   idle and shares the (empty) answer through an all_sum every time; JACCL waits on a completion
#   queue created WITHOUT a completion channel (jaccl/rdma.cpp `create_cq(..., nullptr, nullptr,
#   0)`), so a waiting rank spins `ibv_poll_cq` — the worker ranks, parked in that all_sum while
#   rank 0 sleeps in queue.get, burned a whole core idle (Studio ~100%, MacBook 0.1%). With the
#   doorbell, an idle rank 0 shares only a request that exists, ringing one byte per worker first,
#   and an idle worker parks in recv(1) (kernel-blocking, no clock). While a batch runs every step
#   is shared exactly as upstream. The fork's pipeline runner has done the same since 286ed77f7;
# - the prompt cache's bounds (plan.rs RankPlan::prompt_cache_limit_bytes / prompt_cache_entries):
#   mlx_lm 0.31.3 trims its LRU prompt cache to `--prompt-cache-bytes` MINUS the live batch's KV,
#   and only when a request is admitted (server.py:795-798); `run` builds the cache without
#   max_bytes (server.py:1743), so the inserts between admissions are unbounded, and the default
#   `--prompt-cache-size` (10) evicts by entry count on its own. goose hands the flag the plan's
#   whole KV charge (live + cached), builds the cache with the same `max_bytes`, and sizes the
#   count so it never evicts before the bytes do. Every rank gets the same two numbers: each rank
#   runs its own cache over the same requests, and one that evicted differently would reuse a
#   different prefix than its peers (E2E #1, 2026-09-25: the agent's 48,647-token system prefix
#   was evicted and a 55,977-token turn was read cold, ~2.5 min on the split). An older
#   requester's spec (`prompt_cache_bytes` alone) runs its own wrapper's policy here unchanged:
#   that one number as the flag, mlx_lm's default count, no max_bytes — so both ranks still evict
#   alike.
# - the prefill's memory (Q-104, rank_prefill.py): a launch whose spec carries `prefill` sizes
#   every prefill step's chunk to the plan's workspace (the attention scores of a head_dim-256
#   layer exist whole: rows × heads × chunk × width), moves a batch whose every row finished its
#   prompt into generation without mlx_lm's deep copy of the whole padded KV, charges the live
#   batch at its PROJECTED padded KV when the prompt cache yields room, and rank 0 holds a request
#   the batch it would join cannot fit inside the plan's KV charge until the batch drains enough.
# - the rank's own account of its loop (Q-114, rank_state.py): its step count, where the loop waits
#   (poll / doorbell / share / batch), the rings, the batch's rows and each generating row's token
#   trail, published at every step and printed by the reporter; and, on SIGTERM, every thread's
#   Python stack. Both land in the rank's durable log (goosed's rank_log.rs), so a stall leaves each
#   rank's position behind — Q-114's could not be told apart for want of rank 1's.
group = mx.distributed.init(strict=True, backend=spec["backend"])
emit("RANK_GROUP", {"rank": group.rank(), "size": group.size(), "mlx": mx.__version__})
# MLX's counters from before the load, so the weights arriving are the load's progress (measured
# 2026-09-24: a reporter thread read 21.9 of 31.0 GB active mid `mx.eval(model.parameters())` —
# the eval releases the GIL).
threading.Thread(target=report_memory, daemon=True).start()
if group.rank() != spec["rank"] or group.size() != spec["size"]:
    raise SystemExit(
        f"goose rank wrapper: MLX reports rank {group.rank()} of {group.size()}, "
        f"goose launched rank {spec['rank']} of {spec['size']}"
    )


import faulthandler  # noqa: E402
import signal  # noqa: E402
import socket  # noqa: E402
from queue import Empty as QueueEmpty  # noqa: E402

# goosed stops a rank with SIGTERM (a hang, a stop, a memory death). The rank first writes every
# thread's Python stack to stderr — its durable log — then dies of the signal as before (`chain`:
# the default action runs after the dump, so the exit status still names SIGTERM). Q-114's rank 1
# sat at 0% CPU for 20 s before its SIGTERM and nothing said which line it was blocked on.
faulthandler.register(signal.SIGTERM, all_threads=True, chain=True)


def rank0_host():
    """Rank 0's address as the launch told every rank: the JACCL coordinator, or ring host 0."""
    coordinator = os.environ.get("MLX_JACCL_COORDINATOR")
    if coordinator:
        return coordinator.rsplit(":", 1)[0]
    with open(os.environ["MLX_HOSTFILE"]) as hostfile:
        return json.load(hostfile)[0][0].rsplit(":", 1)[0]


class Doorbell:
    """One byte from rank 0 to every worker before each collective an idle loop would otherwise
    spin in. Rank 0 listens on an ephemeral port of its launch address; the port reaches the
    workers through ONE all_sum right after the group forms (every rank runs this at the same
    point). A closed socket (rank 0 gone) ends the worker."""

    def __init__(self):
        self.peers = []
        self.link = None
        host = rank0_host()
        server = socket.create_server((host, 0)) if group.rank() == 0 else None
        port = server.getsockname()[1] if server is not None else 0
        port = int(mx.distributed.all_sum(mx.array([port], dtype=mx.int32)).item())
        if server is not None:
            for _ in range(group.size() - 1):
                peer, _ = server.accept()
                peer.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                self.peers.append(peer)
            server.close()
        else:
            self.link = socket.create_connection((host, port))
            self.link.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        emit("RANK_DOORBELL", {"rank": group.rank(), "port": port})

    def ring(self):
        for peer in self.peers:
            peer.sendall(b"\x01")

    def wait(self):
        if self.link.recv(1) != b"\x01":
            emit("RANK_DOORBELL_CLOSED", {"rank": group.rank()})
            raise SystemExit("goose rank wrapper: rank 0 closed the doorbell")


# Only a launch that asked for it (the spec's `doorbell`): a peer running an older wrapper never
# joins the port all_sum, so the requester refuses that pairing before it forms (launch.rs).
doorbell = Doorbell() if spec.get("doorbell") else None

import copy  # noqa: E402
import traceback  # noqa: E402

# sysexits EX_SOFTWARE: the generation thread died (its RANK_FATAL line names why).
RANK_FATAL_EXIT = 70

import mlx_lm  # noqa: E402
import importlib  # noqa: E402
# NOT `import mlx_lm.generate as ...`: mlx_lm/__init__.py re-exports the `generate` FUNCTION under the
# same name, and `import a.b as c` binds getattr(a, "b") — the function, not the module (3.0.44's
# split died at startup on it: 'function' object has no attribute 'PromptProcessingBatch').
mlx_generate = importlib.import_module("mlx_lm.generate")  # noqa: E402
import mlx_lm.server as server  # noqa: E402
from collections import deque  # noqa: E402

for owner, name in (
    (server, "run"),
    (server.ResponseGenerator, "_next_request"),
    (server.APIHandler, "do_GET"),
    (server.APIHandler, "do_POST"),
    (server.APIHandler, "validate_model_parameters"),
    (server.APIHandler, "_set_completion_headers"),
    (server.ModelProvider, "load"),
    (server.ResponseGenerator, "generate"),
    (server.ResponseGenerator, "_tokenize"),
    (server.ResponseGenerator, "_share_request"),
    (server.ResponseGenerator, "_generate"),
    (server, "LRUPromptCache"),
    (server.LRUPromptCache, "insert_cache"),
    (server.LRUPromptCache, "trim_to"),
    (server, "BatchGenerator"),
    (server.BatchGenerator, "close"),
    (server.BatchGenerator, "remove"),
    (server.BatchGenerator, "prompt_cache_nbytes"),
):
    if not hasattr(owner, name):
        raise SystemExit(
            f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no {getattr(owner, '__name__', owner)}.{name}; "
            "the wrapper was written against mlx_lm 0.31.3"
        )

prefill = spec.get("prefill")
if prefill is not None:
    for owner, name in (
        (mlx_generate.PromptProcessingBatch, "prompt"),
        (mlx_generate.PromptProcessingBatch, "split"),
        (mlx_generate.PromptProcessingBatch, "filter"),
        (server.BatchGenerator, "next"),
        (BatchKVCache, "step"),
    ):
        if not hasattr(owner, name):
            raise SystemExit(
                f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no {owner.__name__}.{name}; "
                "the prefill plan was written against mlx_lm 0.31.3"
            )
    if "prompt_cache_limit_bytes" not in spec:
        raise SystemExit(
            "goose rank wrapper: the spec carries a prefill plan but no prompt_cache_limit_bytes "
            "(the KV charge its admission is measured against)"
        )

served = spec["served_id"]
# Every other name of the served model (goose's one identity, model_identity.rs ServedNames), rank
# 0's only; an older requester's spec carries none (Q-131).
served_aliases = [name for name in spec.get("served_aliases", []) if name != served]
served_names = [served, *served_aliases]
state = {"steps": 0, "inflight": 0, "admission_open": True, "admission_reason": None}
# Where this rank's generation loop is (rank_state.py), published at each step for the reporter.
loop = LoopState(group.rank())
lock = threading.Lock()
# Rank 0's live request table (rank_live.py): the instants each request was measured at, keyed by
# a per-process counter. `generate` returns once the request is tokenized; mlx_lm then delivers
# the prompt progress (processed, total) and the tokens through the iterator the handler drains.
live = {}
live_ids = iter(range(1, 1 << 62))


def apply_caps():
    info = mx.device_info()
    ram = int(info["memory_size"])
    ceiling = int(info["max_recommended_working_set_size"])
    memory_limit = ceiling
    wired_limit = ceiling
    # MLX's free-buffer cache holds the plan's transient allowance and no more
    # (RankPlan::mlx_cache_limit_bytes, planned × (RUNTIME_OVERHEAD_RATIO − 1)). An older
    # requester's spec carries no figure: its own rule, the ceiling less the planned bytes.
    if "mlx_cache_limit_bytes" in spec:
        cache_limit = int(spec["mlx_cache_limit_bytes"])
    else:
        cache_limit = max(0, memory_limit - int(spec["planned_bytes"]))
    mx.set_memory_limit(memory_limit)
    mx.set_wired_limit(wired_limit)
    mx.set_cache_limit(cache_limit)
    emit(
        "RANK_CAPS",
        {
            "ram": ram,
            "memory_limit": memory_limit,
            "wired_limit": wired_limit,
            "cache_limit": cache_limit,
            "planned": int(spec["planned_bytes"]),
        },
    )


original_run = server.run


def run(host, port, model_provider, *args, **kwargs):
    # The goose id resolves to THIS rank's own --model path on every rank (paths differ per node),
    # so a request naming it loads nothing new and the response echoes the id that was asked for.
    model_provider._model_map[served] = model_provider.cli_args.model
    apply_caps()
    return original_run(host, port, model_provider, *args, **kwargs)


server.run = run

def owned(value):
    """`value` with every array replaced by one that owns exactly its own bytes."""
    if isinstance(value, mx.array):
        return mx.contiguous(value)
    if isinstance(value, (list, tuple)):
        return type(value)(owned(item) for item in value)
    return value


def arrays_in(value):
    if isinstance(value, mx.array):
        yield value
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from arrays_in(item)


def compact(prompt_cache):
    """Give a prompt-cache entry its own buffers before the cache keeps it. mlx_lm 0.31.3 hands
    the cache an UNEVALUATED `contiguous(batch keys[i, :, pad:idx])` (BatchKVCache.extract) and a
    VIEW `state[i:i+1]` (ArraysCache.extract): both keep the whole batch buffer alive — every row,
    padded to the longest — while `nbytes` counts one row's tokens. Measured (Q-79): a 5-row
    buffer held 40 MiB for an entry counted at 0–8 MiB, and E2E #2's ranks grew 43 → 77 GB while
    the cache reported 16 GB. Contiguous + eval copies exactly the entry and drops the batch
    buffer; `nbytes` is then the truth the byte bound is enforced on. Eviction is unchanged
    (nbytes was the logical size before and is the same size after), so a peer rank whose
    wrapper does not compact still evicts alike."""
    layers = [layer for layer in prompt_cache if not layer.empty()]
    for layer in layers:
        layer.state = owned(layer.state)
    mx.eval([array for layer in layers for array in arrays_in(layer.state)])


# The BatchGenerator mlx_lm's generation loop is serving (it makes at most one at a time): its
# live KV is what the prompt cache shares the plan's KV charge with between admissions.
live_batch = []
# The bounded prompt cache mlx_lm's `run` built (one per process).
prompt_caches = []


class TrackedBatchGenerator(server.BatchGenerator):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        live_batch[:] = [self]
        loop.new_batch()

    def remove(self, uids, *args, **kwargs):
        loop.drop(uids)
        return super().remove(uids, *args, **kwargs)

    def close(self):
        if live_batch and live_batch[0] is self:
            live_batch.clear()
        super().close()

    @property
    def prompt_cache_nbytes(self):
        # What mlx_lm's admission trim (server.py:795-798) and the live bound hold the prompt cache
        # beside: with a prefill plan, the batch's projected padded KV (rank_prefill.py) when it
        # exceeds what its caches hold now — the rows still to be read grow into it.
        held = super().prompt_cache_nbytes
        if prefill is None:
            return held
        return max(held, batch_kv_charge(prefill, *batch_shape(self)))

    def next(self):
        responses = super().next()
        for response in responses[1]:
            loop.fold(response.uid, response.token, response.finish_reason)
        # Generation grows every row one token a step, past what the last admission charged: the
        # cache yields before the batch outgrows the plan's KV charge. Every rank holds the same
        # batch and the same cache, so every rank evicts alike.
        if prefill is not None and prompt_caches:
            room = prompt_cache_limit - self.prompt_cache_nbytes
            if prompt_caches[0].nbytes > room:
                prompt_caches[0].trim_to(n_bytes=room)
        return responses


server.BatchGenerator = TrackedBatchGenerator

if prefill is not None:
    upstream_prompt = mlx_generate.PromptProcessingBatch.prompt
    upstream_split = mlx_generate.PromptProcessingBatch.split
    overruns = {"reported": False}

    def prompt(self, tokens):
        # One step of the prompt batch: `tokens` holds each row's next slice (at most mlx_lm's
        # step). Its chunk is what the plan's workspace affords this many rows at the width the
        # slice reaches; every rank computes it from the same batch, so the collectives pair.
        if tokens:
            rows = len(tokens)
            width = cache_width(self.prompt_cache) + max(len(t) for t in tokens)
            chunk = prefill_chunk(prefill, rows, width, BatchKVCache.step)
            over = chunk_overruns(prefill, rows, width, chunk)
            if over and not overruns["reported"]:
                overruns["reported"] = True
                emit(
                    "RANK_PREFILL_OVER",
                    {"rows": rows, "width": width, "chunk": chunk, "over_bytes": over},
                )
            self.prefill_step_size = chunk
        return upstream_prompt(self, tokens)

    def split(self, indices):
        return moving_split(self, indices, upstream_split)

    mlx_generate.PromptProcessingBatch.prompt = prompt
    mlx_generate.PromptProcessingBatch.split = split

if "prompt_cache_limit_bytes" in spec:
    prompt_cache_limit = int(spec["prompt_cache_limit_bytes"])
    # A spec that asks for it (`prompt_cache_live_bound`, tag mlxLmServerBounded) bounds cached
    # + live at EVERY insert, not only at admission (mlx_lm's own trim, server.py:795-798): the
    # plan charges the KV once — state + prompt cache — and a cache refilled to the whole charge
    # while a batch still holds its live KV overran it by that batch (E2E #2: 16–17.3 GB cached
    # beside 1–7 GB live). It changes what is evicted, so only a launch whose every rank runs it
    # asks for it (an older peer's goosed cannot read the tag and refuses the rank).
    live_bound = bool(spec.get("prompt_cache_live_bound"))
    prompt_cache_flags = [
        "--prompt-cache-size",
        str(int(spec["prompt_cache_entries"])),
        "--prompt-cache-bytes",
        str(prompt_cache_limit),
    ]

    class BoundedPromptCache(server.LRUPromptCache):
        def __init__(self, max_size):
            super().__init__(max_size, prompt_cache_limit)
            prompt_caches[:] = [self]

        def insert_cache(self, model, tokens, prompt_cache, *, cache_type="assistant"):
            compact(prompt_cache)
            super().insert_cache(model, tokens, prompt_cache, cache_type=cache_type)
            if live_bound and live_batch:
                self.trim_to(n_bytes=prompt_cache_limit - live_batch[0].prompt_cache_nbytes)

    server.LRUPromptCache = BoundedPromptCache
else:
    prompt_cache_flags = ["--prompt-cache-bytes", str(int(spec["prompt_cache_bytes"]))]

    class CompactPromptCache(server.LRUPromptCache):
        def insert_cache(self, model, tokens, prompt_cache, *, cache_type="assistant"):
            compact(prompt_cache)
            super().insert_cache(model, tokens, prompt_cache, cache_type=cache_type)

    server.LRUPromptCache = CompactPromptCache

original_generate_loop = server.ResponseGenerator._generate


def _generate(self):
    # mlx_lm's generation thread: an exception ends the thread, and a worker rank's main thread
    # only joins it (server.py `run`: `response_generator.join()`), so the process then exits 0
    # with the traceback as its last words — E2E #2's Studio rank died of a Metal OOM and was
    # reported "exit status: 0". The death is named and the exit is not a success; on rank 0 the
    # HTTP server would otherwise keep accepting requests nothing will ever answer.
    try:
        original_generate_loop(self)
    except BaseException as death:
        message = f"{type(death).__name__}: {death}"
        traceback.print_exc()
        emit(
            "RANK_FATAL",
            {
                "rank": group.rank(),
                "thread": "generation",
                "error": message,
                "out_of_memory": "Insufficient Memory" in message
                or "OutOfMemory" in message,
            },
        )
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(RANK_FATAL_EXIT)


server.ResponseGenerator._generate = _generate

original_next = server.ResponseGenerator._next_request


# Rank 0's requests waiting for room in the batch (settled: tokenized, budgeted), oldest first.
held = deque()


def room_for(prompt_tokens):
    batch = live_batch[0] if live_batch else None
    rows, width = batch_shape(batch) if batch is not None else (0, 0)
    return admits(prefill, prompt_cache_limit, rows, width, prompt_tokens)


def rank0_request(self, timeout):
    """The request rank 0 shares now, or None: the oldest held one once the batch has room for
    it, else the next arrival — held instead when the batch it would join cannot fit it (behind
    any request already held, so arrivals keep their order)."""
    if held and room_for(held[0][1]):
        request, tokens = held.popleft()
        emit("RANK_ADMISSION", {"released_tokens": tokens, "still_held": len(held)})
        return request
    try:
        if timeout is None or held:
            request = self.requests.get_nowait()
        else:
            request = self.requests.get(timeout=timeout)
    except QueueEmpty:
        return None
    request, tokens = settle(self, request)
    if request is None:
        return None
    if not held and room_for(tokens):
        return request
    held.append((request, tokens))
    rows, width = batch_shape(live_batch[0]) if live_batch else (0, 0)
    emit(
        "RANK_ADMISSION",
        {
            "held_tokens": tokens,
            "held": len(held),
            "rows": rows,
            "width": width,
            "charge": batch_kv_charge(prefill, rows + 1, max(width, tokens)),
            "limit": prompt_cache_limit,
        },
    )
    return None


def _next_request(self, timeout=None):
    # `timeout` is None exactly while a batch runs (mlx_lm's `_generate`), and every rank's loop
    # state is the same, so every rank takes the same branch here. With a prefill plan rank 0
    # decides which request is shared (rank0_request); the workers receive exactly what it shares.
    # Each wait is published before it is entered (rank_state.py), so a rank that stops inside one
    # has already said which.
    loop.mode = "busy" if timeout is None else "idle"
    loop.rows, loop.width = batch_shape(live_batch[0]) if live_batch else (0, 0)
    loop.held = len(held)
    if prefill is not None and group.rank() == 0:
        loop.publish("poll")
        request = rank0_request(self, timeout)
        if doorbell is None or timeout is None:
            loop.publish("share")
            request = original_share_request(self, request)
        elif request is not None:
            doorbell.ring()
            loop.rings += 1
            loop.publish("share")
            request = original_share_request(self, request)
    elif doorbell is None or timeout is None:
        loop.publish("share")
        request = original_next(self, timeout)
    elif group.rank() == 0:
        loop.publish("poll")
        try:
            request = self.requests.get(timeout=timeout)
        except QueueEmpty:
            request = None
        if request is not None:
            doorbell.ring()
            loop.rings += 1
            loop.publish("share")
            request = self._share_request(request)
    else:
        loop.publish("doorbell")
        doorbell.wait()
        loop.rings += 1
        loop.publish("share")
        request = self._share_request(None)
    state["steps"] += 1
    loop.steps = state["steps"]
    loop.publish("batch" if timeout is None or request is not None else "idle")
    return request


server.ResponseGenerator._next_request = _next_request

original_generate = server.ResponseGenerator.generate


def generate(self, request, generation_args, progress_callback=None):
    with lock:
        request_id = f"req-{next(live_ids)}"
        entry = {
            "arrived": time.monotonic(),
            "max_tokens": generation_args.max_tokens,
            "prompt_tokens": None,
            "cached_tokens": None,
            "prefill_started": None,
            "prefilled": 0,
            "first_token": None,
            "last_token": None,
            "completion": 0,
        }
        live[request_id] = entry

    def progress(processed, total):
        entry["prefilled"] = processed
        if progress_callback is not None:
            progress_callback(processed, total)

    def leave():
        with lock:
            live.pop(request_id, None)

    try:
        ctx, tokens = original_generate(self, request, generation_args, progress)
    except ContextFull as full:
        leave()
        raise Refused(400, str(full), "context_length_exceeded") from None
    except BaseException:
        leave()
        raise
    entry["max_tokens"] = generation_args.max_tokens
    # The generation thread hands back the context as it takes the request into its batch: from
    # here the engine is reading the prompt, though mlx_lm reports the first progress only after
    # its first chunks (measured: 6,144 of 15,249 tokens, 30 s in, on a 2-rank 27B).
    entry["prefill_started"] = time.monotonic()
    entry["prompt_tokens"] = len(ctx.prompt)
    if ctx.prompt_cache_count >= 0:
        entry["cached_tokens"] = ctx.prompt_cache_count

    def counted():
        try:
            for response in tokens:
                now = time.monotonic()
                if entry["first_token"] is None:
                    entry["first_token"] = now
                    entry["prefilled"] = entry["prompt_tokens"]
                entry["last_token"] = now
                entry["completion"] += 1
                yield response
        finally:
            leave()

    return ctx, counted()


server.ResponseGenerator.generate = generate

original_share_request = server.ResponseGenerator._share_request


def settle(self, request):
    """(request, prompt tokens), or (None, 0) for a request answered on its own queue.

    Rank 0's generation thread, the model loaded, before the request reaches the other ranks: the
    prompt is counted with mlx_lm's own _tokenize on a COPY (_tokenize rewrites messages in place
    — tool-call arguments become dicts — and a second pass over the same objects would fail), and
    the budget replaces the client's absence (None, kept by validate_model_parameters below). A
    request that cannot be counted or has no room is answered on its own queue and never shared,
    as mlx_lm answers a tokenization failure."""
    rqueue, completion, args = request
    try:
        prompt = original_tokenize(
            self, self.model_provider.tokenizer, copy.deepcopy(completion), args
        )[0]
        args.max_tokens = generation_budget(spec["context_window"], len(prompt), args.max_tokens)
    except Exception as refusal:
        rqueue.put(refusal)
        return None, 0
    return request, len(prompt)


def _share_request(self, request):
    if request is not None and group.rank() == 0:
        request = settle(self, request)[0]
    return original_share_request(self, request)


original_tokenize = server.ResponseGenerator._tokenize
server.ResponseGenerator._share_request = _share_request


# A BaseException so mlx_lm's handle_completion (`except Exception` → 404) lets it through to
# do_POST, which answers with the status it names.
class Refused(BaseException):
    def __init__(self, status, message, code=None):
        super().__init__(message)
        self.status = status
        self.message = message
        self.code = code


def send_json(handler, status, payload):
    handler._set_completion_headers(status)
    handler.end_headers()
    handler.wfile.write(json.dumps(payload).encode())
    handler.wfile.flush()


original_get = server.APIHandler.do_GET
original_post = server.APIHandler.do_POST
original_validate = server.APIHandler.validate_model_parameters


def do_GET(self):
    if self.path == "/goose/progress":
        return send_json(
            self,
            200,
            {
                "steps": state["steps"],
                "inflight": state["inflight"],
                "admission_open": state["admission_open"],
                "active": mx.get_active_memory(),
                "peak": mx.get_peak_memory(),
            },
        )
    if self.path == "/v1/status":
        # Every accepted-and-unfinished request is counted in num_running; mlx_lm does not split
        # queued from batched, so num_waiting carries none of them (the sum is the busy fact). The
        # request table tells prefill from generation per request.
        now = time.monotonic()
        with lock:
            rows = [
                live_request(request_id, now=now, **entry)
                for request_id, entry in live.items()
            ]
        return send_json(
            self,
            200,
            live_status({"num_running": state["inflight"], "num_waiting": 0}, rows),
        )
    if self.path.startswith("/v1/models"):
        return send_json(
            self,
            200,
            {
                "object": "list",
                "data": [
                    {
                        "id": name,
                        "object": "model",
                        "owned_by": "goose-distributed",
                        "context_window": spec["context_window"],
                    }
                    for name in served_names
                ],
            },
        )
    return original_get(self)


def do_POST(self):
    if self.path == "/goose/admission":
        length = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(length) or b"{}")
        state["admission_open"] = bool(body.get("open"))
        state["admission_reason"] = body.get("reason")
        if not state["admission_open"]:
            # Memory is short: MLX's free-buffer cache goes back to the OS now (per process, no
            # collective, nothing a peer must mirror).
            mx.clear_cache()
        return send_json(self, 200, {"admission_open": state["admission_open"]})
    if not state["admission_open"]:
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        return send_json(
            self,
            503,
            {
                "error": {
                    "message": "goose distributed engine is not admitting new requests: "
                    + str(state["admission_reason"]),
                    "type": "server_busy",
                }
            },
        )
    with lock:
        state["inflight"] += 1
    try:
        original_post(self)
    except Refused as refusal:
        error = {"message": refusal.message}
        if refusal.code is not None:
            error["code"] = refusal.code
            error["type"] = "invalid_request_error"
        send_json(self, refusal.status, {"error": error})
    finally:
        with lock:
            state["inflight"] -= 1


def validate_model_parameters(self):
    # mlx_lm read an absent max_tokens as its `--max-tokens` default; the absence is kept instead
    # (None rides the shared request to every rank), and _tokenize turns it into the room left.
    absent = all(
        self.body.get(key) is None for key in ("max_completion_tokens", "max_tokens")
    )
    if absent:
        self.max_tokens = spec["context_window"]
    original_validate(self)
    if absent:
        self.max_tokens = None
    if self.requested_model not in (*served_names, "default_model"):
        also = f" (also answering to {', '.join(repr(n) for n in served_aliases)})" if served_aliases else ""
        raise Refused(
            404,
            f"model '{self.requested_model}' is not served here; this distributed engine serves '{served}'{also}",
        )
    # The shared request names the model to every rank, and only the served id maps to each rank's
    # own --model path (`run`): an alias is answered as the served id, so no rank — a peer running
    # an older wrapper included — ever loads by a name it cannot resolve.
    if self.requested_model in served_aliases:
        self.requested_model = served
    if self.adapter is not None or self.body.get("draft_model") not in (None, "default_model"):
        raise Refused(400, "adapters and draft models are not supported by the distributed engine")


server.APIHandler.do_GET = do_GET
server.APIHandler.do_POST = do_POST
server.APIHandler.validate_model_parameters = validate_model_parameters

sys.argv = [
    "mlx_lm.server",
    "--model",
    spec["model_dir"],
    "--host",
    "127.0.0.1",
    "--port",
    str(spec["port"]),
    *prompt_cache_flags,
    *(["--prefill-step-size", str(int(prefill["step"]))] if prefill is not None else []),
]
server.main()
