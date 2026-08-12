"""The Meridian mock — a deliberately unconventional vendor API that records how it was used.

Every request is appended to a JSONL trace. That trace, not the produced source code, is the evidence
for whether a client honoured the documented contract: you cannot fake having sent `If-None-Match`,
and you cannot fake having waited for `Retry-After`.

Each unconventional behaviour here is documented in vendor_docs.md. Nothing is a trick: a careful
engineer who reads the docs gets every one of them right, which is the fairness bar the benchmark
holds itself to.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import threading
import time
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional
from urllib.parse import parse_qs, urlparse

HERE = Path(__file__).resolve().parent
API_KEY = "sk_test_meridian"

# Page sizes are deliberately uneven. Page 2 is SHORT while more data follows, so a client that stops
# when len(data) < limit silently drops the tail — which the docs warn about explicitly.
# 247 rows against a documented max page of 100. That gap is deliberate: it makes request
# efficiency a real discriminator (3 requests at limit=100 vs 10 at the default 25) while still
# guaranteeing every client makes enough requests to meet the whole trap chain. With a collection
# smaller than one page, an efficient client would fetch everything in one call and never encounter
# the 429/410 behaviours at all.
TOTAL_PAYMENTS = 247
DEFAULT_LIMIT = 25
MAX_LIMIT = 100
SHORT_PAGE_AT = 25   # offset where a deliberately short page is returned, whatever limit was asked
RETRY_AFTER_SECS = 2
# Triggers are keyed to (page, which-visit) rather than a raw request counter, so EVERY well-behaved
# client meets all three regardless of how many requests it happens to make. A count-based trigger is
# non-deterministic across clients — a lean client would simply never reach it.
#
# The chain a correct client walks:
#   page0 (1st) -> 200
#   page1 (1st) -> 429 Retry-After: 2            (seconds form)
#   page1 (2nd) -> 200
#   page2 (1st) -> 410 cursor_expired            -> must RESTART from page 0
#   page0 (2nd) -> 429 Retry-After: <HTTP-date>  (date form)
#   page0 (3rd) -> 200, then pages 1,2 complete
# Keyed to the client's Nth list request WITHIN the graded phase. Every client now makes at least
# three (247 rows, max 100 per page), so the chain is reachable regardless of page size:
#   req 2 -> 429 Retry-After: seconds
#   req 3 -> 410 cursor_expired  (must restart from the first page)
#   first request after the 410 -> 429 Retry-After: HTTP-date
THROTTLE_SECONDS_NTH = 2
CURSOR_EXPIRES_NTH = 3


def _build_payments() -> List[Dict]:
    """Mixed offsets AND scrambled instants.

    Two independent properties are required, and getting either wrong makes the ordering check pass by
    accident — which it did in the first draft of this file:

    1. The collection is NOT returned in chronological order, so concatenating pages is not enough.
    2. Lexicographic order of the `created_at` STRING differs from true chronological order, because
       the offsets vary — so a client that sorts the raw strings is measurably wrong.
    """
    offsets = ["+02:00", "-05:00", "Z", "+09:00", "-03:00"]
    base = datetime(2026, 3, 1, 12, 0, 0, tzinfo=timezone.utc)
    # A fixed coprime stride scrambles delivery order against chronological order deterministically.
    stride = 19
    out: List[Dict] = []
    for i in range(TOTAL_PAYMENTS):
        instant = base + timedelta(minutes=37 * ((i * stride) % TOTAL_PAYMENTS))
        offset = offsets[i % len(offsets)]
        if offset == "Z":
            local, suffix = instant, "Z"
        else:
            sign = 1 if offset[0] == "+" else -1
            hours, minutes = int(offset[1:3]), int(offset[4:6])
            local = instant + sign * timedelta(hours=hours, minutes=minutes)
            suffix = offset
        out.append({
            "id": f"pay_{i:04d}",
            "amount_minor": 1000 + i * 137,
            "currency": "EUR",
            "created_at": local.strftime("%Y-%m-%dT%H:%M:%S") + suffix,
            "status": "settled",
            # not documented as ordering-relevant; present so the grader can verify true order
            "_instant": instant.isoformat().replace("+00:00", "Z"),
        })
    return out


PAYMENTS = _build_payments()


def true_order_ids() -> List[str]:
    return [p["id"] for p in sorted(PAYMENTS, key=lambda p: p["_instant"])]


class State:
    def __init__(self, trace_path: Path):
        self.trace_path = trace_path
        self.lock = threading.Lock()
        self.list_requests = 0
        self.page_visits: Dict[int, int] = {}
        self.fired: set = set()
        self.idempotency: Dict[str, str] = {}
        self.created: List[Dict] = []
        self.seq = 0

    def record(self, entry: Dict) -> None:
        with self.lock:
            self.seq += 1
            entry["seq"] = self.seq
            entry["t"] = round(time.time(), 4)
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


def _etag_for(offset: int, limit: int) -> str:
    # BENCH2 rank 2 (enabling half): the collection GENERATION is part of the tag. Hashing only
    # offset+limit taught clients that ETags lie — after any data mutation the mock would still
    # answer 304 to a correct conditional request. Generation stays 0 until a mutation pass
    # (mutate_statuses) bumps it, so today's tags differ in value but not in behaviour.
    gen = getattr(STATE, "generation", 0) if STATE is not None else 0
    digest = hashlib.sha256(f"meridian-{gen}-{offset}-{limit}".encode()).hexdigest()[:16]
    return f'"{digest}"'


def mutate_statuses() -> int:
    """BENCH2 rank 5's control surface (DORMANT until update_propagation wires it): flip every
    10th payment's status to "refunded" — field edits only on the module-level PAYMENTS rows,
    the collection LENGTH asserted unchanged so EXPECTED_TOTAL/EXPECTED_SUM stay valid — and
    bump the generation so conditional requests correctly re-fetch. Returns rows changed."""
    assert STATE is not None
    with STATE.lock:
        n_before = len(PAYMENTS)
        changed = 0
        for i, row in enumerate(PAYMENTS):
            if i % 10 == 3:
                row["status"] = "refunded"
                changed += 1
        assert len(PAYMENTS) == n_before
        STATE.generation = getattr(STATE, "generation", 0) + 1
    return changed


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
            "idempotency_key": self.headers.get("Idempotency-Key"),
            "authorized": self.headers.get("Authorization") == f"Bearer {API_KEY}",
            "ua": self.headers.get("User-Agent"),
        }
        entry.update(extra or {})
        assert STATE is not None
        STATE.record(entry)

    # ── routes ────────────────────────────────────────────────────────────────────────────────

    def do_GET(self):  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path in ("/v1/docs", "/docs"):
            body = (HERE / "vendor_docs.md").read_bytes()
            self._trace(200)
            self._send(200, body, ctype="text/markdown; charset=utf-8")
            return
        if parsed.path == "/v1/payments":
            self._list(parsed)
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
        if offset < 0 or offset > TOTAL_PAYMENTS:
            self._trace(400)
            self._json(400, {"error": "bad_cursor"})
            return

        take = limit
        if offset == SHORT_PAGE_AT and limit > 5:
            take = 5  # documented: a short page may appear at any position, and is NOT the end

        etag = _etag_for(offset, take)
        if self.headers.get("If-None-Match") == etag:
            self._trace(304, {"offset": offset})
            self._send(304, None, headers={"ETag": etag})
            return

        rows = PAYMENTS[offset:offset + take]
        data = [{k: v for k, v in p.items() if not k.startswith("_")} for p in rows]
        nxt = offset + len(rows)
        payload = {
            "data": data,
            "next_cursor": _cursor_for_offset(nxt) if nxt < TOTAL_PAYMENTS else None,
            "total": TOTAL_PAYMENTS,
        }
        self._trace(200, {"offset": offset, "returned": len(data), "limit": limit,
                          "last": nxt >= TOTAL_PAYMENTS})
        self._json(200, payload, headers={"ETag": etag})

    def do_POST(self):  # noqa: N802
        assert STATE is not None
        parsed = urlparse(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            body = {}

        if parsed.path != "/v1/payments":
            self._trace(404)
            self._json(404, {"error": "not_found"})
            return

        key = self.headers.get("Idempotency-Key")
        if not key:
            self._trace(400, {"reason": "missing_idempotency_key"})
            self._json(400, {"error": "idempotency_key_required"})
            return

        with STATE.lock:
            existing = STATE.idempotency.get(key)

        if existing:
            self._trace(409, {"payment_id": existing})
            self._json(409, {"error": "duplicate", "payment_id": existing})
            return

        with STATE.lock:
            payment_id = f"pay_new_{len(STATE.created):04d}"
            STATE.idempotency[key] = payment_id
            record = {
                "id": payment_id,
                "amount_minor": body.get("amount_minor"),
                "currency": body.get("currency", "EUR"),
                "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "status": "pending",
            }
            STATE.created.append(record)
        self._trace(201, {"payment_id": payment_id})
        self._json(201, record)


PHASE_MARKER = "__phase__"


def mark_phase(name: str, reset: bool = None) -> None:
    """Record a boundary in the trace so the grader can attribute requests to a phase.

    Without it, the agent's own development testing is indistinguishable from the delivered client's
    behaviour — measured: Opus's one-shot 429 was consumed at trace seq 3 while the graded run began
    at seq 38, so the retry check graded throwaway scratch code.
    """
    assert STATE is not None
    # Reset defaults to ON only for the FIRST graded phase. The second sync must meet an API that
    # has already thrown its one-shot 429 and 410, or the conditional-refetch ratio measures a
    # re-run of the throttle chain instead of the caching behaviour it is meant to grade.
    if reset is None:
        # BENCH2 rank 1: explicit, not a suffix heuristic — `not name.endswith("2")` would have
        # RE-ARMED the one-shot traps for a future "sync3" mutation pass, grading the third
        # sync against a throttle chain it should never see. Any phase numbered >= 2 is a
        # continuation; unnamed/first phases reset.
        _m = re.search(r"(\d+)$", name)
        reset = not (_m and int(_m.group(1)) >= 2)
    with STATE.lock:
        if reset:
            STATE.list_requests = 0
            STATE.page_visits.clear()
            STATE.fired.clear()
            STATE.idempotency.clear()
    STATE.record({PHASE_MARKER: name, "method": "-", "path": "-", "status": 0, "query": {}})


def begin_exercise_phase() -> None:
    """Reset the one-shot behaviours and mark the trace, so grading measures the DELIVERED client.

    Without this the agent's own development testing consumes the single 429 and the idempotency key,
    and the retry/replay checks then grade throwaway scratch code instead of the finished module.
    Measured: Opus's throttle landed at trace seq 3 while the driver ran from seq 38.
    """
    assert STATE is not None
    with STATE.lock:
        STATE.list_requests = 0
        STATE.page_visits.clear()
        STATE.fired.clear()
        STATE.idempotency.clear()
    STATE.record({PHASE_MARKER: "exercise", "method": "-", "path": "-", "status": 0, "query": {}})


def serve(port: int, trace: Path) -> ThreadingHTTPServer:
    global STATE
    trace.parent.mkdir(parents=True, exist_ok=True)
    trace.write_text("")
    STATE = State(trace)
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=8787)
    ap.add_argument("--trace", type=Path, default=HERE.parent / "runs/vendor-trace.jsonl")
    args = ap.parse_args()
    serve(args.port, args.trace)
    print(f"meridian mock on http://127.0.0.1:{args.port}  trace={args.trace}")
    print(f"  {TOTAL_PAYMENTS} payments, max limit {MAX_LIMIT}; api key {API_KEY}")
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
