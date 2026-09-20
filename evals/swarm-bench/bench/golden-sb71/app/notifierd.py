"""notifierd — the idempotent notification consumer.

Dedupe key is the ledger event seq, held in a DURABLE processed set in notifier.db; the
relay is at-least-once, this set makes it exactly-once. Exactly four event types produce
exactly one notification row each; payment.sent is processed but notifies nothing.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, Optional

from .util import envelope, json_bytes, now_rfc3339, parse_int_param

NOTIFY_TYPES = {"draft.submitted", "draft.approved", "draft.rejected", "reversal.created"}

_SCHEMA = """
CREATE TABLE IF NOT EXISTS processed (
    seq INTEGER PRIMARY KEY, type TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY, event_seq INTEGER NOT NULL UNIQUE, kind TEXT NOT NULL,
    message TEXT NOT NULL, at TEXT NOT NULL);
"""


def _message(ev: Dict) -> str:
    kind = ev.get("type")
    did = ev.get("draft_id") or "a draft"
    pid = ev.get("payment_id") or "a payment"
    if kind == "draft.submitted":
        return f"Draft {did} was submitted for approval."
    if kind == "draft.approved":
        return f"Draft {did} was approved by the checker."
    if kind == "draft.rejected":
        return f"Draft {did} was rejected by the checker."
    if kind == "reversal.created":
        return f"A reversal was recorded for payment {pid}."
    return f"Event {kind} was processed."


class Notifier:
    def __init__(self, db_dir: Path):
        db_dir.mkdir(parents=True, exist_ok=True)
        self.lock = threading.Lock()
        self.db = sqlite3.connect(str(db_dir / "notifier.db"), check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=NORMAL")
        self.db.execute("PRAGMA busy_timeout=10000")
        with self.lock:
            self.db.executescript(_SCHEMA)
            self.db.commit()
        self.received = 0
        self.applied = 0
        self.duplicate = 0

    def ingest(self, events) -> Dict:
        accepted, duplicate = [], []
        with self.lock:
            for ev in events:
                seq = ev.get("seq")
                if not isinstance(seq, int):
                    continue
                self.received += 1
                have = self.db.execute("SELECT 1 FROM processed WHERE seq=?",
                                       (seq,)).fetchone()
                if have:
                    self.duplicate += 1
                    duplicate.append(seq)
                    continue
                self.db.execute("INSERT INTO processed(seq, type, at) VALUES(?,?,?)",
                                (seq, str(ev.get("type")), now_rfc3339()))
                if ev.get("type") in NOTIFY_TYPES:
                    self.db.execute(
                        "INSERT OR IGNORE INTO notifications(id, event_seq, kind, "
                        "message, at) VALUES(?,?,?,?,?)",
                        (f"ntf_{seq:06d}", seq, ev["type"], _message(ev), now_rfc3339()))
                self.applied += 1
                accepted.append(seq)
            self.db.commit()
        return {"accepted": accepted, "duplicate": duplicate}

    def health(self) -> Dict:
        with self.lock:
            n = self.db.execute("SELECT COUNT(*) c FROM notifications").fetchone()["c"]
        return {"status": "ok", "received": self.received, "applied": self.applied,
                "duplicate": self.duplicate, "notifications": n}

    def processed_after(self, after: int) -> Dict:
        with self.lock:
            rows = self.db.execute(
                "SELECT seq, type FROM processed WHERE seq>? ORDER BY seq",
                (after,)).fetchall()
            latest = self.db.execute("SELECT MAX(seq) m FROM processed").fetchone()["m"]
        return {"processed": [{"seq": r["seq"], "type": r["type"]} for r in rows],
                "latest_seq": latest or 0}

    def notifications(self, limit: int, offset: int) -> Dict:
        with self.lock:
            total = self.db.execute("SELECT COUNT(*) c FROM notifications").fetchone()["c"]
            rows = self.db.execute(
                "SELECT * FROM notifications ORDER BY event_seq DESC LIMIT ? OFFSET ?",
                (limit, offset)).fetchall()
        return {"data": [{"id": r["id"], "event_seq": r["event_seq"], "kind": r["kind"],
                          "message": r["message"], "at": r["at"]} for r in rows],
                "total": total}


NOTIFIER: Optional[Notifier] = None


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_a):
        return

    def _json(self, code: int, payload) -> None:
        body = json_bytes(payload)
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except OSError:
            pass

    def do_GET(self):  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        qs = {k: v[0] for k, v in urllib.parse.parse_qs(parsed.query).items()}
        if parsed.path == "/health":
            self._json(200, NOTIFIER.health())
            return
        if parsed.path == "/notify/processed":
            after, err = parse_int_param(qs.get("after"), "after", 0)
            if err:
                self._json(400, envelope("bad_request", "invalid after", [err]))
                return
            self._json(200, NOTIFIER.processed_after(after))
            return
        if parsed.path == "/notify/notifications":
            limit, e1 = parse_int_param(qs.get("limit"), "limit", 50)
            offset, e2 = parse_int_param(qs.get("offset"), "offset", 0)
            errs = [e for e in (e1, e2) if e]
            if errs:
                self._json(400, envelope("bad_request", "invalid parameters", errs))
                return
            self._json(200, NOTIFIER.notifications(min(limit, 500), offset))
            return
        self._json(404, envelope("not_found", "no such path"))

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path == "/notify/events":
            try:
                body = json.loads(raw or b"{}")
            except json.JSONDecodeError:
                body = {}
            events = body.get("events") if isinstance(body, dict) else None
            if not isinstance(events, list):
                self._json(400, envelope("bad_request", "events must be a list"))
                return
            self._json(200, NOTIFIER.ingest([e for e in events if isinstance(e, dict)]))
            return
        self._json(404, envelope("not_found", "no such path"))


def serve(db_dir: Path, port: int):
    global NOTIFIER
    NOTIFIER = Notifier(db_dir)
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db-dir", type=Path, required=True)
    ap.add_argument("--port", type=int, required=True)
    args = ap.parse_args(argv)
    serve(args.db_dir, args.port)
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    raise SystemExit(main())
