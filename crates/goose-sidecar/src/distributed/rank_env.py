# goose distributed rank environment: the prelude of every goose rank program.
#
# Embedded in goose-sidecar (distributed/launch.rs), concatenated IN FRONT of the rank's program
# (rank_wrapper.py for mlx_lm.server tensor ranks, pipeline_rank.py for the fork's pipeline
# ranks), and run as
#   python -c 'import base64,sys;exec(base64.b64decode(sys.argv[1]))' <program b64> <spec b64> goose-distributed-rank [goose-distributed-owner=<token>]
# so nothing is installed on a node. What every rank gets, and why:
# - the distributed env mlx.launch would have written (MLX_RANK, MLX_IBV_DEVICES /
#   MLX_JACCL_COORDINATOR or MLX_HOSTFILE — ONE backend's, never both: MLX's init("any") tries
#   ring (MLX_HOSTFILE) before jaccl, so the env alone decides the backend), so goose launches the
#   ranks itself and reads each rank's own exit (mlx.launch exits 0 on a rank death and spins
#   >1 core for its life, STEP1b);
# - HF_HUB_OFFLINE=1: a rank never downloads;
# - `emit(tag, payload)`: one `GOOSE_<tag> <json>` line the supervisor reads;
# - `report_memory()`: MLX's own active/peak/cache counters as GOOSE_RANK_MEM, every
#   `memory_report_seconds` (the program starts the thread when it is ready to).
# - the Mac's load lock (rank_load_lock.py, embedded in front of this file), taken before MLX is
#   imported and released by the RANK_CAPS report (both programs send it once the weights are in):
#   one model load at a time per Mac, beside goose's single engine and every other split's ranks.
import base64
import json
import os
import sys
import tempfile
import threading
import time

spec = json.loads(base64.b64decode(sys.argv[2]))
take_load_lock(
    spec.get("load_lock") or load_lock_path(),
    # One launch's ranks share its API port, served id and coordinator (JACCL) or host list (ring).
    f"split:{spec['port']}:{spec['served_id']}:{spec.get('coordinator') or json.dumps(spec.get('ring_hosts'))}",
    f"goose rank {spec['rank']} of {spec['size']} (pid {os.getpid()}) is loading {spec['served_id']} "
    f"from {spec['model_dir']}",
)


def emit(tag, payload):
    print(f"GOOSE_{tag} {json.dumps(payload)}", flush=True)
    if tag == "RANK_CAPS":
        release_load_lock()


def spec_file(content):
    fd, path = tempfile.mkstemp(prefix="goose-dist-")
    with os.fdopen(fd, "w") as handle:
        handle.write(content)
    return path


os.environ["MLX_RANK"] = str(spec["rank"])
os.environ["HF_HUB_OFFLINE"] = "1"
if spec["backend"] == "jaccl":
    os.environ["MLX_IBV_DEVICES"] = spec_file(json.dumps(spec["ibv_devices"]))
    os.environ["MLX_JACCL_COORDINATOR"] = spec["coordinator"]
else:
    os.environ["MLX_HOSTFILE"] = spec_file(json.dumps(spec["ring_hosts"]))

import mlx.core as mx  # noqa: E402


def report_memory():
    while True:
        emit(
            "RANK_MEM",
            {
                "active": mx.get_active_memory(),
                "peak": mx.get_peak_memory(),
                "cache": mx.get_cache_memory(),
            },
        )
        time.sleep(spec["memory_report_seconds"])
