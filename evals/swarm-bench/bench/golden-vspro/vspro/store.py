"""SQLite persistence with Europe/Berlin day-bucketing.

Every operation opens its own short-lived connection in WAL mode so reads keep answering while
a sync writes from another thread (red-team F8). Instants are normalized to UTC at the door
(created_utc) so sorting and bucketing always happen on the instant, never on the vendor's
mixed-offset strings.
"""

from __future__ import annotations

import sqlite3
import threading
from contextlib import contextmanager
from datetime import datetime, timedelta, timezone
from typing import Dict, List, Optional, Tuple
from zoneinfo import ZoneInfo

from . import STATUSES
from .meridian import parse_instant

BERLIN = ZoneInfo("Europe/Berlin")

ROW_KEYS = ("id", "amount_minor", "currency", "created_at", "settled_at", "status",
            "version", "note", "counterparty_name", "country")

SORT_SQL = {
    "created_at": "created_utc ASC, id ASC",
    "-created_at": "created_utc DESC, id DESC",
    "amount_minor": "amount_minor ASC, id ASC",
    "-amount_minor": "amount_minor DESC, id DESC",
}

SCHEMA = """
CREATE TABLE IF NOT EXISTS payments (
    id TEXT PRIMARY KEY,
    amount_minor INTEGER NOT NULL,
    currency TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_utc TEXT NOT NULL,
    settled_at TEXT,
    status TEXT NOT NULL,
    version INTEGER NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    counterparty_name TEXT NOT NULL DEFAULT '',
    country TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_payments_created ON payments(created_utc);
CREATE INDEX IF NOT EXISTS idx_payments_status ON payments(status);
CREATE INDEX IF NOT EXISTS idx_payments_currency ON payments(currency);
CREATE TABLE IF NOT EXISTS webhook_events (id TEXT PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
"""


def normalize_payment(raw: dict) -> dict:
    """Flatten a vendor payment (or webhook event data) into the local row shape."""
    counterparty = raw.get("counterparty") or {}
    amount = raw.get("amount") or {}
    created_at = raw.get("created_at") or raw.get("occurred_at")
    return {
        "id": raw["id"],
        "amount_minor": int(raw.get("amount_minor", amount.get("value_minor", 0))),
        "currency": raw.get("currency") or amount.get("currency") or "",
        "created_at": created_at,
        "settled_at": raw.get("settled_at"),
        "status": raw.get("status") or "",
        "version": int(raw.get("version") or 1),
        "note": raw.get("note") or "",
        "counterparty_name": counterparty.get("name") or raw.get("counterparty_name") or "",
        "country": counterparty.get("country") or raw.get("country") or "",
    }


class Store:
    def __init__(self, path: str) -> None:
        self.path = path
        self._write_lock = threading.Lock()
        with self._conn() as conn:
            conn.executescript(SCHEMA)

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.path, timeout=10)
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA synchronous=NORMAL")
        return conn

    @contextmanager
    def _conn(self):
        conn = self._connect()
        try:
            with conn:
                yield conn
        finally:
            conn.close()

    # ── writes ────────────────────────────────────────────────────────────────────────────────

    def upsert_many(self, payments: List[dict]) -> Tuple[int, int]:
        inserted = updated = 0
        with self._write_lock, self._conn() as conn:
            for raw in payments:
                row = normalize_payment(raw)
                row["created_utc"] = (parse_instant(row["created_at"])
                                      .astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
                existing = conn.execute("SELECT version FROM payments WHERE id = ?",
                                        (row["id"],)).fetchone()
                # One atomic statement: insert, or merge in place — and a list snapshot older
                # than (or equal to) a webhook-applied version must never regress it, which the
                # WHERE guard enforces inside the same statement.
                cur = conn.execute(
                    "INSERT INTO payments (id, amount_minor, currency, created_at,"
                    " created_utc, settled_at, status, version, note, counterparty_name,"
                    " country) VALUES (:id, :amount_minor, :currency, :created_at,"
                    " :created_utc, :settled_at, :status, :version, :note,"
                    " :counterparty_name, :country)"
                    " ON CONFLICT(id) DO UPDATE SET"
                    " amount_minor=excluded.amount_minor, currency=excluded.currency,"
                    " created_at=excluded.created_at, created_utc=excluded.created_utc,"
                    " settled_at=excluded.settled_at, status=excluded.status,"
                    " version=excluded.version, note=excluded.note,"
                    " counterparty_name=excluded.counterparty_name, country=excluded.country"
                    " WHERE excluded.version > payments.version", row)
                if existing is None:
                    inserted += 1
                elif cur.rowcount > 0 and row["version"] > existing["version"]:
                    updated += 1
        return inserted, updated

    def apply_event(self, event: dict) -> str:
        event_id = event.get("id")
        data = event.get("data") or {}
        with self._write_lock, self._conn() as conn:
            seen = conn.execute("SELECT 1 FROM webhook_events WHERE id = ?",
                                (event_id,)).fetchone()
            if seen:
                return "duplicate"
            row = normalize_payment(data)
            row["created_utc"] = (parse_instant(row["created_at"])
                                  .astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
            existing = conn.execute("SELECT version FROM payments WHERE id = ?",
                                    (row["id"],)).fetchone()
            if existing is not None and row["version"] <= existing["version"]:
                return "stale"
            if existing is None:
                conn.execute(
                    "INSERT INTO payments (id, amount_minor, currency, created_at, created_utc,"
                    " settled_at, status, version, note, counterparty_name, country)"
                    " VALUES (:id, :amount_minor, :currency, :created_at, :created_utc,"
                    " :settled_at, :status, :version, :note, :counterparty_name, :country)", row)
            else:
                conn.execute(
                    "UPDATE payments SET amount_minor=:amount_minor, currency=:currency,"
                    " created_at=:created_at, created_utc=:created_utc, settled_at=:settled_at,"
                    " status=:status, version=:version, note=:note,"
                    " counterparty_name=:counterparty_name, country=:country WHERE id=:id", row)
            conn.execute("INSERT INTO webhook_events (id, applied_at) VALUES (?, ?)",
                         (event_id, datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")))
            return "applied"

    def upsert_one(self, raw: dict) -> None:
        self.upsert_many([raw])

    # ── reads ─────────────────────────────────────────────────────────────────────────────────

    def query(self, limit: int, offset: int, status: Optional[str] = None,
              currency: Optional[str] = None, sort: str = "created_at") -> Tuple[List[dict], int]:
        where, params = [], []
        if status:
            where.append("status = ?")
            params.append(status)
        if currency:
            where.append("currency = ?")
            params.append(currency)
        clause = (" WHERE " + " AND ".join(where)) if where else ""
        order = SORT_SQL.get(sort, SORT_SQL["created_at"])
        with self._conn() as conn:
            total = conn.execute(f"SELECT COUNT(*) FROM payments{clause}", params).fetchone()[0]
            rows = conn.execute(
                f"SELECT * FROM payments{clause} ORDER BY {order} LIMIT ? OFFSET ?",
                params + [limit, offset]).fetchall()
        return [{k: r[k] for k in ROW_KEYS} for r in rows], total

    def get(self, payment_id: str) -> Optional[dict]:
        with self._conn() as conn:
            row = conn.execute("SELECT * FROM payments WHERE id = ?", (payment_id,)).fetchone()
        return {k: row[k] for k in ROW_KEYS} if row else None

    def count(self) -> int:
        with self._conn() as conn:
            return conn.execute("SELECT COUNT(*) FROM payments").fetchone()[0]

    def summary(self) -> dict:
        with self._conn() as conn:
            count = conn.execute("SELECT COUNT(*) FROM payments").fetchone()[0]
            bounds = conn.execute(
                "SELECT MIN(created_utc), MAX(created_utc) FROM payments").fetchone()
            by_currency = conn.execute(
                "SELECT currency, COUNT(*) AS n, SUM(amount_minor) AS s FROM payments"
                " GROUP BY currency ORDER BY currency ASC").fetchall()
        return {
            "count": count,
            "last_sync": self.last_sync(),
            "oldest": bounds[0],
            "newest": bounds[1],
            "by_currency": [{"currency": r["currency"], "count": r["n"],
                             "total_minor": r["s"]} for r in by_currency],
        }

    def buckets(self) -> List[dict]:
        """One cell per (day, status), Berlin calendar days of the INSTANT, zero-filled across
        the full span, day-major, statuses in the frozen order."""
        with self._conn() as conn:
            rows = conn.execute("SELECT created_utc, status FROM payments").fetchall()
        counts: Dict[Tuple[str, str], int] = {}
        days_seen = []
        for row in rows:
            instant = parse_instant(row["created_utc"])
            day = instant.astimezone(BERLIN).date()
            days_seen.append(day)
            key = (day.isoformat(), row["status"])
            counts[key] = counts.get(key, 0) + 1
        if not days_seen:
            return []
        first, last = min(days_seen), max(days_seen)
        cells = []
        day = first
        while day <= last:
            iso = day.isoformat()
            for status in STATUSES:
                cells.append({"day": iso, "status": status,
                              "count": counts.get((iso, status), 0)})
            day += timedelta(days=1)
        return cells

    def bucket_days(self) -> List[str]:
        cells = self.buckets()
        days = []
        for cell in cells:
            if not days or days[-1] != cell["day"]:
                days.append(cell["day"])
        return days

    # ── meta ──────────────────────────────────────────────────────────────────────────────────

    def last_sync(self) -> Optional[str]:
        with self._conn() as conn:
            row = conn.execute("SELECT value FROM meta WHERE key = 'last_sync'").fetchone()
        return row[0] if row else None

    def set_last_sync(self, when: str) -> None:
        with self._write_lock, self._conn() as conn:
            conn.execute("INSERT INTO meta (key, value) VALUES ('last_sync', ?)"
                         " ON CONFLICT(key) DO UPDATE SET value = excluded.value", (when,))
