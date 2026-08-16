"""PARALLEL-N1 (F838): run the curve's 1-node arm three-at-a-time on pinned devices.

During an n1 unit two of Mihai's three nodes idle; this driver runs THREE 1-node units
concurrently — each pinned to a distinct device via GOOSE_SWARM_PIN_DEVICE (hard-fails on a
bad pin, engine-side) — collapsing the n1 half of the curve from ~14 h serial to ~5 h.
NEVER run while the sweep is mid-n3-unit: an n3 run owns lanes on every node and concurrent
generations degrade each other (the 2-per-node Apple ceiling); the plan runs the n1 block
with the sweep STOPPED, then hands the fleet to the n3 sweep.

Rows land as sweep-compatible nodeloop-result.json under runs/parallel-n1/swarm-1node-r{rep}/
and curve.py reads them as the n1 source. Each row is gated, not trusted: the run's own
pool_resolved must show EXACTLY the pinned device, or the row is refused (void with reason).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
OUT = HERE.parent / "runs" / "parallel-n1"
sys.path.insert(0, str(HERE))
import sweep  # noqa: E402  (engine_build; REGIME.env applied at import)

PINS = ["gabee", "mihai", "workh"]
PORT_BASE = 8600  # clear of the sweep's 8930s and run_build's default 8850


def run_one(rep: int, pin: str) -> subprocess.Popen:
    env = {**os.environ,
           "GOOSE_SWARM_MAX_NODES": "1",
           "GOOSE_SWARM_PIN_DEVICE": pin}
    log = OUT / f"unit-r{rep}.log"
    OUT.mkdir(parents=True, exist_ok=True)
    return subprocess.Popen(
        [sys.executable, str(BENCH / "run_build.py"), "--entrant", "swarm-1node",
         "--only-rep", str(rep), "--timeout", "9000",
         "--port", str(PORT_BASE + rep * 7), "--out", str(OUT)],
        cwd=BENCH.parent, env=env,
        stdout=open(log, "a"), stderr=subprocess.STDOUT, start_new_session=True)


def assemble_row(rep: int, pin: str, wall_secs: float) -> dict:
    d = OUT / f"swarm-1node-r{rep}"
    row = {"arm": "baseline", "nodes": 1, "rep": rep, "env": {},
           "gate": "parallel-n1 (F838)", "source": "parallel-n1",
           "finished_at": datetime.now().isoformat(timespec="seconds"),
           "pin": pin, "wall_secs": round(wall_secs, 1),
           "engine_build": sweep.engine_build(),
           "audit_version": sweep.dispatch_audit.AUDIT_VERSION,
           "score": None, "void": True, "harness_ok": True,
           "void_reason": "row assembled before verdict was read"}
    try:
        v = json.loads((d / "verdict.json").read_text())
        row["score"] = v.get("score")
        row["scorer_version"] = v.get("scorer_version")
        row["verdict_tiers"] = {k: t.get("mean") for k, t in (v.get("tiers") or {}).items()}
        agent = v.get("agent") or {}
        row["engine_exit"] = agent.get("exit")
        row["agent_secs"] = agent.get("secs")
        row["timed_out"] = bool(agent.get("timed_out"))
        # KILL/YOUNG-DEATH GATE (F839): all 8 first-attempt rows scored 0.045 in minutes —
        # the revived sweep's evictor killed every pinned engine and the corpses assembled as
        # void=False. A swarm prologue alone runs 15+ min; an agent that exited non-zero or
        # returned in under 10 minutes did not run a unit, whatever it left on disk.
        if agent.get("exit") not in (0, None) or (agent.get("secs") or 0) < 600:
            row["void_reason"] = (f"engine died young (exit {agent.get('exit')}, "
                                  f"{agent.get('secs')}s) — kill artifact, not a measurement")
            return row
    except Exception as e:
        row["void_reason"] = f"no readable verdict: {e}"
        return row
    # THE PIN GATE: the run's own pool must be exactly ONE device matching the pin. The
    # pool_resolved event's `id` is the HOST label ("local", "worksmacstudio"), not the model
    # instance id the engine filter matched — verified live on the first batch, where a naive
    # substring check would have refused three correctly-pinned rows. The alias map carries
    # both namings; the match scans every string the device object exposes.
    aliases = {"gabee": ["gabee"], "mihai": ["mihai", "local"],
               "workh": ["workh", "worksmacstudio"]}
    try:
        devices = None
        for line in (d / "run.jsonl").read_text(errors="replace").splitlines():
            ev = json.loads(line)
            if ev.get("event") == "pool_resolved":
                devices = ev.get("devices") or []
                break
        row["actual_pool"] = [x.get("id", "") for x in devices] if devices else None
        row["actual_nodes"] = len(devices) if devices else None
        blob = json.dumps(devices or [])
        if not devices or len(devices) != 1 \
                or not any(a in blob for a in aliases.get(pin, [pin])):
            row["void_reason"] = (f"pin gate: pool {row['actual_pool']} does not match "
                                  f"pin '{pin}' (aliases {aliases.get(pin, [pin])})")
            return row
    except Exception as e:
        row["void_reason"] = f"pin gate unreadable: {e}"
        return row
    row["void"] = False
    row.pop("void_reason", None)
    return row


def main() -> int:
    reps = [int(r) for r in sys.argv[1:]] or list(range(8))
    for batch_start in range(0, len(reps), len(PINS)):
        batch = reps[batch_start:batch_start + len(PINS)]
        started = time.time()
        procs = {rep: run_one(rep, PINS[i]) for i, rep in enumerate(batch)}
        print(f"[batch] reps {batch} launched on pins {PINS[:len(batch)]}", flush=True)
        for rep, p in procs.items():
            p.wait()
        wall = time.time() - started
        for i, rep in enumerate(batch):
            row = assemble_row(rep, PINS[i], wall)
            rd = OUT / f"swarm-1node-r{rep}"
            rd.mkdir(parents=True, exist_ok=True)
            (rd / "nodeloop-result.json").write_text(json.dumps(row, indent=2))
            print(f"[row] r{rep} pin={row.get('pin')} score={row.get('score')} "
                  f"void={row.get('void')} ({row.get('void_reason', 'ok')})", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
