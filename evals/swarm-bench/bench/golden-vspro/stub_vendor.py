"""A minimal Meridian v2 stub — the reference app's assumed vendor contract, executable.

This is NOT the graded mock (that is vendor_service_v2's workstream); it exists so the golden
app can be exercised end-to-end today, and so the mock author can read the exact request/response
shapes the reference client speaks. Where vendor_service_v2 lands with different paths or header
conventions, vspro/meridian.py's ENDPOINTS block is the single place to align.

Assumed v2 surface (v1 conventions extended):
  GET   /v2/payments                cursor pagination, ETag/If-None-Match/304, Retry-After both
                                    forms (one-shot), 410 cursor_expired (one-shot)
  GET   /v2/payments/<id>           single resource incl. "version"
  PATCH /v2/payments/<id>           If-Match: <version> (bare integer; quoted also accepted);
                                    missing -> 428; mismatch -> 412
  POST  /v2/payments                Idempotency-Key header; retry -> 409 {"payment_id"} success
  POST  /v2/payments/batch          {"items": [...]} -> {"results": [...]} input order;
                                    business rule: value_minor > AMOUNT_LIMIT -> amount_over_limit
  POST  /v2/webhooks                {"url"} -> {"id","secret"}; idempotent by URL; challenge
                                    handshake POSTed to the URL during the call
Admin (test driver only): POST /admin/force-412 {"count": N}, POST /admin/deliver {"events": ...},
GET /admin/trace.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import secrets
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Dict, List, Optional
from urllib.parse import parse_qs, urlparse

AMOUNT_LIMIT = 1_000_000
PAGE_MAX = 100
WEBHOOK_SECRET_PREFIX = "whsec_"


def build_fixture() -> List[Dict]:
    """130 payments over Berlin days 2026-03-26 .. 2026-04-01 (spanning the 03-29 DST switch),
    4 statuses interleaved, 4 currencies, mixed offsets, delivery order scrambled vs instant.
    Two rows sit on UTC-vs-Berlin day boundaries so wrong bucketing is measurable. >100 rows
    forces multi-page pagination so the one-shot 429/410/429-date chain is reachable."""
    from datetime import timedelta
    statuses = ["settled", "pending", "refunded", "failed"]
    currencies = ["EUR", "USD", "JPY", "KWD"]
    offsets = ["+02:00", "-05:00", "Z", "+09:00"]
    rows = []
    for i in range(128):
        day_shift = (i * 11) % 7
        instant = datetime(2026, 3, 26, 6 + (i * 7) % 12, (i * 13) % 60,
                           tzinfo=timezone.utc) + timedelta(days=day_shift)
        offset = offsets[i % 4]
        if offset == "Z":
            local, suffix = instant, "Z"
        else:
            sign = 1 if offset[0] == "+" else -1
            hh, mm = int(offset[1:3]), int(offset[4:6])
            local = instant + sign * timedelta(hours=hh, minutes=mm)
            suffix = offset
        rows.append({
            "id": f"pay_{i:04d}",
            "amount_minor": 900 + i * 137,
            "currency": currencies[i % 4],
            "created_at": local.strftime("%Y-%m-%dT%H:%M:%S") + suffix,
            "settled_at": None,
            "status": statuses[i % 4],
            "version": 1,
            "note": "",
            "counterparty": {"name": f"Vendor {i:02d}", "country": ["DE", "US", "JP", "KW"][i % 4]},
        })
    # UTC-vs-Berlin discriminators: 23:30Z lands on the NEXT Berlin day; on 03-29 the offset
    # changes from +01:00 to +02:00 mid-night.
    rows.append({"id": "pay_dst1", "amount_minor": 5000, "currency": "EUR",
                 "created_at": "2026-03-28T23:30:00Z", "settled_at": None, "status": "settled",
                 "version": 1, "note": "", "counterparty": {"name": "DST One", "country": "DE"}})
    rows.append({"id": "pay_dst2", "amount_minor": 7000, "currency": "USD",
                 "created_at": "2026-03-29T22:30:00Z", "settled_at": None, "status": "pending",
                 "version": 1, "note": "", "counterparty": {"name": "DST Two", "country": "US"}})
    # Scramble delivery order against chronology.
    rows.sort(key=lambda r: hashlib.sha256(r["id"].encode()).hexdigest())
    return rows


class StubState:
    def __init__(self):
        self.lock = threading.Lock()
        self.payments = build_fixture()
        self.by_id = {p["id"]: p for p in self.payments}
        self.etags: Dict[str, str] = {}
        self.generation = 0
        self.list_requests = 0
        self.fired = set()
        self.idempotency: Dict[str, str] = {}
        self.force_412 = 0
        self.webhooks: Dict[str, Dict] = {}
        self.trace: List[Dict] = []
        self.list_delay = 0.0

    def record(self, entry: Dict):
        with self.lock:
            entry["t"] = round(time.time(), 4)
            self.trace.append(entry)


STATE = StubState()


def _etag(offset: int, limit: int) -> str:
    return '"' + hashlib.sha256(
        f"{STATE.generation}-{offset}-{limit}".encode()).hexdigest()[:16] + '"'


def _cursor(offset: int) -> str:
    return json.dumps({"o": offset}).encode().hex()


def _offset(cursor: Optional[str]) -> int:
    if not cursor:
        return 0
    try:
        return int(json.loads(bytes.fromhex(cursor).decode())["o"])
    except Exception:
        return -1


def sign_event(secret: str, body: bytes, stamp: Optional[int] = None) -> str:
    stamp = stamp or int(time.time())
    mac = hmac.new(secret.encode(), f"{stamp}.".encode() + body, hashlib.sha256).hexdigest()
    return f"t={stamp},v1={mac}"


def deliver(url: str, body: bytes, signature: Optional[str]) -> int:
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if signature:
        req.add_header("Meridian-Signature", signature)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status
    except urllib.error.HTTPError as err:
        return err.code


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        return

    def _json(self, code: int, payload, headers: Optional[Dict] = None):
        body = json.dumps(payload).encode()
        self.send_response(code)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _empty(self, code: int, headers: Optional[Dict] = None):
        self.send_response(code)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", "0")
        self.end_headers()

    # ── GET ───────────────────────────────────────────────────────────────────────────────────

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/v2/payments":
            self._list(parsed)
            return
        if parsed.path.startswith("/v2/payments/"):
            pid = parsed.path.rsplit("/", 1)[1]
            row = STATE.by_id.get(pid)
            STATE.record({"m": "GET", "p": parsed.path, "s": 200 if row else 404})
            if row:
                self._json(200, self._public(row))
            else:
                self._json(404, {"error": "not_found"})
            return
        if parsed.path == "/admin/trace":
            with STATE.lock:
                self._json(200, {"trace": STATE.trace})
            return
        self._json(404, {"error": "not_found"})

    @staticmethod
    def _public(row: Dict) -> Dict:
        return {k: v for k, v in row.items() if not k.startswith("_")}

    def _list(self, parsed):
        if STATE.list_delay:
            time.sleep(STATE.list_delay)
        params = parse_qs(parsed.query)
        cursor = (params.get("cursor") or [None])[0]
        limit = min(max(int((params.get("limit") or ["25"])[0]), 1), PAGE_MAX)
        with STATE.lock:
            STATE.list_requests += 1
            nth = STATE.list_requests
            throttle = nth == 2 and "secs" not in STATE.fired
            if throttle:
                STATE.fired.add("secs")
            expire = nth == 3 and bool(cursor) and "gone" not in STATE.fired
            if expire:
                STATE.fired.add("gone")
            httpdate = ("gone" in STATE.fired and "date" not in STATE.fired
                        and not expire and not throttle)
            if httpdate:
                STATE.fired.add("date")
        if throttle:
            STATE.record({"m": "GET", "p": "/v2/payments", "s": 429, "form": "seconds"})
            self._json(429, {"error": "rate_limited"}, {"Retry-After": "1"})
            return
        if expire:
            STATE.record({"m": "GET", "p": "/v2/payments", "s": 410})
            self._json(410, {"error": "cursor_expired"})
            return
        if httpdate:
            when = datetime.now(timezone.utc)
            stamp = when.strftime("%a, %d %b %Y %H:%M:%S GMT")
            STATE.record({"m": "GET", "p": "/v2/payments", "s": 429, "form": "http-date"})
            self._json(429, {"error": "rate_limited"}, {"Retry-After": stamp})
            return
        off = _offset(cursor)
        if off < 0 or off > len(STATE.payments):
            self._json(400, {"error": "bad_cursor"})
            return
        take = limit
        if off == 10 and limit > 5:
            take = 5  # documented: short pages may appear anywhere and are not the end
        etag = _etag(off, take)
        if self.headers.get("If-None-Match") == etag:
            STATE.record({"m": "GET", "p": "/v2/payments", "s": 304, "off": off, "cond": True})
            self._empty(304, {"ETag": etag})
            return
        rows = STATE.payments[off:off + take]
        nxt = off + len(rows)
        STATE.record({"m": "GET", "p": "/v2/payments", "s": 200, "off": off,
                      "cond": bool(self.headers.get("If-None-Match"))})
        self._json(200, {
            "data": [self._public(r) for r in rows],
            "next_cursor": _cursor(nxt) if nxt < len(STATE.payments) else None,
            "total": len(STATE.payments),
        }, {"ETag": etag})

    # ── POST/PATCH ────────────────────────────────────────────────────────────────────────────

    def do_POST(self):
        parsed = urlparse(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            body = {}
        if parsed.path == "/v2/payments":
            self._create(body)
            return
        if parsed.path == "/v2/payments/batch":
            self._batch(body)
            return
        if parsed.path == "/v2/webhooks":
            self._register(body)
            return
        if parsed.path == "/admin/force-412":
            with STATE.lock:
                STATE.force_412 = int(body.get("count", 1))
            self._json(200, {"ok": True})
            return
        if parsed.path == "/admin/list-delay":
            with STATE.lock:
                STATE.list_delay = float(body.get("seconds", 0))
            self._json(200, {"ok": True})
            return
        if parsed.path == "/admin/deliver":
            self._admin_deliver(body)
            return
        self._json(404, {"error": "not_found"})

    def do_PATCH(self):
        parsed = urlparse(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        body = json.loads(raw or b"{}")
        pid = parsed.path.rsplit("/", 1)[1]
        row = STATE.by_id.get(pid)
        if not row:
            self._json(404, {"error": "not_found"})
            return
        if_match = self.headers.get("If-Match")
        STATE.record({"m": "PATCH", "p": parsed.path, "if_match": if_match,
                      "fields": sorted(body.keys())})
        if if_match is None:
            self._json(428, {"error": "precondition_required"})
            return
        claimed = if_match.strip().strip('"')
        with STATE.lock:
            forced = STATE.force_412 > 0
            if forced:
                STATE.force_412 -= 1
                # Simulate a concurrent writer winning the race: bump the version out from
                # under the caller so a refetch sees a fresh one.
                row["version"] += 1
                row["note"] = row.get("note") or "someone-else"
            if claimed != str(row["version"]):
                self._json(412, {"error": "version_conflict", "current_version": row["version"]})
                return
            for key, value in body.items():
                if key in ("note", "status", "settled_at"):
                    row[key] = value
            row["version"] += 1
            self.__class__._last_patched = pid
        self._json(200, self._public(row))

    def _create(self, body):
        key = self.headers.get("Idempotency-Key")
        if not key:
            self._json(400, {"error": "idempotency_key_required"})
            return
        with STATE.lock:
            if key in STATE.idempotency:
                self._json(409, {"error": "duplicate", "payment_id": STATE.idempotency[key]})
                return
            pid = f"pay_new_{len(STATE.idempotency):04d}"
            STATE.idempotency[key] = pid
            amount = body.get("amount") or {}
            row = {"id": pid, "amount_minor": amount.get("value_minor"),
                   "currency": amount.get("currency", "EUR"),
                   "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                   "settled_at": None, "status": "pending", "version": 1, "note": "",
                   "counterparty": body.get("counterparty") or {}}
            STATE.payments.append(row)
            STATE.by_id[pid] = row
            STATE.generation += 1
        self._json(201, self._public(row))

    def _batch(self, body):
        items = body.get("items") or []
        results = []
        with STATE.lock:
            for i, item in enumerate(items):
                amount = item.get("amount") or {}
                value = amount.get("value_minor") or 0
                key = item.get("idempotency_key") or f"batch-{i}"
                if value > AMOUNT_LIMIT:
                    results.append({"index": i, "status": "error",
                                    "error": {"code": "amount_over_limit",
                                              "message": f"amount exceeds {AMOUNT_LIMIT}"}})
                    continue
                if key in STATE.idempotency:
                    results.append({"index": i, "status": "created",
                                    "id": STATE.idempotency[key]})
                    continue
                pid = f"pay_new_{len(STATE.idempotency):04d}"
                STATE.idempotency[key] = pid
                results.append({"index": i, "status": "created", "id": pid})
            STATE.generation += 1
        STATE.record({"m": "POST", "p": "/v2/payments/batch", "n": len(items)})
        self._json(200, {"results": results})

    def _register(self, body):
        url = body.get("url")
        if not url:
            self._json(400, {"error": "bad_request"})
            return
        with STATE.lock:
            existing = STATE.webhooks.get(url)
        if existing:
            STATE.record({"m": "POST", "p": "/v2/webhooks", "idempotent": True})
            self._json(200, existing)
            return
        challenge = secrets.token_hex(16)
        status = deliver(url, json.dumps({"type": "webhook.verify",
                                          "challenge": challenge}).encode(), None)
        if status != 200:
            self._json(400, {"error": "verification_failed", "status": status})
            return
        hook = {"id": f"wh_{len(STATE.webhooks):04d}",
                "secret": WEBHOOK_SECRET_PREFIX + secrets.token_hex(12)}
        with STATE.lock:
            STATE.webhooks[url] = hook
        STATE.record({"m": "POST", "p": "/v2/webhooks", "idempotent": False})
        self._json(201, hook)

    def _admin_deliver(self, body):
        """Test driver: deliver the scripted events to the (single) registered webhook."""
        with STATE.lock:
            if not STATE.webhooks:
                self._json(400, {"error": "no_webhook"})
                return
            url, hook = next(iter(STATE.webhooks.items()))
        outcomes = []
        for event in body.get("events") or []:
            raw = json.dumps(event["body"]).encode()
            if event.get("forged"):
                sig = sign_event("whsec_wrong_secret", raw)
            elif event.get("unsigned"):
                sig = None
            else:
                sig = sign_event(hook["secret"], raw)
            outcomes.append(deliver(url, raw, sig))
        self._json(200, {"statuses": outcomes})


def serve(port: int) -> ThreadingHTTPServer:
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


if __name__ == "__main__":
    import sys
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8899
    serve(port)
    print(f"stub meridian v2 on http://127.0.0.1:{port} ({len(STATE.payments)} payments)")
    while True:
        time.sleep(3600)
