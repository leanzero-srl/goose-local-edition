# goose distributed pipeline rank: one rank of the fork's OpenAI server over the qwen4_exp layer
# split (`rapid_mlx.distributed.pipeline_qwen4_serve`, pinned by commit in provision.rs). Runs
# after rank_env.py (the shared prelude: `spec`, `emit`, the backend env, `mx`,
# `report_memory`), under the fork's interpreter (NodeConfig.pipeline_python).
#
# The server carries everything the tensor wrapper adds around mlx_lm.server itself — its own
# `mx.distributed.init` (then GOOSE_RANK_GROUP), its caps as ratios of this node's RAM (then
# GOOSE_RANK_CAPS), a kernel warm-up, then GOOSE_READY; rank 0's /v1/models (the served id, then
# each `--served-model-alias` goose's identity passes rank 0 — the only names its chat accepts),
# /v1/status, /goose/progress, /goose/admission and /v1/chat/completions; SIGTERM on rank 0
# broadcasts a shutdown every rank obeys. So this program only parses goose's argv with the fork's
# OWN parser (the exact `pipeline_qwen4 serve` arguments, the split preflight approved included)
# and hands the server goose's `emit`. serve() forms the group (`mx.distributed.init(strict=True)`,
# then GOOSE_RANK_GROUP); a launch whose spec carries `formation` initialises it first, on the
# spec's backend, and runs the formation handshake (rank_formation.py, Q-136) before the fork's first
# collective — MLX caches an initialised group under its backend and "any", so serve()'s init
# returns that same group. Rank 0's
# /v1/chat/completions is goose's too, only to resolve the thinking switch the way the single
# engine does (rank_thinking.py, Q-135) before the fork's own handler reads the request.
import argparse  # noqa: E402
import traceback  # noqa: E402
import weakref  # noqa: E402

from rapid_mlx.distributed import pipeline_qwen4_serve  # noqa: E402

# Rank 0's live request table (rank_live.py) over the fork's own jobs, measured at the fork's own
# seams (pinned commit, provision.rs): a job's arrival (`_Job`), the moment its row starts
# prefilling (`_Engine._start` — rank 0's plan admitted it, Q-134's continuous admission,
# c8d6d5faf), each prefill chunk's collective (`_Engine.prefill`: an all_sum every rank joins, so
# the chunk is done on EVERY stage when it returns), its tokens (`produced`), and the `/v1/status`
# route `_build_app` registers, answered with the request table added. A row prefills alone in its
# own cache before it joins the running batch: its ranges are the fork's own `prefill_chunks`, from
# `row.cached` (a restored prefix-cache entry) to the prompt's end, one ending at `row.store_at`,
# so the prompt position after a chunk is that chunk's end — no batch padding to subtract.
for name in ("_Job", "_Engine", "_build_app", "prefill_chunks"):
    if not hasattr(pipeline_qwen4_serve, name):
        raise SystemExit(
            f"goose pipeline rank: the fork's pipeline_qwen4_serve has no {name}; the live "
            "request table was written against fork c8d6d5faf"
        )
for name in ("_start", "prefill"):
    if not hasattr(pipeline_qwen4_serve._Engine, name):
        raise SystemExit(
            f"goose pipeline rank: the fork's _Engine has no {name}; the live request table "
            "was written against fork c8d6d5faf"
        )

jobs_by_row = weakref.WeakValueDictionary()


class LiveJob(pipeline_qwen4_serve._Job):
    # The fork's dataclass has no __post_init__, so its generated __init__ calls none: extend it.
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.arrived = time.monotonic()
        self.prefill_started = None
        self.prefilled = 0
        jobs_by_row[id(self.row)] = self

    @property
    def produced(self):
        return self.__dict__.get("_produced", 0)

    @produced.setter
    def produced(self, value):
        if value > 0:
            now = time.monotonic()
            if self.__dict__.get("first_token") is None:
                self.first_token = now
                self.prefilled = len(self.row.ids)
            self.last_token = now
        self.__dict__["_produced"] = value


pipeline_qwen4_serve._Job = LiveJob
fork_engine = pipeline_qwen4_serve._Engine
fork_start = fork_engine._start
fork_prefill = fork_engine.prefill
fork_build_app = pipeline_qwen4_serve._build_app


def _start(engine, row):
    fork_start(engine, row)
    # Rank 0 admits the row object its job carries; other ranks rebuild rows from the plan.
    job = jobs_by_row.get(id(row)) if engine.stage.is_first else None
    if job is not None:
        job.prefill_started = time.monotonic()
        job.prefilled = row.cached if row.reuse_id else 0


def prefill(engine, words):
    joining = engine.joining
    stop = joining.ranges[0][1]
    job = jobs_by_row.get(id(joining.row)) if engine.stage.is_first else None
    result = fork_prefill(engine, words)
    if job is not None:
        job.prefilled = max(job.prefilled, stop)
    return result


def live_row(job, now):
    return live_request(
        job.id,
        job.arrived,
        now,
        prompt_tokens=len(job.row.ids),
        # Rank 0 decides the restore when its plan admits the job: unknown (None) until then.
        cached_tokens=job.row.cached if job.prefill_started is not None else None,
        max_tokens=job.row.max_tokens,
        prefill_started=job.prefill_started,
        prefilled=job.prefilled,
        first_token=job.__dict__.get("first_token"),
        last_token=job.__dict__.get("last_token"),
        completion=job.produced,
    )


def fork_route(app, path):
    routes = [r for r in app.router.routes if getattr(r, "path", None) == path]
    if len(routes) != 1:
        raise SystemExit(
            f"goose pipeline rank: the fork's app has {len(routes)} {path} routes, expected 1"
        )
    app.router.routes.remove(routes[0])
    return routes[0].endpoint


def _build_app(state, tokenizer, *args, **kwargs):
    # Imported where the fork imports them, inside its own _build_app.
    from fastapi import Request
    from fastapi.responses import JSONResponse

    app = fork_build_app(state, tokenizer, *args, **kwargs)
    fork_status = fork_route(app, "/v1/status")
    fork_chat = fork_route(app, "/v1/chat/completions")
    reasons = template_reasons(getattr(tokenizer, "chat_template", None))

    # rank_thinking.py (Q-135): the fork pops an absent `enable_thinking` as None, which its
    # apply_chat_template renders ON; the single engine renders the same request OFF. The body is
    # resolved in place before the fork reads it (Starlette caches `request.json()`).
    @app.post("/v1/chat/completions")
    async def chat(request: Request):
        body = await request.json()
        if isinstance(body, dict):
            try:
                body["chat_template_kwargs"] = resolved_template_kwargs(body, reasons)
            except ThinkingRefused as refusal:
                return JSONResponse(
                    status_code=400,
                    content={"error": {"message": str(refusal), "type": "invalid_request_error",
                                       "code": "unsupported_parameter"}},
                )
            body.pop("enable_thinking", None)
        return await fork_chat(request)

    @app.get("/v1/status")
    async def status():
        base = await fork_status()
        now = time.monotonic()
        with state.jobs.mutex:
            queued = [job for job in state.jobs.queue if job is not None]
        held = [state.held] if state.held is not None else []
        rows = [live_row(job, now) for job in list(state.active) if not job.finished]
        rows += [live_row(job, now) for job in held + queued if not job.cancelled]
        return live_status(base, rows)

    return app


fork_engine._start = _start
fork_engine.prefill = prefill
pipeline_qwen4_serve._build_app = _build_app

parser = argparse.ArgumentParser(prog="python -m rapid_mlx.distributed.pipeline_qwen4 serve")
pipeline_qwen4_serve.add_arguments(parser)
options = parser.parse_args(spec["serve_args"])
threading.Thread(target=report_memory, daemon=True).start()
if spec.get("formation") is not None:
    form_group(mx.distributed.init(strict=True, backend=spec["backend"]), spec["formation"])
try:
    code = pipeline_qwen4_serve.serve(options, emit=emit)
except BaseException:
    traceback.print_exc()
    code = 1
# Every rank has left the collective loop (or failed): interpreter teardown must not wait on the
# server's HTTP/executor threads — the same exit the fork's own `serve` entry takes.
sys.stdout.flush()
sys.stderr.flush()
os._exit(code)
