"""The qualitative pass: per-model tool-call integrity (dropped/failed tool calls, missing MCP) and
a mistakes-left-behind source scan. This is what catches a model narrating instead of acting, or
leftover stubs/debug code that 'it built something' would otherwise hide."""

from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Tuple

from ..contracts import DimensionVerdict, Evidence, Finding

# High-signal leftover-mistake smells in NON-test source.
SMELLS = [
    (re.compile(r"\bNotImplementedError\b"), "unimplemented stub left in", "high"),
    (re.compile(r"\bpdb\.set_trace\b|\bbreakpoint\s*\("), "debugger breakpoint left in", "high"),
    (re.compile(r"\bTODO\b|\bFIXME\b|\bXXX\b"), "leftover TODO/FIXME", "low"),
    (re.compile(r"\bconsole\.log\s*\("), "leftover console.log", "low"),
    (re.compile(r"raise\s+Exception\([\"']TODO"), "TODO exception stub", "high"),
]
SKIP_SUFFIX = (".md", ".txt", ".toml", ".json", ".lock", ".cfg", ".ini", ".yaml", ".yml")


def assess(ev: Evidence) -> Tuple[DimensionVerdict, List[Finding], Dict]:
    findings: List[Finding] = []

    # --- per-model tool-call integrity ---
    by_model: Dict[str, Dict[str, int]] = defaultdict(
        lambda: {"tasks": 0, "tool_calls": 0, "failed": 0, "mcp": 0, "zero_tool_tasks": 0}
    )
    for t in ev.run.tasks:
        m = t.model or "unknown"
        s = by_model[m]
        s["tasks"] += 1
        s["tool_calls"] += len(t.tool_calls)
        nfail = sum(1 for tc in t.tool_calls if tc.ok is False)
        s["failed"] += nfail
        s["mcp"] += sum(1 for tc in t.tool_calls if tc.is_mcp)
        # The hybrid footgun: a task that completed but made ZERO tool calls likely narrated instead
        # of acting (no files written / no shell run).
        if t.status == "done" and len(t.tool_calls) == 0:
            s["zero_tool_tasks"] += 1
            findings.append(
                Finding(
                    id=f"notool-{t.task_id}",
                    dimension="tool_integrity",
                    severity="medium",
                    text=f"task '{t.task_id}' on {m} finished with ZERO tool calls — likely narrated instead of acting (hybrid tool-calling drop?)",
                    evidence=f"session {t.session_id}",
                )
            )
        if nfail:
            findings.append(
                Finding(
                    id=f"toolfail-{t.task_id}",
                    dimension="tool_integrity",
                    severity="low",
                    text=f"task '{t.task_id}' on {m} had {nfail} failed tool call(s)",
                )
            )

    # expected MCP servers that were never actually called
    called_mcp = {tc.name for t in ev.run.tasks for tc in t.tool_calls if tc.is_mcp}
    for want in ev.task.expected_mcp:
        key = want.split("-")[0]
        if not any(key in name for name in called_mcp):
            findings.append(
                Finding(
                    id=f"mcp-missing-{want}",
                    dimension="tool_integrity",
                    severity="medium",
                    text=f"expected MCP '{want}' was never called",
                    fix_hint="confirm the extension is enabled (--mcp) and its secret env var is set",
                )
            )

    # --- mistakes left behind ---
    ws = Path(ev.task.workspace)
    for f in ev.files:
        low = f.path.lower()
        if low.endswith(SKIP_SUFFIX) or "test" in low:
            continue
        try:
            lines = (ws / f.path).read_text(errors="replace").splitlines()
        except Exception:
            continue
        for pat, label, sev in SMELLS:
            hit = next((i for i, ln in enumerate(lines, 1) if pat.search(ln)), None)
            if hit:
                findings.append(
                    Finding(
                        id=f"smell-{f.path}-{label[:8]}",
                        dimension="hygiene",
                        severity=sev,
                        text=f"{label} in {f.path}:{hit}",
                        evidence=lines[hit - 1].strip()[:140],
                    )
                )

    has_high = any(f.severity == "high" for f in findings)
    has_med = any(f.severity == "medium" for f in findings)
    has_zero = any(s["zero_tool_tasks"] for s in by_model.values())
    # Low-severity findings (a leftover TODO, or a transient failed-then-recovered tool call — normal
    # during development) are REPORTED for visibility but must NOT warn the dimension: a warn drops
    # `overall` to partial via _overall, which made every functionally-correct app read "partial"
    # (clean-rate stuck at 0%). Only a medium+ concern (zero-tool narration, missing MCP) warns; a
    # high smell (NotImplementedError stub / breakpoint) fails.
    status = "fail" if has_high else ("warn" if (has_med or has_zero) else "pass")
    return DimensionVerdict(status=status, source="deterministic", detail={"by_model": dict(by_model)}), findings, dict(by_model)
