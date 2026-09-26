# goose distributed pipeline rank: one rank of the fork's OpenAI server over the qwen4_exp layer
# split (`rapid_mlx.distributed.pipeline_qwen4_serve`, pinned by commit in provision.rs). Runs
# after rank_env.py (the shared prelude: `spec`, `emit`, the backend env, `mx`,
# `report_memory`), under the fork's interpreter (NodeConfig.pipeline_python).
#
# The server carries everything the tensor wrapper adds around mlx_lm.server itself — its own
# `mx.distributed.init` (then GOOSE_RANK_GROUP), its caps as ratios of this node's RAM (then
# GOOSE_RANK_CAPS), a kernel warm-up, then GOOSE_READY; rank 0's /v1/models (the served id only),
# /v1/status, /goose/progress, /goose/admission and /v1/chat/completions; SIGTERM on rank 0
# broadcasts a shutdown every rank obeys. So this program only parses goose's argv with the fork's
# OWN parser (the exact `pipeline_qwen4 serve` arguments, the split preflight approved included)
# and hands the server goose's `emit`. It must NOT call mx.distributed.init: serve() does. Rank 0's
# /v1/chat/completions is goose's too, only to resolve the thinking switch the way the single
# engine does (rank_thinking.py, Q-135) before the fork's own handler reads the request.
import argparse  # noqa: E402
import traceback  # noqa: E402
import weakref  # noqa: E402

from rapid_mlx.distributed import pipeline_qwen4_serve  # noqa: E402

# Rank 0's live request table (rank_live.py) over the fork's own jobs, measured at the fork's own
# seams (pinned commit, provision.rs): a job's arrival (`_Job`), its batch's prefill start
# (`run_batch`), each prefill chunk's collective (`_step` with sample=False — an all_sum every rank
# joins, so a chunk is done on EVERY stage when it returns), its tokens (`produced`), and the
# `/v1/status` route `_build_app` registers, answered with the request table added. The prefill's
# ranges are the fork's own `prefill_chunks` (b7bd1afc2): a restored prefix-cache entry starts the
# prefill past the prompt's head (`row.cached`) and a snapshot ends one range at `row.store_at`.
for name in ("_Job", "run_batch", "_step", "_build_app", "prefill_chunks"):
    if not hasattr(pipeline_qwen4_serve, name):
        raise SystemExit(
            f"goose pipeline rank: the fork's pipeline_qwen4_serve has no {name}; the live "
            "request table was written against fork b7bd1afc2"
        )

jobs_by_row = weakref.WeakValueDictionary()
batch_now = {"current": None}


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
fork_run_batch = pipeline_qwen4_serve.run_batch
fork_step = pipeline_qwen4_serve._step
fork_build_app = pipeline_qwen4_serve._build_app


def run_batch(stage, guard, rows, prefill_step, *args, **kwargs):
    jobs = [jobs_by_row.get(id(row)) for row in rows] if stage.is_first else []
    if jobs and all(job is not None for job in jobs):
        width = max(len(row.ids) for row in rows)
        head = rows[0]
        one = len(rows) == 1
        start = head.cached if one and head.reuse_id else 0
        split = head.store_at if one and head.store_id else 0
        ranges = pipeline_qwen4_serve.prefill_chunks(start, width - 1, prefill_step, split)
        now = time.monotonic()
        for job in jobs:
            job.prefill_started = now
            job.prefilled = start
        batch_now["current"] = {
            "jobs": jobs,
            "pads": [width - len(row.ids) for row in rows],
            "ends": [end for _, end in ranges],
            "start": start,
            "chunks": 0,
        }
    try:
        return fork_run_batch(stage, guard, rows, prefill_step, *args, **kwargs)
    finally:
        batch_now["current"] = None


def _step(*args, sample, **kwargs):
    result = fork_step(*args, sample=sample, **kwargs)
    current = batch_now["current"]
    if current is not None and not sample:
        current["chunks"] += 1
        for job, pad in zip(current["jobs"], current["pads"]):
            job.prefilled = prefill_position(
                current["ends"], current["chunks"], current["start"], pad
            )
    return result


def live_row(job, now):
    return live_request(
        job.id,
        job.arrived,
        now,
        prompt_tokens=len(job.row.ids),
        # Rank 0 decides the restore when the job's batch forms: unknown (None) until then.
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


pipeline_qwen4_serve.run_batch = run_batch
pipeline_qwen4_serve._step = _step
pipeline_qwen4_serve._build_app = _build_app

parser = argparse.ArgumentParser(prog="python -m rapid_mlx.distributed.pipeline_qwen4 serve")
pipeline_qwen4_serve.add_arguments(parser)
options = parser.parse_args(spec["serve_args"])
threading.Thread(target=report_memory, daemon=True).start()
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
