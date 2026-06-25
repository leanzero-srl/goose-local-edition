"""Combine deterministic + cluster + AI-judge checks into one Verdict."""

from __future__ import annotations

from typing import Dict, List, Optional

from ..brain.base import Brain
from ..contracts import DimensionVerdict, Evidence, Finding, Verdict
from . import cluster, deterministic, diagnostics
from . import judge as judge_mod


def verify(ev: Evidence, judge_brain: Optional[Brain]) -> Verdict:
    dims: Dict[str, DimensionVerdict] = {}
    findings: List[Finding] = []

    swarm_ok = ev.run.exit_code == 0 and not ev.run.failed
    dims["swarm"] = DimensionVerdict(
        status="pass" if swarm_ok else "fail",
        source="deterministic",
        detail={"done": ev.run.done, "failed": ev.run.failed, "exit_code": ev.run.exit_code},
    )
    if not swarm_ok:
        findings.append(
            Finding(
                id="swarm-fail",
                dimension="swarm",
                severity="high",
                text=f"swarm exit={ev.run.exit_code}, failed={ev.run.failed}",
                evidence=ev.run.stderr_tail,
            )
        )

    det_results, det_findings = deterministic.run_checks(ev)
    findings += det_findings
    if det_results:
        passed = sum(1 for r in det_results if r["ok"])
        total = len(det_results)
        st = "pass" if passed == total else ("partial" if passed > 0 else "fail")
        dims["checks"] = DimensionVerdict(
            status=st, source="deterministic", detail={"passed": passed, "total": total, "results": det_results}
        )
    else:
        dims["checks"] = DimensionVerdict(status="skip", source="deterministic")

    cluster_v, cluster_findings = cluster.assess(ev)
    dims["cluster"] = cluster_v
    findings += cluster_findings

    diag_v, diag_findings, _by_model = diagnostics.assess(ev)
    dims["diagnostics"] = diag_v
    findings += diag_findings

    judge_model = None
    if judge_brain is not None:
        try:
            j = judge_mod.judge(judge_brain, ev)
            judge_model = judge_brain.name
            for key in ("requirements", "code_quality", "bugs"):
                v = j.get(key)
                if isinstance(v, dict) and "status" in v:
                    dims[key] = DimensionVerdict(
                        status=_norm(v["status"]),
                        source="judge",
                        detail={k: val for k, val in v.items() if k != "status"},
                    )
            for f in j.get("findings", []):
                try:
                    f.setdefault("dimension", "bugs")
                    findings.append(Finding(**f))
                except Exception:
                    pass
        except Exception as e:
            dims["judge"] = DimensionVerdict(status="skip", source="judge", detail=f"judge error: {e}")

    return Verdict(
        turn=ev.task.turn,
        overall=_overall(dims, findings),
        dimensions=dims,
        findings=findings,
        judge_model=judge_model,
    )


def _norm(s: str) -> str:
    s = str(s).lower()
    return s if s in ("pass", "partial", "fail", "warn", "skip") else "partial"


def _overall(dims: Dict[str, DimensionVerdict], findings: List[Finding]) -> str:
    statuses = [d.status for d in dims.values()]
    if any(f.severity == "high" for f in findings) or "fail" in statuses:
        return "fail"
    if "partial" in statuses or "warn" in statuses:
        return "partial"
    return "pass"
