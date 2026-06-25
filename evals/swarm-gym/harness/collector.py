"""Gather an Evidence bundle: the run result + JSONL event log + session traces + built files."""

from __future__ import annotations

import hashlib
import json
import sqlite3
from pathlib import Path
from typing import Dict, List, Optional

from .contracts import Evidence, FileEntry, RunResult, TaskSpec

SESSIONS_DB = Path.home() / ".local/share/goose/sessions/sessions.db"
SKIP_PARTS = {".swarm", "__pycache__", ".git", "node_modules", ".venv", "target"}


def collect(task: TaskSpec, run: RunResult) -> Evidence:
    events: List[Dict] = []
    if run.jsonl_path and Path(run.jsonl_path).exists():
        for line in Path(run.jsonl_path).read_text(errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except Exception:
                pass

    traces: List[Dict] = []
    for t in run.tasks:
        if t.session_id:
            tr = _fetch_trace(t.session_id)
            if tr:
                tr["task_id"] = t.task_id
                traces.append(tr)

    files = _snapshot_files(Path(task.workspace))
    return Evidence(task=task, run=run, events=events, session_traces=traces, files=files)


def _fetch_trace(session_id: str) -> Optional[Dict]:
    if not SESSIONS_DB.exists():
        return None
    try:
        con = sqlite3.connect(f"file:{SESSIONS_DB}?mode=ro", uri=True)
        con.row_factory = sqlite3.Row
        cur = con.cursor()
        cols = [r[1] for r in cur.execute("PRAGMA table_info(messages)").fetchall()]
        content_col = next((c for c in ("content_json", "content") if c in cols), None)
        role_col = "role" if "role" in cols else None
        rows = cur.execute(
            "SELECT * FROM messages WHERE session_id=? ORDER BY rowid", (session_id,)
        ).fetchall()
        con.close()
        msgs = []
        for r in rows:
            d = dict(r)
            msgs.append(
                {
                    "role": d.get(role_col) if role_col else None,
                    "content": str(d.get(content_col))[:2000] if content_col else "",
                }
            )
        return {"session_id": session_id, "message_count": len(msgs), "messages": msgs}
    except Exception as e:
        return {"session_id": session_id, "error": str(e)}


def _snapshot_files(ws: Path) -> List[FileEntry]:
    out: List[FileEntry] = []
    if not ws.exists():
        return out
    for p in sorted(ws.rglob("*")):
        if p.is_dir() or any(part in SKIP_PARTS for part in p.relative_to(ws).parts):
            continue
        try:
            data = p.read_bytes()
            out.append(
                FileEntry(
                    path=str(p.relative_to(ws)),
                    bytes=len(data),
                    sha256=hashlib.sha256(data).hexdigest()[:16],
                )
            )
        except Exception:
            pass
    return out
