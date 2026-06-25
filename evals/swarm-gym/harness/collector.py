"""Gather an Evidence bundle: the run result + JSONL event log + session traces + built files."""

from __future__ import annotations

import hashlib
import json
import sqlite3
from pathlib import Path
from typing import Dict, List, Optional

from .contracts import Evidence, FileEntry, RunResult, TaskOutcome, TaskSpec, ToolCall

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

    # On a timeout/crash the final JSON report never prints (run.tasks empty), but the JSONL event
    # log captured every task — rebuild the per-task data from it so verification still works.
    if not run.tasks and events:
        _backfill_from_events(run, events)

    traces: List[Dict] = []
    for t in run.tasks:
        if t.session_id:
            tr = _fetch_trace(t.session_id)
            if tr:
                tr["task_id"] = t.task_id
                traces.append(tr)

    files = _snapshot_files(Path(task.workspace))
    return Evidence(task=task, run=run, events=events, session_traces=traces, files=files)


def _backfill_from_events(run: RunResult, events: List[Dict]) -> None:
    tasks: List[TaskOutcome] = []
    done: List[str] = []
    failed: List[str] = []
    per_device: Dict[str, Dict] = {}
    for e in events:
        if e.get("event") != "task_completed":
            continue
        tcs = [
            ToolCall(name=t.get("name", ""), is_mcp=bool(t.get("is_mcp")), ok=t.get("ok"))
            for t in (e.get("tool_calls") or [])
        ]
        status = e.get("status", "done")
        tid = e.get("task_id", "")
        tasks.append(
            TaskOutcome(
                task_id=tid,
                status=status,
                device=e.get("device"),
                model=e.get("model"),
                attempts=int(e.get("attempts", 0) or 0),
                elapsed_ms=e.get("elapsed_ms"),
                session_id=e.get("session_id"),
                tool_calls=tcs,
            )
        )
        (done if status == "done" else failed).append(tid)
        dev = e.get("device")
        if dev:
            d = per_device.setdefault(
                dev, {"dispatched": 0, "tool_calls": 0, "mcp_calls": 0, "retries": 0}
            )
            d["dispatched"] += 1
            d["tool_calls"] += len(tcs)
            d["mcp_calls"] += sum(1 for t in tcs if t.is_mcp)
    run.tasks = tasks
    if not run.done:
        run.done = done
    if not run.failed:
        run.failed = failed
    if not run.per_device:
        run.per_device = per_device


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
