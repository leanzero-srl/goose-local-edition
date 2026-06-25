"""Propose + apply swarm-only knob changes (the scope guard rejects anything touching core)."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Dict, List, Tuple

from .brain.base import Brain
from .contracts import KnobChange, KnobDelta, RunResult, Verdict

ALLOWED_PREFIXES = (
    "pool.",
    "planner_model",
    "worker_max_turns",
    "max_attempts",
    "env.GOOSE_LOCAL_CONTEXT_CAP",
    "env.GOOSE_MAX_BACKGROUND_TASKS",
)

_SYS = (
    "You tune ONLY the local-AI swarm knobs to fix a systemic issue you see in the verdict/cluster "
    "data. Allowed knobs: pool.<device>.weight, pool.<device>.instances, pool.<device>.enabled, "
    "planner_model, worker_max_turns, max_attempts, env.GOOSE_LOCAL_CONTEXT_CAP, "
    "env.GOOSE_MAX_BACKGROUND_TASKS. NEVER propose anything touching upstream goose core. "
    'Return JSON: {"rationale":"...","changes":[{"knob":"pool.workhorse.weight","from_value":2,"to_value":3}]}'
)


def propose(brain: Brain, verdict: dict, run: RunResult) -> KnobDelta:
    user = json.dumps({"verdict": verdict, "per_device": run.per_device}, indent=2)[:12000]
    data = brain.json_call(_SYS, user, max_tokens=1200)
    changes = []
    for c in data.get("changes", []):
        try:
            changes.append(KnobChange(**c))
        except Exception:
            pass
    kd = KnobDelta(rationale=data.get("rationale", ""), changes=changes)
    kd.scope_ok = bool(changes) and all(_in_scope(c.knob) for c in changes)
    return kd


def _in_scope(knob: str) -> bool:
    return any(knob.startswith(p) for p in ALLOWED_PREFIXES)


def apply(delta: KnobDelta, binary: Path) -> Tuple[Dict[str, str], List[str]]:
    """Apply a scoped delta. Returns (env_overrides_for_rerun, notes). Rejects out-of-scope deltas."""
    notes: List[str] = []
    env: Dict[str, str] = {}
    if not delta.scope_ok:
        return env, ["REJECTED: delta touches non-swarm knobs"]
    for ch in delta.changes:
        knob = ch.knob
        if knob.startswith("env."):
            env[knob[4:]] = str(ch.to_value)
            notes.append(f"env {knob[4:]}={ch.to_value}")
        elif knob.startswith("pool.") and knob.endswith(".weight"):
            dev = knob[len("pool.") : -len(".weight")]
            try:
                subprocess.run(
                    [str(binary), "swarm", "pool", "weight", dev, str(int(ch.to_value))],
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                notes.append(f"pool {dev} weight={ch.to_value}")
            except Exception as e:
                notes.append(f"pool weight apply failed: {e}")
        elif knob.startswith("pool.") and knob.endswith(".enabled"):
            dev = knob[len("pool.") : -len(".enabled")]
            verb = "enable" if str(ch.to_value).lower() in ("true", "1") else "disable"
            try:
                subprocess.run(
                    [str(binary), "swarm", "pool", verb, dev], capture_output=True, text=True, timeout=30
                )
                notes.append(f"pool {dev} {verb}")
            except Exception as e:
                notes.append(f"pool enable apply failed: {e}")
        else:
            notes.append(f"(manual knob, not auto-applied: {knob})")
    return env, notes
