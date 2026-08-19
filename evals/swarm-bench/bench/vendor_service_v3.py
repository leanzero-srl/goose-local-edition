"""The Meridian v3 mock — sb-7's vendor (DESIGN §2 + §4), recording how it was used.

Same doctrine as v1/v2: every request is appended to a JSONL trace, and the trace — not the
produced source — is the evidence a client honoured the contract. v3 additionally keeps a
COMMIT LEDGER in the same trace (global `commit_seq` lines): every vendor-side state change
is a numbered commit, which is the ground truth the X-tier linearizability battery (L1–L5)
and the money batteries (M1–M4) grade against.

What v3 is, per the sb-7 design:

- ONE seeded collection (fixtures_v3): N = 12,288 payments over 96 Europe/Berlin days with
  one seeded DST transition, currencies EUR/USD/JPY/KWD (exponents 2/2/0/3), all values
  derived from the per-run seed. `serve(port, trace, seed=...)` builds it and its resolved
  schedule (schedule_sb7) — the vendor never invents a number the scorer cannot recompute.
- SERVER-FIXED page size 64 (red-team R3: no `limit` param — a passed one is ignored and
  traced). 12,288/64 = 192 pages, a fixture constant both sides compute. Cursor pagination
  with per-page ETags that embed the collection generation, plus `X-Collection-Generation`
  on every list response.
- The RACING WINDOW (§4.3) as request-order BARRIERS, never wall-clock: on the k-th
  200-served list response of sync #1 with k in `race_pages`, the vendor commits that page's
  scripted mutations, then either delivers the signed webhooks and awaits the app's response
  per delivery BEFORE flushing the page body (order A) or flushes first and holds the app's
  NEXT list response until deliveries complete (order B). The refund transaction group
  (payment flip + reversal, both webhooks carrying `txn`) commits atomically at its own page;
  the out-of-order pair, the byte-identical duplicate, the forged signature, and the
  value-dated mid-walk create ride their seeded race pages. The walk is NOT snapshot-isolated
  for values (mutations to not-yet-served rows are served mutated); the mid-walk create lands
  outside sync #1's captured walk bound, so the walk stays 192 pages and the webhook is
  authoritative — exactly the documented "may or may not appear" contract.
- Seeded FAULT BOUNDARIES owned by the vendor: B1 (connection dropped on the request after
  the j-th 200-serve of sync #1; resume with the same cursor costs exactly one extra
  request), B2 (one 500 + `Retry-After` on the attempt for page j2 of sync #1), B4
  (`arm_boot_refusal` — connections refused for w seconds while the app boots), B5 (the
  armed LYING 304 on the (s-2)-th sync that PRESENTS a stale conditional, s in {3, 4}:
  answered 304 with a disagreeing `X-Collection-Generation`; identical repeats keep being
  lied to, so the only documented exit — ONE unconditional refetch — is observable; keying
  on staleness rather than wall order means the arm can never land on a sync whose
  validator is genuinely current), and the A2 send-window widener (`arm_hold_create`).
  B3/B6/B7/B8 kills are driver-side by design; this module provides their vendor halves
  (`fire_partition_commits`, idempotent creates, durable commit truth).
- The documented WRITE surface: `PATCH /v3/payments/<id>` updates the note under If-Match
  optimistic concurrency (no If-Match -> 428, version mismatch -> 412, match -> version+1
  commit); cursors embed the collection reset-epoch and a cross-reset cursor gets
  `410 cursor_expired` (restart the walk from scratch, as documented); payment rows carry
  `settled_at` (an instant for initially-settled rows, else null).
- Idempotent `POST /v3/payments`: the vendor VALUE-DATES created payments from the seeded
  in-span slot queue (§2.3 — the 3D layout basis provably never moves) and returns the same
  payment for a reused Idempotency-Key (200 replay). A fresh-key retry of identical content
  is the seeded dupe the trap ledger counts.
- The TRAP LEDGER (`trap_results()`): retry_after / drop_resume / stale_refetch /
  delivery_acks / no_fresh_key_dupe in [0,1], graded over every instance in this serve()'s
  trace, mirroring vendor_service_v2's surface for run_build one-line integration
  (serve/mark_phase/trap_results/API_KEY), plus the score_sb7 control surface:
  arm_boot_refusal, quiescent, commit_ledger, commit_versions, set_down,
  fire_partition_commits, arm_hold_create, fire_d1_mutation, fire_burst,
  vendor_payment_count, idempotency_replays.

Docs: GET /v3/docs serves vendor_docs_v3.md when it exists and refuses loudly (503) until
that artifact ships — an agent graded on reading the docs must never see an empty page that
looks like docs (v2 doctrine, carried).
"""

from __future__ import annotations

import argparse
import hashlib
import hmac as hmac_mod
import json
import re
import socket
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from urllib.parse import parse_qs, urlparse

try:
    import fixtures_v3
    import schedule_sb7
except ImportError:                    # imported as a package module (bench.vendor_service_v3)
    from . import fixtures_v3, schedule_sb7

HERE = Path(__file__).resolve().parent
API_KEY = "sk_test_meridian"           # carried from v2 — run_build substitutes {API_KEY}
LIST_PATH = "/v3/payments"             # the one list path — score_sb7's trace splitter reads it
DOCS_PATH = "/v3/docs"
MIDWALK_ID = "pay_midwalk"             # the vendor-created mid-walk payment's frozen id

# One home for every constant: fixtures_v3 / schedule_sb7 (the twin-constant rule). Re-bound
# here only for readability; consumers may import from either name and cannot disagree.
DEFAULT_SEED = fixtures_v3.DEFAULT_SEED
PHASE_MARKER = fixtures_v3.PHASE_MARKER
N_RECORDS = fixtures_v3.N_RECORDS
PAGE_SIZE = fixtures_v3.PAGE_SIZE
PAGE_COUNT = fixtures_v3.PAGE_COUNT
RETRY_AFTER_SECS = fixtures_v3.RETRY_AFTER_SECS
DELIVERY_TIMEOUT_SECS = fixtures_v3.DELIVERY_TIMEOUT_SECS
BURST_COUNT = fixtures_v3.BURST_COUNT


def _now_rfc3339() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _offset_for_cursor(cursor: Optional[str]) -> int:
    if not cursor:
        return 0
    try:
        return int(json.loads(bytes.fromhex(cursor).decode())["o"])
    except Exception:
        return -1


def _epoch_for_cursor(cursor: Optional[str]) -> Optional[int]:
    """The collection reset-epoch the cursor was issued under (None: opaque/legacy shape)."""
    if not cursor:
        return None
    try:
        e = json.loads(bytes.fromhex(cursor).decode()).get("e")
        return int(e) if e is not None else None
    except Exception:
        return None


def _cursor_for_offset(offset: int, epoch: int = 0) -> str:
    return json.dumps({"o": offset, "e": epoch}).encode().hex()


def _etag_for(generation: int, offset: int) -> str:
    # The collection generation is part of the tag (v1's BENCH2 rank-2 lesson): any commit
    # bumps it, so an HONEST 304 never lies about changed data — only the armed B5 one does,
    # and its disagreeing X-Collection-Generation is the documented tell.
    digest = hashlib.sha256(f"meridian3-{generation}-{offset}".encode()).hexdigest()[:16]
    return f'"{digest}"'


def _public(p: Dict) -> Dict:
    return {k: (dict(v) if isinstance(v, dict) else v)
            for k, v in p.items() if not k.startswith("_")}


def _fresh_stale304() -> Dict:
    return {"fired": False, "validator": None, "offset": None, "repeats": 0,
            "fired_t": None, "sync": None}


class State:
    def __init__(self, trace_path: Path, seed: str):
        self.trace_path = trace_path
        self.seed = seed
        self.fx = fixtures_v3.build(seed)          # runs the per-seed assert battery
        self.sch = self.fx.schedule
        self.lock = threading.Lock()
        self.payments: List[Dict] = self.fx.fresh_delivery_order()
        self.index: Dict[str, Dict] = {p["id"]: p for p in self.payments}
        self.reversals: List[Dict] = self.fx.fresh_reversals()
        self.created: List[Dict] = []
        self.idempotency: Dict[str, str] = {}
        self.idempotency_hits: Dict[str, int] = {}
        self.create_ops: List[Dict] = []
        self.generation = 1
        self.versions: set = {(p["id"], 1) for p in self.payments}
        self.commit_seq = 0
        # sync accounting: a new sync begins on a no-cursor list request only when no sync
        # is active; a sync deactivates when its last page is 200-served. This keeps the B5
        # unconditional refetch INSIDE its sync while restarts stay countable.
        self.sync_ordinal = 0
        self.sync_active = False
        self.sync_served = 0                       # 200-serves in the current sync
        self.sync_total: Optional[int] = None      # walk bound captured at sync start
        self.drop_armed = False
        self.stale304 = _fresh_stale304()
        self.stale_syncs: List[int] = []           # sync ordinals that presented a stale
                                                   # conditional (B5 arms on the (s-2)-th)
        self.reset_epoch = 0                       # bumped per _reset_run; cursors embed it
                                                   # and a cross-reset cursor gets 410
        self.fired: set = set()
        self.pending_barrier: Optional[Dict] = None
        self.inflight = 0                          # async delivery jobs in flight
        self.sync1_walk_done = False
        self.quiesce_signaled = False
        self.webhook: Optional[Dict] = None        # {"url","id","secret"}
        self.sent_events: Dict[str, Dict] = {}     # event_id -> {"raw","sig"} for dupes
        self.deliveries: List[Dict] = []           # every outbound delivery attempt
        self.value_slot_next = 1                   # slot 0 is the mid-walk create (§2.3)
        self.hold_create_secs = 0.0
        self.refuse_until = 0.0
        self.down = False
        self.seq = 0
        self.events: List[Dict] = []               # in-memory trace mirror (trap_results)
        # the scheduled delivery script, grouped by trigger page; page None = partition
        script = fixtures_v3.scheduled_delivery_script(self.fx)
        self.by_page: Dict[int, List[Dict]] = {}
        self.partition_script: List[Dict] = []
        for e in script:
            ev = dict(e)
            if ev["type"] == "payment.created":
                ev["row_id"] = MIDWALK_ID
            if ev.get("page") is None:
                self.partition_script.append(ev)
            else:
                self.by_page.setdefault(ev["page"], []).append(ev)

    def record(self, entry: Dict) -> None:
        with self.lock:
            self.seq += 1
            entry["seq"] = self.seq
            entry["t"] = round(time.time(), 4)
            self.events.append(entry)
            with self.trace_path.open("a") as fh:
                fh.write(json.dumps(entry) + "\n")


STATE: Optional[State] = None


def _base_rec(extra: Dict) -> Dict:
    return {"method": "-", "path": "-", "status": 0, "query": {}, **extra}


# ── commits (caller holds the lock; trace writes happen after release) ───────────────────────

def _commit_script_event(st: State, ev: Dict) -> Dict:
    """Apply one scheduled script event to vendor state and return its commit-ledger entry.
    Absolute snapshots come from fixtures' one derivation — never recomputed here."""
    st.commit_seq += 1
    n = st.commit_seq
    if ev["type"] == "payment.created":
        c = ev["create"]
        slot = st.fx.value_slots[c["value_slot"]]
        row = {"id": ev.get("row_id", MIDWALK_ID), "amount_minor": c["amount_minor"],
               "currency": c["currency"], "created_at": c["created_at"],
               "settled_at": None,
               "status": "pending", "version": 1, "note": c["note"],
               "counterparty": dict(c["counterparty"]),
               "_instant": slot["_instant"], "_day": c["day"]}
        st.payments.append(row)
        st.index[row["id"]] = row
        st.versions.add((row["id"], 1))
        st.generation += 1
        return {"commit": "payment.created", "commit_seq": n, "payment_id": row["id"],
                "version": 1, "payment_status": "pending", "value_dated": c["created_at"],
                "sched": ev.get("sched")}
    if ev["type"] == "reversal.created":
        rev = {"id": st.sch.refund_txn["reversal_id"], "payment_id": ev["payment_id"],
               "amount_minor": ev["reversal"]["amount_minor"],
               "currency": ev["reversal"]["currency"], "created_at": _now_rfc3339()}
        st.reversals.append(rev)
        st.generation += 1
        return {"commit": "reversal.created", "commit_seq": n,
                "payment_id": ev["payment_id"], "reversal_id": rev["id"],
                "txn": ev.get("txn"), "sched": ev.get("sched")}
    row = st.index[ev["payment_id"]]
    row["version"], row["status"], row["note"] = ev["version"], ev["status"], ev["note"]
    st.versions.add((ev["payment_id"], ev["version"]))
    st.generation += 1
    return {"commit": "payment.updated", "commit_seq": n, "payment_id": ev["payment_id"],
            "version": ev["version"], "payment_status": ev["status"],
            "txn": ev.get("txn"), "sched": ev.get("sched")}


def _commit_flip(st: State, payment_id: str, to_status: str, sched: str) -> Tuple[int, int, str]:
    """Post-schedule scripted flip (D1, burst): version += 1, never touching 'refunded'."""
    with st.lock:
        row = st.index[payment_id]
        row["version"] += 1
        row["status"] = to_status
        version, note = row["version"], row["note"]
        st.versions.add((payment_id, version))
        st.commit_seq += 1
        n = st.commit_seq
        st.generation += 1
    st.record(_base_rec({"commit": "payment.updated", "commit_seq": n,
                         "payment_id": payment_id, "version": version,
                         "payment_status": to_status, "sched": sched}))
    return n, version, note


# ── webhook deliveries ───────────────────────────────────────────────────────────────────────

def _build_delivery(st: State, ev: Dict) -> Tuple[bytes, str]:
    """Serialize + sign one event. A 'duplicate' reuses the recorded bytes (byte-identical
    redelivery); a 'forged' one is signed with the wrong secret and claims a mutation the
    vendor never committed."""
    if ev["kind"] == "duplicate":
        sent = st.sent_events[ev["event_id"]]
        return sent["raw"], sent["sig"]
    with st.lock:
        secret = st.webhook["secret"] if st.webhook else ""
        if ev["type"] == "payment.created":
            data = _public(st.index[ev.get("row_id", MIDWALK_ID)])
        elif ev["type"] == "reversal.created":
            rid = st.sch.refund_txn["reversal_id"]
            data = dict(next(r for r in st.reversals if r["id"] == rid))
        else:
            row = st.index[ev["payment_id"]]
            data = {"id": row["id"], "amount_minor": row["amount_minor"],
                    "currency": row["currency"], "created_at": row["created_at"],
                    "status": ev["status"], "version": ev["version"], "note": ev["note"],
                    "counterparty": dict(row["counterparty"])}
    event = {"id": ev["event_id"], "type": ev["type"], "created_at": _now_rfc3339(),
             "txn": ev.get("txn"), "data": data}
    raw = json.dumps(event).encode()
    t = int(time.time())
    signing = "whsec_not_the_secret" if ev["kind"] == "forged" else secret
    sig = hmac_mod.new(signing.encode(), f"{t}.".encode() + raw, hashlib.sha256).hexdigest()
    sig_header = f"t={t},v1={sig}"
    if ev["kind"] != "forged":
        with st.lock:
            st.sent_events[ev["event_id"]] = {"raw": raw, "sig": sig_header}
    return raw, sig_header


def _deliver(st: State, ev: Dict) -> Dict:
    """POST one signed event to the registered webhook, awaiting the app's response (§4.3:
    attempts time out at DELIVERY_TIMEOUT_SECS -> sched-partial logged, never a hang)."""
    sched = ev.get("sched")
    with st.lock:
        hook = dict(st.webhook) if st.webhook else None
    if hook is None:
        out = {"event_id": ev.get("event_id"), "kind": ev.get("kind"), "status": None,
               "error": "no_webhook_registered", "ms": 0.0, "sched": sched}
        with st.lock:
            st.deliveries.append(out)
        st.record(_base_rec({"sched-partial": sched, "event": ev.get("event_id"),
                             "reason": "no_webhook_registered"}))
        return out
    raw, sig = _build_delivery(st, ev)
    status, err = None, None
    t0 = time.time()
    try:
        req = urllib.request.Request(hook["url"], data=raw, method="POST",
                                     headers={"Content-Type": "application/json",
                                              "Meridian-Signature": sig})
        with urllib.request.urlopen(req, timeout=DELIVERY_TIMEOUT_SECS) as resp:
            status = resp.status
            resp.read()
    except urllib.error.HTTPError as exc:
        status = exc.code
        exc.read()
    except (urllib.error.URLError, OSError) as exc:
        err = type(exc).__name__
    ms = round((time.time() - t0) * 1000, 1)
    out = {"event_id": ev.get("event_id"), "kind": ev.get("kind"), "status": status,
           "error": err, "ms": ms, "sched": sched}
    with st.lock:
        st.deliveries.append(out)
    st.record({"webhook_delivery": ev.get("event_id"), "kind": ev.get("kind"),
               "sched": sched, "page": ev.get("page"), "delivery_status": status,
               "delivery_error": err, "ms": ms, "method": "POST(out)",
               "path": hook["url"], "status": status or 0, "query": {}})
    # A correct app 401s the forged event — that is its expected outcome, not a partial.
    if err is not None or (ev.get("kind") != "forged"
                           and not (status and 200 <= status < 300)):
        st.record(_base_rec({"sched-partial": sched, "event": ev.get("event_id"),
                             "delivery_status": status, "delivery_error": err}))
    return out


def _spawn_barrier_batch(st: State, events: List[Dict]) -> None:
    """Order B: the page body already flushed; deliveries run now and the app's NEXT list
    response is held until they complete (the request handler waits on this event)."""
    barrier = threading.Event()
    cap = len(events) * DELIVERY_TIMEOUT_SECS + 5
    with st.lock:
        st.pending_barrier = {"event": barrier, "cap": cap}
        st.inflight += 1

    def _job():
        try:
            for ev in events:
                _deliver(st, ev)
        finally:
            barrier.set()
            with st.lock:
                st.inflight -= 1
            _maybe_quiesce(st)

    threading.Thread(target=_job, daemon=True).start()


def _spawn_app_create_delivery(st: State, row_id: str, event_id: str) -> None:
    with st.lock:
        st.inflight += 1

    def _job():
        try:
            _deliver(st, {"event_id": event_id, "kind": "apply", "type": "payment.created",
                          "payment_id": row_id, "row_id": row_id, "sched": "app_create"})
        finally:
            with st.lock:
                st.inflight -= 1
            _maybe_quiesce(st)

    threading.Thread(target=_job, daemon=True).start()


def _maybe_quiesce(st: State) -> None:
    """§4.3: quiescence is vendor-signaled in the trace — sync #1 walk complete and every
    delivery resolved. Signaled once."""
    with st.lock:
        pb = st.pending_barrier
        ready = (st.sync1_walk_done and st.inflight == 0
                 and (pb is None or pb["event"].is_set()) and not st.quiesce_signaled)
        if ready:
            st.quiesce_signaled = True
    if ready:
        st.record(_base_rec({"sched": "quiesce"}))


# ── HTTP handler ─────────────────────────────────────────────────────────────────────────────

class _Server(ThreadingHTTPServer):
    daemon_threads = True


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):  # silence stderr spam
        return

    # ── plumbing ──────────────────────────────────────────────────────────────────────────────

    def _send(self, code: int, body: Optional[bytes] = None,
              headers: Optional[Dict[str, str]] = None,
              ctype: str = "application/json") -> None:
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

    def _json(self, code: int, payload: Dict,
              headers: Optional[Dict[str, str]] = None) -> None:
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

    def _abort(self) -> None:
        """Close the connection without writing a byte — the B1 drop / B4 refusal / down."""
        self.close_connection = True
        try:
            self.connection.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

    def _gated(self) -> bool:
        assert STATE is not None
        with STATE.lock:
            down = STATE.down
            refusing = time.time() < STATE.refuse_until
        if down or refusing:
            STATE.record({"method": self.command, "path": urlparse(self.path).path,
                          "status": 0, "query": {}, "refused": True, "down": down})
            self._abort()
            return False
        return True

    # ── routes ────────────────────────────────────────────────────────────────────────────────

    def do_GET(self):  # noqa: N802
        assert STATE is not None
        parsed = urlparse(self.path)
        if parsed.path == "/admin/status":
            self._admin_status()
            return
        if not self._gated():
            return
        if parsed.path in (DOCS_PATH, "/docs"):
            docs = HERE / "vendor_docs_v3.md"
            if docs.exists():
                body = docs.read_bytes()
                self._trace(200)
                self._send(200, body, ctype="text/markdown; charset=utf-8")
            else:
                # Refuse loudly, never improvise: an agent graded on reading the docs must
                # not be served an empty page that looks like docs.
                self._trace(503, {"reason": "vendor_docs_v3.md missing"})
                self._json(503, {"error": "docs_missing"})
            return
        if parsed.path == LIST_PATH:
            self._list(parsed)
            return
        if parsed.path == "/v3/reversals":
            with STATE.lock:
                rows = [dict(r) for r in STATE.reversals]
            self._trace(200, {"returned": len(rows)})
            self._json(200, {"data": rows, "total": len(rows)})
            return
        m = re.fullmatch(r"/v3/payments/([A-Za-z0-9_]+)", parsed.path)
        if m:
            with STATE.lock:
                row = STATE.index.get(m.group(1))
                payload = _public(row) if row is not None else None
            if payload is None:
                self._trace(404)
                self._json(404, {"error": "not_found"})
                return
            self._trace(200, {"payment_id": payload["id"], "version": payload["version"]})
            self._json(200, payload)
            return
        self._trace(404)
        self._json(404, {"error": "not_found"})

    def do_POST(self):  # noqa: N802
        assert STATE is not None
        parsed = urlparse(self.path)
        if parsed.path.startswith("/admin/"):
            self._admin(parsed.path, self._body())
            return
        if not self._gated():
            return
        if parsed.path == "/v3/payments":
            self._create(self._body())
            return
        if parsed.path == "/v3/webhooks":
            self._register_webhook(self._body())
            return
        self._trace(404)
        self._json(404, {"error": "not_found"})

    def do_PATCH(self):  # noqa: N802
        """Note update with the documented If-Match dance (v2 lineage): no If-Match -> 428,
        version mismatch -> 412, match -> note applied, version += 1, commit-ledger line."""
        assert STATE is not None
        st = STATE
        parsed = urlparse(self.path)
        if not self._gated():
            return
        m = re.fullmatch(r"/v3/payments/([A-Za-z0-9_]+)", parsed.path)
        if not m:
            self._trace(404)
            self._json(404, {"error": "not_found"})
            return
        with st.lock:
            row = st.index.get(m.group(1))
        if row is None:
            self._trace(404)
            self._json(404, {"error": "not_found"})
            return
        raw_if_match = self.headers.get("If-Match")
        body = self._body()
        if raw_if_match is None:
            # 428 is a BUG in the client — the docs say every write carries If-Match.
            self._trace(428, {"payment_id": row["id"], "current_version": row["version"]})
            self._json(428, {"error": "precondition_required"})
            return
        wanted = re.sub(r'^(W/)?"?|"?$', "", raw_if_match.strip())
        if not wanted.isdigit():
            self._trace(400, {"reason": "bad_if_match", "payment_id": row["id"]})
            self._json(400, {"error": "bad_if_match"})
            return
        note = body.get("note")
        if not isinstance(note, str) or not (1 <= len(note) <= 280):
            self._trace(400, {"reason": "invalid_note", "payment_id": row["id"]})
            self._json(400, {"error": "invalid_note"})
            return
        with st.lock:
            if int(wanted) != row["version"]:
                current = row["version"]
                conflict = True
            else:
                conflict = False
                row["version"] += 1
                row["note"] = note
                st.versions.add((row["id"], row["version"]))
                st.commit_seq += 1
                n = st.commit_seq
                st.generation += 1
                payload = _public(row)
                version = row["version"]
        if conflict:
            self._trace(412, {"payment_id": row["id"], "current_version": current})
            self._json(412, {"error": "version_conflict"})
            return
        st.record(_base_rec({"commit": "payment.updated", "commit_seq": n,
                             "payment_id": row["id"], "version": version,
                             "payment_status": row["status"], "sched": "note_write"}))
        self._trace(200, {"payment_id": row["id"], "version": version,
                          "applied_fields": ["note"]})
        self._json(200, payload)

    # ── the list walk: fixed pages, seeded faults, the racing window ─────────────────────────

    def _list(self, parsed) -> None:
        st = STATE
        params = parse_qs(parsed.query)
        cursor = (params.get("cursor") or [None])[0]
        limit_param = (params.get("limit") or [None])[0]   # v3 has no limit param (R3)
        inm = self.headers.get("If-None-Match")

        # order-B barrier: the app's NEXT list response waits for the delivery batch
        with st.lock:
            pb = st.pending_barrier
        if pb is not None:
            pb["event"].wait(pb["cap"])
            with st.lock:
                if st.pending_barrier is pb:
                    st.pending_barrier = None

        with st.lock:
            if not st.sync_active:
                st.sync_ordinal += 1
                st.sync_active = True
                st.sync_served = 0
                st.sync_total = len(st.payments)
                sync_started = True
            else:
                sync_started = False
            sync_no = st.sync_ordinal
            total = st.sync_total
            offset = 0 if cursor is None else _offset_for_cursor(cursor)
            will_drop = st.drop_armed
            if will_drop:
                st.drop_armed = False
                st.fired.add("drop_fired")
            will_500 = (not will_drop and sync_no == 1 and "http500" not in st.fired
                        and st.sync_served == st.sch.http500_page - 1)
            if will_500:
                st.fired.add("http500")
        if sync_started:
            st.record(_base_rec({"sync_start": sync_no, "walk_total": total}))

        if will_drop:
            self._trace(0, {"dropped": True, "offset": offset, "sync": sync_no,
                            "sched": "drop_after_page"})
            self._abort()
            return
        if will_500:
            self._trace(500, {"offset": offset, "sync": sync_no,
                              "retry_after": RETRY_AFTER_SECS, "sched": "http500_page"})
            self._json(500, {"error": "internal_error"},
                       headers={"Retry-After": str(RETRY_AFTER_SECS)})
            return
        epoch = _epoch_for_cursor(cursor)
        with st.lock:
            expired = (cursor is not None and epoch is not None
                       and epoch != st.reset_epoch)
        if expired:
            # documented: a cursor issued before a collection rebuild has expired —
            # restart the walk from scratch (410 cursor_expired)
            self._trace(410, {"reason": "cursor_expired", "sync": sync_no,
                              "cursor_epoch": epoch})
            self._json(410, {"error": "cursor_expired"})
            return
        if offset < 0 or offset > total:
            self._trace(400, {"sync": sync_no})
            self._json(400, {"error": "bad_cursor"})
            return

        with st.lock:
            gen = st.generation
        etag = _etag_for(gen, offset)

        if inm is not None:
            # B5: the first stale conditional of the (s-2)-th sync that PRESENTS a stale
            # validator gets the lying 304 whose X-Collection-Generation disagrees with what
            # the client stored (s in {3, 4} maps to the 1st/2nd stale-presenting sync —
            # staleness, not wall order, arms it, so the arm never lands on a sync whose
            # validator is genuinely current); identical repeats keep being lied to — the
            # only documented exit is ONE unconditional refetch. A validator that genuinely
            # matches gets an honest 304.
            with st.lock:
                s3 = st.stale304
                stale_cond = inm != etag
                if stale_cond and sync_no not in st.stale_syncs:
                    st.stale_syncs.append(sync_no)
                want_nth = max(1, st.sch.stale_304_sync - 2)
                arm = (stale_cond and not s3["fired"]
                       and len(st.stale_syncs) == want_nth
                       and sync_no == st.stale_syncs[-1])
                if arm:
                    s3.update({"fired": True, "validator": inm, "offset": offset,
                               "fired_t": time.time(), "sync": sync_no})
                lying_repeat = (s3["fired"] and not arm and sync_no == s3["sync"]
                                and inm == s3["validator"] and offset == s3["offset"])
                if lying_repeat:
                    s3["repeats"] += 1
                repeats = s3["repeats"]
            if arm or lying_repeat:
                self._trace(304, {"offset": offset, "sync": sync_no, "lying": True,
                                  "generation": gen, "repeats": repeats,
                                  **({"sched": "stale_304_sync"} if arm else {})})
                self._send(304, None, headers={"ETag": inm,
                                               "X-Collection-Generation": str(gen)})
                return
            if inm == etag:
                self._trace(304, {"offset": offset, "sync": sync_no, "generation": gen})
                self._send(304, None, headers={"ETag": etag,
                                               "X-Collection-Generation": str(gen)})
                return

        # This response will be the sync's nth 200-serve — the §4.3 keying quantity
        # (retries, 304s and dropped attempts never counted).
        with st.lock:
            nth = st.sync_served + 1
            trigger = (sync_no == 1 and nth in st.by_page
                       and f"page:{nth}" not in st.fired)
            events: List[Dict] = []
            commit_entries: List[Dict] = []
            order = ""
            if trigger:
                st.fired.add(f"page:{nth}")
                events = st.by_page[nth]
                order = st.sch.race_order.get(nth, "B")
                # Commit in true per-payment version order (the ooo pair's low bump precedes
                # its high one), atomically for the whole page batch — the refund pair's L5
                # atomicity is this single lock hold.
                commit_batch = sorted(
                    [e for e in events if e["kind"] in ("apply", "stale")],
                    key=lambda e: (str(e.get("payment_id")), e.get("version") or 0))
                commit_entries = [_commit_script_event(st, e) for e in commit_batch]
        if trigger:
            st.record(_base_rec({"sched": sorted({e.get("sched") for e in events
                                                  if e.get("sched")}),
                                 "page": nth, "order": order, "sync": sync_no,
                                 "events": [e["event_id"] for e in events]}))
            for entry in commit_entries:
                st.record(_base_rec(entry))
            if order == "A":
                # webhook-first: deliver and await the app's response per delivery,
                # THEN flush the page body
                for ev in events:
                    _deliver(st, ev)

        with st.lock:
            end = min(offset + PAGE_SIZE, total)
            rows = [_public(p) for p in st.payments[offset:end]]
            nxt = _cursor_for_offset(end, st.reset_epoch) if end < total else None
            st.sync_served += 1
            served_nth = st.sync_served
            if (sync_no == 1 and served_nth == st.sch.drop_after_page
                    and "drop_fired" not in st.fired):
                st.drop_armed = True
            last = nxt is None
            if last:
                st.sync_active = False
                if sync_no == 1:
                    st.sync1_walk_done = True
            gen_now = st.generation
        etag_now = _etag_for(gen_now, offset)
        self._trace(200, {"offset": offset, "returned": len(rows), "page": served_nth,
                          "sync": sync_no, "last": last, "generation": gen_now,
                          **({"limit_param_ignored": limit_param} if limit_param else {}),
                          **({"race_page": True, "order": order} if trigger else {})})
        self._json(200, {"data": rows, "next_cursor": nxt, "total": total},
                   headers={"ETag": etag_now, "X-Collection-Generation": str(gen_now)})
        if trigger and order == "B":
            _spawn_barrier_batch(st, events)
        if last:
            _maybe_quiesce(st)

    # ── creates: idempotent, value-dated by the vendor ───────────────────────────────────────

    def _validate_create(self, body: Dict) -> Optional[str]:
        amount = body.get("amount_minor")
        if not isinstance(amount, int) or isinstance(amount, bool) or amount <= 0:
            return "invalid_amount"
        if body.get("currency") not in fixtures_v3.CURRENCY_EXPONENTS:
            return "unsupported_currency"
        cp = body.get("counterparty") if isinstance(body.get("counterparty"), dict) else {}
        if not cp.get("name") or not re.fullmatch(r"[A-Z]{2}", str(cp.get("country") or "")):
            return "invalid_counterparty"
        note = body.get("note")
        if note is not None and not isinstance(note, str):
            return "invalid_note"
        return None

    def _create(self, body: Dict) -> None:
        st = STATE
        key = self.headers.get("Idempotency-Key")
        if not key:
            self._trace(400, {"reason": "missing_idempotency_key"})
            self._json(400, {"error": "idempotency_key_required"})
            return
        err = self._validate_create(body)
        if err:
            _log_create_op(st, key, body, f"error:{err}")
            self._trace(400, {"reason": err})
            self._json(400, {"error": err})
            return
        with st.lock:
            existing = st.idempotency.get(key)
            if existing:
                st.idempotency_hits[key] = st.idempotency_hits.get(key, 0) + 1
                payload = _public(st.index[existing])
        if existing:
            # v3-documented: the vendor returns the SAME payment for a reused key — the
            # replay is the success path a crashed sender retries into (A2/B8).
            _log_create_op(st, key, body, "replayed")
            self._trace(200, {"payment_id": existing, "replayed": True})
            self._json(200, payload)
            return
        with st.lock:
            hold = 0.0
            if st.hold_create_secs and "hold_create_fired" not in st.fired:
                st.fired.add("hold_create_fired")
                hold = st.hold_create_secs
            # §2.3 value-dating: seeded in-span slots with R0-2 headroom; slot 0 belongs to
            # the mid-walk create. Rule-breaking extra creates (a fresh-key dupe storm) get
            # deterministic shifted instants off the last slot — still in-span.
            slot_i = min(st.value_slot_next, len(st.fx.value_slots) - 1)
            overflow = max(0, st.value_slot_next - (len(st.fx.value_slots) - 1))
            slot = st.fx.value_slots[slot_i]
            created_at, instant = slot["created_at"], slot["_instant"]
            if overflow:
                base = datetime.fromisoformat(instant.replace("Z", "+00:00"))
                instant = (base + timedelta(seconds=overflow)).isoformat() \
                    .replace("+00:00", "Z")
                created_at = instant
            st.value_slot_next += 1
            row = {"id": f"pay_new_{len(st.created):04d}",
                   "amount_minor": body["amount_minor"], "currency": body["currency"],
                   "created_at": created_at, "settled_at": None,
                   "status": "pending", "version": 1,
                   "note": str(body.get("note") or ""),
                   "counterparty": {"name": body["counterparty"]["name"],
                                    "country": body["counterparty"]["country"]},
                   "_instant": instant, "_day": slot["day"]}
            st.created.append(row)
            st.payments.append(row)
            st.index[row["id"]] = row
            st.idempotency[key] = row["id"]
            st.versions.add((row["id"], 1))
            st.commit_seq += 1
            n = st.commit_seq
            st.generation += 1
            event_id = f"evt_app_{len(st.created):04d}"
            payload = _public(row)
        _log_create_op(st, key, body, "created")
        st.record(_base_rec({"commit": "payment.created", "commit_seq": n,
                             "payment_id": row["id"], "version": 1,
                             "payment_status": "pending", "value_dated": created_at,
                             "sched": "app_create"}))
        if hold:
            # A2: widen the send window — the payment IS committed; the held response is
            # what the driver kills the app inside, and the same-key retry replays into it.
            st.record(_base_rec({"hold_create_secs": hold, "payment_id": row["id"]}))
            time.sleep(hold)
        self._trace(201, {"payment_id": row["id"], "value_dated": created_at})
        self._json(201, payload)
        _spawn_app_create_delivery(st, row["id"], event_id)

    # ── webhooks ──────────────────────────────────────────────────────────────────────────────

    def _register_webhook(self, body: Dict) -> None:
        url = body.get("url")
        if not isinstance(url, str) or not url.startswith("http"):
            self._trace(400, {"reason": "bad_url"})
            self._json(400, {"error": "bad_url"})
            return
        ident = _webhook_identity(url)
        # The unsigned challenge handshake runs on EVERY registration call (idempotent by
        # URL; the server must already be listening — including after a restart). F13
        # lineage: it counts nothing.
        challenge = ident["challenge"]
        outcome = "error"
        try:
            req = urllib.request.Request(
                url, data=json.dumps({"type": "webhook.verify",
                                      "challenge": challenge}).encode(),
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

    # ── admin (localhost-only grader plumbing, undocumented in vendor docs) ──────────────────

    def _admin_status(self) -> None:
        if not self._is_local_admin():
            self._json(403, {"error": "forbidden"})
            return
        with STATE.lock:
            payments = len(STATE.payments)
            generation = STATE.generation
            sync = STATE.sync_ordinal
        self._json(200, {"schedule": schedule_status(), "quiescent": quiescent(),
                         "payments": payments, "generation": generation,
                         "sync_ordinal": sync, "fixture_seed": STATE.seed})

    def _admin(self, path: str, body: Dict) -> None:
        if not self._is_local_admin():
            self._trace(403, {"admin": path})
            self._json(403, {"error": "forbidden"})
            return
        if path == "/admin/phase":
            name = str(body.get("name") or "phase")
            mark_phase(name)
            self._trace(200, {"admin": path, "name": name})
            self._json(200, {"ok": True})
            return
        if path == "/admin/partition-commits":
            result = fire_partition_commits()
            self._trace(200, {"admin": path, **{k: v for k, v in result.items()
                                                if k != "results"}})
            self._json(200, result)
            return
        if path == "/admin/d1-mutation":
            result = fire_d1_mutation()
            self._trace(200, {"admin": path, **result})
            self._json(200, result)
            return
        if path == "/admin/burst":
            result = fire_burst()
            self._trace(200, {"admin": path, **result})
            self._json(200, result)
            return
        if path == "/admin/hold-create":
            secs = arm_hold_create(float(body.get("secs") or 6.0))
            self._trace(200, {"admin": path, "secs": secs})
            self._json(200, {"ok": True, "secs": secs})
            return
        if path == "/admin/boot-refusal":
            secs = arm_boot_refusal(body.get("secs"))
            self._trace(200, {"admin": path, "secs": secs})
            self._json(200, {"ok": True, "secs": secs})
            return
        if path == "/admin/down":
            set_down(bool(body.get("on")))
            self._trace(200, {"admin": path, "on": bool(body.get("on"))})
            self._json(200, {"ok": True, "down": STATE.down})
            return
        self._trace(404, {"admin": path})
        self._json(404, {"error": "not_found"})


def _webhook_identity(url: str) -> Dict:
    """Registration is idempotent BY CONSTRUCTION: id, secret, and challenge are pure
    functions of the URL, so re-registering after an app restart returns the same pair."""
    sha = hashlib.sha256(("meridian-wh3-" + url).encode()).hexdigest()
    return {"url": url, "id": "wh_" + sha[:12], "secret": "whsec_" + sha[12:44],
            "challenge": hashlib.sha256(("challenge-" + sha).encode()).hexdigest()[:32]}


def _item_chash(body: Dict) -> str:
    """Content hash of a create MINUS its idempotency key — the fingerprint that catches a
    fresh-key retry of the same payment (r_no_dupe_effect's seeded dupe)."""
    cp = body.get("counterparty") if isinstance(body.get("counterparty"), dict) else {}
    blob = json.dumps([body.get("amount_minor"), body.get("currency"), cp.get("name"),
                       cp.get("country"), body.get("note")], sort_keys=True)
    return hashlib.sha256(blob.encode()).hexdigest()[:16]


def _log_create_op(st: State, key: Optional[str], body: Dict, outcome: str) -> None:
    with st.lock:
        st.create_ops.append({"key": key, "chash": _item_chash(body), "outcome": outcome,
                              "t": time.time()})


# ── score_sb7 control surface (module functions; /admin mirrors where the driver needs HTTP) ─

def arm_boot_refusal(secs: Optional[float] = None) -> float:
    """B4: refuse every connection for the next `secs` seconds (default: the schedule's
    seeded vendor_down_boot_secs) — armed by the driver just before it boots the app."""
    st = STATE
    w = float(secs if secs is not None else st.sch.vendor_down_boot_secs)
    with st.lock:
        st.refuse_until = time.time() + w
        st.fired.add("boot_refusal")
    st.record(_base_rec({"sched": "vendor_down_boot_secs", "secs": w}))
    return w


def set_down(on: bool) -> None:
    """Error-journey lever: while down, every non-admin connection is severed unanswered."""
    st = STATE
    with st.lock:
        st.down = bool(on)
    st.record(_base_rec({"vendor_down": bool(on)}))


def arm_hold_create(secs: float = 6.0) -> float:
    """A2: the next first-time create commits, then its response is held `secs` seconds —
    the widened send window the driver SIGKILLs the app inside. One-shot."""
    st = STATE
    with st.lock:
        st.hold_create_secs = float(secs)
        st.fired.discard("hold_create_fired")
    st.record(_base_rec({"sched": "hold_create_armed", "secs": float(secs)}))
    return float(secs)


def fire_partition_commits() -> Dict:
    """B7's vendor half: commit the 5 seeded vendor flips (of the frozen K = 8; the other 3
    are the F3 workflow's outbox-crossing events, driven against the app) and deliver their
    webhooks, awaiting the app's response per delivery. Fires once."""
    st = STATE
    with st.lock:
        already = "partition" in st.fired
        if not already:
            st.fired.add("partition")
            entries = [_commit_script_event(st, e) for e in st.partition_script]
    if already:
        return {"already_fired": True}
    st.record(_base_rec({"sched": "partition_commits", "count": len(st.partition_script)}))
    for entry in entries:
        st.record(_base_rec(entry))
    results = [_deliver(st, ev) for ev in st.partition_script]
    acked = sum(1 for r in results if r["status"] and 200 <= r["status"] < 300)
    return {"commits": len(entries), "delivered": acked, "results": results}


def fire_d1_mutation() -> Dict:
    """D1 corner: mutate the seeded brushed-record target (fixtures' d1_target — disjoint
    from every schedule target) and deliver its webhook; the probe verifies documented ==
    observed brush behavior across it."""
    st = STATE
    t = st.fx.d1_target
    _, version, note = _commit_flip(st, t["payment_id"], t["to_status"], "d1_mutation")
    with st.lock:
        st.fired.add("d1")
    ev = {"event_id": f"evt_d1_{version:04d}", "kind": "apply", "type": "payment.updated",
          "payment_id": t["payment_id"], "version": version, "status": t["to_status"],
          "note": note, "sched": "d1_mutation"}
    delivery = _deliver(st, ev)
    return {"payment_id": t["payment_id"], "version": version,
            "to_status": t["to_status"], "delivery": delivery}


def fire_burst() -> Dict:
    """P tier's under-stream load: commit + deliver the frozen-count seeded burst
    (BURST_COUNT status flips, disjoint targets), returning the wall window."""
    st = STATE
    t0 = time.time()
    results = []
    for i, t in enumerate(st.fx.burst_targets):
        _, version, note = _commit_flip(st, t["payment_id"], t["to_status"], "burst")
        ev = {"event_id": f"evt_burst_{i:04d}", "kind": "apply",
              "type": "payment.updated", "payment_id": t["payment_id"],
              "version": version, "status": t["to_status"], "note": note,
              "sched": "burst"}
        results.append(_deliver(st, ev))
    t1 = time.time()
    with st.lock:
        st.fired.add("burst")
    acked = sum(1 for r in results if r["status"] and 200 <= r["status"] < 300)
    return {"t0": round(t0, 4), "t1": round(t1, 4), "count": len(results), "acked": acked}


def quiescent() -> bool:
    st = STATE
    with st.lock:
        pb = st.pending_barrier
        ok = (st.sync1_walk_done and st.inflight == 0
              and (pb is None or pb["event"].is_set()))
    if ok:
        _maybe_quiesce(st)
    return ok


def commit_ledger() -> Dict[str, Dict]:
    """Current committed state of every payment — L4/M3's vendor ground truth. Carries
    created_at + the value-dated Berlin day so the scorer can place vendor-created payments
    in the §3.1 load-order model without re-deriving slots."""
    with STATE.lock:
        return {p["id"]: {"version": p["version"], "status": p["status"],
                          "amount_minor": p["amount_minor"], "currency": p["currency"],
                          "created_at": p.get("created_at"), "day": p.get("_day")}
                for p in STATE.payments}


def commit_versions() -> set:
    """Every (payment_id, version) the vendor ever committed — L1/L2's membership truth
    (the ooo pair contributes both bumps; the forged event contributes nothing)."""
    with STATE.lock:
        return set(STATE.versions)


def vendor_payment_count() -> int:
    with STATE.lock:
        return len(STATE.payments)


def idempotency_replays() -> Dict:
    """B8/A2 dupe truth: how many keys exist and how often each was replayed."""
    with STATE.lock:
        hits = dict(STATE.idempotency_hits)
        keys = len(STATE.idempotency)
    return {"keys": keys, "replayed_keys": sum(1 for v in hits.values() if v),
            "total_replays": sum(hits.values()), "by_key": hits}


def delivery_ledger() -> List[Dict]:
    with STATE.lock:
        return [dict(d) for d in STATE.deliveries]


def reversal_rows() -> List[Dict]:
    with STATE.lock:
        return [dict(r) for r in STATE.reversals]


def schedule_status() -> Dict:
    """Vendor-owned schedule entries with fired state — the driver's sched-unreached input
    ([U] unfired = structural refusal; [A] unfired = zero the dependent rungs, §4.1)."""
    st = STATE
    with st.lock:
        fired = set(st.fired)
        sch = st.sch
    return {
        "race_pages": {k: f"page:{k}" in fired for k in sch.race_pages},
        "refund_txn": f"page:{sch.refund_page}" in fired,
        "midwalk_create": f"page:{sch.race_pages[1]}" in fired,
        "ooo_pair": f"page:{sch.extras_page['ooo']}" in fired,
        "dup_event": f"page:{sch.extras_page['dup']}" in fired,
        "forged_event": f"page:{sch.extras_page['forged']}" in fired,
        "drop_after_page": "drop_fired" in fired,
        "http500_page": "http500" in fired,
        "stale_304_sync": "stale304" in fired or st.stale304["fired"],
        "partition_commits": "partition" in fired,
        "vendor_down_boot_secs": "boot_refusal" in fired,
        "hold_create": "hold_create_fired" in fired,
        "d1_mutation": "d1" in fired,
        "burst": "burst" in fired,
        "classes": schedule_sb7.CLASS,
    }


def _reset_run() -> None:
    """Restore the fixture collection and re-arm every scheduled one-shot for a fresh graded
    exercise in this process (phase semantics carried from v2; the trace keeps appending and
    phase markers delimit)."""
    st = STATE
    with st.lock:
        if st.pending_barrier is not None:
            st.pending_barrier["event"].set()
        st.pending_barrier = None
        st.payments = st.fx.fresh_delivery_order()
        st.index = {p["id"]: p for p in st.payments}
        st.reversals = st.fx.fresh_reversals()
        st.created = []
        st.idempotency.clear()
        st.idempotency_hits.clear()
        st.create_ops = []
        st.sent_events.clear()
        st.deliveries = []
        st.versions = {(p["id"], 1) for p in st.payments}
        st.generation += 1
        st.sync_ordinal = 0
        st.sync_active = False
        st.sync_served = 0
        st.sync_total = None
        st.drop_armed = False
        st.stale304 = _fresh_stale304()
        st.stale_syncs = []
        st.reset_epoch += 1
        st.fired.clear()
        st.value_slot_next = 1
        st.sync1_walk_done = False
        st.quiesce_signaled = False
        st.hold_create_secs = 0.0
        st.refuse_until = 0.0
        st.down = False


def mark_phase(name: str, reset: bool = None) -> None:
    """Record a phase boundary in the trace so the grader can attribute requests. Reset
    (default for first phases; names ending in a number >= 2 are continuations) restores the
    collection and re-arms every scheduled one-shot."""
    if reset is None:
        m = re.search(r"(\d+)$", name)
        reset = not (m and int(m.group(1)) >= 2)
    if reset:
        _reset_run()
    STATE.record(_base_rec({PHASE_MARKER: name}))


def write_tokens_file(path: Path, seed: str = DEFAULT_SEED) -> Dict[str, str]:
    """§2.2 boot contract: the harness writes the tokens file before booting the app."""
    tokens = fixtures_v3.derive_tokens(seed)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(tokens, indent=2) + "\n")
    return tokens


# ── the trap ledger ──────────────────────────────────────────────────────────────────────────

def trap_results() -> Dict:
    """{retry_after, drop_resume, stale_refetch, delivery_acks, no_fresh_key_dupe} in [0,1],
    graded over EVERY instance in this serve()'s trace."""
    st = STATE
    with st.lock:
        events = list(st.events)
        s3 = dict(st.stale304)
        deliveries = list(st.deliveries)
        ops = list(st.create_ops)

    def _next_list(i: int) -> Optional[Dict]:
        return next((x for x in events[i + 1:]
                     if x.get("path") == LIST_PATH and x.get("method") == "GET"), None)

    ra_scores = []
    for i, e in enumerate(events):
        if e.get("path") != LIST_PATH or e.get("status") != 500:
            continue
        nxt = _next_list(i)
        if nxt is None:
            ra_scores.append(0.0)
            continue
        ra_scores.append(1.0 if (nxt["t"] - e["t"]) >= max(0.1, RETRY_AFTER_SECS - 0.3)
                         else 0.0)

    drop_scores = []
    for i, e in enumerate(events):
        if e.get("path") != LIST_PATH or not e.get("dropped"):
            continue
        nxt = _next_list(i)
        resumed = (nxt is not None
                   and _offset_for_cursor((nxt.get("query") or {}).get("cursor"))
                   == e.get("offset"))
        drop_scores.append(1.0 if resumed else 0.0)

    stale = {"fired": bool(s3.get("fired")), "repeats": s3.get("repeats", 0),
             "unconditional_refetches": 0}
    stale_score = 0.0
    if stale["fired"]:
        refetches = [e for e in events
                     if e.get("path") == LIST_PATH and e.get("method") == "GET"
                     and e.get("status") == 200 and e.get("sync") == s3.get("sync")
                     and e.get("offset") == s3.get("offset")
                     and not e.get("if_none_match")
                     and e.get("t", 0) > (s3.get("fired_t") or 0)]
        stale["unconditional_refetches"] = len(refetches)
        if stale["repeats"] > 3 or not refetches:
            stale_score = 0.0                # the loop bug, or stale trusted as fresh
        elif len(refetches) == 1 and stale["repeats"] == 0:
            stale_score = 1.0                # the documented dance, exactly
        else:
            stale_score = 0.5

    ackable = [d for d in deliveries if d.get("kind") != "forged"]
    ack_frac = (sum(1 for d in ackable if d.get("status") and 200 <= d["status"] < 300)
                / len(ackable)) if ackable else 0.0

    fresh_key_dupes = 0
    first_key_for: Dict[str, str] = {}
    for op in ops:
        if op["outcome"] != "created":
            continue
        prior = first_key_for.get(op["chash"])
        if prior is not None and prior != op["key"]:
            fresh_key_dupes += 1
        first_key_for.setdefault(op["chash"], op["key"])

    return {"retry_after": min(ra_scores) if ra_scores else 0.0,
            "drop_resume": min(drop_scores) if drop_scores else 0.0,
            "stale_refetch": stale_score,
            "delivery_acks": round(ack_frac, 4),
            "no_fresh_key_dupe": 1.0 if fresh_key_dupes == 0 else 0.0,
            "detail": {"n_500": len(ra_scores), "n_drops": len(drop_scores),
                       "stale": stale, "deliveries": len(deliveries),
                       "fresh_key_dupes": fresh_key_dupes}}


def serve(port: int, trace: Path, seed: str = DEFAULT_SEED) -> ThreadingHTTPServer:
    global STATE
    trace = Path(trace)
    trace.parent.mkdir(parents=True, exist_ok=True)
    trace.write_text("")
    STATE = State(trace, seed)
    STATE.record(_base_rec({"trace_header": "meridian-v3", "fixture_seed": seed,
                            "n": N_RECORDS, "page_size": PAGE_SIZE,
                            "pages": PAGE_COUNT}))
    server = _Server(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


# ══ smoke: serve, walk every trap docs-perfectly, PASS/FAIL per trap ═════════════════════════

def _req(method: str, url: str, body: Optional[Dict] = None,
         headers: Optional[Dict[str, str]] = None, timeout: float = 20):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    if data is not None:
        req.add_header("Content-Type", "application/json")

    def _parse(raw: bytes) -> Dict:
        try:
            return json.loads(raw or b"{}")
        except json.JSONDecodeError:          # /v3/docs serves markdown, not JSON
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
                {"raw": raw, "sig": self.headers.get("Meridian-Signature"), "body": body,
                 "t": time.time()})
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


def _replay_receiver_quad(secret: str, deliveries: List[Dict]) -> Dict[str, int]:
    """The perfect consumer over the wire-received deliveries — same semantics as
    fixtures_v3._replay_counters, but fed from real bytes + real signatures."""
    quad = {"received": 0, "applied": 0, "ignored": 0, "rejected": 0}
    seen: set = set()
    applied_version: Dict[str, int] = {}
    for d in deliveries:
        quad["received"] += 1
        if not _verify_sig(secret, d["raw"], d["sig"]):
            quad["rejected"] += 1
            continue
        ev = d["body"]
        if ev["id"] in seen:
            quad["ignored"] += 1
            continue
        seen.add(ev["id"])
        if ev["type"] in ("payment.created", "reversal.created"):
            quad["applied"] += 1
            continue
        data = ev["data"]
        if data["version"] <= applied_version.get(data["id"], 1):
            quad["ignored"] += 1
            continue
        applied_version[data["id"]] = data["version"]
        quad["applied"] += 1
    return quad


def _digest_from_rows(rows: List[Dict]) -> Dict:
    """Recompute the §3.1 scene digest from SERVED rows only — wire-truth twin of
    fixtures_v3.Fixture.expected_scene_digest (amount/day-derived, mutation-invariant)."""
    from datetime import date as _date
    enriched = []
    for r in rows:
        inst = datetime.fromisoformat(r["created_at"].replace("Z", "+00:00"))
        enriched.append((inst, r["id"], fixtures_v3.berlin_day(r["created_at"]),
                         r["amount_minor"], r["currency"]))
    enriched.sort(key=lambda e: (e[0], e[1]))
    day_counts: Dict[str, int] = {}
    for _i, _id, day, _a, _c in enriched:
        day_counts[day] = day_counts.get(day, 0) + 1
    d0 = _date.fromisoformat(min(day_counts))
    r0 = max(day_counts.values())
    d_count = fixtures_v3.SPAN_DAYS
    pitch = fixtures_v3.CELL_PITCH
    rank: Dict[str, int] = {}
    sh = sh2 = sx = sz = sxh = szh = 0.0
    for _i, _id, day, amount, cur in enriched:
        d = (_date.fromisoformat(day) - d0).days
        rr = rank.get(day, 0)
        rank[day] = rr + 1
        x = (d - (d_count - 1) / 2) * pitch
        z = (rr - (r0 - 1) / 2) * pitch
        h = fixtures_v3.height_for(amount, cur)
        sh += h
        sh2 += h * h
        sx += x
        sz += z
        sxh += x * h
        szh += z * h
    return {"count": len(enriched), "Sh": round(sh, 4), "Sh2": round(sh2, 4),
            "Sx": round(sx, 4), "Sz": round(sz, 4), "Sxh": round(sxh, 4),
            "Szh": round(szh, 4), "brushedCount": 0}


def _walk(base: str, conditional_first: Optional[str] = None) -> Dict:
    """Docs-perfect walk: retry the same cursor on a drop, honour Retry-After on a 500,
    drop the validator and refetch unconditionally ONCE on a generation-disagreeing 304."""
    pages, reqs, drops, five_hundreds, rows = 0, 0, 0, 0, []
    page_times: List[float] = []
    lying_304 = None
    etag_p1 = gen_p1 = None
    cursor = None
    first = True
    while True:
        url = base + LIST_PATH + (f"?cursor={cursor}" if cursor else "")
        headers = {}
        if first and conditional_first:
            headers["If-None-Match"] = conditional_first
        try:
            status, hdrs, body = _req("GET", url, headers=headers, timeout=60)
        except OSError:
            drops += 1
            reqs += 1
            continue
        reqs += 1
        if status == 304:
            lying_304 = {"etag": hdrs.get("ETag"),
                         "generation": hdrs.get("X-Collection-Generation")}
            first = False        # the documented dance: ONE unconditional refetch
            continue
        if status == 500:
            five_hundreds += 1
            time.sleep(float(hdrs.get("Retry-After") or RETRY_AFTER_SECS))
            continue
        assert status == 200, (status, body)
        first = False
        pages += 1
        page_times.append(time.time())
        if pages == 1:
            etag_p1, gen_p1 = hdrs.get("ETag"), hdrs.get("X-Collection-Generation")
        rows.extend(body["data"])
        if body["next_cursor"] is None:
            total = body["total"]
            break
        cursor = body["next_cursor"]
    return {"pages": pages, "reqs": reqs, "drops": drops, "five_hundreds": five_hundreds,
            "rows": rows, "total": total, "etag_p1": etag_p1, "gen_p1": gen_p1,
            "lying_304": lying_304, "page_times": page_times}


def smoke(port: int = 8901, receiver_port: int = 8902, seed: str = DEFAULT_SEED) -> int:
    import tempfile
    base = f"http://127.0.0.1:{port}"
    trace = Path(tempfile.mkdtemp(prefix="meridian3-smoke-")) / "trace.jsonl"
    serve(port, trace, seed=seed)
    fx = STATE.fx
    sch = STATE.sch
    receiver = _Server(("127.0.0.1", receiver_port), _SmokeReceiver)
    threading.Thread(target=receiver.serve_forever, daemon=True).start()
    results: List[tuple] = []

    def report(name: str, ok: bool, detail: str) -> None:
        results.append((name, ok))
        print(f"{'PASS' if ok else 'FAIL'}  {name:24s} {detail}")

    # 0) the task's double-derivation identity: a from-scratch rebuild recomputes the same
    #    expectation pack, byte for byte
    fixtures_v3._CACHE.clear()
    fx2 = fixtures_v3.build(seed)
    same = (fx2.DIGEST == fx.DIGEST
            and fx2.EXPECTED_BY_CURRENCY == fx.EXPECTED_BY_CURRENCY
            and fx2.EXPECTED_BUCKETS == fx.EXPECTED_BUCKETS
            and fx2.EXPECTED_WEBHOOK_COUNTERS == fx.EXPECTED_WEBHOOK_COUNTERS
            and fx2.EXPECTED_LEDGER_FINAL == fx.EXPECTED_LEDGER_FINAL
            and fx2.PROBE_EXPECT == fx.PROBE_EXPECT)
    report("seed_recompute_x2", same, "build(seed) twice -> identical expectation packs")
    assert same, "seed derivation must be deterministic"

    # register the webhook (idempotent, challenge handshake)
    hook_url = f"http://127.0.0.1:{receiver_port}/hook"
    s1, _, reg1 = _req("POST", base + "/v3/webhooks", {"url": hook_url})
    s2, _, reg2 = _req("POST", base + "/v3/webhooks", {"url": hook_url})
    report("register_idempotent", s1 == 200 and s2 == 200 and reg1 == reg2
           and len(_SmokeReceiver.challenges) == 2,
           f"id={reg1.get('id')} handshakes={len(_SmokeReceiver.challenges)}")
    secret = reg1["secret"]

    # 1) sync #1: the full docs-perfect walk through B1 + B2 + the racing window
    w1 = _walk(base)
    report("page_count_192", w1["pages"] == PAGE_COUNT == 192,
           f"{w1['pages']} pages of {PAGE_SIZE} (want {PAGE_COUNT})")
    assert w1["pages"] == 192, "sync #1 must serve exactly 192 pages"
    ids = [r["id"] for r in w1["rows"]]
    report("full_collection", len(ids) == N_RECORDS and len(set(ids)) == N_RECORDS,
           f"{len(set(ids))}/{N_RECORDS} unique rows, total={w1['total']}")
    report("optimal_requests",
           w1["reqs"] == fx.OPTIMAL_REQUESTS and w1["drops"] == 1
           and w1["five_hundreds"] == 1,
           f"{w1['reqs']} requests (simulated optimum {fx.OPTIMAL_REQUESTS}); "
           f"drop x{w1['drops']} at page {sch.drop_after_page}+1, "
           f"500 x{w1['five_hundreds']} at page {sch.http500_page}")
    by_cur = {c: {"count": 0, "total_minor": 0} for c in fixtures_v3.CURRENCY_EXPONENTS}
    for r in w1["rows"]:
        by_cur[r["currency"]]["count"] += 1
        by_cur[r["currency"]]["total_minor"] += r["amount_minor"]
    report("currency_totals", by_cur == fx.EXPECTED_BY_CURRENCY,
           "recomputed from served rows == EXPECTED_BY_CURRENCY")
    served_digest = _digest_from_rows(w1["rows"])
    report("scene_digest_wire", served_digest == fx.DIGEST,
           f"digest from served rows == seed digest (count {served_digest['count']})")
    assert by_cur == fx.EXPECTED_BY_CURRENCY and served_digest == fx.DIGEST, \
        "served aggregates must recompute identically from the seed"
    no_day_leak = all("day" not in r and "_instant" not in r for r in w1["rows"][:64])
    report("no_day_precomputed", no_day_leak,
           "the vendor never serves Berlin days — the app owns DST (b_buckets_dst)")

    # 2) barrier orders, from the vendor's own trace: order-A deliveries precede the page's
    #    200; order-B deliveries follow it (and the next response waited on them)
    with STATE.lock:
        tr = list(STATE.events)
    page_200_t = {e.get("page"): e["t"] for e in tr
                  if e.get("path") == LIST_PATH and e.get("status") == 200
                  and e.get("sync") == 1}
    order_ok, order_detail = True, []
    for k in sch.race_pages + [sch.refund_page]:
        # match by trigger page, not bare event_id — the byte-identical dup reuses its
        # source's event_id at a LATER page
        d_ts = [e["t"] for e in tr if e.get("webhook_delivery") and e.get("page") == k]
        order = sch.race_order.get(k, "B")
        if order == "A":
            ok = bool(d_ts) and max(d_ts) <= page_200_t[k]
        else:
            nxt = page_200_t.get(k + 1)
            ok = bool(d_ts) and min(d_ts) >= page_200_t[k] \
                and (nxt is None or max(d_ts) <= nxt + 0.05)
        order_ok &= ok
        order_detail.append(f"p{k}:{order}:{'ok' if ok else 'BAD'}")
    report("race_barriers", order_ok, " ".join(order_detail))

    # 3) mid-walk create: invisible to sync #1's bound, exactly once in sync #2
    w2 = _walk(base)
    mid_seen = sum(1 for r in w2["rows"] if r["id"] == MIDWALK_ID)
    report("midwalk_create", vendor_payment_count() == N_RECORDS + 1
           and w2["pages"] == PAGE_COUNT + 1 and mid_seen == 1
           and w2["total"] == N_RECORDS + 1,
           f"sync2: {w2['pages']} pages (want {PAGE_COUNT + 1}), {MIDWALK_ID} x{mid_seen}, "
           f"value-dated {fx.value_slots[0]['created_at']}")

    # 4) B7 vendor half AFTER sync #2 (the driver flow: sync1 -> kill -> sync2 ->
    #    partition -> heal -> syncs 3/4): the 5 seeded flips commit + deliver, and they are
    #    what makes sync #2's stored validators genuinely stale for the B5 arm
    part = fire_partition_commits()
    report("partition_commits", part.get("commits") == schedule_sb7.PARTITION_VENDOR_FLIPS
           and part.get("delivered") == schedule_sb7.PARTITION_VENDOR_FLIPS,
           f"{part.get('commits')} commits, {part.get('delivered')} acked deliveries")

    # 5) the full scheduled quad over real bytes + real signatures
    quad = _replay_receiver_quad(secret, list(_SmokeReceiver.deliveries))
    report("webhook_quad", quad == fx.EXPECTED_WEBHOOK_COUNTERS,
           f"perfect-consumer quad={quad} want={fx.EXPECTED_WEBHOOK_COUNTERS}")

    # 6) final vendor state == EXPECTED_LEDGER_FINAL (every touched row spot-read)
    touched = fx.EXPECTED_LEDGER_FINAL["touched"]
    bad = []
    for pid, want in touched.items():
        _s, _h, row = _req("GET", base + f"/v3/payments/{pid}")
        if row.get("version") != want["version"] or row.get("status") != want["status"] \
                or row.get("note") != want["note"]:
            bad.append(pid)
    report("ledger_final_touched", not bad,
           f"{len(touched)} touched rows re-read == expected" + (f"; BAD {bad}" if bad else ""))

    # 7) refund txn: reversal appended atomically, M2 pair conservation from vendor truth
    _s, _h, revs = _req("GET", base + "/v3/reversals")
    new_revs = [r for r in revs["data"] if r["id"] == sch.refund_txn["reversal_id"]]
    _s, _h, rp = _req("GET", base + f"/v3/payments/{sch.refund_txn['payment_id']}")
    ledger = commit_ledger()
    m2_ok = True
    for cur in fixtures_v3.CURRENCIES:
        rev_sum = sum(r["amount_minor"] for r in revs["data"] if r["currency"] == cur)
        ref_sum = sum(v["amount_minor"] for v in ledger.values()
                      if v["currency"] == cur and v["status"] == "refunded")
        m2_ok &= rev_sum == ref_sum
    report("refund_txn_m2", len(new_revs) == 1 and rp.get("status") == "refunded"
           and new_revs[0]["amount_minor"] == sch.refund_txn["amount_minor"]
           and revs["total"] == len(fx.reversals) + 1 and m2_ok,
           f"reversal {sch.refund_txn['reversal_id']} present once; per-currency "
           f"reversals == refunded rows: {m2_ok}")

    # 8) B5: syncs 3/4 — the armed one lies 304 with a disagreeing generation; the dance is
    #    one unconditional refetch; the other sync answers the stale validator honestly
    stale_ok = True
    stale_detail = []
    for expect_sync in (3, 4):
        w = _walk(base, conditional_first=w2["etag_p1"])
        armed = expect_sync == sch.stale_304_sync
        if armed:
            ok = (w["lying_304"] is not None
                  and w["lying_304"]["generation"] != w2["gen_p1"]
                  and w["pages"] == PAGE_COUNT + 1)
        else:
            ok = w["lying_304"] is None and w["pages"] == PAGE_COUNT + 1
        stale_ok &= ok
        stale_detail.append(f"sync{expect_sync}:{'armed' if armed else 'honest'}:"
                            f"{'ok' if ok else 'BAD'}")
    report("stale_304_arm", stale_ok,
           " ".join(stale_detail) + f" (s={sch.stale_304_sync})")

    # 9) idempotent create + vendor value-dating (slot 1)
    item = {"amount_minor": 4500, "currency": "EUR", "note": "smoke-create",
            "counterparty": {"name": "Smoke Co", "country": "DE"}}
    c1, _, b1 = _req("POST", base + "/v3/payments", item, {"Idempotency-Key": "smoke-k1"})
    c2, _, b2 = _req("POST", base + "/v3/payments", item, {"Idempotency-Key": "smoke-k1"})
    bad_s, _, bad_b = _req("POST", base + "/v3/payments",
                           dict(item, currency="GBP"), {"Idempotency-Key": "smoke-k3"})
    report("create_replay_valuedated",
           c1 == 201 and c2 == 200 and b2.get("id") == b1.get("id")
           and b1.get("created_at") == fx.value_slots[1]["created_at"]
           and bad_s == 400 and bad_b.get("error") == "unsupported_currency"
           and vendor_payment_count() == N_RECORDS + 2,
           f"201 then 200 same id={b1.get('id')}, created_at == slot1, bad currency -> 400")

    # 10) A2 window: commit-then-hold; the same-key retry replays into exactly one payment
    arm_hold_create(1.2)
    item2 = {"amount_minor": 990, "currency": "JPY",
             "counterparty": {"name": "Held Co", "country": "JP"}, "note": "held"}
    held_timed_out = False
    try:
        _req("POST", base + "/v3/payments", item2, {"Idempotency-Key": "smoke-k2"},
             timeout=0.4)
    except OSError:
        held_timed_out = True
    h2, _, hb = _req("POST", base + "/v3/payments", item2, {"Idempotency-Key": "smoke-k2"})
    rep = idempotency_replays()
    report("hold_create_a2", held_timed_out and h2 == 200 and hb.get("id")
           and vendor_payment_count() == N_RECORDS + 3
           and rep["total_replays"] == 2,
           f"held request timed out; retry replayed -> {hb.get('id')}; "
           f"replays={rep['total_replays']} (k1+k2), payments={vendor_payment_count()}")

    # 11) D1 mutation + burst (post-schedule scripted deliveries)
    before = commit_ledger()[fx.d1_target["payment_id"]]["version"]
    d1 = fire_d1_mutation()
    burst = fire_burst()
    ledger2 = commit_ledger()
    burst_ok = all(ledger2[t["payment_id"]]["status"] == t["to_status"]
                   for t in fx.burst_targets)
    report("d1_and_burst", d1["version"] == before + 1
           and d1["to_status"] == fx.d1_target["to_status"]
           and burst["count"] == BURST_COUNT == burst["acked"] and burst_ok
           and burst["t1"] >= burst["t0"],
           f"d1 v{before}->{d1['version']}; burst {burst['count']} flips acked in "
           f"{round((burst['t1'] - burst['t0']) * 1000)}ms")

    # 12) commit truth surfaces
    versions = commit_versions()
    refund_pid = sch.refund_txn["payment_id"]
    want_v = touched[refund_pid]["version"]
    report("commit_truth", (refund_pid, want_v) in versions
           and (MIDWALK_ID, 1) in versions and (refund_pid, want_v + 1) not in versions,
           f"versions set holds ({refund_pid},{want_v}) and ({MIDWALK_ID},1); "
           f"{len(versions)} pairs")

    # 13) the trap ledger over everything this smoke did
    traps = trap_results()
    report("trap_ledger", traps["retry_after"] == 1.0 and traps["drop_resume"] == 1.0
           and traps["stale_refetch"] == 1.0 and traps["delivery_acks"] == 1.0
           and traps["no_fresh_key_dupe"] == 1.0,
           json.dumps({k: v for k, v in traps.items() if k != "detail"}))

    # 14) set_down severs; recovery serves
    set_down(True)
    down_blocked = False
    try:
        _req("GET", base + LIST_PATH, timeout=1.0)
    except OSError:
        down_blocked = True
    set_down(False)
    up_status = _req("GET", base + f"/v3/payments/{refund_pid}")[0]
    report("vendor_down_lever", down_blocked and up_status == 200,
           f"down severed the connection; recovery -> {up_status}")

    # 15) B4 boot refusal window
    arm_boot_refusal(0.7)
    refused = False
    try:
        _req("GET", base + f"/v3/payments/{refund_pid}", timeout=1.0)
    except OSError:
        refused = True
    time.sleep(0.85)
    after = _req("GET", base + f"/v3/payments/{refund_pid}")[0]
    report("boot_refusal_b4", refused and after == 200,
           f"refused during the window; after it -> {after} "
           f"(seeded w={sch.vendor_down_boot_secs}s)")

    # 16) quiescence + schedule status
    stat = schedule_status()
    all_fired = (all(stat["race_pages"].values()) and stat["refund_txn"]
                 and stat["midwalk_create"] and stat["drop_after_page"]
                 and stat["http500_page"] and stat["stale_304_sync"]
                 and stat["partition_commits"])
    report("quiesce_and_status", quiescent() and all_fired,
           f"quiescent={quiescent()}; every vendor-owned schedule entry fired")

    docs_status = _req("GET", base + DOCS_PATH)[0]
    print(f"note  vendor_docs_v3.md: "
          f"{'served' if docs_status == 200 else 'MISSING (503 until the docs artifact ships)'}")

    failed = [n for n, ok in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} traps PASS"
          + (f" — FAILED: {failed}" if failed else ""))
    print(f"seed: {seed}  race_pages={sch.race_pages} order={sch.race_order} "
          f"j={sch.drop_after_page} j2={sch.http500_page} refund_page={sch.refund_page} "
          f"stale_sync={sch.stale_304_sync}")
    print(f"trace: {trace}")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=8790)
    ap.add_argument("--trace", type=Path, default=HERE.parent / "runs/vendor-trace-v3.jsonl")
    ap.add_argument("--seed", default=DEFAULT_SEED, help="16-hex run seed")
    ap.add_argument("--smoke", action="store_true",
                    help="serve on 8901 and exercise every trap; PASS/FAIL per trap")
    args = ap.parse_args()
    if args.smoke:
        return smoke(seed=args.seed)
    serve(args.port, args.trace, seed=args.seed)
    print(f"meridian v3 mock on http://127.0.0.1:{args.port}  trace={args.trace}")
    print(f"  {N_RECORDS} payments, fixed page size {PAGE_SIZE} ({PAGE_COUNT} pages), "
          f"seed {args.seed}; api key {API_KEY}")
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
