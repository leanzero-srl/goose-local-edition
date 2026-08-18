"""The Meridian v2 mock — the sb-6 "VendorSync Pro" vendor, recording how it was used.

Same doctrine as v1 (vendor_service.py): every request is appended to a JSONL trace, and the
trace — not the produced source — is the evidence a client honoured the contract. You cannot
fake having sent `If-Match`, and you cannot fake having waited out a stall. Each behaviour is
(or will be, see /v2/docs) documented in vendor_docs_v2.md: a careful engineer who reads the
docs gets every one right, which is the fairness bar the benchmark holds itself to.

What v2 adds on top of v1's cursor pagination / per-page ETags / Retry-After (both forms) /
410 cursor-expiry / Idempotency-Key replay — all of which carry over unchanged:

- 4-currency, 4-status, 14-Berlin-day fixture with the 2026-03-29 DST crossing (fixtures_v2).
- Optimistic concurrency on the mutable resource: PATCH requires `If-Match: "<version>"`;
  a write without it is `428 Precondition Required`, a mismatch is `412`, and ONE 412 is
  injected per phase on an otherwise-correct write (a phantom concurrent editor that really
  bumps the version, so the documented refetch-reapply-retry dance genuinely succeeds).
  `/admin/conflict {"mode": "always"}` makes the resource permanently contended for the
  scripted second-412 case (h_conflict_dance).
- Partial-failure batch create: up to 20 items, applied independently, per-item outcomes in
  input order; the per-payment amount limit (fixtures_v2.AMOUNT_LIMIT_MINOR) is the business
  rule that fails individual items.
- HMAC-signed webhook push. Registration is idempotent by URL (same id and secret, every
  time) and performs the unsigned challenge handshake on every call. Deliveries fire ONLY
  when the grader scripts them via localhost-only `POST /admin/deliver-script` (red-team F3:
  a delivery fired at registration time would be consumed by the agent's own dev-phase app,
  the exact seq-3-vs-seq-38 failure begin_exercise_phase() exists to prevent). The challenge
  handshake increments no counter (F13).
- `--stall` (F8): when armed, the FINAL sync page is held open for a fixed window (one-shot
  per phase) so the under-load measurement provably overlaps an in-flight sync, and the
  documented client timeout (fixtures_v2.CLIENT_TIMEOUT_DOC_SECS, F14) is testable behaviour
  instead of a source grep.
- "halfset" (F2): a vendor-side cap serving only the first HALFSET_ROWS of the collection,
  so the re-based j_sync_journey can boot an app whose first sync lands a half collection and
  whose #sync-now click observably grows it — half-seeding through the vendor, never through
  the app's private schema.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac as hmac_mod
import json
import re
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional
from urllib.parse import parse_qs, urlparse

try:
    import fixtures_v2
except ImportError:                    # imported as a package module (bench.vendor_service_v2)
    from . import fixtures_v2

HERE = Path(__file__).resolve().parent
API_KEY = "sk_test_meridian"
LIST_PATH = "/v2/payments"             # the one list path — score_sb6's trace splitter reads it
DOCS_PATH = "/v2/docs"                 # run_build substitutes {DOCS_URL} from this
DELIVERY_TIMEOUT_SECS = 5              # the spec gives the endpoint 3 s to answer; margin on top

# One home for every trap constant: fixtures_v2 (the twin-constant rule). Re-bound here only
# for readability; consumers may import from either name and cannot disagree.
DEFAULT_LIMIT = fixtures_v2.DEFAULT_LIMIT
MAX_LIMIT = fixtures_v2.MAX_LIMIT
SHORT_PAGE_AT = fixtures_v2.SHORT_PAGE_AT
SHORT_PAGE_TAKE = fixtures_v2.SHORT_PAGE_TAKE
RETRY_AFTER_SECS = fixtures_v2.RETRY_AFTER_SECS
THROTTLE_SECONDS_NTH = fixtures_v2.THROTTLE_SECONDS_NTH
CURSOR_EXPIRES_NTH = fixtures_v2.CURSOR_EXPIRES_NTH
AMOUNT_LIMIT_MINOR = fixtures_v2.AMOUNT_LIMIT_MINOR
BATCH_MAX_ITEMS = fixtures_v2.BATCH_MAX_ITEMS
HALFSET_ROWS = fixtures_v2.HALFSET_ROWS
EXPECTED_WEBHOOK_COUNTERS = fixtures_v2.EXPECTED_WEBHOOK_COUNTERS
PHASE_MARKER = fixtures_v2.PHASE_MARKER

PAYMENTS: List[Dict] = fixtures_v2.fresh_payments_delivery_order()
INDEX: Dict[str, Dict] = {p["id"]: p for p in PAYMENTS}


def true_order_ids() -> List[str]:
    """Ground-truth chronological order (by instant, not string) — the ONE source every
    order check reads."""
    return [p["id"] for p in sorted(PAYMENTS, key=lambda p: p["_instant"])]


class State:
    def __init__(self, trace_path: Path, stall_enabled: bool = False,
                 stall_secs: Optional[float] = None):
        self.trace_path = trace_path
        self.lock = threading.Lock()
        self.list_requests = 0
        self.fired: set = set()
        self.idempotency: Dict[str, str] = {}
        self.created: List[Dict] = []
        self.generation = 0
        self.visible_rows: Optional[int] = None          # F2: halfset cap (None = full)
        self.conflict_mode = "once"                      # "once" | "always" | "off"
        self.stall_enabled = stall_enabled               # F8: armed capability
        self.stall_secs = fixtures_v2.STALL_HOLD_SECS if stall_secs is None else stall_secs
        self.webhook: Optional[Dict] = None              # {"url","id","secret"}
        self.sent_events: Dict[str, Dict] = {}           # event_id -> {"raw","sig"} for dupes
        self.seq = 0
        self.events: List[Dict] = []                     # in-memory trace mirror (trap_results)
        self.patch_log: List[Dict] = []                  # every PATCH outcome (conflict_trace)
        self.batch_ops: List[Dict] = []                  # every create item (batch_trace)
        self.stall_info: Dict = {"began_t": None, "resolved_t": None,
                                 "client_dropped": False, "served_after_hold": False}
        self.mutated: Dict[str, str] = {}                # id -> original status (mutate/unmutate)

    def record(self, entry: Dict) -> None:
        with self.lock:
            self.seq += 1
            entry["seq"] = self.seq
            entry["t"] = round(time.time(), 4)
            self.events.append(entry)
            with self.trace_path.open("a") as fh:
                fh.write(json.dumps(entry) + "\n")


STATE: Optional[State] = None


def _offset_for_cursor(cursor: Optional[str]) -> int:
    if not cursor:
        return 0
    try:
        return int(json.loads(bytes.fromhex(cursor).decode())["o"])
    except Exception:
        return -1


def _cursor_for_offset(offset: int) -> str:
    return json.dumps({"o": offset}).encode().hex()


def _etag_for(offset: int, take: int) -> str:
    # The collection GENERATION is part of the tag (v1's BENCH2 rank-2 lesson): any mutation
    # — webhook script, PATCH, halfset flip — bumps it, so a 304 never lies about changed data.
    gen = getattr(STATE, "generation", 0) if STATE is not None else 0
    digest = hashlib.sha256(f"meridian2-{gen}-{offset}-{take}".encode()).hexdigest()[:16]
    return f'"{digest}"'


def _public(p: Dict) -> Dict:
    return {k: (dict(v) if isinstance(v, dict) else v)
            for k, v in p.items() if not k.startswith("_")}


def _find_payment(payment_id: str) -> Optional[Dict]:
    row = INDEX.get(payment_id)
    if row is not None:
        return row
    for r in STATE.created if STATE else []:
        if r["id"] == payment_id:
            return r
    return None


def _item_chash(item: Dict) -> str:
    """Content hash of a create item MINUS its idempotency key — the fingerprint that catches
    a fresh-key retry of a failed item (same payment, new key)."""
    amount = item.get("amount") if isinstance(item.get("amount"), dict) else {}
    cp = item.get("counterparty") if isinstance(item.get("counterparty"), dict) else {}
    blob = json.dumps([amount.get("value_minor"), amount.get("currency"), cp.get("name"),
                       cp.get("country"), item.get("occurred_at")], sort_keys=True)
    return hashlib.sha256(blob.encode()).hexdigest()[:16]


def _log_create_op(kind: str, key: Optional[str], item: Dict, outcome: str) -> None:
    with STATE.lock:
        STATE.batch_ops.append({"kind": kind, "key": key, "chash": _item_chash(item),
                                "outcome": outcome, "t": time.time()})


def _webhook_identity(url: str) -> Dict:
    """Registration is idempotent BY CONSTRUCTION: id, secret, and challenge are pure
    functions of the URL, so re-registering after an app restart returns the same pair."""
    sha = hashlib.sha256(("meridian-wh-" + url).encode()).hexdigest()
    return {"url": url, "id": "wh_" + sha[:12], "secret": "whsec_" + sha[12:44],
            "challenge": hashlib.sha256(("challenge-" + sha).encode()).hexdigest()[:32]}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):  # silence stderr spam
        return

    # ── plumbing ──────────────────────────────────────────────────────────────────────────────

    def _send(self, code: int, body: Optional[bytes] = None,
              headers: Optional[Dict[str, str]] = None, ctype: str = "application/json") -> None:
        self.send_response(code)
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        if body is None:
            self.send_header("Content-Length", "0")
            self.end_headers()
        else:
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    def _json(self, code: int, payload: Dict, headers: Optional[Dict[str, str]] = None) -> None:
        self._send(code, json.dumps(payload).encode(), headers)

    def _trace(self, status: int, extra: Optional[Dict] = None) -> None:
        parsed = urlparse(self.path)
        entry = {
            "method": self.command,
            "path": parsed.path,
            "query": {k: v[0] for k, v in parse_qs(parsed.query).items()},
            "status": status,
            "if_none_match": self.headers.get("If-None-Match"),
            "if_match": self.headers.get("If-Match"),
            "idempotency_key": self.headers.get("Idempotency-Key"),
            "authorized": self.headers.get("Authorization") == f"Bearer {API_KEY}",
            "ua": self.headers.get("User-Agent"),
        }
        entry.update(extra or {})
        assert STATE is not None
        STATE.record(entry)

    def _body(self) -> Dict:
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            parsed = json.loads(raw or b"{}")
            return parsed if isinstance(parsed, dict) else {}
        except json.JSONDecodeError:
            return {}

    def _is_local_admin(self) -> bool:
        return self.client_address[0] in ("127.0.0.1", "::1")

    # ── routes ────────────────────────────────────────────────────────────────────────────────

    def do_GET(self):  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path in ("/v2/docs", "/docs"):
            docs = HERE / "vendor_docs_v2.md"
            if docs.exists():
                body = docs.read_bytes()
                self._trace(200)
                self._send(200, body, ctype="text/markdown; charset=utf-8")
            else:
                # Refuse loudly, never improvise: an agent graded on reading the docs must
                # not be served an empty page that looks like docs.
                self._trace(503, {"reason": "vendor_docs_v2.md missing"})
                self._json(503, {"error": "docs_missing"})
            return
        if parsed.path == "/v2/payments":
            self._list(parsed)
            return
        m = re.fullmatch(r"/v2/payments/([A-Za-z0-9_]+)", parsed.path)
        if m:
            row = _find_payment(m.group(1))
            if row is None:
                self._trace(404)
                self._json(404, {"error": "not_found"})
                return
            with STATE.lock:
                payload = _public(row)
            self._trace(200, {"payment_id": row["id"], "version": payload["version"]})
            self._json(200, payload)
            return
        self._trace(404)
        self._json(404, {"error": "not_found"})

    def _list(self, parsed) -> None:
        assert STATE is not None
        params = parse_qs(parsed.query)
        cursor = (params.get("cursor") or [None])[0]
        raw_limit = (params.get("limit") or [str(DEFAULT_LIMIT)])[0]
        try:
            limit = min(max(int(raw_limit), 1), MAX_LIMIT)
        except ValueError:
            limit = DEFAULT_LIMIT

        with STATE.lock:
            STATE.list_requests += 1
            nth = STATE.list_requests
            visible = STATE.visible_rows if STATE.visible_rows is not None else len(PAYMENTS)
            halfset = STATE.visible_rows is not None
            throttle = nth == THROTTLE_SECONDS_NTH and "secs" not in STATE.fired
            if throttle:
                STATE.fired.add("secs")
            expire = (nth == CURSOR_EXPIRES_NTH and bool(cursor)
                      and "gone" not in STATE.fired)
            if expire:
                STATE.fired.add("gone")
            httpdate = ("gone" in STATE.fired and "date" not in STATE.fired
                        and not expire and not throttle)
            if httpdate:
                STATE.fired.add("date")

        if throttle:
            self._trace(429, {"retry_after": RETRY_AFTER_SECS, "form": "seconds"})
            self._json(429, {"error": "rate_limited"},
                       headers={"Retry-After": str(RETRY_AFTER_SECS)})
            return

        if expire:
            self._trace(410, {"reason": "cursor_expired"})
            self._json(410, {"error": "cursor_expired"})
            return

        if httpdate:
            when = datetime.now(timezone.utc) + timedelta(seconds=RETRY_AFTER_SECS)
            stamp = when.strftime("%a, %d %b %Y %H:%M:%S GMT")
            self._trace(429, {"retry_after": stamp, "form": "http-date"})
            self._json(429, {"error": "rate_limited"}, headers={"Retry-After": stamp})
            return

        offset = _offset_for_cursor(cursor)
        if offset < 0 or offset > visible:
            self._trace(400)
            self._json(400, {"error": "bad_cursor"})
            return

        take = limit
        if offset == SHORT_PAGE_AT and limit > SHORT_PAGE_TAKE:
            take = SHORT_PAGE_TAKE  # documented: a short page may appear anywhere; NOT the end

        end = min(offset + take, visible)
        nxt = end
        is_last = nxt >= visible

        # F8: the one-shot stall — the final page (200 OR 304: a held 304 keeps the sync just
        # as in-flight) is held open a fixed window. Armed only when stall_enabled; the fired
        # set makes it one-shot per phase, so the client's timeout-and-retry lands instantly.
        stall_held = 0.0
        if is_last:
            with STATE.lock:
                do_stall = STATE.stall_enabled and "stall" not in STATE.fired
                if do_stall:
                    STATE.fired.add("stall")
                    stall_held = STATE.stall_secs
            if stall_held:
                t0 = round(time.time(), 4)
                with STATE.lock:
                    STATE.stall_info = {"began_t": t0, "resolved_t": None, "offset": offset,
                                        "client_dropped": False, "served_after_hold": False}
                time.sleep(stall_held)

        etag = _etag_for(offset, take)
        try:
            if self.headers.get("If-None-Match") == etag:
                self._trace(304, {"offset": offset, "halfset": halfset,
                                  **({"stall_held_secs": stall_held, "stall_began_t": t0}
                                     if stall_held else {})})
                self._send(304, None, headers={"ETag": etag})
                if stall_held:
                    with STATE.lock:
                        STATE.stall_info["served_after_hold"] = True
                        STATE.stall_info["resolved_t"] = round(time.time(), 4)
                return

            with STATE.lock:
                rows = [_public(p) for p in PAYMENTS[offset:end]]
            payload = {
                "data": rows,
                "next_cursor": _cursor_for_offset(nxt) if not is_last else None,
                "total": visible,
            }
            self._trace(200, {"offset": offset, "returned": len(rows), "limit": limit,
                              "last": is_last, "halfset": halfset,
                              **({"stall_held_secs": stall_held, "stall_began_t": t0}
                                 if stall_held else {})})
            self._json(200, payload, headers={"ETag": etag})
            if stall_held:
                with STATE.lock:
                    STATE.stall_info["served_after_hold"] = True
                    STATE.stall_info["resolved_t"] = round(time.time(), 4)
        except (BrokenPipeError, ConnectionResetError, TimeoutError, OSError):
            # The client hung up mid-stall — that IS the documented timeout behaviour, and
            # the trace is where the scorer reads it.
            if stall_held:
                with STATE.lock:
                    STATE.stall_info["client_dropped"] = True
                    STATE.stall_info["resolved_t"] = round(time.time(), 4)
                self._trace(0, {"offset": offset, "stall_client_dropped": True,
                                "stall_held_secs": stall_held, "stall_began_t": t0})

    # ── writes ────────────────────────────────────────────────────────────────────────────────

    def do_POST(self):  # noqa: N802
        assert STATE is not None
        parsed = urlparse(self.path)
        body = self._body()

        if parsed.path.startswith("/admin/"):
            self._admin(parsed.path, body)
            return
        if parsed.path == "/v2/payments":
            self._create_single(body)
            return
        if parsed.path == "/v2/payments/batch":
            self._create_batch(body)
            return
        if parsed.path == "/v2/webhooks":
            self._register_webhook(body)
            return
        self._trace(404)
        self._json(404, {"error": "not_found"})

    def _validate_item(self, item: Dict) -> Optional[Dict]:
        """The vendor's own business validation; returns an error dict or None."""
        amount = item.get("amount") if isinstance(item.get("amount"), dict) else {}
        value = amount.get("value_minor")
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            return {"code": "invalid_amount", "message": "amount.value_minor must be a positive integer"}
        if amount.get("currency") not in fixtures_v2.CURRENCY_EXPONENTS:
            return {"code": "unsupported_currency", "message": "currency not supported"}
        if value > AMOUNT_LIMIT_MINOR:
            return {"code": "amount_over_limit",
                    "message": f"per-payment limit is {AMOUNT_LIMIT_MINOR} minor units"}
        cp = item.get("counterparty") if isinstance(item.get("counterparty"), dict) else {}
        if not cp.get("name") or not re.fullmatch(r"[A-Z]{2}", str(cp.get("country") or "")):
            return {"code": "invalid_counterparty", "message": "counterparty name/country invalid"}
        occurred = item.get("occurred_at")
        try:
            datetime.fromisoformat(str(occurred).replace("Z", "+00:00"))
        except (ValueError, TypeError):
            return {"code": "bad_format", "message": "occurred_at must be RFC 3339"}
        return None

    def _make_payment(self, item: Dict) -> Dict:
        payment_id = f"pay_new_{len(STATE.created):04d}"
        record = {
            "id": payment_id,
            "amount_minor": item["amount"]["value_minor"],
            "currency": item["amount"]["currency"],
            "created_at": item["occurred_at"],   # deterministic: the vendor stamps the instant given
            "settled_at": None,
            "status": "pending",
            "version": 1,
            "note": "",
            "counterparty": {"name": item["counterparty"]["name"],
                             "country": item["counterparty"]["country"]},
        }
        STATE.created.append(record)
        return record

    def _create_single(self, body: Dict) -> None:
        key = self.headers.get("Idempotency-Key")
        if not key:
            self._trace(400, {"reason": "missing_idempotency_key"})
            self._json(400, {"error": "idempotency_key_required"})
            return
        err = self._validate_item(body)
        if err:
            _log_create_op("single", key, body, f"error:{err['code']}")
            self._trace(400, {"reason": err["code"]})
            self._json(400, {"error": err["code"]})
            return
        with STATE.lock:
            existing = STATE.idempotency.get(key)
        if existing:
            # v1 semantics, unchanged: 409 duplicate IS the success path for a replayed create.
            _log_create_op("single", key, body, "duplicate")
            self._trace(409, {"payment_id": existing})
            self._json(409, {"error": "duplicate", "payment_id": existing})
            return
        with STATE.lock:
            record = self._make_payment(body)
            STATE.idempotency[key] = record["id"]
        _log_create_op("single", key, body, "created")
        self._trace(201, {"payment_id": record["id"]})
        self._json(201, _public(record))

    def _create_batch(self, body: Dict) -> None:
        items = body.get("items")
        if not isinstance(items, list) or not items:
            self._trace(400, {"reason": "no_items"})
            self._json(400, {"error": "no_items"})
            return
        if len(items) > BATCH_MAX_ITEMS:
            self._trace(400, {"reason": "too_many_items", "count": len(items)})
            self._json(400, {"error": "too_many_items"})
            return
        results, traced = [], []
        succeeded = failed = 0
        for index, item in enumerate(items):
            item = item if isinstance(item, dict) else {}
            key = item.get("idempotency_key")
            if not key or not isinstance(key, str):
                results.append({"index": index, "status": "error",
                                "error": {"code": "idempotency_key_required",
                                          "message": "each item needs a non-empty idempotency_key"}})
                traced.append({"index": index, "outcome": "error:idempotency_key_required"})
                failed += 1
                continue
            err = self._validate_item(item)
            if err:
                # Items are INDEPENDENT: one failure never discards or retries its neighbours.
                results.append({"index": index, "status": "error", "error": err})
                traced.append({"index": index, "key": key, "outcome": f"error:{err['code']}"})
                failed += 1
                continue
            with STATE.lock:
                existing = STATE.idempotency.get(key)
                if existing:
                    results.append({"index": index, "status": "created", "id": existing,
                                    "duplicate": True})
                    traced.append({"index": index, "key": key, "outcome": "duplicate",
                                   "payment_id": existing})
                    succeeded += 1
                    continue
                record = self._make_payment(item)
                STATE.idempotency[key] = record["id"]
            results.append({"index": index, "status": "created", "id": record["id"]})
            traced.append({"index": index, "key": key, "outcome": "created",
                           "payment_id": record["id"]})
            succeeded += 1
        for tr in traced:
            item = items[tr["index"]]
            _log_create_op("batch", tr.get("key"),
                           item if isinstance(item, dict) else {}, tr["outcome"])
        self._trace(200, {"batch_items": traced, "succeeded": succeeded, "failed": failed})
        self._json(200, {"results": results, "succeeded": succeeded, "failed": failed})

    def do_PATCH(self):  # noqa: N802
        assert STATE is not None
        parsed = urlparse(self.path)
        m = re.fullmatch(r"/v2/payments/([A-Za-z0-9_]+)", parsed.path)
        if not m:
            self._trace(404)
            self._json(404, {"error": "not_found"})
            return
        row = _find_payment(m.group(1))
        if row is None:
            self._trace(404)
            self._json(404, {"error": "not_found"})
            return
        raw_if_match = self.headers.get("If-Match")
        if raw_if_match is None:
            # 428 is a BUG in the client, not a retry case — the docs say every write
            # carries If-Match. The trace makes writes_without_if_match countable.
            with STATE.lock:
                STATE.patch_log.append({"id": row["id"], "if_match": None, "status": 428,
                                        "injected": False, "mode": STATE.conflict_mode,
                                        "t": time.time()})
            self._trace(428, {"payment_id": row["id"], "current_version": row["version"]})
            self._json(428, {"error": "precondition_required"})
            return
        wanted = re.sub(r'^(W/)?"?|"?$', "", raw_if_match.strip())
        if not wanted.isdigit():
            self._trace(400, {"reason": "bad_if_match", "payment_id": row["id"]})
            self._json(400, {"error": "bad_if_match"})
            return
        fields = self._body()
        # Decide under the lock, respond AFTER releasing it — _trace() takes the same lock.
        with STATE.lock:
            current = row["version"]
            mode = STATE.conflict_mode
            if int(wanted) != current:
                verdict = ("mismatch", current)
            else:
                inject = mode == "always" or (mode == "once" and "c412" not in STATE.fired)
                if inject:
                    if mode == "once":
                        STATE.fired.add("c412")
                    # A phantom concurrent editor that REALLY happened: the version bumps,
                    # so the documented refetch sees fresh state and the one retry succeeds.
                    row["version"] += 1
                    STATE.generation += 1
                    verdict = ("injected", row["version"])
                else:
                    applied = {}
                    for key in ("note", "status"):
                        if key in fields and isinstance(fields[key], str):
                            row[key] = fields[key]
                            applied[key] = fields[key]
                    row["version"] += 1
                    STATE.generation += 1
                    verdict = ("ok", _public(row))
        with STATE.lock:
            STATE.patch_log.append({
                "id": row["id"], "if_match": wanted, "mode": mode, "t": time.time(),
                "status": 412 if verdict[0] in ("mismatch", "injected") else 200,
                "injected": verdict[0] == "injected",
                "version_after": row["version"]})
        if verdict[0] == "mismatch":
            self._trace(412, {"payment_id": row["id"], "current_version": verdict[1],
                              "injected_412": False, "mode": mode})
            self._json(412, {"error": "version_conflict"})
        elif verdict[0] == "injected":
            self._trace(412, {"payment_id": row["id"], "current_version": verdict[1],
                              "injected_412": True, "mode": mode})
            self._json(412, {"error": "version_conflict"})
        else:
            payload = verdict[1]
            self._trace(200, {"payment_id": row["id"], "applied_fields": sorted(applied),
                              "new_version": payload["version"]})
            self._json(200, payload)

    # ── webhooks ──────────────────────────────────────────────────────────────────────────────

    def _register_webhook(self, body: Dict) -> None:
        url = body.get("url")
        if not isinstance(url, str) or not url.startswith("http"):
            self._trace(400, {"reason": "bad_url"})
            self._json(400, {"error": "bad_url"})
            return
        ident = _webhook_identity(url)
        # The challenge handshake runs on EVERY registration call (the server must already be
        # listening — including after a restart re-registration). It is unsigned, because the
        # secret does not exist for the app until this call returns. F13: it counts nothing.
        challenge = ident["challenge"]
        outcome = "error"
        try:
            req = urllib.request.Request(
                url, data=json.dumps({"type": "webhook.verify", "challenge": challenge}).encode(),
                headers={"Content-Type": "application/json"}, method="POST")
            with urllib.request.urlopen(req, timeout=DELIVERY_TIMEOUT_SECS) as resp:
                answer = json.loads(resp.read() or b"{}")
                outcome = "ok" if (resp.status == 200
                                   and answer.get("challenge") == challenge) else "mismatch"
        except (urllib.error.URLError, json.JSONDecodeError, OSError) as exc:
            outcome = f"error:{type(exc).__name__}"
        if outcome != "ok":
            self._trace(400, {"webhook_url": url, "handshake": outcome})
            self._json(400, {"error": "verification_failed"})
            return
        with STATE.lock:
            STATE.webhook = {"url": url, "id": ident["id"], "secret": ident["secret"]}
        self._trace(200, {"webhook_url": url, "webhook_id": ident["id"], "handshake": "ok"})
        self._json(200, {"id": ident["id"], "secret": ident["secret"]})

    # ── admin (localhost-only, undocumented in vendor docs — grader plumbing, F3) ─────────────

    def _admin(self, path: str, body: Dict) -> None:
        if not self._is_local_admin():
            self._trace(403, {"admin": path})
            self._json(403, {"error": "forbidden"})
            return
        if path == "/admin/deliver-script":
            with STATE.lock:
                registered = STATE.webhook is not None
            if not registered:
                self._trace(409, {"admin": path, "reason": "no_webhook_registered"})
                self._json(409, {"error": "no_webhook_registered"})
                return
            result = run_delivery_script()
            self._trace(200, {"admin": path, "delivered": result["delivered"]})
            self._json(200, result)
            return
        if path == "/admin/halfset":
            set_halfset(bool(body.get("on")))
            self._trace(200, {"admin": path, "on": bool(body.get("on"))})
            self._json(200, {"ok": True, "visible_rows": STATE.visible_rows})
            return
        if path == "/admin/conflict":
            mode = body.get("mode")
            if mode not in ("once", "always", "off"):
                self._trace(400, {"admin": path, "reason": "bad_mode"})
                self._json(400, {"error": "bad_mode"})
                return
            set_conflict_mode(mode)
            self._trace(200, {"admin": path, "mode": mode})
            self._json(200, {"ok": True, "mode": mode})
            return
        if path == "/admin/stall":
            arm_stall(bool(body.get("on")), body.get("secs"))
            self._trace(200, {"admin": path, "on": bool(body.get("on")),
                              "secs": STATE.stall_secs})
            self._json(200, {"ok": True, "on": STATE.stall_enabled, "secs": STATE.stall_secs})
            return
        if path == "/admin/phase":
            name = str(body.get("name") or "phase")
            mark_phase(name)
            self._trace(200, {"admin": path, "name": name})
            self._json(200, {"ok": True})
            return
        self._trace(404, {"admin": path})
        self._json(404, {"error": "not_found"})


# ── grader-facing controls (module functions mirror the /admin endpoints) ────────────────────

def set_halfset(on: bool) -> None:
    """F2: cap the visible collection to the first HALFSET_ROWS delivery-order rows.
    Flipping the cap is a genuine collection change, so the generation bumps — a client's
    cached ETags from the half collection must NOT 304 against the full one."""
    assert STATE is not None
    with STATE.lock:
        STATE.visible_rows = HALFSET_ROWS if on else None
        STATE.generation += 1


def set_conflict_mode(mode: str) -> None:
    assert STATE is not None
    with STATE.lock:
        STATE.conflict_mode = mode
        if mode == "once":
            STATE.fired.discard("c412")   # explicit re-arm, matching the phase-reset semantics


def arm_stall(on=True, secs: Optional[float] = None) -> None:
    """Arm (re-arm) the one-shot final-page stall. Accepts both call shapes the harness has
    used: arm_stall(True, 12.0) and arm_stall(12.0) — a bare number is the hold in seconds."""
    assert STATE is not None
    if isinstance(on, (int, float)) and not isinstance(on, bool):
        secs, on = float(on), True
    with STATE.lock:
        STATE.stall_enabled = bool(on)
        if secs is not None:
            STATE.stall_secs = float(secs)
        if on:
            STATE.fired.discard("stall")


# ── score_sb6-facing surface (the exact names gather() consumes) ─────────────────────────────

def set_visible_cap(n: int) -> None:
    """F2 half-seed: serve only the first n delivery-order rows. A cap change is a genuine
    collection change, so the generation bumps and stale ETags stop matching."""
    assert STATE is not None
    with STATE.lock:
        STATE.visible_rows = max(0, min(int(n), len(PAYMENTS)))
        STATE.generation += 1


def clear_visible_cap() -> None:
    assert STATE is not None
    with STATE.lock:
        STATE.visible_rows = None
        STATE.generation += 1


def arm_conflict_once() -> None:
    """Re-arm the one-shot injected 412 for the graded note write."""
    set_conflict_mode("once")


def arm_conflict_second() -> None:
    """Make the resource permanently contended for the scripted second-412 case."""
    set_conflict_mode("always")


def conflict_trace() -> Dict:
    """The 412-dance evidence, derived from the PATCH log (h_conflict_dance's inputs). The
    graded exercise is the LAST mode-'once' injected 412; always-mode injections (the scripted
    second-412 case) are excluded — gather supplies that case's local-status keys itself."""
    assert STATE is not None
    with STATE.lock:
        log = list(STATE.patch_log)
    out: Dict = {"patches": len(log),
                 "writes_without_if_match": sum(1 for e in log if e["status"] == 428)}
    injected = [i for i, e in enumerate(log) if e.get("injected") and e.get("mode") == "once"]
    if injected:
        i0 = injected[-1]
        pid, fresh = log[i0]["id"], log[i0]["version_after"]
        upto: List[Dict] = []
        for e in log[i0 + 1:]:
            if e["id"] != pid:
                continue
            upto.append(e)
            if e["status"] == 200:
                break
        out["retries_after_412"] = len(upto)
        out["retried_once_with_fresh_version"] = bool(
            upto and upto[-1]["status"] == 200 and upto[0].get("if_match") == str(fresh))
    else:
        out["retries_after_412"] = 0
        out["retried_once_with_fresh_version"] = False
    return out


def batch_trace() -> Dict:
    """Create-op evidence for c_batch_partial: no re-created items, no retry of a failed item
    (same key), no fresh-key retry (same content, new key)."""
    assert STATE is not None
    with STATE.lock:
        ops = list(STATE.batch_ops)
    duplicate_creates = retries_of_failed = fresh_key_retries = 0
    failed_keys: set = set()
    failed_chashes: set = set()
    for op in ops:
        outcome = op["outcome"]
        if outcome == "duplicate":
            duplicate_creates += 1
        if op.get("key") and op["key"] in failed_keys:
            retries_of_failed += 1
        elif not outcome.startswith("error") and op["chash"] in failed_chashes:
            fresh_key_retries += 1
        if outcome.startswith("error"):
            if op.get("key"):
                failed_keys.add(op["key"])
            failed_chashes.add(op["chash"])
    return {"ops": len(ops), "duplicate_creates": duplicate_creates,
            "retries_of_failed_item": retries_of_failed,
            "fresh_key_retries": fresh_key_retries}


def trap_results() -> Dict:
    """{retry_after, cursor_expiry, stall} in [0,1], graded over EVERY instance in this
    serve()'s trace. Blocks until an in-flight stall resolves — its verdict (client dropped
    vs waited it out) is only written when the handler wakes from the hold."""
    assert STATE is not None
    deadline = time.time() + STATE.stall_secs + 3
    while time.time() < deadline:
        with STATE.lock:
            info = dict(STATE.stall_info)
        if info["began_t"] is None or info["resolved_t"] is not None:
            break
        time.sleep(0.2)
    with STATE.lock:
        events = list(STATE.events)
        info = dict(STATE.stall_info)

    def _next_list(i: int) -> Optional[Dict]:
        return next((x for x in events[i + 1:]
                     if x.get("path") == LIST_PATH and x.get("method") == "GET"), None)

    ra_scores = []
    for i, e in enumerate(events):
        if e.get("path") != LIST_PATH or e.get("status") != 429:
            continue
        nxt = _next_list(i)
        if nxt is None:
            ra_scores.append(0.0)
            continue
        # http-date has 1 s wire granularity, so its floor is a second looser
        floor = (RETRY_AFTER_SECS - 1.3) if e.get("form") == "http-date" \
            else (RETRY_AFTER_SECS - 0.3)
        ra_scores.append(1.0 if (nxt["t"] - e["t"]) >= max(0.1, floor) else 0.0)
    ce_scores = []
    for i, e in enumerate(events):
        if e.get("path") != LIST_PATH or e.get("status") != 410:
            continue
        nxt = _next_list(i)
        ce_scores.append(1.0 if nxt is not None and not (nxt.get("query") or {}).get("cursor")
                         else 0.0)
    stall = 0.0
    retried_during_hold = False
    if info["began_t"] is not None:
        resolved = info.get("resolved_t") or (info["began_t"] + STATE.stall_secs + 1)
        # The PROOF of a client-side timeout is the retry LANDING WHILE THE HOLD IS STILL
        # OPEN — a same-page request inside (began, resolved). The write-failure signal
        # (client_dropped) is kept as corroboration but is unreliable on loopback: a small
        # response fits the kernel socket buffer and "succeeds" into a dead socket
        # (measured: a 10s-timeout client read as served_after_hold).
        retried_during_hold = any(
            e.get("path") == LIST_PATH and e.get("method") == "GET"
            and e.get("offset") == info.get("offset") and not e.get("stall_held_secs")
            and info["began_t"] + 0.05 < e.get("t", 0) < resolved - 0.05
            for e in events)
        completed_after = any(
            e.get("path") == LIST_PATH and e.get("status") in (200, 304)
            and not e.get("stall_held_secs") and e.get("offset") == info.get("offset")
            and e.get("t", 0) >= info["began_t"]
            for e in events)
        if (retried_during_hold or info["client_dropped"]) and completed_after:
            stall = 1.0                    # documented behaviour: timed out inside the hold, retried
        elif info["served_after_hold"] or completed_after:
            stall = 0.5                    # no client timeout: waited the full hold out
    return {"retry_after": min(ra_scores) if ra_scores else 0.0,
            "cursor_expiry": min(ce_scores) if ce_scores else 0.0,
            "stall": stall,
            "detail": {"n_429": len(ra_scores), "n_410": len(ce_scores),
                       "retried_during_hold": retried_during_hold, "stall_info": info}}


MUTATE_TARGET_IDS = [f"pay_{100 * (k + 1):04d}" for k in range(12)]   # pay_0100 … pay_1200:
# disjoint from the webhook script's targets (pay_0010/0020/0030/0040) and from pay_0000
# (the note/conflict target), so no exercise invalidates another's expected versions.


def mutate_statuses() -> Dict[str, str]:
    """Flip 12 spread payments to a different status (version + generation bump) and return
    {id: new_status} — gather verifies each id locally after sync3. unmutate_statuses()
    restores the originals with ANOTHER version bump so a further sync propagates the
    restoration instead of being blocked by the app's version-regression guard; buckets and
    currency expectations are valid again once that sync lands."""
    assert STATE is not None
    changed: Dict[str, str] = {}
    with STATE.lock:
        for pid in MUTATE_TARGET_IDS:
            row = INDEX.get(pid)
            if row is None:
                continue
            STATE.mutated[pid] = row["status"]
            row["status"] = "failed" if row["status"] != "failed" else "settled"
            row["version"] += 1
            changed[pid] = row["status"]
        STATE.generation += 1
    return changed


def unmutate_statuses() -> int:
    assert STATE is not None
    with STATE.lock:
        n = 0
        for pid, original in STATE.mutated.items():
            row = INDEX.get(pid)
            if row is not None:
                row["status"] = original
                row["version"] += 1
                n += 1
        STATE.mutated.clear()
        STATE.generation += 1
    return n


def restore_payments() -> None:
    """Undo every mutation (webhook script, PATCHes) for the NEXT scoring run in this
    process — PAYMENTS is module state. Bumps the generation: the restored state is a new
    data generation, and an ETag claiming it matches the mutated one would lie."""
    global PAYMENTS, INDEX
    fresh = fixtures_v2.fresh_payments_delivery_order()
    PAYMENTS[:] = fresh
    INDEX = {p["id"]: p for p in PAYMENTS}
    if STATE is not None:
        with STATE.lock:
            STATE.generation += 1


def mark_phase(name: str, reset: bool = None) -> None:
    """Record a phase boundary in the trace so the grader can attribute requests (v1's
    seq-3-vs-seq-38 lesson, unchanged). Reset re-arms every one-shot — the 429/410/date
    chain, the injected 412, and the stall — and clears the idempotency store. Phases whose
    name ends in a number >= 2 are continuations and reset nothing.

    F2 sugar: the literal phase name "halfset" additionally applies the half-collection cap
    (and, being a first phase, resets the one-shots for the sync it gates)."""
    assert STATE is not None
    if reset is None:
        _m = re.search(r"(\d+)$", name)
        reset = not (_m and int(_m.group(1)) >= 2)
    with STATE.lock:
        if reset:
            STATE.list_requests = 0
            STATE.fired.clear()
            STATE.idempotency.clear()
    if name == "halfset":
        set_halfset(True)
    STATE.record({PHASE_MARKER: name, "method": "-", "path": "-", "status": 0, "query": {}})


def begin_exercise_phase() -> None:
    """Reset the one-shot behaviours and mark the trace, so grading measures the DELIVERED
    client, not the agent's own development testing (measured in v1: Opus's one-shot 429 was
    consumed at trace seq 3 while the graded run began at seq 38). Registration and the
    halfset cap survive on purpose: both are controlled explicitly by the grader."""
    assert STATE is not None
    with STATE.lock:
        STATE.list_requests = 0
        STATE.fired.clear()
        STATE.idempotency.clear()
    STATE.record({PHASE_MARKER: "exercise", "method": "-", "path": "-", "status": 0, "query": {}})


def run_delivery_script() -> Dict:
    """Execute fixtures_v2.WEBHOOK_SCRIPT verbatim against the registered URL (F3: this is
    the ONLY thing that ever fires a delivery). Mutating steps really mutate the vendor's
    rows first, so every signed payload is a true snapshot — except the forged one, whose
    claimed mutation never happened and whose signature does not verify."""
    assert STATE is not None
    with STATE.lock:
        hook = dict(STATE.webhook or {})
    assert hook, "run_delivery_script() requires a registered webhook"
    url, secret = hook["url"], hook["secret"]
    snapshots: Dict[str, Dict] = {}
    results = []
    delivered = 0
    for step in fixtures_v2.WEBHOOK_SCRIPT:
        kind = step["kind"]
        if kind == "mutate_only":
            with STATE.lock:
                row = INDEX[step["target"]]
                row["version"] += 1
                row["note"] = step["note"]
                snapshots[step["target"]] = _public(row)
            continue
        if kind == "duplicate":
            sent = STATE.sent_events[step["event_id"]]
            raw, sig_header = sent["raw"], sent["sig"]      # byte-identical redelivery
        else:
            if kind == "apply":
                with STATE.lock:
                    row = INDEX[step["target"]]
                    row["version"] += 1
                    row["note"] = step["note"]
                    data = _public(row)
            elif kind == "stale":
                data = snapshots[step["target"]]            # the older, already-superseded state
            else:  # forged: claims a mutation the vendor never made
                with STATE.lock:
                    row = INDEX[step["target"]]
                    data = _public(row)
                data["version"] = data["version"] + 1
                data["note"] = step["note"]
            event = {"id": step["event_id"], "type": "payment.updated",
                     "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                     "data": data}
            raw = json.dumps(event).encode()
            t = int(time.time())
            signing_secret = "whsec_not_the_secret" if kind == "forged" else secret
            sig = hmac_mod.new(signing_secret.encode(), f"{t}.".encode() + raw,
                               hashlib.sha256).hexdigest()
            sig_header = f"t={t},v1={sig}"
            STATE.sent_events[step["event_id"]] = {"raw": raw, "sig": sig_header}
        status, err = None, None
        t0 = time.time()
        try:
            req = urllib.request.Request(
                url, data=raw, method="POST",
                headers={"Content-Type": "application/json", "Meridian-Signature": sig_header})
            with urllib.request.urlopen(req, timeout=DELIVERY_TIMEOUT_SECS) as resp:
                status = resp.status
                resp.read()
        except urllib.error.HTTPError as exc:
            status = exc.code
            exc.read()
        except (urllib.error.URLError, OSError) as exc:
            err = type(exc).__name__
        ms = round((time.time() - t0) * 1000, 1)
        delivered += 1
        STATE.record({"webhook_delivery": step["event_id"], "kind": kind, "delivery_status": status,
                      "delivery_error": err, "ms": ms, "method": "POST(out)", "path": url,
                      "status": status or 0, "query": {}})
        results.append({"event_id": step["event_id"], "kind": kind, "status": status,
                        "error": err, "ms": ms})
    with STATE.lock:
        STATE.generation += 1   # the script mutated rows: cached ETags must not 304
    return {"delivered": delivered, "results": results,
            "expected_counters": EXPECTED_WEBHOOK_COUNTERS}


def serve(port: int, trace: Path, stall: bool = False,
          stall_secs: Optional[float] = None) -> ThreadingHTTPServer:
    global STATE
    restore_payments()   # PAYMENTS is module state: a prior run's mutations must not leak in
    trace.parent.mkdir(parents=True, exist_ok=True)
    trace.write_text("")
    STATE = State(trace, stall_enabled=stall, stall_secs=stall_secs)
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


# ══ smoke: serve on 8899 and walk EVERY trap with urllib, PASS/FAIL per trap ═════════════════

def _req(method: str, url: str, body: Optional[Dict] = None,
         headers: Optional[Dict[str, str]] = None, timeout: float = 10):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    if data is not None:
        req.add_header("Content-Type", "application/json")

    def _parse(raw: bytes) -> Dict:
        try:
            return json.loads(raw or b"{}")
        except json.JSONDecodeError:          # /v2/docs serves markdown, not JSON
            return {"_raw": raw.decode(errors="replace")[:200]}

    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, dict(resp.headers), _parse(resp.read())
    except urllib.error.HTTPError as exc:
        return exc.code, dict(exc.headers), _parse(exc.read())


class _SmokeReceiver(BaseHTTPRequestHandler):
    challenges: List[str] = []
    deliveries: List[Dict] = []

    def log_message(self, *_args):
        return

    def do_POST(self):  # noqa: N802
        raw = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        body = json.loads(raw or b"{}")
        if body.get("type") == "webhook.verify":
            _SmokeReceiver.challenges.append(body.get("challenge"))
            out = json.dumps({"challenge": body.get("challenge")}).encode()
        else:
            _SmokeReceiver.deliveries.append(
                {"raw": raw, "sig": self.headers.get("Meridian-Signature"), "body": body})
            out = json.dumps({"received": True}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


def _verify_sig(secret: str, raw: bytes, header: Optional[str]) -> bool:
    m = re.fullmatch(r"t=(\d+),v1=([0-9a-f]+)", header or "")
    if not m:
        return False
    want = hmac_mod.new(secret.encode(), f"{m.group(1)}.".encode() + raw,
                        hashlib.sha256).hexdigest()
    return hmac_mod.compare_digest(want, m.group(2))


def smoke(port: int = 8899, receiver_port: int = 8900) -> int:
    import tempfile
    fx = fixtures_v2
    base = f"http://127.0.0.1:{port}"
    trace = Path(tempfile.mkdtemp(prefix="meridian2-smoke-")) / "trace.jsonl"
    serve(port, trace)
    receiver = ThreadingHTTPServer(("127.0.0.1", receiver_port), _SmokeReceiver)
    threading.Thread(target=receiver.serve_forever, daemon=True).start()
    results: List[tuple] = []

    def report(name: str, ok: bool, detail: str) -> None:
        results.append((name, ok))
        print(f"{'PASS' if ok else 'FAIL'}  {name:22s} {detail}")

    begin_exercise_phase()

    # 1) pagination chain: 429-seconds, 410 restart, 429-date, short page, optimal count
    rows, events, page_sizes, reqs, cursor, etag0, last_offset = [], [], [], 0, None, None, 0
    while True:
        q = f"?limit={fx.MAX_LIMIT}" + (f"&cursor={cursor}" if cursor else "")
        status, hdrs, body = _req("GET", base + "/v2/payments" + q)
        reqs += 1
        if status == 429:
            events.append(("429", hdrs.get("Retry-After", "")))
            continue
        if status == 410:
            events.append(("410", body.get("error")))
            cursor, rows, page_sizes = None, [], []
            continue
        assert status == 200, (status, body)
        if cursor is None and etag0 is None:
            etag0 = hdrs.get("ETag")
        rows.extend(body["data"])
        page_sizes.append(len(body["data"]))
        if body["next_cursor"] is None:
            last_offset = body["total"] - len(body["data"])
            break
        cursor = body["next_cursor"]
    chain_ok = (len(events) == 3
                and events[0][0] == "429" and events[0][1].isdigit()
                and events[1] == ("410", "cursor_expired")
                and events[2][0] == "429" and "GMT" in events[2][1])
    report("paging_chain", chain_ok, f"events={events}")
    ids = [r["id"] for r in rows]
    report("full_collection", len(ids) == fx.EXPECTED_TOTAL and len(set(ids)) == fx.EXPECTED_TOTAL,
           f"{len(set(ids))}/{fx.EXPECTED_TOTAL} unique rows")
    report("optimal_requests", reqs == fx.OPTIMAL_REQUESTS,
           f"{reqs} requests (simulated optimum {fx.OPTIMAL_REQUESTS})")
    report("short_page", fx.SHORT_PAGE_TAKE in page_sizes[:-1],
           f"page sizes {page_sizes} (short {fx.SHORT_PAGE_TAKE} at offset {fx.SHORT_PAGE_AT}, "
           f"and NOT the end)")
    by_cur = {c: {"count": 0, "total_minor": 0} for c in fx.CURRENCY_EXPONENTS}
    for r in rows:
        by_cur[r["currency"]]["count"] += 1
        by_cur[r["currency"]]["total_minor"] += r["amount_minor"]
    report("currency_totals", by_cur == fx.EXPECTED_BY_CURRENCY, "recomputed from delivered rows")
    got_buckets = fx.compute_buckets(
        [{"_instant": r["created_at"], "status": r["status"]} for r in rows])
    report("berlin_buckets", got_buckets == fx.EXPECTED_BUCKETS,
           f"{len(got_buckets['cells'])} cells from public created_at strings")

    # 2) conditional requests: the walk's first-page ETag must 304
    status, _, _ = _req("GET", base + f"/v2/payments?limit={fx.MAX_LIMIT}",
                        headers={"If-None-Match": etag0})
    report("etag_304", status == 304, f"If-None-Match {etag0} -> {status}")

    # 3) halfset cap (F2)
    _req("POST", base + "/admin/halfset", {"on": True})
    _, _, half = _req("GET", base + "/v2/payments?limit=1")
    _req("POST", base + "/admin/halfset", {"on": False})
    _, _, full = _req("GET", base + "/v2/payments?limit=1")
    report("halfset_cap", half.get("total") == fx.HALFSET_ROWS
           and full.get("total") == fx.EXPECTED_TOTAL,
           f"capped total {half.get('total')} (want {fx.HALFSET_ROWS}), restored {full.get('total')}")

    # 4) create + idempotency replay + amount limit
    item = {"amount": {"value_minor": 4500, "currency": "EUR"},
            "counterparty": {"name": "Smoke Co", "country": "DE"},
            "occurred_at": "2026-04-06T10:00:00Z"}
    s1, _, b1 = _req("POST", base + "/v2/payments", item, {"Idempotency-Key": "smoke-k1"})
    s2, _, b2 = _req("POST", base + "/v2/payments", item, {"Idempotency-Key": "smoke-k1"})
    over = dict(item, amount={"value_minor": fx.AMOUNT_LIMIT_MINOR + 1, "currency": "EUR"})
    s3, _, b3 = _req("POST", base + "/v2/payments", over, {"Idempotency-Key": "smoke-k2"})
    report("create_replay", s1 == 201 and s2 == 409 and b2.get("payment_id") == b1.get("id")
           and s3 == 400 and b3.get("error") == "amount_over_limit",
           f"201/{s2} dup->{b2.get('payment_id')} over-limit->{s3}:{b3.get('error')}")

    # 5) batch partial failure — succeeded items stay, the failed one reports its own error
    batch = {"items": [
        dict(item, idempotency_key="smoke-b1",
             amount={"value_minor": 1000, "currency": "USD"}),
        dict(item, idempotency_key="smoke-b2",
             amount={"value_minor": fx.AMOUNT_LIMIT_MINOR + 1, "currency": "USD"}),
        dict(item, idempotency_key="smoke-b3", amount={"value_minor": 250, "currency": "JPY"}),
    ]}
    s4, _, b4 = _req("POST", base + "/v2/payments/batch", batch)
    r = b4.get("results", [])
    batch_ok = (s4 == 200 and [x.get("index") for x in r] == [0, 1, 2]
                and r[0].get("status") == "created" and r[2].get("status") == "created"
                and r[1].get("status") == "error"
                and r[1].get("error", {}).get("code") == "amount_over_limit"
                and b4.get("succeeded") == 2 and b4.get("failed") == 1)
    s5, _, b5 = _req("POST", base + "/v2/payments/batch", batch)
    replay_ok = (b5.get("results", [{}])[0].get("duplicate") is True
                 and b5["results"][0].get("id") == r[0].get("id"))
    report("batch_partial", batch_ok and replay_ok,
           f"outcomes={[x.get('status') for x in r]} replay-dup={replay_ok}")

    # 6) optimistic concurrency: 428, one injected 412, recovery, then the always-mode pair
    target = "pay_0000"
    s6, _, _ = _req("PATCH", base + f"/v2/payments/{target}", {"note": "n"})
    _, _, row = _req("GET", base + f"/v2/payments/{target}")
    v1 = row["version"]
    s7, _, _ = _req("PATCH", base + f"/v2/payments/{target}", {"note": "n"},
                    {"If-Match": f'"{v1}"'})
    _, _, row = _req("GET", base + f"/v2/payments/{target}")
    v2 = row["version"]
    s8, _, b8 = _req("PATCH", base + f"/v2/payments/{target}", {"note": "smoke-note"},
                     {"If-Match": f'"{v2}"'})
    report("conflict_dance", s6 == 428 and s7 == 412 and v2 == v1 + 1 and s8 == 200
           and b8.get("note") == "smoke-note" and b8.get("version") == v2 + 1,
           f"428={s6} injected-412={s7} v{v1}->v{v2} retry->{s8} v{b8.get('version')}")
    _req("POST", base + "/admin/conflict", {"mode": "always"})
    _, _, row = _req("GET", base + f"/v2/payments/{target}")
    s9, _, _ = _req("PATCH", base + f"/v2/payments/{target}", {"note": "x"},
                    {"If-Match": f'"{row["version"]}"'})
    _, _, row = _req("GET", base + f"/v2/payments/{target}")
    s10, _, _ = _req("PATCH", base + f"/v2/payments/{target}", {"note": "x"},
                     {"If-Match": f'"{row["version"]}"'})
    _req("POST", base + "/admin/conflict", {"mode": "off"})
    report("second_412_mode", s9 == 412 and s10 == 412, f"always-mode PATCHes -> {s9}, {s10}")

    # 7) webhooks: idempotent registration, scripted deliveries, the counter quad (F3/F13)
    hook_url = f"http://127.0.0.1:{receiver_port}/hook"
    s11, _, reg1 = _req("POST", base + "/v2/webhooks", {"url": hook_url})
    s12, _, reg2 = _req("POST", base + "/v2/webhooks", {"url": hook_url})
    report("register_idempotent", s11 == 200 and s12 == 200 and reg1 == reg2
           and reg1.get("id") and reg1.get("secret") and len(_SmokeReceiver.challenges) == 2,
           f"id={reg1.get('id')} handshakes={len(_SmokeReceiver.challenges)}")
    premature_deliveries = len(_SmokeReceiver.deliveries)
    s13, _, script = _req("POST", base + "/admin/deliver-script", {})
    secret = reg1["secret"]
    quad = {"received": 0, "applied": 0, "ignored": 0, "rejected": 0}
    local: Dict[str, Dict] = {}
    seen: set = set()
    for d in _SmokeReceiver.deliveries:
        quad["received"] += 1
        if not _verify_sig(secret, d["raw"], d["sig"]):
            quad["rejected"] += 1
            continue
        ev = d["body"]
        if ev["id"] in seen:
            quad["ignored"] += 1
            continue
        seen.add(ev["id"])
        data = ev["data"]
        cur = local.get(data["id"], {"version": 1, "note": ""})
        if data["version"] <= cur["version"]:
            quad["ignored"] += 1
            continue
        local[data["id"]] = {"version": data["version"], "note": data["note"]}
        quad["applied"] += 1
    report("deliver_script", s13 == 200 and premature_deliveries == 0
           and script.get("delivered") == EXPECTED_WEBHOOK_COUNTERS["received"]
           and quad == EXPECTED_WEBHOOK_COUNTERS,
           f"quad={quad} want={EXPECTED_WEBHOOK_COUNTERS} premature={premature_deliveries}")
    report("webhook_final_state", local == fx.EXPECTED_WEBHOOK_FINAL
           and _req("GET", base + "/v2/payments/pay_0030")[2].get("version") == 1,
           f"applied-state={local}")

    # 8) stall (F8): the held final page times out at the documented bound; the retry is instant
    _req("POST", base + "/admin/stall", {"on": True, "secs": 1.2})
    final_cursor = _cursor_for_offset(last_offset)
    stall_timed_out = False
    try:
        _req("GET", base + f"/v2/payments?limit={fx.MAX_LIMIT}&cursor={final_cursor}", timeout=0.4)
    except OSError:
        stall_timed_out = True
    t0 = time.time()
    s14, _, b14 = _req("GET", base + f"/v2/payments?limit={fx.MAX_LIMIT}&cursor={final_cursor}",
                       timeout=5)
    retry_ms = (time.time() - t0) * 1000
    report("stall_trap", stall_timed_out and s14 == 200 and retry_ms < 1000
           and b14.get("next_cursor") is None,
           f"first attempt timed out; retry {retry_ms:.0f}ms, last page {len(b14.get('data', []))} rows")

    docs_status = _req("GET", base + "/v2/docs")[0]
    print(f"note  vendor_docs_v2.md: {'served' if docs_status == 200 else 'MISSING (503 until the docs artifact ships)'}")

    failed = [n for n, ok in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} traps PASS"
          + (f" — FAILED: {failed}" if failed else ""))
    print(f"trace: {trace}")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=8788)
    ap.add_argument("--trace", type=Path, default=HERE.parent / "runs/vendor-trace-v2.jsonl")
    ap.add_argument("--stall", action="store_true",
                    help="arm the one-shot final-page stall (F8)")
    ap.add_argument("--smoke", action="store_true",
                    help="serve on 8899 and exercise every trap; PASS/FAIL per trap")
    args = ap.parse_args()
    if args.smoke:
        return smoke()
    serve(args.port, args.trace, stall=args.stall)
    print(f"meridian v2 mock on http://127.0.0.1:{args.port}  trace={args.trace}")
    print(f"  {fixtures_v2.EXPECTED_TOTAL} payments, max limit {MAX_LIMIT}, "
          f"optimum {fixtures_v2.OPTIMAL_REQUESTS} requests; api key {API_KEY}"
          + ("; stall ARMED" if args.stall else ""))
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
