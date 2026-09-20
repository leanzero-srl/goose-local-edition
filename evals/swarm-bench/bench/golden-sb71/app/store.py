"""ledger.db — payments, reversals, the append-only event ledger, the outbox, drafts.

One connection, one lock, short transactions. Every state change appends EXACTLY ONE event
in the SAME transaction (outbox rows too — commit-then-POST is the dual-write bug this
schema exists to prevent). seq is the rowid of an append-only table: contiguous from 1.
"""

from __future__ import annotations

import json
import sqlite3
import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from .util import STATUS_ORDER, berlin_day, epoch_of, now_rfc3339

OUTBOX_TYPES = {"draft.submitted", "draft.approved", "draft.rejected",
                "reversal.created", "payment.sent"}

_SCHEMA = """
CREATE TABLE IF NOT EXISTS payments (
    id TEXT PRIMARY KEY, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL,
    created_at TEXT NOT NULL, settled_at TEXT, status TEXT NOT NULL,
    version INTEGER NOT NULL, note TEXT, counterparty_name TEXT, country TEXT,
    day TEXT NOT NULL, instant REAL NOT NULL);
CREATE INDEX IF NOT EXISTS idx_pay_instant ON payments(instant, id);
CREATE INDEX IF NOT EXISTS idx_pay_day ON payments(day, status);
CREATE INDEX IF NOT EXISTS idx_pay_amount ON payments(amount_minor, id);
CREATE TABLE IF NOT EXISTS reversals (
    id TEXT PRIMARY KEY, payment_id TEXT NOT NULL, amount_minor INTEGER NOT NULL,
    currency TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS events (
    seq INTEGER PRIMARY KEY, type TEXT NOT NULL, payment_id TEXT, version INTEGER,
    source TEXT NOT NULL, txn TEXT, at TEXT NOT NULL, draft_id TEXT);
CREATE TABLE IF NOT EXISTS outbox (
    seq INTEGER PRIMARY KEY, delivered INTEGER NOT NULL DEFAULT 0, delivered_at TEXT);
CREATE TABLE IF NOT EXISTS drafts (
    id TEXT PRIMARY KEY, state TEXT NOT NULL, amount_minor INTEGER NOT NULL,
    currency TEXT NOT NULL, cp_name TEXT NOT NULL, cp_country TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL,
    submitted_by TEXT, idempotency_key TEXT, sent_payment_id TEXT);
CREATE TABLE IF NOT EXISTS webhook_seen (event_id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS txn_stage (
    txn_id TEXT NOT NULL, part INTEGER NOT NULL, total INTEGER NOT NULL,
    payload TEXT NOT NULL, PRIMARY KEY (txn_id, part));
CREATE TABLE IF NOT EXISTS validators (
    offset INTEGER PRIMARY KEY, etag TEXT NOT NULL, gen TEXT NOT NULL,
    next_cursor TEXT);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
"""


def _pub(r: sqlite3.Row) -> Dict:
    return {"id": r["id"], "amount_minor": r["amount_minor"], "currency": r["currency"],
            "created_at": r["created_at"], "settled_at": r["settled_at"],
            "status": r["status"], "version": r["version"], "note": r["note"],
            "counterparty_name": r["counterparty_name"], "country": r["country"]}


class LedgerStore:
    def __init__(self, path: Path):
        self.lock = threading.RLock()
        self.db = sqlite3.connect(str(path), check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=NORMAL")
        self.db.execute("PRAGMA busy_timeout=10000")
        with self.lock:
            self.db.executescript(_SCHEMA)
            self.db.commit()

    @contextmanager
    def tx(self):
        with self.lock:
            try:
                yield self.db
                self.db.commit()
            except Exception:
                self.db.rollback()
                raise

    # ── meta ─────────────────────────────────────────────────────────────────────────────────

    def meta_get(self, key: str) -> Optional[str]:
        with self.lock:
            r = self.db.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone()
        return r["value"] if r else None

    def meta_set(self, key: str, value: str) -> None:
        with self.tx() as db:
            db.execute("INSERT INTO meta(key, value) VALUES(?, ?) "
                       "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (key, value))

    # ── events + outbox (caller is inside tx) ────────────────────────────────────────────────

    def _append_event(self, db, etype: str, payment_id: Optional[str],
                      version: Optional[int], source: str, txn: Optional[Dict],
                      draft_id: Optional[str] = None) -> int:
        cur = db.execute(
            "INSERT INTO events(type, payment_id, version, source, txn, at, draft_id) "
            "VALUES(?,?,?,?,?,?,?)",
            (etype, payment_id, version, source,
             json.dumps(txn) if txn is not None else None, now_rfc3339(), draft_id))
        seq = cur.lastrowid
        if etype in OUTBOX_TYPES:
            db.execute("INSERT INTO outbox(seq) VALUES(?)", (seq,))
        return seq

    # ── payments ─────────────────────────────────────────────────────────────────────────────

    def upsert_payment(self, row: Dict, source: str,
                       txn: Optional[Dict] = None, db=None) -> Tuple[str, Optional[Dict]]:
        """Version-compared upsert. Returns (outcome, changed_record):
        outcome in insert|update|unchanged; changed_record is the SSE-shaped record when the
        row changed. Appends the matching ledger event in the same transaction."""
        own = db is None

        def _run(db):
            cp = row.get("counterparty") or {}
            day = berlin_day(row["created_at"])
            cur = self.db.execute("SELECT version FROM payments WHERE id=?",
                                  (row["id"],)).fetchone()
            if cur is None:
                db.execute(
                    "INSERT INTO payments(id, amount_minor, currency, created_at, "
                    "settled_at, status, version, note, counterparty_name, country, day, "
                    "instant) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                    (row["id"], row["amount_minor"], row["currency"], row["created_at"],
                     row.get("settled_at"), row["status"], row["version"],
                     row.get("note") or "", cp.get("name"), cp.get("country"), day,
                     epoch_of(row["created_at"])))
                self._append_event(db, "payment.created", row["id"], row["version"],
                                   source, txn)
                return "insert"
            if row["version"] <= cur["version"]:
                return "unchanged"
            db.execute(
                "UPDATE payments SET amount_minor=?, currency=?, settled_at=?, status=?, "
                "version=?, note=?, counterparty_name=?, country=? WHERE id=?",
                (row["amount_minor"], row["currency"], row.get("settled_at"),
                 row["status"], row["version"], row.get("note") or "",
                 cp.get("name"), cp.get("country"), row["id"]))
            self._append_event(db, "payment.updated", row["id"], row["version"],
                               source, txn)
            return "update"

        if own:
            with self.tx() as db2:
                outcome = _run(db2)
        else:
            outcome = _run(db)
        rec = None
        if outcome != "unchanged":
            rec = {"id": row["id"], "amount_minor": row["amount_minor"],
                   "currency": row["currency"], "status": row["status"],
                   "created_at": row["created_at"],
                   "day": berlin_day(row["created_at"]), "version": row["version"]}
        return outcome, rec

    def insert_reversal(self, rev: Dict, source: str, txn: Optional[Dict] = None,
                        with_event: bool = True, db=None) -> bool:
        own = db is None

        def _run(db):
            cur = self.db.execute("SELECT 1 FROM reversals WHERE id=?",
                                  (rev["id"],)).fetchone()
            if cur is not None:
                return False
            db.execute("INSERT INTO reversals(id, payment_id, amount_minor, currency, "
                       "created_at) VALUES(?,?,?,?,?)",
                       (rev["id"], rev["payment_id"], rev["amount_minor"],
                        rev["currency"], rev["created_at"]))
            if with_event:
                self._append_event(db, "reversal.created", rev["payment_id"], None,
                                   source, txn)
            return True

        if own:
            with self.tx() as db2:
                return _run(db2)
        return _run(db)

    def apply_note(self, payment: Dict) -> Optional[Dict]:
        """Persist a vendor-confirmed note write; returns the SSE record when changed."""
        _o, rec = self.upsert_payment(payment, "local")
        return rec

    def payment(self, pid: str) -> Optional[Dict]:
        with self.lock:
            r = self.db.execute("SELECT * FROM payments WHERE id=?", (pid,)).fetchone()
        return _pub(r) if r else None

    def payment_row(self, pid: str) -> Optional[sqlite3.Row]:
        with self.lock:
            return self.db.execute("SELECT * FROM payments WHERE id=?", (pid,)).fetchone()

    def count(self) -> int:
        with self.lock:
            return self.db.execute("SELECT COUNT(*) c FROM payments").fetchone()["c"]

    def list_payments(self, limit: int, offset: int, status: Optional[str],
                      currency: Optional[str], sort: str) -> Tuple[List[Dict], int]:
        where, args = [], []
        if status:
            where.append("status=?")
            args.append(status)
        if currency:
            where.append("currency=?")
            args.append(currency)
        w = ("WHERE " + " AND ".join(where)) if where else ""
        order = {"created_at": "instant ASC, id ASC",
                 "-created_at": "instant DESC, id DESC",
                 "amount_minor": "amount_minor ASC, id ASC",
                 "-amount_minor": "amount_minor DESC, id DESC"}[sort]
        with self.lock:
            total = self.db.execute(f"SELECT COUNT(*) c FROM payments {w}",
                                    args).fetchone()["c"]
            rows = self.db.execute(
                f"SELECT * FROM payments {w} ORDER BY {order} LIMIT ? OFFSET ?",
                args + [limit, offset]).fetchall()
        return [_pub(r) for r in rows], total

    def summary(self) -> Dict:
        with self.lock:
            n = self.db.execute("SELECT COUNT(*) c FROM payments").fetchone()["c"]
            span = self.db.execute(
                "SELECT MIN(instant) lo, MAX(instant) hi FROM payments").fetchone()
            lo = self.db.execute(
                "SELECT created_at FROM payments ORDER BY instant ASC, id ASC LIMIT 1"
            ).fetchone()
            hi = self.db.execute(
                "SELECT created_at FROM payments ORDER BY instant DESC, id DESC LIMIT 1"
            ).fetchone()
            by = self.db.execute(
                "SELECT currency, COUNT(*) c, SUM(amount_minor) t FROM payments "
                "GROUP BY currency ORDER BY currency ASC").fetchall()
            rev = self.db.execute(
                "SELECT currency, COUNT(*) c, SUM(amount_minor) t FROM reversals "
                "GROUP BY currency ORDER BY currency ASC").fetchall()
        _ = span
        from .util import utc_rfc3339
        return {
            "count": n,
            "last_sync": self.meta_get("last_sync"),
            "oldest": utc_rfc3339(lo["created_at"]) if lo else None,
            "newest": utc_rfc3339(hi["created_at"]) if hi else None,
            "by_currency": [{"currency": r["currency"], "count": r["c"],
                             "total_minor": r["t"]} for r in by],
            "reversals": [{"currency": r["currency"], "count": r["c"],
                           "total_minor": r["t"]} for r in rev],
        }

    def buckets(self) -> Dict:
        with self.lock:
            days = self.db.execute(
                "SELECT MIN(day) lo, MAX(day) hi FROM payments").fetchone()
            cells = self.db.execute(
                "SELECT day, status, COUNT(*) c FROM payments GROUP BY day, status"
            ).fetchall()
        if not days or days["lo"] is None:
            return {"timezone": "Europe/Berlin", "days": [],
                    "statuses": list(STATUS_ORDER), "cells": []}
        from datetime import date, timedelta
        first = date.fromisoformat(days["lo"])
        last = date.fromisoformat(days["hi"])
        all_days = [(first + timedelta(days=k)).isoformat()
                    for k in range((last - first).days + 1)]
        got = {(r["day"], r["status"]): r["c"] for r in cells}
        return {"timezone": "Europe/Berlin", "days": all_days,
                "statuses": list(STATUS_ORDER),
                "cells": [{"day": d, "status": s, "count": got.get((d, s), 0)}
                          for d in all_days for s in STATUS_ORDER]}

    def viz_records(self) -> Dict:
        with self.lock:
            rows = self.db.execute(
                "SELECT id, amount_minor, currency, status, created_at, day, version "
                "FROM payments ORDER BY instant ASC, id ASC").fetchall()
        cols = ("id", "amount_minor", "currency", "status", "created_at", "day", "version")
        out: Dict = {"count": len(rows)}
        for c in cols:
            out[c] = [r[c] for r in rows]
        return out

    def rank_under(self, pid: str, status: Optional[str], currency: Optional[str],
                   sort: str) -> Optional[int]:
        row = self.payment_row(pid)
        if row is None:
            return None
        where, args = [], []
        if status:
            where.append("status=?")
            args.append(status)
        if currency:
            where.append("currency=?")
            args.append(currency)
        cmp = {
            "created_at": "(instant < ? OR (instant = ? AND id < ?))",
            "-created_at": "(instant > ? OR (instant = ? AND id > ?))",
            "amount_minor": "(amount_minor < ? OR (amount_minor = ? AND id < ?))",
            "-amount_minor": "(amount_minor > ? OR (amount_minor = ? AND id > ?))",
        }[sort]
        key = row["instant"] if "created_at" in sort else row["amount_minor"]
        w = " AND ".join(where + [cmp])
        with self.lock:
            r = self.db.execute(
                f"SELECT COUNT(*) c FROM payments WHERE {w}",
                args + [key, key, row["id"]]).fetchone()
        return r["c"]

    # ── webhook consumption ──────────────────────────────────────────────────────────────────

    def seen_event(self, event_id: str) -> bool:
        with self.lock:
            return self.db.execute("SELECT 1 FROM webhook_seen WHERE event_id=?",
                                   (event_id,)).fetchone() is not None

    def mark_event_seen(self, event_id: str, db=None) -> None:
        target = db if db is not None else self.db
        with self.lock:
            target.execute("INSERT OR IGNORE INTO webhook_seen(event_id) VALUES(?)",
                           (event_id,))
            if db is None:
                self.db.commit()

    def stage_txn_part(self, txn_id: str, part: int, total: int, payload: Dict) -> List[Dict]:
        """Stage one part durably; returns the FULL ordered part list when complete, else []."""
        with self.tx() as db:
            db.execute("INSERT OR REPLACE INTO txn_stage(txn_id, part, total, payload) "
                       "VALUES(?,?,?,?)", (txn_id, part, total, json.dumps(payload)))
            rows = db.execute("SELECT part, payload FROM txn_stage WHERE txn_id=? "
                              "ORDER BY part", (txn_id,)).fetchall()
            if len(rows) < total:
                return []
            db.execute("DELETE FROM txn_stage WHERE txn_id=?", (txn_id,))
            return [json.loads(r["payload"]) for r in rows]

    def version_of(self, pid: str) -> Optional[int]:
        with self.lock:
            r = self.db.execute("SELECT version FROM payments WHERE id=?", (pid,)).fetchone()
        return r["version"] if r else None

    # ── events API + outbox relay ────────────────────────────────────────────────────────────

    def events_after(self, after: int, limit: int) -> Tuple[List[Dict], int]:
        with self.lock:
            rows = self.db.execute(
                "SELECT * FROM events WHERE seq>? ORDER BY seq LIMIT ?",
                (after, limit)).fetchall()
            latest = self.db.execute("SELECT MAX(seq) m FROM events").fetchone()["m"] or 0
        return [self._event_pub(r) for r in rows], latest

    @staticmethod
    def _event_pub(r: sqlite3.Row) -> Dict:
        return {"seq": r["seq"], "type": r["type"], "payment_id": r["payment_id"],
                "version": r["version"], "source": r["source"],
                "txn": json.loads(r["txn"]) if r["txn"] else None, "at": r["at"],
                **({"draft_id": r["draft_id"]} if r["draft_id"] else {})}

    def outbox_pending(self, batch: int = 50) -> List[Dict]:
        with self.lock:
            rows = self.db.execute(
                "SELECT e.* FROM outbox o JOIN events e ON e.seq=o.seq "
                "WHERE o.delivered=0 ORDER BY o.seq LIMIT ?", (batch,)).fetchall()
        return [self._event_pub(r) for r in rows]

    def outbox_mark_delivered(self, seqs: List[int]) -> None:
        if not seqs:
            return
        with self.tx() as db:
            db.executemany("UPDATE outbox SET delivered=1, delivered_at=? WHERE seq=?",
                           [(now_rfc3339(), s) for s in seqs])

    def outbox_status(self) -> Dict:
        with self.lock:
            pend = self.db.execute(
                "SELECT COUNT(*) c FROM outbox WHERE delivered=0").fetchone()["c"]
            deliv = self.db.execute(
                "SELECT COUNT(*) c, MAX(seq) m FROM outbox WHERE delivered=1").fetchone()
        return {"pending": pend, "delivered": deliv["c"],
                "last_delivered_seq": deliv["m"] or 0}

    # ── drafts ───────────────────────────────────────────────────────────────────────────────

    def create_draft(self, body: Dict) -> Dict:
        with self.tx() as db:
            n = db.execute("SELECT COUNT(*) c FROM drafts").fetchone()["c"]
            did = f"draft_{n:04d}"
            cp = body["counterparty"]
            db.execute("INSERT INTO drafts(id, state, amount_minor, currency, cp_name, "
                       "cp_country, note, created_at) VALUES(?,?,?,?,?,?,?,?)",
                       (did, "draft", body["amount_minor"], body["currency"],
                        cp["name"], cp["country"], body.get("note") or "",
                        now_rfc3339()))
            self._append_event(db, "draft.created", None, None, "approval", None, did)
        return self.draft(did)

    def draft(self, did: str) -> Optional[Dict]:
        with self.lock:
            r = self.db.execute("SELECT * FROM drafts WHERE id=?", (did,)).fetchone()
        if not r:
            return None
        return {"id": r["id"], "state": r["state"], "amount_minor": r["amount_minor"],
                "currency": r["currency"],
                "counterparty": {"name": r["cp_name"], "country": r["cp_country"]},
                "note": r["note"], "created_at": r["created_at"],
                "sent_payment_id": r["sent_payment_id"]}

    def draft_internal(self, did: str) -> Optional[sqlite3.Row]:
        with self.lock:
            return self.db.execute("SELECT * FROM drafts WHERE id=?", (did,)).fetchone()

    def list_drafts(self, state: Optional[str]) -> Tuple[List[Dict], int]:
        q = "SELECT id FROM drafts" + (" WHERE state=?" if state else "") + \
            " ORDER BY created_at DESC, id DESC"
        with self.lock:
            rows = self.db.execute(q, (state,) if state else ()).fetchall()
        data = [self.draft(r["id"]) for r in rows]
        return data, len(data)

    def transition_draft(self, did: str, state: str, etype: str,
                         submitted_by: Optional[str] = None,
                         idempotency_key: Optional[str] = None) -> None:
        with self.tx() as db:
            sets, args = ["state=?"], [state]
            if submitted_by is not None:
                sets.append("submitted_by=?")
                args.append(submitted_by)
            if idempotency_key is not None:
                sets.append("idempotency_key=?")
                args.append(idempotency_key)
            db.execute(f"UPDATE drafts SET {', '.join(sets)} WHERE id=?", args + [did])
            self._append_event(db, etype, None, None, "approval", None, did)

    def mark_draft_sent(self, did: str, payment_id: str, version: int) -> None:
        with self.tx() as db:
            db.execute("UPDATE drafts SET state='sent', sent_payment_id=? WHERE id=?",
                       (payment_id, did))
            self._append_event(db, "payment.sent", payment_id, version, "approval",
                               None, did)

    def unsent_approved(self) -> List[sqlite3.Row]:
        with self.lock:
            return self.db.execute(
                "SELECT * FROM drafts WHERE state='approved'").fetchall()

    # ── validators (conditional walk state) ──────────────────────────────────────────────────

    def validator(self, offset: int) -> Optional[sqlite3.Row]:
        with self.lock:
            return self.db.execute("SELECT * FROM validators WHERE offset=?",
                                   (offset,)).fetchone()

    def validator_set(self, offset: int, etag: str, gen: str,
                      next_cursor: Optional[str]) -> None:
        with self.tx() as db:
            db.execute("INSERT INTO validators(offset, etag, gen, next_cursor) "
                       "VALUES(?,?,?,?) ON CONFLICT(offset) DO UPDATE SET "
                       "etag=excluded.etag, gen=excluded.gen, "
                       "next_cursor=excluded.next_cursor", (offset, etag, gen, next_cursor))

    def validator_drop(self, offset: int) -> None:
        with self.tx() as db:
            db.execute("DELETE FROM validators WHERE offset=?", (offset,))

    def validators_clear(self) -> None:
        with self.tx() as db:
            db.execute("DELETE FROM validators")
