"""sb-6 fixture + derived constants — the ONE home for everything the v2 mock, the probe,
and the scorer compare against (fixtures.py's doctrine, extended to the VendorSync Pro tier).

Every EXPECTED_* here is DERIVED — from the generated payment list, from the frozen webhook
delivery plan, or by simulating a perfect client against the mock's own trap constants.
Nothing a check compares against is hand-written: a hand-written twin of a computed truth is
a constant that will eventually disagree (fixtures.py's founding defect, and the sb-6 rule
that OPTIMAL_REQUESTS is "computed from page size + trap overhead — never hand-written").

The fixture answers the measured sb-5 defects by name:
- 4 statuses interleaved in CHRONOLOGICAL order so the default-sort first page always
  carries all four (v_status_distinct's 0.5 fixture ceiling was 30% of all top-band loss);
- 4 currencies with minor-unit exponents 2/2/0/3, all four present on page 1 (JPY-with-
  decimals and truncated-KWD are the rendered wrong-money traps);
- 14 Europe/Berlin days spanning the 2026-03-29 DST switch, with payments deliberately
  landing in the after-midnight window where the Berlin calendar day differs from the UTC
  day — UTC-day bucketing produces measurably wrong /api/buckets cells (asserted at import);
- one guaranteed zero-count bucket cell, (DST day, failed), so "zero-count cells draw
  nothing and are unpickable" is testable on the FULL fixture, not only on the empty db.

Red-team amendments that live here: F2 (HALFSET_ROWS — the vendor-side half-collection cap
that re-bases j_sync_journey), F8/F14 (STALL_HOLD_SECS + CLIENT_TIMEOUT_DOC_SECS — the
stall window and the client-timeout bound vendor_docs_v2 must document), F13
(EXPECTED_WEBHOOK_COUNTERS excludes the challenge handshake — it is part of registration,
not an event delivery, so it increments NO counter).
"""

from __future__ import annotations

from datetime import date, datetime, time as dtime, timedelta, timezone
from typing import Dict, List, Optional
from zoneinfo import ZoneInfo

BERLIN = ZoneInfo("Europe/Berlin")
PHASE_MARKER = "__phase__"

# ── frozen vocabulary (SB6-PACKAGE §4) ───────────────────────────────────────────────────────
STATUS_ORDER = ["settled", "pending", "refunded", "failed"]
CURRENCY_EXPONENTS = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}

# ── collection shape ─────────────────────────────────────────────────────────────────────────
DAY_COUNT = 14
FIRST_DAY = date(2026, 3, 23)
DST_DAY = date(2026, 3, 29)            # Europe/Berlin springs forward: 02:00 CET → 03:00 CEST
_TOTAL_TARGET = 1553                   # frozen design input (SB6-PACKAGE §4); prime, so any
                                       # delivery stride is coprime with it

# ── paging + trap constants (vendor_service_v2 imports these — one home, zero twins) ─────────
DEFAULT_LIMIT = 25
MAX_LIMIT = 100
SHORT_PAGE_AT = 300                    # offset where a short page appears; 300 is a multiple of
SHORT_PAGE_TAKE = 61                   # 25/50/100 so every common page size lands on it, but the
                                       # short page only fires for limit > SHORT_PAGE_TAKE
RETRY_AFTER_SECS = 2
THROTTLE_SECONDS_NTH = 2               # v1's nth-based one-shot chain, unchanged (429 seconds)
CURSOR_EXPIRES_NTH = 3                 # 410 cursor_expired on the 3rd list request WITH a cursor
STALL_HOLD_SECS = 12.0                 # F8: the final sync page is held open this long (one-shot,
                                       # armed only for the under-load phase)
CLIENT_TIMEOUT_DOC_SECS = 10           # F14: spec-build-v3 freezes "a request timeout of at most
                                       # 10 seconds"; vendor_docs_v2 documents the same bound, and
                                       # 10 < 12 so a compliant client provably times out inside
                                       # the stall window (the hold must exceed the doc bound or
                                       # the trap can never observe a timeout)
AMOUNT_LIMIT_MINOR = 5_000_000         # the per-payment amount business rule batch items hit
BATCH_MAX_ITEMS = 20
DELIVERY_STRIDE = 619                  # coprime with 1553: delivery order is scrambled against
                                       # chronological order, exactly v1's property
HALFSET_ROWS = _TOTAL_TARGET // 2      # F2: the "halfset" phase serves only this many rows

# ── deterministic generation patterns ────────────────────────────────────────────────────────
# Period-8 cycles over the CHRONOLOGICAL rank: any 8 consecutive rows carry all four statuses
# and all four currencies, so the default-sort first page (50 rows) always shows the full mix.
_STATUS_PATTERN = ["settled", "pending", "settled", "refunded", "settled", "failed", "pending", "settled"]
_CURRENCY_PATTERN = ["EUR", "USD", "JPY", "EUR", "KWD", "USD", "EUR", "JPY"]
# Mixed offsets: lexicographic order of the created_at STRING differs from instant order, and
# several offsets place after-midnight-Berlin rows on the previous UTC day (v1's twin traps).
_OFFSETS = ["+01:00", "Z", "-05:00", "+09:00", "+02:00", "-03:00"]
_NAMES = ["Aurora Freight", "Baltic Traders", "Cinder Works", "Delta Provisions",
          "Ember Logistics", "Fjord Supply", "Granite Metals", "Harbor Textiles",
          "Iris Components", "Juniper Foods", "Krait Marine", "Lumen Energy",
          "Meridian Paper", "Nordwind Tools", "Opal Ceramics", "Pinnacle Grain"]
_COUNTRIES = ["DE", "US", "JP", "KW", "FR", "GB", "NL"]


def _day_counts() -> List[int]:
    """Pseudo-varied per-day row counts, deterministic, summing to the frozen total."""
    counts = [70 + ((d * 53) % 97) for d in range(DAY_COUNT - 1)]
    counts.append(_TOTAL_TARGET - sum(counts))
    assert all(60 <= n <= 170 for n in counts), counts
    return counts


def _rfc3339_with_offset(instant_utc: datetime, suffix: str) -> str:
    if suffix == "Z":
        return instant_utc.strftime("%Y-%m-%dT%H:%M:%S") + "Z"
    sign = 1 if suffix[0] == "+" else -1
    hours, minutes = int(suffix[1:3]), int(suffix[4:6])
    local = instant_utc + sign * timedelta(hours=hours, minutes=minutes)
    return local.strftime("%Y-%m-%dT%H:%M:%S") + suffix


def build_payments() -> List[Dict]:
    """The 1,553-payment fixture in CHRONOLOGICAL order (ids follow chronological rank).

    Wall-clock minutes inside each Berlin day come from a coprime stride, so they are
    distinct and pseudo-uniform — which guarantees rows in the 00:00–01:59 window whose
    UTC date is the previous day. On the DST day the minute space is the day's real 1,380
    minutes and the nonexistent 02:00–02:59 hour is mapped past the gap, so every generated
    wall time exists exactly once.
    """
    out: List[Dict] = []
    i = 0
    for d, n in enumerate(_day_counts()):
        day = FIRST_DAY + timedelta(days=d)
        if day == DST_DAY:
            raw = sorted((k * 977) % 1380 for k in range(n))
            minutes = [m if m < 120 else m + 60 for m in raw]
        else:
            minutes = sorted((k * 977) % 1440 for k in range(n))
        for m in minutes:
            wall = datetime.combine(day, dtime(m // 60, m % 60), tzinfo=BERLIN)
            instant = wall.astimezone(timezone.utc)
            status = _STATUS_PATTERN[i % 8]
            if day == DST_DAY and status == "failed":
                status = "refunded"        # the guaranteed zero-count cell: (DST day, failed)
            currency = _CURRENCY_PATTERN[i % 8]
            exp = CURRENCY_EXPONENTS[currency]
            units = 5 + ((i * 7919) % 1495)
            amount_minor = units * 10 ** exp + ((i * 271) % (10 ** exp) if exp else 0)
            settled_at = (
                (instant + timedelta(minutes=45 + i % 37)).strftime("%Y-%m-%dT%H:%M:%SZ")
                if status in ("settled", "refunded") else None
            )
            out.append({
                "id": f"pay_{i:04d}",
                "amount_minor": amount_minor,
                "currency": currency,
                "created_at": _rfc3339_with_offset(instant, _OFFSETS[i % len(_OFFSETS)]),
                "settled_at": settled_at,
                "status": status,
                "version": 1,
                "note": "",
                "counterparty": {"name": _NAMES[i % len(_NAMES)],
                                 "country": _COUNTRIES[i % len(_COUNTRIES)]},
                # not documented as ordering-relevant; present so the grader can verify true order
                "_instant": instant.isoformat().replace("+00:00", "Z"),
            })
            i += 1
    return out


def fresh_payments_delivery_order() -> List[Dict]:
    """Deep-copied rows in the mock's scrambled DELIVERY order — the mock mutates versions
    and notes, so it must never alias this module's canonical list."""
    chrono = build_payments()
    n = len(chrono)
    return [dict(chrono[(k * DELIVERY_STRIDE) % n],
                 counterparty=dict(chrono[(k * DELIVERY_STRIDE) % n]["counterparty"]))
            for k in range(n)]


_P = build_payments()
EXPECTED_TOTAL = len(_P)


def _local_day(instant_iso: str, tz) -> str:
    return (datetime.fromisoformat(instant_iso.replace("Z", "+00:00"))
            .astimezone(tz).date().isoformat())


def compute_buckets(payments: List[Dict], tz=BERLIN, tz_name: str = "Europe/Berlin") -> Dict:
    """Full day×status grid from the payments' INSTANTS: every calendar day first→last with
    no gaps, one cell per (day, status) pair including zeros, day-major, frozen status order."""
    if not payments:
        return {"timezone": tz_name, "days": [], "statuses": list(STATUS_ORDER), "cells": []}
    days_present = sorted({_local_day(p["_instant"], tz) for p in payments})
    first = date.fromisoformat(days_present[0])
    last = date.fromisoformat(days_present[-1])
    days = [(first + timedelta(days=k)).isoformat() for k in range((last - first).days + 1)]
    counts: Dict[tuple, int] = {}
    for p in payments:
        key = (_local_day(p["_instant"], tz), p["status"])
        counts[key] = counts.get(key, 0) + 1
    cells = [{"day": d, "status": s, "count": counts.get((d, s), 0)}
             for d in days for s in STATUS_ORDER]
    return {"timezone": tz_name, "days": days, "statuses": list(STATUS_ORDER), "cells": cells}


EXPECTED_BUCKETS = compute_buckets(_P)
EMPTY_BUCKETS = {"timezone": "Europe/Berlin", "days": [], "statuses": list(STATUS_ORDER), "cells": []}

EXPECTED_BY_CURRENCY = {
    cur: {"count": sum(1 for p in _P if p["currency"] == cur),
          "total_minor": sum(p["amount_minor"] for p in _P if p["currency"] == cur)}
    for cur in sorted(CURRENCY_EXPONENTS)
}
EXPECTED_BY_STATUS = {s: sum(1 for p in _P if p["status"] == s) for s in STATUS_ORDER}

# The forbidden value, exported ONLY so the scorer can detect it (red-team F16): a summary
# field whose value equals this sum is an actual cross-currency money total — the sin is the
# VALUE, never a key name. Nothing legitimate ever serves this number.
CROSS_CURRENCY_SUM_FORBIDDEN = sum(v["total_minor"] for v in EXPECTED_BY_CURRENCY.values())


# ── the perfect-client walk: OPTIMAL_REQUESTS is simulated, never hand-written ──────────────
def simulate_optimal_walk(total: Optional[int] = None, limit: int = MAX_LIMIT) -> int:
    """Replays the mock's exact trigger logic (throttle → expire → http-date precedence and
    the short page) for a client that reads the docs perfectly: max limit, waits out both
    Retry-After forms, restarts on 410, never repeats a page it already holds. The request
    count this returns IS the optimum request_efficiency compares against.

    The stall trap adds no request here: it is armed only for the under-load measurement
    phase, never during the graded sync #1 (see OPTIMAL_REQUESTS_STALLED)."""
    total = EXPECTED_TOTAL if total is None else total
    nth, fired, offset, cursor, requests = 0, set(), 0, None, 0
    while True:
        requests += 1
        nth += 1
        if nth == THROTTLE_SECONDS_NTH and "secs" not in fired:
            fired.add("secs")
            continue                                   # 429 seconds → retry same request
        if nth == CURSOR_EXPIRES_NTH and cursor is not None and "gone" not in fired:
            fired.add("gone")
            offset, cursor = 0, None                   # 410 → restart from the first page
            continue
        if "gone" in fired and "date" not in fired:
            fired.add("date")
            continue                                   # 429 HTTP-date → retry same request
        take = SHORT_PAGE_TAKE if (offset == SHORT_PAGE_AT and limit > SHORT_PAGE_TAKE) else limit
        nxt = offset + max(0, min(take, total - offset))
        if nxt >= total:
            return requests
        offset, cursor = nxt, "c"


OPTIMAL_REQUESTS = simulate_optimal_walk()
# A stall-armed sync costs exactly one extra request: the held final page times out at the
# documented client bound and is retried once (the one-shot is consumed by the first attempt).
OPTIMAL_REQUESTS_STALLED = OPTIMAL_REQUESTS + 1
# The F2 half-seed journey grades sync #1 against the HALFSET_ROWS-capped collection: the
# optimum for that walk is simulated the same way, never hand-written (request_efficiency
# compares sync1 against this constant whenever the half-seed phase was live).
OPTIMAL_REQUESTS_HALFSET = simulate_optimal_walk(HALFSET_ROWS)


# ── the frozen webhook delivery plan (F3: fired ONLY by /admin/deliver-script) ──────────────
# One plan, executed verbatim by vendor_service_v2.run_delivery_script(): dupes, an
# out-of-order pair, and one forged signature. "mutate_only" steps change vendor state
# WITHOUT a delivery — they create the older snapshot the stale delivery later carries.
# Events mutate only note+version, never amount/status/created_at, so EXPECTED_BUCKETS and
# EXPECTED_BY_CURRENCY stay valid whenever the script runs relative to the viz probe.
WEBHOOK_SCRIPT = [
    {"kind": "apply",     "event_id": "evt_0001", "target": "pay_0010", "note": "wh-a-1"},
    {"kind": "mutate_only",                       "target": "pay_0020", "note": "wh-b-1"},
    {"kind": "apply",     "event_id": "evt_0002", "target": "pay_0020", "note": "wh-b-2"},
    {"kind": "stale",     "event_id": "evt_0003", "target": "pay_0020"},   # the v2 snapshot, late
    {"kind": "duplicate", "event_id": "evt_0001"},                          # byte-identical redelivery
    {"kind": "forged",    "event_id": "evt_0004", "target": "pay_0030", "note": "wh-c-x"},
    {"kind": "apply",     "event_id": "evt_0005", "target": "pay_0010", "note": "wh-a-2"},
    {"kind": "apply",     "event_id": "evt_0006", "target": "pay_0040", "note": "wh-d-1"},
]

# F13: the verification challenge is part of registration, not an event delivery — it
# increments NO counter, which is why "received" counts only the script's deliveries.
EXPECTED_WEBHOOK_COUNTERS = {
    "received": sum(1 for s in WEBHOOK_SCRIPT if s["kind"] != "mutate_only"),
    "applied":  sum(1 for s in WEBHOOK_SCRIPT if s["kind"] == "apply"),
    "ignored":  sum(1 for s in WEBHOOK_SCRIPT if s["kind"] in ("stale", "duplicate")),
    "rejected": sum(1 for s in WEBHOOK_SCRIPT if s["kind"] == "forged"),
}


def _simulate_webhook_final() -> Dict[str, Dict]:
    """End state of every touched payment after a CORRECT app consumed the script — which
    equals the vendor's own end state, because forged events never mutate the vendor and
    stale/duplicate deliveries carry no new mutation."""
    state: Dict[str, Dict] = {}
    for step in WEBHOOK_SCRIPT:
        if step["kind"] in ("apply", "mutate_only"):
            cur = state.setdefault(step["target"], {"version": 1, "note": ""})
            cur["version"] += 1
            cur["note"] = step["note"]
    return state


EXPECTED_WEBHOOK_FINAL = _simulate_webhook_final()


# ── import-time proofs: the traps are live, or this module refuses to load ──────────────────
assert EXPECTED_TOTAL == _TOTAL_TARGET
assert [p["_instant"] for p in _P] == sorted(p["_instant"] for p in _P), "chronological ids broken"
assert len(set(p["_instant"] for p in _P)) == EXPECTED_TOTAL, "instants must be distinct"
_first_page = _P[:50]
assert len({p["status"] for p in _first_page}) == 4, "page 1 must carry all four statuses"
assert len({p["currency"] for p in _first_page}) == 4, "page 1 must carry all four currencies"
assert len(EXPECTED_BUCKETS["days"]) == DAY_COUNT
assert len(EXPECTED_BUCKETS["cells"]) == DAY_COUNT * len(STATUS_ORDER)
assert sum(c["count"] for c in EXPECTED_BUCKETS["cells"]) == EXPECTED_TOTAL
_dst_failed = next(c for c in EXPECTED_BUCKETS["cells"]
                   if c["day"] == DST_DAY.isoformat() and c["status"] == "failed")
assert _dst_failed["count"] == 0, "the guaranteed zero-count cell is gone"
assert all(c["count"] > 0 for c in EXPECTED_BUCKETS["cells"]
           if not (c["day"] == DST_DAY.isoformat() and c["status"] == "failed"))
# The DST/UTC discriminator must actually discriminate: UTC-day bucketing has to disagree
# on enough cells that b_buckets_dst resolves it (an observed-equal grid licenses nothing).
_utc_cells = {(c["day"], c["status"]): c["count"]
              for c in compute_buckets(_P, tz=timezone.utc, tz_name="UTC")["cells"]}
_berlin_cells = {(c["day"], c["status"]): c["count"] for c in EXPECTED_BUCKETS["cells"]}
_differing = sum(1 for k, v in _berlin_cells.items() if _utc_cells.get(k, 0) != v)
assert _differing >= 8, f"UTC bucketing differs on only {_differing} cells — trap is dead"
assert 1 < HALFSET_ROWS < EXPECTED_TOTAL
assert OPTIMAL_REQUESTS > (EXPECTED_TOTAL // MAX_LIMIT), "walk simulation degenerated"
assert OPTIMAL_REQUESTS_HALFSET > (HALFSET_ROWS // MAX_LIMIT), "halfset walk degenerated"
assert CLIENT_TIMEOUT_DOC_SECS < STALL_HOLD_SECS, "a compliant client must time out in the stall"
assert sum(EXPECTED_BY_STATUS.values()) == EXPECTED_TOTAL
assert sum(v["count"] for v in EXPECTED_BY_CURRENCY.values()) == EXPECTED_TOTAL
