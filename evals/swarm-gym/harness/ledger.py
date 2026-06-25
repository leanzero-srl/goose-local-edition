"""Append-only session ledger (JSONL) + a queryable SQLite projection for trend/A-B analysis."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path
from typing import Any, Dict, List


class Ledger:
    def __init__(self, ledger_dir: Path):
        self.dir = ledger_dir
        self.dir.mkdir(parents=True, exist_ok=True)
        self.jsonl = self.dir / "sessions.jsonl"
        self.db = self.dir / "ledger.sqlite"
        con = sqlite3.connect(self.db)
        con.execute(
            "CREATE TABLE IF NOT EXISTS sessions (session_id TEXT, ts TEXT, archetype TEXT, "
            "app_slug TEXT, persona TEXT, turns INT, overall TEXT, judge_model TEXT, run_dir TEXT)"
        )
        con.commit()
        con.close()

    def record(self, session: Dict[str, Any]) -> None:
        with open(self.jsonl, "a") as f:
            f.write(json.dumps(session) + "\n")
        con = sqlite3.connect(self.db)
        con.execute(
            "INSERT INTO sessions VALUES (?,?,?,?,?,?,?,?,?)",
            (
                session.get("session_id"),
                session.get("ts"),
                session.get("archetype"),
                session.get("app_slug"),
                session.get("persona"),
                int(session.get("turns", 0)),
                session.get("overall"),
                session.get("judge_model"),
                session.get("run_dir"),
            ),
        )
        con.commit()
        con.close()

    def all_sessions(self) -> List[Dict[str, Any]]:
        if not self.jsonl.exists():
            return []
        out = []
        for line in self.jsonl.read_text(errors="replace").splitlines():
            try:
                out.append(json.loads(line))
            except Exception:
                pass
        return out

    def green_apps(self, apps_dir: Path) -> List[Dict[str, Any]]:
        """Apps from non-failing sessions that still exist on disk — amendment substrate."""
        seen, out = set(), []
        for s in reversed(self.all_sessions()):
            slug = s.get("app_slug")
            if not slug or slug in seen:
                continue
            if s.get("overall") in ("pass", "partial") and (apps_dir / slug).exists():
                seen.add(slug)
                out.append({"app_slug": slug, "ts": s.get("ts")})
        return out
