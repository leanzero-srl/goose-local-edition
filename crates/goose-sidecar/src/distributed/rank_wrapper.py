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
#   memory watchdog stop admitting new requests.
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

original_next = server.ResponseGenerator._next_request


def _next_request(self, timeout=None):
    request = original_next(self, timeout)
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
    except BaseException:
        leave()
        raise
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


class Refused(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status
        self.message = message


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
        send_json(self, refusal.status, {"error": {"message": refusal.message}})
    finally:
        with lock:
            state["inflight"] -= 1


def validate_model_parameters(self):
    original_validate(self)
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
    "--prompt-cache-bytes",
    str(spec["prompt_cache_bytes"]),
]
server.main()
