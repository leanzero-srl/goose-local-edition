"""The vibing-session control loop: open -> run -> collect -> verify -> next_move, for N turns."""

from __future__ import annotations

import datetime as dt
import json
import random
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from . import generator, monitor, report, runner, tweaker
from . import verifier as verifier_mod
from .brain import make_brain
from .collector import collect
from .ledger import Ledger

_SKIP = {".swarm", "__pycache__", ".git", "node_modules", ".venv", "target"}


def _log(msg: str) -> None:
    print(f"[gym] {msg}", file=sys.stderr, flush=True)


def _now() -> str:
    return dt.datetime.now().strftime("%Y%m%d-%H%M%S")


def _tree(ws: Path, cap: int = 200) -> List[str]:
    if not ws.exists():
        return []
    out: List[str] = []
    for p in sorted(ws.rglob("*")):
        if p.is_dir():
            continue
        rel = p.relative_to(ws)
        if any(part in _SKIP for part in rel.parts):
            continue
        out.append(str(rel))
        if len(out) >= cap:
            break
    return out


def _session_overall(turns: List[Dict[str, Any]]) -> str:
    if not turns:
        return "fail"
    overalls = [t["verdict"]["overall"] for t in turns]
    if all(o == "pass" for o in overalls):
        return "pass"
    if all(o == "fail" for o in overalls):
        return "fail"
    return "partial"


def run_session(
    cfg: Dict[str, Any],
    root: Path,
    archetype: str,
    persona: Optional[str],
    seed: int,
    turns: int,
    do_judge: bool,
    do_tweak: bool,
    variant: Optional[str] = None,
    tier: Optional[str] = None,
    mode: str = "exploratory",
) -> Dict[str, Any]:
    apps_dir = (root / cfg["paths"]["apps"]).resolve()
    runs_dir = (root / cfg["paths"]["runs"]).resolve()
    ledger = Ledger((root / cfg["paths"]["ledger"]).resolve())
    binary = (root / cfg["swarm"]["binary"]).resolve()
    endpoint = cfg["swarm"]["endpoint"]
    mcp = cfg["swarm"].get("mcp", []) or []
    timeout = int(cfg["swarm"].get("run_timeout_secs", 1200))

    rng = random.Random(seed)
    persona = persona or rng.choice(cfg["session"]["personas"])

    gen_brain = make_brain(cfg, "generate")
    judge_brain = make_brain(cfg, "judge") if do_judge else None
    tweak_brain = make_brain(cfg, "tweak") if do_tweak else None

    session_id = f"{archetype}-{_now()}-{seed}"
    run_dir = runs_dir / session_id

    existing = None
    if archetype == "continue-existing":
        greens = ledger.green_apps(apps_dir)
        if greens:
            pick = rng.choice(greens)
            existing = {"app_slug": pick["app_slug"], "files": _tree(apps_dir / pick["app_slug"])}
        else:
            archetype = "heavy-spec"  # nothing kept to continue yet

    _log(f"session {session_id} [{archetype}/{persona}] generating opener on {gen_brain.name} ...")
    task = generator.open_session(gen_brain, archetype, persona, seed, apps_dir, existing)
    _log(f"opener: app={task.app_slug} ({task.language}) — {task.prompt[:90]!r}")

    turns_detail: List[Dict[str, Any]] = []
    last_verdict = None
    t0 = time.monotonic()
    for turn in range(1, turns + 1):
        if turn > 1:
            try:
                move = generator.next_move(
                    gen_brain,
                    task,
                    last_verdict.model_dump() if last_verdict else {},
                    _tree(Path(task.workspace)),
                    persona,
                    turn,
                )
            except Exception as e:
                _log(f"next_move failed ({e}); ending session early")
                break
            if not isinstance(move, dict) or move.get("done"):
                break
            task = generator.move_to_taskspec(move, task, turn)
            kind = str(move.get("kind", "feature"))
        else:
            kind = "open"

        _log(f"turn {turn} ({kind}): swarm run [{len(task.requirements)} reqs, {len(task.deterministic_checks)} checks] ...")
        rr = runner.run_swarm(binary, task, endpoint, task.expected_mcp or mcp, timeout)
        ev = collect(task, rr)
        v = verifier_mod.verify(ev, judge_brain)
        last_verdict = v
        _log(
            f"turn {turn}: {v.overall} | "
            + ",".join(f"{k}={d.status}" for k, d in v.dimensions.items())
            + f" | {len(v.findings)} findings"
        )

        detail: Dict[str, Any] = {
            "turn": turn,
            "kind": kind,
            "prompt": task.prompt,
            "verdict": v.model_dump(),
            "run": {"done": rr.done, "failed": rr.failed, "per_device": rr.per_device, "jsonl": rr.jsonl_path},
        }
        run_dir.mkdir(parents=True, exist_ok=True)
        (run_dir / f"turn-{turn}.json").write_text(
            json.dumps({"task": task.model_dump(), "verdict": v.model_dump(), "run": rr.model_dump()}, indent=2, default=str)
        )

        if do_tweak and tweak_brain and any(f.dimension == "cluster" for f in v.findings):
            kd = tweaker.propose(tweak_brain, v.model_dump(), rr)
            env, notes = tweaker.apply(kd, binary)
            if kd.scope_ok and env is not None:
                rr2 = runner.run_swarm(binary, task, endpoint, task.expected_mcp or mcp, timeout, env_extra=env)
                detail["tweak"] = {"delta": kd.model_dump(), "notes": notes, "after_per_device": rr2.per_device}

        turns_detail.append(detail)

    overall = _session_overall(turns_detail)
    wall_secs = round(time.monotonic() - t0, 1)
    tasks_done = sum(len((d.get("run") or {}).get("done") or []) for d in turns_detail)
    tasks_failed = sum(len((d.get("run") or {}).get("failed") or []) for d in turns_detail)
    per_device_runs = [(d.get("run") or {}).get("per_device") or {} for d in turns_detail]
    last_v = (turns_detail[-1].get("verdict") if turns_detail else {}) or {}
    dims = {
        k: (dd.get("status") if isinstance(dd, dict) else dd)
        for k, dd in (last_v.get("dimensions") or {}).items()
    }
    session = {
        "session_id": session_id,
        "ts": dt.datetime.now().isoformat(),
        "archetype": task.archetype,
        "app_slug": task.app_slug,
        "persona": persona,
        "turns": len(turns_detail),
        "overall": overall,
        "judge_model": judge_brain.name if judge_brain else None,
        "run_dir": str(run_dir),
        "mode": mode,
        # --- benchmark fields (bench mode) ---
        "variant": variant,
        "tier": tier,
        "wall_secs": wall_secs,
        "tasks_done": tasks_done,
        "tasks_failed": tasks_failed,
        "dims": dims,
        "per_device_runs": per_device_runs,
        # EXPLORATORY active-monitoring: deterministic idle-node scan over the session's dispatch.
        "idle_report": monitor.idle_scan(monitor.aggregate(per_device_runs)),
        "turns_detail": turns_detail,
    }
    report.write_session_report(run_dir, session)
    ledger.record({k: val for k, val in session.items() if k != "turns_detail"})
    _log(f"session overall={overall} turns={len(turns_detail)} report={run_dir}/report.html")
    return session
