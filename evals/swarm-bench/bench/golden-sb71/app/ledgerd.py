"""ledgerd — vendor sync, the append-only event ledger, the outbox relay, API + UI host.

Boot contract: bind 127.0.0.1:<port> immediately (vendor down or not), then self-drive:
register the webhook, load reversals, walk the collection — retrying no less often than
every 5 seconds until the first sync lands. Reads are always served from local state; a
user write never waits on the notifier; the webhook endpoint answers within 3 seconds and
never calls the vendor.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import re
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional

from .meridian import MeridianClient, VendorConflict, VendorError, VendorUnavailable
from .store import LedgerStore
from .util import (CURRENCY_EXPONENTS, STATUS_ORDER, envelope, fe, json_bytes,
                   now_rfc3339, parse_int_param)

WEB_DIR = Path(__file__).resolve().parent.parent / "web"
PAGE_SIZE = 64
SORTS = ("created_at", "-created_at", "amount_minor", "-amount_minor")
CTYPES = {".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8",
          ".js": "application/javascript; charset=utf-8"}


class Counters:
    """The four live webhook counters — in-memory, per process, by design."""

    def __init__(self):
        self.lock = threading.Lock()
        self.registered = False
        self.received = 0
        self.applied = 0
        self.ignored = 0
        self.rejected = 0

    def bump(self, name: str, k: int = 1):
        with self.lock:
            setattr(self, name, getattr(self, name) + k)

    def snapshot(self) -> Dict:
        with self.lock:
            return {"registered": self.registered, "received": self.received,
                    "applied": self.applied, "ignored": self.ignored,
                    "rejected": self.rejected}


class SseHub:
    """One atomic batch per committed change set; batch numbers are per-connection."""

    def __init__(self):
        self.lock = threading.Lock()
        self.clients: List[Dict] = []

    def register(self) -> Dict:
        import queue
        client = {"q": queue.Queue(maxsize=1000), "batch": 0}
        with self.lock:
            self.clients.append(client)
        return client

    def unregister(self, client: Dict):
        with self.lock:
            if client in self.clients:
                self.clients.remove(client)

    def publish(self, records: List[Dict]):
        if not records:
            return
        with self.lock:
            clients = list(self.clients)
        for c in clients:
            try:
                c["q"].put_nowait(list(records))
            except Exception:
                pass


class App:
    def __init__(self, db_dir: Path, port: int, notifier_base: str, vendor_url: str,
                 tokens_file: Path):
        db_dir.mkdir(parents=True, exist_ok=True)
        self.store = LedgerStore(db_dir / "ledger.db")
        self.port = port
        self.notifier_base = notifier_base.rstrip("/")
        self.client = MeridianClient(vendor_url, "sk_test_meridian")
        self.tokens: Dict[str, str] = {}
        try:
            raw = json.loads(Path(tokens_file).read_text())
            self.tokens = {v: k for k, v in raw.items() if isinstance(v, str)}
        except Exception:
            self.tokens = {}
        self.counters = Counters()
        self.sse = SseHub()
        self.secret = self.store.meta_get("webhook_secret") or ""
        self.sync_lock = threading.Lock()
        self.stage_lock = threading.Lock()
        self.relay_wake = threading.Event()
        self.notifier_state = "down"
        self.stop = False

    # ── vendor sync ──────────────────────────────────────────────────────────────────────────

    def ensure_registered(self):
        if self.counters.registered and self.secret:
            return
        url = f"http://127.0.0.1:{self.port}/api/webhooks/meridian"
        resp = self.client.register_webhook(url)
        self.secret = resp["secret"]
        self.store.meta_set("webhook_secret", self.secret)
        with self.counters.lock:
            self.counters.registered = True

    def sync_reversals(self):
        data = self.client.reversals()
        rows = data.get("data") or []
        with self.store.lock:
            have = self.store.db.execute("SELECT COUNT(*) c FROM reversals").fetchone()["c"]
        first_load = have == 0
        recs: List[Dict] = []
        with self.store.tx() as db:
            for rev in rows:
                if self.store.db.execute("SELECT 1 FROM reversals WHERE id=?",
                                         (rev["id"],)).fetchone():
                    continue
                if first_load:
                    self.store.insert_reversal(rev, "sync", with_event=False, db=db)
                    continue
                # Incremental: apply only when the local payment already reads refunded —
                # otherwise the webhook transaction group owns the atomic pair (M2).
                p = self.store.db.execute("SELECT status FROM payments WHERE id=?",
                                          (rev["payment_id"],)).fetchone()
                if p is not None and p["status"] == "refunded":
                    self.store.insert_reversal(rev, "sync", with_event=True, db=db)
        _ = recs

    def walk(self) -> Dict:
        fetched = inserted = updated = 0
        cursor: Optional[str] = None
        offset = 0
        restarts = 0
        while True:
            row = self.store.validator(offset)
            validator = ({"etag": row["etag"], "gen": row["gen"]} if row else None)
            res = self._fetch_page(cursor, validator)
            if res["kind"] == "expired":
                restarts += 1
                if restarts > 3:
                    raise VendorError("cursor kept expiring")
                self.store.validators_clear()
                cursor, offset = None, 0
                continue
            if res["kind"] == "cache_miss":
                # The generation rule: drop the validator, refetch unconditionally, once.
                self.store.validator_drop(offset)
                res = self._fetch_page(cursor, None)
                if res["kind"] == "expired":
                    self.store.validators_clear()
                    cursor, offset = None, 0
                    restarts += 1
                    continue
            if res["kind"] == "not_modified":
                nxt = row["next_cursor"] if row else None
                if nxt is None:
                    break
                cursor = nxt
                offset += PAGE_SIZE
                continue
            data = res["data"]
            fetched += len(data)
            recs: List[Dict] = []
            with self.store.tx() as db:
                for p in data:
                    outcome, rec = self.store.upsert_payment(p, "sync", db=db)
                    if outcome == "insert":
                        inserted += 1
                    elif outcome == "update":
                        updated += 1
                    if rec:
                        recs.append(rec)
            self.sse.publish(recs)
            if res.get("etag") and res.get("gen") is not None:
                self.store.validator_set(offset, res["etag"], res["gen"],
                                         res.get("next_cursor"))
            if res.get("next_cursor") is None:
                break
            cursor = res["next_cursor"]
            offset += PAGE_SIZE
        return {"fetched": fetched, "inserted": inserted, "updated": updated,
                "total": self.store.count()}

    def _fetch_page(self, cursor: Optional[str], validator: Optional[Dict]) -> Dict:
        retried_500 = False
        while True:
            res = self.client.list_page(cursor, validator)
            if res["kind"] == "retry_after":
                if retried_500:
                    raise VendorError("500 twice on one page")
                retried_500 = True
                time.sleep(res["secs"])
                continue
            return res

    def run_sync(self) -> Dict:
        with self.sync_lock:
            self.ensure_registered()
            self.sync_reversals()
            counts = self.walk()
            self.store.meta_set("last_sync", now_rfc3339())
            return counts

    def boot_sync_loop(self):
        while not self.stop:
            try:
                self.run_sync()
                return
            except (VendorUnavailable, VendorError):
                time.sleep(2.5)
            except Exception:
                time.sleep(2.5)

    # ── the send half of approve (async; retried with the SAME idempotency key) ─────────────

    def send_approved(self, did: str):
        row = self.store.draft_internal(did)
        if row is None or row["state"] != "approved" or not row["idempotency_key"]:
            return
        body = {"amount_minor": row["amount_minor"], "currency": row["currency"],
                "note": row["note"],
                "counterparty": {"name": row["cp_name"], "country": row["cp_country"]}}
        hard_errors = 0
        while not self.stop:
            try:
                payment = self.client.create_payment(body, row["idempotency_key"])
                break
            except VendorUnavailable:
                time.sleep(2.0)
            except VendorError:
                hard_errors += 1
                if hard_errors > 5:
                    return
                time.sleep(2.0)
        else:
            return
        _outcome, rec = self.store.upsert_payment(payment, "approval")
        self.store.mark_draft_sent(did, payment["id"], payment.get("version", 1))
        if rec:
            self.sse.publish([rec])
        self.relay_wake.set()

    def recover_sends(self):
        for row in self.store.unsent_approved():
            threading.Thread(target=self.send_approved, args=(row["id"],),
                             daemon=True).start()

    # ── outbox relay (background only; at-least-once; backoff capped at 2 s) ────────────────

    def relay_loop(self):
        backoff = 0.25
        while not self.stop:
            batch = self.store.outbox_pending(50)
            if not batch:
                self.notifier_state = "up" if self._ping_notifier() else "down"
                self.relay_wake.wait(timeout=1.5)
                self.relay_wake.clear()
                continue
            try:
                req = urllib.request.Request(
                    f"{self.notifier_base}/notify/events",
                    data=json_bytes({"events": batch}), method="POST",
                    headers={"Content-Type": "application/json"})
                with urllib.request.urlopen(req, timeout=5) as resp:
                    body = json.loads(resp.read() or b"{}")
                seqs = list(body.get("accepted") or []) + list(body.get("duplicate") or [])
                self.store.outbox_mark_delivered([s for s in seqs if isinstance(s, int)])
                self.notifier_state = "up"
                backoff = 0.25
            except Exception:
                self.notifier_state = "down"
                time.sleep(backoff)
                backoff = min(2.0, backoff * 2)

    def _ping_notifier(self, timeout: float = 0.5) -> bool:
        try:
            with urllib.request.urlopen(f"{self.notifier_base}/health",
                                        timeout=timeout) as resp:
                return resp.status == 200
        except Exception:
            return False

    # ── webhook consumption ──────────────────────────────────────────────────────────────────

    def verify_signature(self, raw: bytes, header: Optional[str]) -> bool:
        m = re.fullmatch(r"t=(\d+),v1=([0-9a-f]+)", header or "")
        if not m or not self.secret:
            return False
        want = hmac.new(self.secret.encode(), f"{m.group(1)}.".encode() + raw,
                        hashlib.sha256).hexdigest()
        return hmac.compare_digest(want, m.group(2))

    def apply_webhook_event(self, ev: Dict) -> str:
        """apply | ignore for one verified event. Transaction groups stage durably and
        apply atomically when complete; no read ever observes half a group."""
        eid = ev.get("id")
        if not isinstance(eid, str) or self.store.seen_event(eid):
            return "ignore"
        etype = ev.get("type")
        data = ev.get("data") or {}
        txn = ev.get("txn")
        if isinstance(txn, dict) and txn.get("id") and etype in ("payment.updated",
                                                                "payment.created",
                                                                "reversal.created"):
            with self.stage_lock:
                self.store.mark_event_seen(eid)
                parts = self.store.stage_txn_part(
                    txn["id"], int(txn.get("part") or 1), int(txn.get("of") or 1),
                    {"type": etype, "data": data, "txn": txn})
                if not parts:
                    return "apply"          # staged; the group applies on its last part
                applied_any = False
                recs: List[Dict] = []
                with self.store.tx() as db:
                    for part in parts:
                        if part["type"] in ("payment.updated", "payment.created"):
                            outcome, rec = self.store.upsert_payment(
                                part["data"], "webhook", txn=part["txn"], db=db)
                            applied_any = applied_any or outcome != "unchanged"
                            if rec:
                                recs.append(rec)
                        elif part["type"] == "reversal.created":
                            ok = self.store.insert_reversal(
                                part["data"], "webhook", txn=part["txn"],
                                with_event=True, db=db)
                            applied_any = applied_any or ok
                self.sse.publish(recs)
                self.relay_wake.set()
                return "apply" if applied_any else "ignore"
        self.store.mark_event_seen(eid)
        if etype in ("payment.created", "payment.updated"):
            outcome, rec = self.store.upsert_payment(data, "webhook", txn=None)
            if rec:
                self.sse.publish([rec])
            return "apply" if outcome != "unchanged" else "ignore"
        if etype == "reversal.created":
            ok = self.store.insert_reversal(data, "webhook", with_event=True)
            if ok:
                self.relay_wake.set()
            return "apply" if ok else "ignore"
        return "ignore"


APP: Optional[App] = None


def _role_of(headers) -> Optional[str]:
    auth = headers.get("Authorization") or ""
    if not auth.startswith("Bearer "):
        return None
    return APP.tokens.get(auth[7:].strip())


def _has_token(headers) -> bool:
    return bool((headers.get("Authorization") or "").startswith("Bearer "))


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_a):
        return

    # ── plumbing ─────────────────────────────────────────────────────────────────────────────

    def _json(self, code: int, payload) -> None:
        body = json_bytes(payload)
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(body)
        except OSError:
            pass

    def _err(self, code: int, ecode: str, message: str, field_errors=None) -> None:
        self._json(code, envelope(ecode, message, field_errors))

    def _body(self) -> Dict:
        if not hasattr(self, "_raw_body"):
            length = int(self.headers.get("Content-Length") or 0)
            self._raw_body = self.rfile.read(length) if length else b""
        try:
            parsed = json.loads(self._raw_body or b"{}")
            return parsed if isinstance(parsed, dict) else {}
        except json.JSONDecodeError:
            return {}

    def _static(self, name: str) -> bool:
        f = WEB_DIR / name
        if not f.is_file():
            return False
        body = f.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", CTYPES.get(f.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except OSError:
            pass
        return True

    # ── routes ───────────────────────────────────────────────────────────────────────────────

    def do_GET(self):  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        qs = {k: v[0] for k, v in urllib.parse.parse_qs(parsed.query).items()}
        if path == "/":
            self._static("index.html")
            return
        if path in ("/index.html", "/styles.css", "/app.js", "/viz.js"):
            self._static(path.lstrip("/"))
            return
        if path.startswith("/web/"):
            name = path[5:]
            if name in ("index.html", "styles.css", "app.js", "viz.js") \
                    and self._static(name):
                return
            self._err(404, "not_found", "no such asset")
            return
        if path == "/api/health":
            self._json(200, {"status": "ok", "payments": APP.store.count(),
                             "last_sync": APP.store.meta_get("last_sync"),
                             "webhook": APP.counters.snapshot()})
            return
        if path == "/api/payments":
            self._payments(qs)
            return
        m = re.fullmatch(r"/api/payments/([A-Za-z0-9_.-]+)", path)
        if m:
            row = APP.store.payment(m.group(1))
            if row is None:
                self._err(404, "not_found", "no such payment")
            else:
                self._json(200, row)
            return
        if path == "/api/summary":
            self._json(200, APP.store.summary())
            return
        if path == "/api/buckets":
            self._json(200, APP.store.buckets())
            return
        if path == "/api/viz/records":
            self._json(200, APP.store.viz_records())
            return
        if path == "/api/events":
            if _role_of(self.headers) is None:
                self._err(401, "unauthorized", "a bearer token is required")
                return
            after, e1 = parse_int_param(qs.get("after"), "after", 0)
            limit, e2 = parse_int_param(qs.get("limit"), "limit", 500)
            errs = [e for e in (e1, e2) if e]
            if errs:
                self._err(400, "bad_request", "invalid parameters", errs)
                return
            events, latest = APP.store.events_after(after, min(limit or 500, 2000))
            self._json(200, {"events": events, "latest_seq": latest})
            return
        if path == "/api/outbox/status":
            status = APP.store.outbox_status()
            up = APP._ping_notifier(0.4)
            status["notifier"] = "up" if up else "down"
            APP.notifier_state = status["notifier"]
            self._json(200, status)
            return
        if path == "/api/notifications":
            self._proxy_notifications(parsed.query)
            return
        if path == "/api/stream":
            self._stream()
            return
        if path == "/api/drafts":
            self._drafts_list(qs)
            return
        self._err(404, "not_found", "no such path")

    def do_POST(self):  # noqa: N802
        # Always drain the request body — keep-alive reuses the connection and unread
        # bytes corrupt the next request line. The handler INSTANCE is reused across
        # requests on one connection, so the per-request cache must be cleared first.
        if hasattr(self, "_raw_body"):
            del self._raw_body
        self._body()
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        if path == "/api/webhooks/meridian":
            self._webhook()
            return
        if path == "/api/sync":
            try:
                counts = APP.run_sync()
                self._json(200, counts)
            except (VendorUnavailable, VendorError):
                self._err(502, "vendor_unavailable",
                          "the vendor is unreachable; serving local data")
            return
        m = re.fullmatch(r"/api/payments/([A-Za-z0-9_.-]+)/note", path)
        if m:
            self._note(m.group(1))
            return
        if path == "/api/drafts":
            self._draft_create()
            return
        m = re.fullmatch(r"/api/drafts/([A-Za-z0-9_.-]+)/(submit|approve|reject)", path)
        if m:
            self._draft_action(m.group(1), m.group(2))
            return
        self._err(404, "not_found", "no such path")

    # ── payments list ────────────────────────────────────────────────────────────────────────

    def _payments(self, qs: Dict[str, str]) -> None:
        errs = []
        limit, e = parse_int_param(qs.get("limit"), "limit", 50)
        if e:
            errs.append(e)
        offset, e = parse_int_param(qs.get("offset"), "offset", 0)
        if e:
            errs.append(e)
        status = qs.get("status") or None
        if status is not None and status not in STATUS_ORDER:
            errs.append(fe("status", "unsupported"))
        currency = qs.get("currency") or None
        if currency is not None and currency not in CURRENCY_EXPONENTS:
            errs.append(fe("currency", "unsupported"))
        sort = qs.get("sort") or "created_at"
        if sort not in SORTS:
            errs.append(fe("sort", "unsupported"))
        if errs:
            self._err(400, "bad_request", "invalid parameters", errs)
            return
        limit = min(limit, 200)
        data, total = APP.store.list_payments(limit, offset, status, currency, sort)
        self._json(200, {"data": data, "total": total, "limit": limit, "offset": offset})

    # ── note write-through ───────────────────────────────────────────────────────────────────

    def _note(self, pid: str) -> None:
        body = self._body()
        note = body.get("note")
        if not isinstance(note, str) or not (1 <= len(note) <= 280):
            code = "too_long" if isinstance(note, str) and len(note) > 280 else \
                ("required" if note is None else "bad_format")
            self._err(400, "bad_request", "note must be 1-280 characters",
                      [fe("note", code)])
            return
        local = APP.store.payment(pid)
        if local is None:
            self._err(404, "not_found", "no such payment")
            return
        try:
            fresh = APP.client.patch_note(pid, note, local["version"])
        except VendorConflict:
            self._err(409, "conflict", "the note lost the race twice; local row unchanged")
            return
        except (VendorUnavailable, VendorError):
            self._err(502, "vendor_unavailable", "the vendor is unreachable")
            return
        rec = APP.store.apply_note(fresh)
        if rec:
            APP.sse.publish([rec])
        self._json(200, {"id": pid, "note": fresh.get("note"),
                         "version": fresh.get("version")})

    # ── webhook endpoint (vendor-facing) ─────────────────────────────────────────────────────

    def _webhook(self) -> None:
        body = self._body()
        raw = getattr(self, "_raw_body", b"")
        sig = self.headers.get("Meridian-Signature")
        if sig is None and body.get("type") == "webhook.verify":
            # The unsigned registration challenge — not an event delivery, never counted.
            self._json(200, {"challenge": body.get("challenge")})
            return
        APP.counters.bump("received")
        if not APP.verify_signature(raw, sig):
            APP.counters.bump("rejected")
            self._err(401, "bad_signature", "signature missing or wrong")
            return
        outcome = APP.apply_webhook_event(body)
        APP.counters.bump("applied" if outcome == "apply" else "ignored")
        self._json(200, {"received": True})

    # ── notifications proxy ──────────────────────────────────────────────────────────────────

    def _proxy_notifications(self, query: str) -> None:
        url = f"{APP.notifier_base}/notify/notifications"
        if query:
            url += "?" + query
        try:
            with urllib.request.urlopen(url, timeout=3) as resp:
                raw = resp.read()
            body = json.loads(raw)
            self._json(200, body)
        except Exception:
            self._err(502, "notifier_unreachable", "the notifier is unreachable")

    # ── SSE ──────────────────────────────────────────────────────────────────────────────────

    def _stream(self) -> None:
        client = APP.sse.register()
        try:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            pad = ": stream-open " + "." * 180 + "\n\n"
            self.wfile.write(pad.encode())
            self.wfile.flush()
            import queue
            while not APP.stop:
                try:
                    records = client["q"].get(timeout=10)
                    client["batch"] += 1
                    msg = {"batch": client["batch"], "records": records}
                    self.wfile.write(f"data: {json.dumps(msg)}\n\n".encode())
                    self.wfile.flush()
                except queue.Empty:
                    self.wfile.write(b": ping\n\n")
                    self.wfile.flush()
        except OSError:
            pass
        finally:
            APP.sse.unregister(client)

    # ── drafts ───────────────────────────────────────────────────────────────────────────────

    def _auth_role(self) -> Optional[str]:
        role = _role_of(self.headers)
        if role is None:
            self._err(401, "unauthorized",
                      "a bearer token (maker, checker or admin) is required")
            return None
        return role

    def _drafts_list(self, qs: Dict[str, str]) -> None:
        role = self._auth_role()
        if role is None:
            return
        state = qs.get("state") or None
        if state is not None and state not in ("draft", "submitted", "approved",
                                               "rejected", "sent"):
            self._err(400, "bad_request", "unknown state", [fe("state", "unsupported")])
            return
        data, total = APP.store.list_drafts(state)
        self._json(200, {"data": data, "total": total})

    def _draft_create(self) -> None:
        role = self._auth_role()
        if role is None:
            return
        if role not in ("maker", "checker"):
            self._err(403, "forbidden", "admin reads everything and writes nothing")
            return
        body = self._body()
        errs = []
        amt = body.get("amount_minor")
        if amt is None:
            errs.append(fe("amount_minor", "required"))
        elif not isinstance(amt, int) or isinstance(amt, bool):
            errs.append(fe("amount_minor", "not_an_integer"))
        elif amt <= 0:
            errs.append(fe("amount_minor", "not_positive"))
        cur = body.get("currency")
        if cur is None:
            errs.append(fe("currency", "required"))
        elif cur not in CURRENCY_EXPONENTS:
            errs.append(fe("currency", "unsupported"))
        cp = body.get("counterparty") if isinstance(body.get("counterparty"), dict) else {}
        name = cp.get("name")
        if not isinstance(name, str) or len(name) < 1:
            errs.append(fe("counterparty.name", "required"))
        elif len(name) > 80:
            errs.append(fe("counterparty.name", "too_long"))
        country = cp.get("country")
        if not isinstance(country, str):
            errs.append(fe("counterparty.country", "required"))
        elif not re.fullmatch(r"[A-Z]{2}", country):
            errs.append(fe("counterparty.country", "bad_format"))
        note = body.get("note")
        if note is not None and not isinstance(note, str):
            errs.append(fe("note", "bad_format"))
        elif isinstance(note, str) and len(note) > 280:
            errs.append(fe("note", "too_long"))
        if errs:
            self._err(400, "bad_request", "invalid draft", errs)
            return
        draft = APP.store.create_draft(body)
        self._json(201, draft)

    def _draft_action(self, did: str, action: str) -> None:
        role = self._auth_role()
        if role is None:
            return
        token = (self.headers.get("Authorization") or "")[7:].strip()
        if action == "submit" and role not in ("maker", "checker"):
            self._err(403, "forbidden", "submit needs the maker or checker role")
            return
        if action in ("approve", "reject") and role != "checker":
            self._err(403, "forbidden", f"{action} needs the checker role")
            return
        row = APP.store.draft_internal(did)
        if row is None:
            self._err(404, "not_found", "no such draft")
            return
        state = row["state"]
        if action == "submit":
            if state != "draft":
                self._err(409, "conflict",
                          f"a {state} draft cannot be submitted (rejected is terminal)")
                return
            APP.store.transition_draft(did, "submitted", "draft.submitted",
                                       submitted_by=token)
            APP.relay_wake.set()
            self._json(200, APP.store.draft(did))
            return
        if state != "submitted":
            self._err(409, "conflict", f"cannot {action} a draft in state {state}")
            return
        if row["submitted_by"] and row["submitted_by"] == token:
            self._err(403, "approval_forbidden",
                      "four-eyes: the approver must not be the submitter")
            return
        if action == "reject":
            APP.store.transition_draft(did, "rejected", "draft.rejected")
            APP.relay_wake.set()
            self._json(200, APP.store.draft(did))
            return
        key = str(uuid.uuid4())
        APP.store.transition_draft(did, "approved", "draft.approved",
                                   idempotency_key=key)
        APP.relay_wake.set()
        self._json(200, APP.store.draft(did))
        threading.Thread(target=APP.send_approved, args=(did,), daemon=True).start()


def serve(db_dir: Path, port: int, notifier: str, vendor: str, tokens_file: Path):
    global APP
    APP = App(db_dir, port, notifier, vendor, tokens_file)
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    threading.Thread(target=APP.boot_sync_loop, daemon=True).start()
    threading.Thread(target=APP.relay_loop, daemon=True).start()
    APP.recover_sends()
    return server


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db-dir", type=Path, required=True)
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--notifier", type=str, required=True)
    ap.add_argument("--vendor", type=str, required=True)
    ap.add_argument("--tokens-file", type=Path, required=True)
    args = ap.parse_args(argv)
    serve(args.db_dir, args.port, args.notifier, args.vendor, args.tokens_file)
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    raise SystemExit(main())
