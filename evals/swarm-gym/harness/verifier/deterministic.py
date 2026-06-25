"""Model-free checks: file_exists/contains/matches, command_succeeds, tool_called. Ported from the
open-model-gym validation rules + build/run/test via command_succeeds."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path
from typing import List, Set, Tuple

from ..contracts import DeterministicCheck, Evidence, Finding


def run_checks(ev: Evidence) -> Tuple[List[dict], List[Finding]]:
    ws = Path(ev.task.workspace)
    results: List[dict] = []
    findings: List[Finding] = []
    for i, chk in enumerate(ev.task.deterministic_checks):
        ok, detail = _eval(chk, ws, ev)
        label = chk.name or chk.path or chk.command or chk.tool or chk.type
        results.append({"check": label, "type": chk.type, "ok": ok, "detail": detail})
        if not ok:
            findings.append(
                Finding(
                    id=f"det-{i + 1}",
                    dimension="checks",
                    severity="high",
                    text=f"{chk.type} failed: {label}",
                    evidence=str(detail)[:500] if detail else None,
                )
            )
    return results, findings


def _eval(chk: DeterministicCheck, ws: Path, ev: Evidence):
    if chk.type == "file_exists":
        return (ws / (chk.path or "")).exists(), chk.path
    if chk.type in ("file_contains", "file_matches"):
        p = ws / (chk.path or "")
        if not p.exists():
            return False, f"{chk.path} missing"
        text = p.read_text(errors="replace")
        if chk.type == "file_contains":
            return (chk.regex or "") in text, None
        return bool(re.search(chk.regex or "", text)), None
    if chk.type == "command_succeeds":
        try:
            r = subprocess.run(
                chk.command or "true",
                shell=True,
                cwd=str(ws),
                capture_output=True,
                text=True,
                timeout=300,
            )
            return r.returncode == 0, (r.stdout + r.stderr)[-600:]
        except Exception as e:
            return False, str(e)
    if chk.type == "tool_called":
        called = _tools_called(ev)
        return any((chk.tool or "") in name for name in called), f"called: {sorted(called)[:25]}"
    return False, "unknown check type"


def _tools_called(ev: Evidence) -> Set[str]:
    names: Set[str] = set()
    for t in ev.run.tasks:
        for tc in t.tool_calls:
            names.add(tc.name)
    return names
