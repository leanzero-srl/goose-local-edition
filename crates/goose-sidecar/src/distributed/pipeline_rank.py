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

from rapid_mlx.distributed import pipeline_qwen4_serve  # noqa: E402

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
