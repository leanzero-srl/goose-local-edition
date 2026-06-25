"""Per-session HTML report (turn-by-turn). Solid status colors, full borders, no left-rail."""

from __future__ import annotations

import html
import json
from pathlib import Path
from typing import Any, Dict

COLORS = {
    "pass": "#1a7f37",
    "partial": "#9a6700",
    "warn": "#9a6700",
    "fail": "#cf222e",
    "skip": "#57606a",
}


def write_session_report(run_dir: Path, session: Dict[str, Any]) -> Path:
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "session.json").write_text(json.dumps(session, indent=2, default=str))
    p = run_dir / "report.html"
    p.write_text(_html(session))
    return p


def _badge(status: str) -> str:
    c = COLORS.get(status, "#57606a")
    return (
        f'<span style="background:{c};color:#fff;padding:2px 8px;font-weight:700">'
        f"{html.escape(str(status))}</span>"
    )


def _html(s: Dict[str, Any]) -> str:
    rows = []
    for t in s.get("turns_detail", []):
        v = t.get("verdict", {})
        dims = " ".join(
            f'{html.escape(k)} {_badge(d.get("status",""))}'
            for k, d in v.get("dimensions", {}).items()
        )
        finds = "".join(
            f'<li><b style="color:{COLORS["fail"] if f.get("severity")=="high" else COLORS["warn"]}">'
            f'[{html.escape(f.get("severity",""))}]</b> {html.escape(f.get("dimension",""))}: '
            f'{html.escape(f.get("text",""))}'
            f'{" — " + html.escape(f.get("fix_hint","")) if f.get("fix_hint") else ""}</li>'
            for f in v.get("findings", [])
        )
        tweak = ""
        if "tweak" in t:
            tweak = (
                f'<p style="background:#0969da;color:#fff;padding:6px">tweak: '
                f'{html.escape(t["tweak"].get("delta",{}).get("rationale",""))} '
                f'→ after: {html.escape(json.dumps(t["tweak"].get("after_per_device",{})))}</p>'
            )
        rows.append(
            f'<div style="border:2px solid #d0d7de;padding:14px;margin:12px 0">'
            f'<h3>Turn {t.get("turn")} · {html.escape(t.get("kind","open"))} {_badge(v.get("overall",""))}</h3>'
            f'<p><b>prompt:</b> {html.escape(str(t.get("prompt",""))[:500])}</p>'
            f"<p>{dims}</p><ul>{finds}</ul>{tweak}</div>"
        )
    return (
        "<!doctype html><html><head><meta charset=utf-8>"
        f"<title>swarm-gym {html.escape(s.get('session_id',''))}</title>"
        "<style>body{font-family:-apple-system,Segoe UI,sans-serif;max-width:920px;"
        "margin:24px auto;color:#24292f;line-height:1.4} h1,h3{margin:6px 0}</style></head><body>"
        f"<h1>swarm-gym session {_badge(s.get('overall','partial'))}</h1>"
        f"<p>{html.escape(s.get('archetype',''))} · app <b>{html.escape(s.get('app_slug',''))}</b> · "
        f"persona {html.escape(str(s.get('persona')))} · judge {html.escape(str(s.get('judge_model')))}</p>"
        + "".join(rows)
        + "</body></html>"
    )
