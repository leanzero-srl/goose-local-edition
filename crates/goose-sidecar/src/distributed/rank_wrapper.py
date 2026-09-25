# goose distributed tensor rank: one tensor-parallel rank of mlx_lm.server. Runs after
# rank_env.py (the shared prelude: `spec`, `emit`, the backend env, `mx`, `report_memory`).
#
# What it adds around mlx_lm.server, and why:
# - in-process memory caps at THIS node's GPU ceiling (max_recommended_working_set_size — the
#   budget preflight planned against is at most that), applied after mlx_lm.server's own
#   set_wired_limit, so an over-allocation raises in MLX instead of the kernel wiring past the
#   ceiling (exo panicked a 96 GB M3 Ultra that way);
# - rank 0's HTTP surface: /v1/models serves ONLY the goose model id (mlx_lm lists the HF cache,
#   and a request naming any of those would make every rank try to load it), the goose id maps to
#   each rank's OWN --model path (paths differ per node), /goose/progress exposes the
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


import socket  # noqa: E402
from queue import Empty as QueueEmpty  # noqa: E402


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

import mlx_lm  # noqa: E402
import mlx_lm.server as server  # noqa: E402

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
):
    if not hasattr(owner, name):
        raise SystemExit(
            f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no {getattr(owner, '__name__', owner)}.{name}; "
            "the wrapper was written against mlx_lm 0.31.3"
        )

served = spec["served_id"]
state = {"steps": 0, "inflight": 0, "admission_open": True, "admission_reason": None}
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

if "prompt_cache_limit_bytes" in spec:
    prompt_cache_limit = int(spec["prompt_cache_limit_bytes"])
    prompt_cache_flags = [
        "--prompt-cache-size",
        str(int(spec["prompt_cache_entries"])),
        "--prompt-cache-bytes",
        str(prompt_cache_limit),
    ]

    class BoundedPromptCache(server.LRUPromptCache):
        def __init__(self, max_size):
            super().__init__(max_size, prompt_cache_limit)

    server.LRUPromptCache = BoundedPromptCache
else:
    prompt_cache_flags = ["--prompt-cache-bytes", str(int(spec["prompt_cache_bytes"]))]

original_next = server.ResponseGenerator._next_request


def _next_request(self, timeout=None):
    # `timeout` is None exactly while a batch runs (mlx_lm's `_generate`), and every rank's loop
    # state is the same, so every rank takes the same branch here.
    if doorbell is None or timeout is None:
        request = original_next(self, timeout)
    elif group.rank() == 0:
        try:
            request = self.requests.get(timeout=timeout)
        except QueueEmpty:
            request = None
        if request is not None:
            doorbell.ring()
            request = self._share_request(request)
    else:
        doorbell.wait()
        request = self._share_request(None)
    state["steps"] += 1
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


def _share_request(self, request):
    # Rank 0's generation thread, the model loaded, before the request reaches the other ranks: the
    # prompt is counted with mlx_lm's own _tokenize on a COPY (_tokenize rewrites messages in place
    # — tool-call arguments become dicts — and a second pass over the same objects would fail), and
    # the budget replaces the client's absence (None, kept by validate_model_parameters below). A
    # request that cannot be counted or has no room is answered on its own queue and never shared,
    # as mlx_lm answers a tokenization failure.
    if request is not None and group.rank() == 0:
        rqueue, completion, args = request
        try:
            prompt = original_tokenize(
                self, self.model_provider.tokenizer, copy.deepcopy(completion), args
            )[0]
            args.max_tokens = generation_budget(
                spec["context_window"], len(prompt), args.max_tokens
            )
        except Exception as refusal:
            rqueue.put(refusal)
            request = None
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
                        "id": served,
                        "object": "model",
                        "owned_by": "goose-distributed",
                        "context_window": spec["context_window"],
                    }
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
    if self.requested_model not in (served, "default_model"):
        raise Refused(
            404,
            f"model '{self.requested_model}' is not served here; this distributed engine serves '{served}'",
        )
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
]
server.main()
