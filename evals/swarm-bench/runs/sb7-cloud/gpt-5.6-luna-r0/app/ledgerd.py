import argparse
import hashlib
import hmac
import json
import os
import re
import sqlite3
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from zoneinfo import ZoneInfo

STATUSES = ["settled", "pending", "refunded", "failed"]
CURRENCIES = ["EUR", "USD", "JPY", "KWD"]
EXPONENT = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
OUTBOX_TYPES = {"draft.submitted", "draft.approved", "draft.rejected", "reversal.created", "payment.sent"}
BERLIN = ZoneInfo("Europe/Berlin")


def now():
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def instant(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)


def error(code, message, status=400, fields=None):
    result = {"error": {"code": code, "message": message}}
    if fields:
        result["error"]["field_errors"] = fields
    return result, status


def flat(payment):
    counterparty = payment.get("counterparty") or {}
    return {"id": payment["id"], "amount_minor": payment["amount_minor"],
            "currency": payment["currency"], "created_at": payment["created_at"],
            "settled_at": payment.get("settled_at"), "status": payment["status"],
            "version": int(payment["version"]), "note": payment.get("note", ""),
            "counterparty_name": counterparty.get("name", ""),
            "country": counterparty.get("country", "")}


def public_draft(row):
    return {"id": row["id"], "state": row["state"], "amount_minor": row["amount_minor"],
            "currency": row["currency"], "counterparty": {"name": row["name"], "country": row["country"]},
            "note": row["note"], "created_at": row["created_at"]}


class Ledger:
    def __init__(self, args):
        self.args = args
        self.db_path = os.path.join(args.db_dir, "ledger.db")
        os.makedirs(args.db_dir, exist_ok=True)
        self.db = sqlite3.connect(self.db_path, check_same_thread=False, timeout=30)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA busy_timeout=30000")
        self.lock = threading.RLock()
        self.subscribers = []
        self.sub_lock = threading.Lock()
        self.tokens = getattr(args, "tokens", {})
        self.token_roles = {token: role for role, token in getattr(args, "token_values", {}).items()}
        self.secret = None
        self.webhook = {"registered": False, "received": 0, "applied": 0, "ignored": 0, "rejected": 0}
        self.webhook_lock = threading.Lock()
        self.sync_lock = threading.Lock()
        self.init_db()

    def init_db(self):
        with self.lock, self.db:
            self.db.executescript("""
            CREATE TABLE IF NOT EXISTS payments(
              id TEXT PRIMARY KEY, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL,
              created_at TEXT NOT NULL, settled_at TEXT, status TEXT NOT NULL, version INTEGER NOT NULL,
              note TEXT NOT NULL DEFAULT '', counterparty_name TEXT NOT NULL, country TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS reversals(
              id TEXT PRIMARY KEY, payment_id TEXT NOT NULL, amount_minor INTEGER NOT NULL,
              currency TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(
              seq INTEGER PRIMARY KEY AUTOINCREMENT, type TEXT NOT NULL, payment_id TEXT,
              version INTEGER, source TEXT NOT NULL, txn TEXT, at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox(seq INTEGER PRIMARY KEY, delivered INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS drafts(
              id TEXT PRIMARY KEY, state TEXT NOT NULL, amount_minor INTEGER NOT NULL, currency TEXT NOT NULL,
              name TEXT NOT NULL, country TEXT NOT NULL, note TEXT NOT NULL, created_at TEXT NOT NULL,
              submitter TEXT, idempotency_key TEXT, sent INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS webhook_ids(id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS txn_parts(tid TEXT NOT NULL, part INTEGER NOT NULL, ofn INTEGER NOT NULL,
              payload TEXT NOT NULL, event_id TEXT NOT NULL, PRIMARY KEY(tid,part));
            CREATE INDEX IF NOT EXISTS payments_status_currency ON payments(status,currency);
            CREATE INDEX IF NOT EXISTS payments_created_at ON payments(created_at,id);
            CREATE INDEX IF NOT EXISTS events_seq_type ON events(seq,type);
            CREATE INDEX IF NOT EXISTS outbox_pending ON outbox(delivered,seq);
            """)
            # These are safe migrations for databases made by the starter implementation.
            for sql in ("ALTER TABLE drafts ADD COLUMN sent INTEGER NOT NULL DEFAULT 0",
                        "ALTER TABLE txn_parts ADD COLUMN event_id TEXT NOT NULL DEFAULT ''"):
                try:
                    self.db.execute(sql)
                except sqlite3.OperationalError:
                    pass

    def meta(self, key, default=None):
        with self.lock:
            row = self.db.execute("SELECT v FROM meta WHERE k=?", (key,)).fetchone()
            return row[0] if row else default

    def set_meta(self, key, value):
        self.db.execute("INSERT INTO meta(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", (key, str(value)))

    def event(self, typ, payment_id=None, version=None, source="sync", txn=None):
        at = now()
        cur = self.db.execute("INSERT INTO events(type,payment_id,version,source,txn,at) VALUES(?,?,?,?,?,?)",
                              (typ, payment_id, version, source, json.dumps(txn) if txn else None, at))
        seq = cur.lastrowid
        if typ in OUTBOX_TYPES:
            self.db.execute("INSERT INTO outbox(seq,delivered) VALUES(?,0)", (seq,))
        return {"seq": seq, "type": typ, "payment_id": payment_id, "version": version,
                "source": source, "txn": txn, "at": at}

    def _upsert_payment(self, payment, source="sync", txn=None):
        old = self.db.execute("SELECT * FROM payments WHERE id=?", (payment["id"],)).fetchone()
        version = int(payment["version"])
        if version < 1:
            raise ValueError("invalid payment version")
        if old and version <= old["version"]:
            return None
        p = flat(payment)
        if old:
            # These fields are immutable in Meridian.  Silently accepting a
            # contradictory update would corrupt the local ledger and make a
            # later version impossible to reconcile.
            for field in ("amount_minor", "currency", "created_at", "counterparty_name", "country"):
                if p[field] != old[field]:
                    raise ValueError("immutable payment field changed")
            # A settlement timestamp, once known, must not be erased by a
            # partial/older representation of a newer status update.
            if old["settled_at"] is not None and p["settled_at"] is None:
                p["settled_at"] = old["settled_at"]
        self.db.execute("""INSERT INTO payments VALUES(?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(id) DO UPDATE SET settled_at=excluded.settled_at,status=excluded.status,
            version=excluded.version,note=excluded.note""", tuple(p.values()))
        return self.event("payment.created" if old is None else "payment.updated", p["id"], version, source, txn), p

    def apply_payment(self, payment, source="sync", txn=None):
        with self.lock, self.db:
            result = self._upsert_payment(payment, source, txn)
        if result:
            self.broadcast([result[1]])
            return True
        return False

    def vendor_request(self, method, path, body=None, headers=None, retries=True):
        base_headers = {"Authorization": "Bearer sk_test_meridian", "Accept": "application/json"}
        if body is not None:
            base_headers["Content-Type"] = "application/json"
        base_headers.update(headers or {})
        data = json.dumps(body, separators=(",", ":")).encode() if body is not None else None
        attempts = 2 if retries else 1
        last = None
        for attempt in range(attempts):
            try:
                request = urllib.request.Request(self.args.vendor.rstrip("/") + path, data=data,
                                                 headers=base_headers, method=method)
                with urllib.request.urlopen(request, timeout=10) as response:
                    raw = response.read()
                    return (json.loads(raw) if raw else {}), dict(response.headers), response.status
            except urllib.error.HTTPError as exc:
                last = exc
                if exc.code == 304:
                    return {}, dict(exc.headers), 304
                # Never transparently repeat a non-idempotent request. Payment sends
                # are safe to retry only when the caller supplied an idempotency key.
                can_retry = method in {"GET", "HEAD", "PATCH"} or "Idempotency-Key" in base_headers
                if exc.code == 500 and attempt == 0 and can_retry:
                    try: time.sleep(min(10, max(0, float(exc.headers.get("Retry-After", "0")))))
                    except ValueError: pass
                    continue
                raise
            except (urllib.error.URLError, TimeoutError, ConnectionError, OSError) as exc:
                last = exc
                can_retry = method in {"GET", "HEAD", "PATCH"} or "Idempotency-Key" in base_headers
                if attempt == 0 and can_retry:
                    time.sleep(0.2)
                    continue
                raise
        raise last or OSError("vendor unavailable")

    def page_key(self, cursor):
        return "page:first" if not cursor else "page:" + cursor

    def sync(self):
        if not self.sync_lock.acquire(blocking=False):
            raise RuntimeError("sync already running")
        try:
            fetched = inserted = updated = 0
            cursor = None
            restarted = False
            while True:
                key = self.page_key(cursor)
                etag = self.meta(key + ":etag")
                stored_generation = self.meta(key + ":generation")
                headers = {"If-None-Match": etag} if etag else {}
                try:
                    body, response_headers, status = self.vendor_request("GET", "/v3/payments" +
                        (("?cursor=" + urllib.parse.quote(cursor, safe="")) if cursor else ""), headers=headers)
                except urllib.error.HTTPError as exc:
                    if exc.code == 410 and not restarted:
                        cursor = None; restarted = True; continue
                    raise
                generation = response_headers.get("X-Collection-Generation")
                if status == 304 and stored_generation != generation:
                    # One unconditional refetch for this page; do not loop on the same validator.
                    body, response_headers, status = self.vendor_request("GET", "/v3/payments" +
                        (("?cursor=" + urllib.parse.quote(cursor, safe="")) if cursor else ""), headers={})
                    generation = response_headers.get("X-Collection-Generation")
                if status == 304:
                    cached = self.meta(key + ":body")
                    if not cached:
                        # A validator without a durable body is not useful.  This can
                        # happen after an interrupted first sync, so recover with one
                        # unconditional request rather than treating an empty cache as
                        # a successful page.
                        body, response_headers, status = self.vendor_request("GET", "/v3/payments" +
                            (("?cursor=" + urllib.parse.quote(cursor, safe="")) if cursor else ""), headers={})
                        generation = response_headers.get("X-Collection-Generation")
                    else:
                        body = json.loads(cached)
                page_records = []
                # The page body, validator, generation, payment mutations, and
                # last-known generation are one durable unit.  A crash cannot leave
                # a page cache claiming work which was never applied.
                with self.lock, self.db:
                    if status != 304:
                        self.set_meta(key + ":body", json.dumps(body, separators=(",", ":")))
                        if response_headers.get("ETag"):
                            self.set_meta(key + ":etag", response_headers["ETag"])
                        if generation:
                            self.set_meta(key + ":generation", generation)
                    if generation:
                        self.set_meta("generation", generation)
                    for payment in body.get("data", []):
                        fetched += 1
                        old = self.db.execute("SELECT version FROM payments WHERE id=?", (payment["id"],)).fetchone()
                        result = self._upsert_payment(payment, "sync")
                        if result:
                            page_records.append(result[1])
                            if old is None:
                                inserted += 1
                            else:
                                updated += 1
                self.broadcast(page_records)
                cursor = body.get("next_cursor")
                if not cursor: break
            reversals, _, _ = self.vendor_request("GET", "/v3/reversals")
            with self.lock, self.db:
                for reversal in reversals.get("data", []):
                    exists = self.db.execute("SELECT 1 FROM reversals WHERE id=?", (reversal["id"],)).fetchone()
                    if not exists:
                        self.db.execute("INSERT INTO reversals VALUES(?,?,?,?,?)", (reversal["id"], reversal["payment_id"], reversal["amount_minor"], reversal["currency"], reversal["created_at"]))
                        self.event("reversal.created", reversal["payment_id"], None, "sync")
                self.set_meta("last_sync", now())
            return {"fetched": fetched, "inserted": inserted, "updated": updated, "total": self.count()}
        finally:
            self.sync_lock.release()

    def set_page_cache(self, key, body, etag, generation):
        with self.lock, self.db:
            self.set_meta(key + ":body", json.dumps(body, separators=(",", ":")))
            if etag: self.set_meta(key + ":etag", etag)
            if generation: self.set_meta(key + ":generation", generation)

    def count(self):
        with self.lock:
            return self.db.execute("SELECT count(*) FROM payments").fetchone()[0]

    def register_loop(self):
        while not self.webhook["registered"]:
            try:
                result, _, _ = self.vendor_request("POST", "/v3/webhooks", {"url": "http://127.0.0.1:%d/api/webhooks/meridian" % self.args.port})
                self.secret = result.get("secret")
                if self.secret: self.webhook["registered"] = True
            except Exception:
                time.sleep(5)

    def broadcast(self, records):
        if not records: return
        with self.sub_lock:
            targets = list(self.subscribers)
        for sub in targets:
            sub.put(records)

    def relay_loop(self):
        while True:
            try:
                with self.lock:
                    rows = self.db.execute("SELECT o.seq,e.type,e.payment_id,e.version,e.source,e.txn,e.at FROM outbox o JOIN events e ON e.seq=o.seq WHERE o.delivered=0 ORDER BY o.seq LIMIT 50").fetchall()
                if not rows:
                    time.sleep(.35); continue
                events = []
                for row in rows:
                    event = dict(row)
                    event["txn"] = json.loads(event["txn"]) if event["txn"] else None
                    events.append(event)
                request = urllib.request.Request(self.args.notifier.rstrip("/") + "/notify/events",
                    data=json.dumps({"events": events}, separators=(",", ":")).encode(),
                    headers={"Content-Type": "application/json"}, method="POST")
                with urllib.request.urlopen(request, timeout=3) as response:
                    if 200 <= response.status < 300:
                        with self.lock, self.db:
                            self.db.executemany("UPDATE outbox SET delivered=1 WHERE seq=? AND delivered=0", [(r["seq"],) for r in rows])
                time.sleep(.05)
            except Exception:
                time.sleep(1)

    def send_approved(self, draft_id):
        with self.lock:
            row = self.db.execute("SELECT * FROM drafts WHERE id=?", (draft_id,)).fetchone()
        if not row or row["state"] != "approved" or row["sent"]: return
        key = row["idempotency_key"]
        try:
            payment, _, _ = self.vendor_request("POST", "/v3/payments", {
                "amount_minor": row["amount_minor"], "currency": row["currency"], "note": row["note"],
                "counterparty": {"name": row["name"], "country": row["country"]}}, {"Idempotency-Key": key})
            with self.lock, self.db:
                current = self.db.execute("SELECT sent,state FROM drafts WHERE id=?", (draft_id,)).fetchone()
                if current and current["state"] == "approved" and not current["sent"]:
                    self.db.execute("UPDATE drafts SET sent=1,state='sent' WHERE id=?", (draft_id,))
                    self.event("payment.sent", payment.get("id"), payment.get("version"), "approval")
        except Exception:
            pass

    def draft_recovery_loop(self):
        while True:
            try:
                with self.lock:
                    rows = self.db.execute("SELECT id FROM drafts WHERE state='approved' AND sent=0 AND idempotency_key IS NOT NULL").fetchall()
                for row in rows: self.send_approved(row["id"])
            except Exception:
                pass
            time.sleep(1)


class StreamQueue:
    def __init__(self):
        self.items = []; self.cond = threading.Condition(); self.closed = False
    def put(self, records):
        with self.cond:
            self.items.append(records); self.cond.notify()
    def get(self):
        with self.cond:
            while not self.items and not self.closed: self.cond.wait(15)
            if self.items: return self.items.pop(0)
            return None


class Handler(BaseHTTPRequestHandler):
    server_version = "MeridianLedger/1"
    def log_message(self, *args): pass
    @property
    def ledger(self): return self.server.ledger
    def json(self, value, status=200, headers=None):
        data = json.dumps(value, separators=(",", ":")).encode()
        self.send_response(status); self.send_header("Content-Type", "application/json; charset=utf-8"); self.send_header("Content-Length", str(len(data)))
        for key, val in (headers or {}).items(): self.send_header(key, str(val))
        self.end_headers(); self.wfile.write(data)
    def read_json(self):
        length = int(self.headers.get("Content-Length", "0")); return json.loads(self.rfile.read(length) or b"{}")
    def auth_identity(self, allowed=None):
        value = self.headers.get("Authorization", "")
        # RFC 6750 bearer credentials have exactly one scheme and a non-empty
        # credential; accepting prefixes or whitespace creates surprising auth bypasses.
        token = value[7:] if value.startswith("Bearer ") and value[7:] and not value[7:].isspace() and " " not in value[7:] else ""
        role = self.ledger.tokens.get(token)
        if not role:
            self.json(*error("unauthorized", "Authentication required", 401)); return None
        if allowed and role not in allowed:
            self.json(*error("forbidden", "This role is not allowed", 403)); return None
        return token, role

    def auth(self, allowed=None):
        identity = self.auth_identity(allowed)
        return identity[1] if identity else None
    def static(self, path):
        filename = "web/index.html" if path == "/" else path.lstrip("/")
        if filename not in {"web/index.html", "web/styles.css", "web/app.js", "web/viz.js"}:
            return self.json(*error("not_found", "Not found", 404))
        try:
            with open(filename, "rb") as f: data = f.read()
        except OSError: return self.json(*error("not_found", "Not found", 404))
        ct = {".html": "text/html", ".css": "text/css", ".js": "text/javascript"}[os.path.splitext(filename)[1]]
        self.send_response(200); self.send_header("Content-Type", ct); self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path); query = urllib.parse.parse_qs(parsed.query); l = self.ledger
        try:
            if parsed.path == "/" or parsed.path.startswith("/web/"): return self.static(parsed.path)
            if parsed.path == "/api/health":
                return self.json({"status": "ok", "payments": l.count(), "last_sync": l.meta("last_sync"), "webhook": dict(l.webhook)})
            if parsed.path == "/api/payments": return self.payments(query)
            if parsed.path.startswith("/api/payments/"):
                pid = parsed.path.rsplit("/", 1)[1]
                with l.lock: row = l.db.execute("SELECT * FROM payments WHERE id=?", (pid,)).fetchone()
                return self.json(dict(row) if row else error("not_found", "Payment not found", 404)[0], 200 if row else 404)
            if parsed.path == "/api/summary": return self.summary()
            if parsed.path == "/api/buckets": return self.buckets()
            if parsed.path == "/api/viz/records": return self.viz()
            if parsed.path == "/api/events":
                if not self.auth(): return
                after = self.integer_query(query, "after", 0, False); limit = self.integer_query(query, "limit", 100, False)
                if after is None or limit is None: return
                with l.lock:
                    rows = l.db.execute("SELECT seq,type,payment_id,version,source,txn,at FROM events WHERE seq>? ORDER BY seq LIMIT ?", (after, min(limit, 200))).fetchall()
                    latest = l.db.execute("SELECT coalesce(max(seq),0) FROM events").fetchone()[0]
                result = [dict(x) for x in rows]
                for x in result: x["txn"] = json.loads(x["txn"]) if x["txn"] else None
                return self.json({"events": result, "latest_seq": latest})
            if parsed.path == "/api/outbox/status": return self.outbox_status()
            if parsed.path == "/api/notifications": return self.notifications(parsed.query)
            if parsed.path == "/api/drafts":
                if not self.auth(): return
                state = query.get("state", [None])[0]
                if state and state not in {"draft", "submitted", "approved", "rejected", "sent"}:
                    return self.json(*error("bad_request", "Invalid state", 400, [{"path": "state", "code": "unsupported"}]))
                with l.lock:
                    rows = l.db.execute("SELECT * FROM drafts" + (" WHERE state=?" if state else "") + " ORDER BY created_at DESC", (state,) if state else ()).fetchall()
                return self.json({"data": [public_draft(x) for x in rows], "total": len(rows)})
            if parsed.path == "/api/stream": return self.stream()
            return self.json(*error("not_found", "Not found", 404))
        except (ValueError, TypeError): return self.json(*error("bad_request", "Invalid parameter", 400))
        except Exception: return self.json(*error("bad_request", "Request could not be completed", 400))
    def integer_query(self, query, name, default, nonnegative=True):
        raw = query.get(name, [str(default)])[0]
        try: value = int(raw)
        except ValueError:
            self.json(*error("bad_request", "Invalid " + name, 400, [{"path": name, "code": "not_an_integer"}])); return None
        if nonnegative and value < 0:
            self.json(*error("bad_request", "Invalid " + name, 400, [{"path": name, "code": "not_positive"}])); return None
        return value
    def payments(self, query):
        l = self.ledger; limit = self.integer_query(query, "limit", 50); offset = self.integer_query(query, "offset", 0)
        if limit is None or offset is None: return
        status = query.get("status", [""])[0]; currency = query.get("currency", [""])[0]; sort = query.get("sort", ["created_at"])[0]
        fields = []
        if status:
            if status not in STATUSES: return self.json(*error("bad_request", "Invalid status", 400, [{"path": "status", "code": "unsupported"}]))
            fields.append(("status", status))
        if currency:
            if currency not in CURRENCIES: return self.json(*error("bad_request", "Invalid currency", 400, [{"path": "currency", "code": "unsupported"}]))
            fields.append(("currency", currency))
        if sort not in {"created_at", "-created_at", "amount_minor", "-amount_minor"}:
            return self.json(*error("bad_request", "Invalid sort", 400, [{"path": "sort", "code": "unsupported"}]))
        where = " AND ".join(k + "=?" for k, _ in fields); vals = [v for _, v in fields]; where_sql = (" WHERE " + where) if where else ""
        direction = "DESC" if sort.startswith("-") else "ASC"; column = sort.lstrip("-")
        order = column + " " + direction + ", id ASC"
        with l.lock:
            all_rows = l.db.execute("SELECT * FROM payments" + where_sql, vals).fetchall()
        total = len(all_rows)
        key = (lambda r: instant(r["created_at"])) if column == "created_at" else (lambda r: r["amount_minor"])
        all_rows.sort(key=lambda r: (key(r), r["id"]), reverse=direction == "DESC")
        rows = all_rows[offset:offset + min(limit, 200)]
        return self.json({"data": [dict(x) for x in rows], "total": total, "limit": min(limit, 200), "offset": offset})
    def summary(self):
        l = self.ledger
        with l.lock:
            currencies = l.db.execute("SELECT currency,count(*) count,sum(amount_minor) total_minor FROM payments GROUP BY currency ORDER BY currency").fetchall()
            reversals = l.db.execute("SELECT currency,count(*) count,sum(amount_minor) total_minor FROM reversals GROUP BY currency ORDER BY currency").fetchall()
            dates = [r[0] for r in l.db.execute("SELECT created_at FROM payments").fetchall()]
        ordered = sorted((instant(x) for x in dates))
        def utc(s): return s.isoformat().replace("+00:00", "Z") if s else None
        return self.json({"count": l.count(), "last_sync": l.meta("last_sync"), "oldest": utc(ordered[0] if ordered else None), "newest": utc(ordered[-1] if ordered else None),
            "by_currency": [{"currency": r["currency"], "count": r["count"], "total_minor": r["total_minor"]} for r in currencies],
            "reversals": [{"currency": r["currency"], "count": r["count"], "total_minor": r["total_minor"]} for r in reversals]})
    def buckets(self):
        l = self.ledger
        with l.lock: rows = l.db.execute("SELECT created_at,status,count(*) n FROM payments GROUP BY created_at,status").fetchall()
        counts = {}
        for row in rows:
            day = instant(row["created_at"]).astimezone(BERLIN).date().isoformat(); counts[(day, row["status"])] = row["n"]
        if not counts: return self.json({"timezone": "Europe/Berlin", "days": [], "statuses": STATUSES, "cells": []})
        first, last = min(x[0] for x in counts), max(x[0] for x in counts); d = datetime.fromisoformat(first).date(); end = datetime.fromisoformat(last).date(); days = []
        while d <= end: days.append(d.isoformat()); d += timedelta(days=1)
        return self.json({"timezone": "Europe/Berlin", "days": days, "statuses": STATUSES, "cells": [{"day": day, "status": st, "count": counts.get((day, st), 0)} for day in days for st in STATUSES]})
    def viz(self):
        l = self.ledger
        with l.lock: rows = l.db.execute("SELECT * FROM payments ORDER BY created_at ASC,id ASC").fetchall()
        return self.json({"count": len(rows), "id": [r["id"] for r in rows], "amount_minor": [r["amount_minor"] for r in rows], "currency": [r["currency"] for r in rows], "status": [r["status"] for r in rows], "created_at": [r["created_at"] for r in rows], "day": [instant(r["created_at"]).astimezone(BERLIN).date().isoformat() for r in rows], "version": [r["version"] for r in rows]})
    def outbox_status(self):
        l = self.ledger
        with l.lock:
            pending = l.db.execute("SELECT count(*) FROM outbox WHERE delivered=0").fetchone()[0]; delivered = l.db.execute("SELECT count(*) FROM outbox WHERE delivered=1").fetchone()[0]; last = l.db.execute("SELECT coalesce(max(seq),0) FROM outbox WHERE delivered=1").fetchone()[0]
        try:
            urllib.request.urlopen(l.args.notifier.rstrip("/") + "/health", timeout=1).close(); state = "up"
        except Exception: state = "down"
        return self.json({"pending": pending, "delivered": delivered, "last_delivered_seq": last, "notifier": state})
    def notifications(self, query):
        try:
            url = self.ledger.args.notifier.rstrip("/") + "/notify/notifications" + ("?" + query if query else "")
            with urllib.request.urlopen(url, timeout=2) as r: return self.json(json.loads(r.read()))
        except Exception: return self.json(*error("notifier_unreachable", "Notifier is unavailable", 502))
    def stream(self):
        queue = StreamQueue()
        with self.ledger.sub_lock: self.ledger.subscribers.append(queue)
        self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.send_header("Cache-Control", "no-cache"); self.send_header("Connection", "keep-alive"); self.end_headers()
        batch = 0
        try:
            while True:
                records = queue.get()
                if records is None: break
                batch += 1
                payload = {"batch": batch, "records": records}
                self.wfile.write(("data: " + json.dumps(payload, separators=(",", ":")) + "\n\n").encode()); self.wfile.flush()
        except Exception: pass
        finally:
            with self.ledger.sub_lock:
                if queue in self.ledger.subscribers: self.ledger.subscribers.remove(queue)
    def webhook(self):
        l = self.ledger
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        signature = self.headers.get("Meridian-Signature", "")
        # The registration challenge is the sole unsigned request. For every delivery,
        # authenticate the exact bytes before allowing JSON fields to influence state.
        if not signature:
            try:
                challenge = json.loads(raw)
            except Exception:
                challenge = None
            if isinstance(challenge, dict) and challenge.get("type") == "webhook.verify":
                return self.json({"challenge": challenge.get("challenge")})
        with l.webhook_lock:
            l.webhook["received"] += 1
        valid = False
        try:
            bits = {}
            for component in signature.split(","):
                key, value = component.split("=", 1)
                bits[key.strip()] = value
            timestamp = bits["t"]
            provided = bits["v1"]
            expected = hmac.new((l.secret or "").encode(), (timestamp + ".").encode() + raw, hashlib.sha256).hexdigest()
            valid = hmac.compare_digest(expected, provided)
        except Exception:
            pass
        if not valid:
            with l.webhook_lock:
                l.webhook["rejected"] += 1
            return self.json(*error("bad_signature", "Invalid signature", 401))
        try:
            obj = json.loads(raw)
            if not isinstance(obj, dict) or not isinstance(obj.get("id"), str):
                raise ValueError("invalid event")
        except Exception:
            with l.webhook_lock:
                l.webhook["rejected"] += 1
            return self.json(*error("bad_request", "Invalid webhook event", 400))
        event_id = obj["id"]
        txn = obj.get("txn")
        if txn is not None:
            if (not isinstance(txn, dict) or not isinstance(txn.get("id"), str)
                    or not isinstance(txn.get("part"), int) or not isinstance(txn.get("of"), int)
                    or txn["of"] < 1 or txn["part"] < 1 or txn["part"] > txn["of"]):
                with l.webhook_lock:
                    l.webhook["rejected"] += 1
                return self.json(*error("bad_request", "Invalid transaction group", 400))
        broadcasts = []
        with l.lock, l.db:
            if l.db.execute("SELECT 1 FROM webhook_ids WHERE id=?", (event_id,)).fetchone():
                with l.webhook_lock:
                    l.webhook["ignored"] += 1
                return self.json({"received": True})
            if txn:
                # A staged part is durable, but no payment/reversal is made
                # visible until every distinct part is present.
                l.db.execute("INSERT OR IGNORE INTO txn_parts(tid,part,ofn,payload,event_id) VALUES(?,?,?,?,?)",
                             (txn["id"], txn["part"], txn["of"], raw.decode("utf-8"), event_id))
                parts = l.db.execute("SELECT payload,event_id,part,ofn FROM txn_parts WHERE tid=? ORDER BY part", (txn["id"],)).fetchall()
                if any(row["ofn"] != txn["of"] for row in parts) or len(parts) < txn["of"]:
                    return self.json({"received": True})
                if len({row["part"] for row in parts}) != txn["of"]:
                    return self.json({"received": True})
                events = [json.loads(row["payload"]) for row in parts]
            else:
                events = [obj]
            changed = False
            for item in events:
                if not isinstance(item.get("data"), dict):
                    raise ValueError("invalid webhook data")
                data = item["data"]
                if item.get("type") == "reversal.created":
                    if not l.db.execute("SELECT 1 FROM reversals WHERE id=?", (data.get("id"),)).fetchone():
                        l.db.execute("INSERT INTO reversals VALUES(?,?,?,?,?)", (data["id"], data["payment_id"], data["amount_minor"], data["currency"], data["created_at"]))
                        l.event("reversal.created", data["payment_id"], None, "webhook", item.get("txn")); changed = True
                elif item.get("type") in {"payment.created", "payment.updated"}:
                    result = l._upsert_payment(data, "webhook", item.get("txn"))
                    if result:
                        broadcasts.append(result[1]); changed = True
                # Mark every part only in the same transaction as the data it
                # caused.  A failed update therefore remains safely retryable.
                l.db.execute("INSERT OR IGNORE INTO webhook_ids(id) VALUES(?)", (item.get("id"),))
            if txn:
                l.db.execute("DELETE FROM txn_parts WHERE tid=?", (txn["id"],))
        if broadcasts:
            l.broadcast(broadcasts)
        with l.webhook_lock:
            l.webhook["applied" if changed else "ignored"] += 1
        return self.json({"received": True})
    def do_POST(self):
        parsed = urllib.parse.urlparse(self.path); path = parsed.path; l = self.ledger
        try:
            if path == "/api/sync":
                try: return self.json(l.sync())
                except RuntimeError as exc:
                    return self.json(*error("conflict", str(exc), 409))
                except Exception: return self.json(*error("vendor_unavailable", "Vendor is unavailable", 502))
            if path == "/api/webhooks/meridian": return self.webhook()
            if path.endswith("/note") and path.startswith("/api/payments/"): return self.note(path.rsplit("/", 2)[1])
            if path == "/api/drafts": return self.create_draft()
            if path.startswith("/api/drafts/"): return self.draft_action(path)
            return self.json(*error("not_found", "Not found", 404))
        except urllib.error.HTTPError as exc:
            if exc.code in {400, 428}: return self.json(*error("bad_request", "Vendor rejected the request", 400))
            return self.json(*error("vendor_unavailable", "Vendor is unavailable", 502))
        except Exception:
            return self.json(*error("bad_request", "Request could not be completed", 400))
    def note(self, pid):
        l = self.ledger; body = self.read_json(); note = body.get("note")
        if not isinstance(note, str) or not 1 <= len(note) <= 280: return self.json(*error("bad_request", "Invalid note", 400, [{"path": "note", "code": "bad_format"}]))
        with l.lock: row = l.db.execute("SELECT * FROM payments WHERE id=?", (pid,)).fetchone()
        if not row: return self.json(*error("not_found", "Payment not found", 404))
        try:
            resource, _, _ = l.vendor_request("PATCH", "/v3/payments/" + urllib.parse.quote(pid, safe=""), {"note": note}, {"If-Match": '"%s"' % row["version"]})
        except urllib.error.HTTPError as exc:
            if exc.code != 412: raise
            resource, _, _ = l.vendor_request("GET", "/v3/payments/" + urllib.parse.quote(pid, safe=""))
            try: resource, _, _ = l.vendor_request("PATCH", "/v3/payments/" + urllib.parse.quote(pid, safe=""), {"note": note}, {"If-Match": '"%s"' % resource["version"]}, retries=False)
            except urllib.error.HTTPError as second:
                if second.code == 412: return self.json(*error("conflict", "Payment changed again", 409))
                raise
        l.apply_payment(resource, "local")
        return self.json({"id": pid, "note": resource["note"], "version": resource["version"]})
    def create_draft(self):
        identity = self.auth_identity({"maker", "checker"})
        if not identity: return
        token, role = identity
        body = self.read_json(); fields = validate_draft(body)
        if fields: return self.json(*error("bad_request", "Validation failed", 400, fields))
        did = "dr_" + uuid.uuid4().hex; created = now()
        with self.ledger.lock, self.ledger.db:
            self.ledger.db.execute("INSERT INTO drafts(id,state,amount_minor,currency,name,country,note,created_at,submitter,idempotency_key,sent) VALUES(?,?,?,?,?,?,?,?,?,?,0)", (did, "draft", body["amount_minor"], body["currency"], body["counterparty"]["name"], body["counterparty"]["country"], body.get("note", ""), created, None, None)); self.ledger.event("draft.created", did, None, "approval")
            row = self.ledger.db.execute("SELECT * FROM drafts WHERE id=?", (did,)).fetchone()
        return self.json(public_draft(row), 201)
    def draft_action(self, path):
        identity = self.auth_identity({"maker", "checker"})
        if not identity: return
        token, role = identity
        bits = path.split("/"); did = bits[3] if len(bits) > 3 else ""; action = bits[4] if len(bits) > 4 else ""
        with self.ledger.lock: row = self.ledger.db.execute("SELECT * FROM drafts WHERE id=?", (did,)).fetchone()
        if not row: return self.json(*error("not_found", "Draft not found", 404))
        if action == "submit" and row["state"] == "draft": new, typ = "submitted", "draft.submitted"
        elif action == "reject" and row["state"] == "submitted" and role == "checker": new, typ = "rejected", "draft.rejected"
        elif action == "approve" and row["state"] == "submitted" and role == "checker": new, typ = "approved", "draft.approved"
        else:
            code = "approval_forbidden" if action in {"approve", "reject"} and row["submitter"] == token else "forbidden"
            return self.json(*error(code, "Action is not allowed", 403))
        if action in {"approve", "reject"} and row["submitter"] == token: return self.json(*error("approval_forbidden", "A different user must approve", 403))
        with self.ledger.lock, self.ledger.db:
            key = "idemp_" + uuid.uuid4().hex if action == "approve" else row["idempotency_key"]
            self.ledger.db.execute("UPDATE drafts SET state=?,submitter=CASE WHEN ?='submit' THEN ? ELSE submitter END,idempotency_key=COALESCE(?,idempotency_key) WHERE id=?", (new, action, token, key, did)); self.ledger.event(typ, did, None, "approval")
            current = self.ledger.db.execute("SELECT * FROM drafts WHERE id=?", (did,)).fetchone()
        if action == "approve": threading.Thread(target=self.ledger.send_approved, args=(did,), daemon=True).start()
        return self.json(public_draft(current))


def validate_draft(body):
    fields = []
    if not isinstance(body.get("amount_minor"), int): fields.append({"path": "amount_minor", "code": "not_an_integer"})
    elif body["amount_minor"] <= 0: fields.append({"path": "amount_minor", "code": "not_positive"})
    if body.get("currency") not in CURRENCIES: fields.append({"path": "currency", "code": "unsupported"})
    cp = body.get("counterparty") if isinstance(body.get("counterparty"), dict) else {}
    if not isinstance(cp.get("name"), str) or not 1 <= len(cp["name"]) <= 80: fields.append({"path": "counterparty.name", "code": "required"})
    if not isinstance(cp.get("country"), str) or not re.fullmatch(r"[A-Z]{2}", cp["country"]): fields.append({"path": "counterparty.country", "code": "bad_format"})
    if not isinstance(body.get("note", ""), str): fields.append({"path": "note", "code": "bad_format"})
    elif len(body.get("note", "")) > 280: fields.append({"path": "note", "code": "too_long"})
    return fields


def main():
    parser = argparse.ArgumentParser(); parser.add_argument("--db-dir", required=True); parser.add_argument("--port", type=int, required=True); parser.add_argument("--notifier", required=True); parser.add_argument("--vendor", required=True); parser.add_argument("--tokens-file", required=True); args = parser.parse_args()
    try:
        raw = json.load(open(args.tokens_file)); args.token_values = raw
        args.tokens = {token: role for role, token in raw.items()}
    except Exception:
        args.token_values = {}
        args.tokens = {}
    ledger = Ledger(args); server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler); server.ledger = ledger
    threading.Thread(target=ledger.register_loop, daemon=True).start(); threading.Thread(target=ledger.relay_loop, daemon=True).start(); threading.Thread(target=ledger.draft_recovery_loop, daemon=True).start()
    def boot_sync():
        while True:
            try: ledger.sync(); return
            except Exception: time.sleep(5)
    threading.Thread(target=boot_sync, daemon=True).start(); server.serve_forever()


if __name__ == "__main__": main()
