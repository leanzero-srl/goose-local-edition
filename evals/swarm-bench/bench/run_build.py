"""Run one BUILD episode end to end: mock up, agent builds, grade the artifact, report.

Works for any entrant — a cloud model through `goose run`, the local fleet, or `goose swarm run` —
because the only thing that varies is how the agent is invoked. Everything downstream reads the
produced tree and the vendor's request trace.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import score_build  # noqa: E402
import vendor_service  # noqa: E402

ROOT = HERE.parent
GOOSE = Path.home() / "Projects/goose/target/release/goose"
MODELS = {
    "opus-5": "us.anthropic.claude-opus-5",
    "sonnet-5": "us.anthropic.claude-sonnet-5",
    "haiku-4.5": "us.anthropic.claude-haiku-4-5-20251001-v1:0",
}


def load_env(path: str = "~/.config/agent-board/bedrock.env") -> Dict[str, str]:
    env: Dict[str, str] = {}
    resolved = Path(path).expanduser()
    if not resolved.is_file():
        return env
    for raw in resolved.read_text().splitlines():
        line = raw.strip().removeprefix("export ").strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        env[key.strip()] = value.strip().strip("'\"")
    return env


def build_prompt(port: int) -> str:
    # BENCH3: an amend arm points BENCH_AMEND_SPEC at the brownfield spec; greenfield unchanged.
    spec_file = os.environ.get("BENCH_AMEND_SPEC", "")
    spec = (Path(spec_file) if spec_file else ROOT / "spec-build.md").read_text()
    return (spec.replace("{DOCS_URL}", f"http://127.0.0.1:{port}/v1/docs")
                .replace("{BASE_URL}", f"http://127.0.0.1:{port}")
                .replace("{API_KEY}", vendor_service.API_KEY))


def invoke(entrant: str, workdir: Path, port: int, env: Dict[str, str], timeout: int) -> Dict:
    prompt = build_prompt(port)
    if entrant in MODELS:
        cmd = [str(GOOSE), "run", "--provider", "aws_bedrock", "--model", MODELS[entrant],
               "-t", prompt]
    elif entrant == "local-single":
        cmd = [str(GOOSE), "run", "--provider", "lmstudio",
               "--model", "mihai-qwopus3.6-27b-coder-mtp", "-t", prompt]
    elif entrant.startswith("swarm"):
        # GOOSE_SWARM_MAX_NODES caps the auto-pool inside the engine. `swarm pool disable` cannot do
        # this: the pool is rebuilt from `lms ps` on every run, so a disabled-but-resident device is
        # silently re-added — measured, a 1-node and a 3-node run both reported the same 2-node pool.
        match = re.match(r"swarm-(\d+)node", entrant)
        nodes = int(match.group(1)) if match else 3
        # GOOSE_SWARM_READ_ON_FIX: the arm under test. A fix worker owns no files and is repairing a
        # defect the gates already reproduced by running the app; the implementer read-prohibitions
        # make a cross-module signature mismatch structurally invisible to it.
        # GOOSE_SWARM_PLANNER_ALSO_WORKS: the engine pushes the PLANNER on as an extra worker device
        # unless the planner's model is already in the pool (swarm.rs, `planner_also_works`, default
        # true). MEASURED: at MAX_NODES=1 the pool was one device, the planner model was not in it, the
        # planner was pushed, and the run dispatched 5 tasks to each with a peak of TWO devices working
        # at once — while `run_started.pool` reported 1, because it is emitted before the push. At
        # MAX_NODES=3 the planner model IS in the pool, nothing is pushed, and the run has three. So a
        # node-count sweep was about to compare 2 against 3 while labelling it 1 against 3.
        # Off here, so N nodes means N workers. The 3-node cell is unaffected (nothing was ever pushed
        # there), which is what makes this a correction rather than a change of subject.
        # GOOSE_SWARM_ASK_WAIT_SECS: when plan confidence lands under the ask floor the engine writes
        # .swarm/clarify-questions.json and BLOCK-POLLS for answers for 1800s — THIRTY MINUTES — then
        # proceeds and decides the questions itself. Nothing in this harness answers, so the wait is
        # always paid in full and always ends the same way. MEASURED on the first post-freeze unit:
        # confidence 68 against a floor of 85, low_confidence_ask at +26.6m, and all three nodes
        # GENERATING = 0 for the whole window.
        #
        # The outcome is IDENTICAL either way — the plan that ships after 1800s is the plan that would
        # have shipped after 5s — so the wait buys nothing and costs ~25% of a unit in node-independent
        # idle, injected straight into the wall-clock and occupancy figures the node curve compares.
        # Not answering (rather than authoring canonical answers) keeps the treatment constant across
        # cells without putting my judgement into the build.
        env = {**env, "GOOSE_SWARM_MAX_NODES": str(nodes),
               "GOOSE_SWARM_PLANNER_ALSO_WORKS": "0",
               "GOOSE_SWARM_ASK_WAIT_SECS": "5",
               "GOOSE_SWARM_READ_ON_FIX": os.environ.get("GOOSE_SWARM_READ_ON_FIX", "1")}
        cmd = [str(GOOSE), "swarm", "run", prompt, "--output-format", "json",
               "--log-file", str(workdir / "run.jsonl")]
    else:
        raise SystemExit(f"unknown entrant {entrant!r}")

    started = time.time()
    try:
        proc = subprocess.run(cmd, cwd=workdir, capture_output=True, text=True,
                              timeout=timeout, env={**os.environ, **env}, start_new_session=True)
        code, tail = proc.returncode, (proc.stdout + proc.stderr)[-1500:]
    except subprocess.TimeoutExpired:
        code, tail = None, "timed out"
    return {"exit": code, "secs": round(time.time() - started, 1), "tail": tail,
            "timed_out": code is None}


def run(entrant: str, rep: int, out_root: Path, timeout: int, port: int) -> Dict:
    workdir = out_root / f"{entrant}-r{rep}"
    if workdir.exists():
        shutil.rmtree(workdir)
    workdir.mkdir(parents=True)
    # BENCH3 (BENCH3-AMEND.md): the brownfield mode. When the arm exports BENCH_SEED_TREE, the
    # unit starts from a COPY of that base app instead of an empty directory — the engine's
    # amendment path (working_dir_has_sources) then sees real sources. Greenfield arms are
    # byte-identical: no env, no copy.
    seed = os.environ.get("BENCH_SEED_TREE", "")
    if seed:
        base = Path(seed)
        if not base.is_dir():
            raise SystemExit(f"BENCH_SEED_TREE does not exist: {seed}")
        for child in base.iterdir():
            if child.name in {"__pycache__", ".swarm", "graded.db", "verdict.json",
                              "process.json", "run.jsonl", "vendor-trace.jsonl",
                              "nodeloop-result.json", "heartbeat"}:
                continue
            if child.is_dir():
                shutil.copytree(child, workdir / child.name)
            else:
                shutil.copy2(child, workdir / child.name)
    trace = out_root / f"trace-{entrant}-r{rep}.jsonl"

    server = vendor_service.serve(port, trace)
    try:
        agent = invoke(entrant, workdir, port, load_env(), timeout)
        ctx = score_build.gather(workdir, port, workdir / "graded.db", trace,
                                 mark_phase=vendor_service.mark_phase)
    finally:
        server.shutdown()

    verdict = score_build.evaluate(ctx)
    # BENCH2/F769: ARCHIVE THE TREE at score time — the sweep wipes the workdir within seconds
    # of [done] (dir reuse), which has already cost the campaign the 0.996 tree and the first
    # two dual-score windows. A tree copy is ~100KB and makes every scored artifact a permanent
    # forensic object (and the sb-3/sb-4 bridge feasible offline).
    try:
        dest = workdir.parent / "_sb4trees" / f"{workdir.name}-{int(time.time())}"
        dest.parent.mkdir(exist_ok=True)
        shutil.copytree(workdir, dest, ignore=shutil.ignore_patterns(
            ".swarm", "__pycache__", ".pytest_cache", "*.pyc"))
        # The verdict rides WITH its tree — nodeloop-result.json is written to the workdir
        # AFTER this block and dies in the next wipe; the first archive proved it (F771's
        # ledger gap). The archive is only a forensic object if it carries its own scoring.
        (dest / "sb4-verdict.json").write_text(json.dumps(verdict, indent=1, default=str))
    except Exception as e:
        print(f"tree-archive failed (non-fatal): {e}", file=sys.stderr)
    # The pool the run REALLY used, straight from run_started. A label like "swarm-3node" is an
    # intention; this is the fact, and a mismatch invalidates any node-scaling claim.
    actual_pool = None
    run_log = workdir / "run.jsonl"
    if run_log.is_file():
        for line in run_log.read_text(errors="replace").splitlines():
            try:
                e = json.loads(line)
            except Exception:
                continue
            if e.get("event") == "run_started":
                actual_pool = [d.get("model_id") for d in (e.get("pool") or [])]
                break
    verdict.update({"entrant": entrant, "rep": rep, "agent": agent,
                    "actual_pool": actual_pool,
                    "actual_nodes": len(actual_pool) if actual_pool is not None else None})
    (workdir / "verdict.json").write_text(json.dumps(verdict, indent=2))
    return verdict


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--entrant", default="opus-5")
    ap.add_argument("--reps", type=int, default=1)
    ap.add_argument("--only-rep", type=int,
                    help="run exactly this rep index instead of 0..reps-1")
    ap.add_argument("--timeout", type=int, default=1800)
    ap.add_argument("--port", type=int, default=8850)
    ap.add_argument("--out", type=Path, default=ROOT / "runs/build")
    args = ap.parse_args()

    verdicts = []
    reps = [args.only_rep] if args.only_rep is not None else list(range(args.reps))
    for rep in reps:
        v = run(args.entrant, rep, args.out, args.timeout, args.port + rep)
        verdicts.append(v)
        print(score_build.format_report(
            v, f"{args.entrant} rep{rep} ({v['agent']['secs']}s)"), flush=True)
        print()

    if len(verdicts) > 1:
        scores = [100 * v["score"] for v in verdicts]
        print(f"spread {min(scores):.1f}% – {max(scores):.1f}%  "
              f"mean {sum(scores) / len(scores):.1f}%")
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / f"{args.entrant}.json").write_text(json.dumps(verdicts, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
