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
# and hands the server goose's `emit`. It must NOT call mx.distributed.init: serve() does.
import argparse  # noqa: E402
import traceback  # noqa: E402
import weakref  # noqa: E402

from rapid_mlx.distributed import pipeline_qwen4_serve  # noqa: E402

# Rank 0's live request table (rank_live.py) over the fork's own jobs, measured at the fork's own
# seams (pinned commit, provision.rs): a job's arrival (`_Job`), its batch's prefill start
# (`run_batch`), each prefill chunk's collective (`_step` with sample=False — an all_sum every rank
# joins, so a chunk is done on EVERY stage when it returns), its tokens (`produced`), and the
# `/v1/status` route `_build_app` registers, answered with the request table added.
for name in ("_Job", "run_batch", "_step", "_build_app"):
    if not hasattr(pipeline_qwen4_serve, name):
        raise SystemExit(
            f"goose pipeline rank: the fork's pipeline_qwen4_serve has no {name}; the live "
            "request table was written against fork 2f02ac645"
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
        now = time.monotonic()
        for job in jobs:
            job.prefill_started = now
        batch_now["current"] = {
            "jobs": jobs,
            "pads": [width - len(row.ids) for row in rows],
            "prefix": width - 1,
            "step": prefill_step,
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
        done = min(current["prefix"], current["chunks"] * current["step"])
        for job, pad in zip(current["jobs"], current["pads"]):
            job.prefilled = max(0, done - pad)
    return result


def live_row(job, now):
    return live_request(
        job.id,
        job.arrived,
        now,
        prompt_tokens=len(job.row.ids),
        max_tokens=job.row.max_tokens,
        prefill_started=job.prefill_started,
        prefilled=job.prefilled,
        first_token=job.__dict__.get("first_token"),
        last_token=job.__dict__.get("last_token"),
        completion=job.produced,
    )


def _build_app(state, *args, **kwargs):
    app = fork_build_app(state, *args, **kwargs)
    routes = [r for r in app.router.routes if getattr(r, "path", None) == "/v1/status"]
    if len(routes) != 1:
        raise SystemExit(
            f"goose pipeline rank: the fork's app has {len(routes)} /v1/status routes, expected 1"
        )
    fork_status = routes[0].endpoint
    app.router.routes.remove(routes[0])

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
