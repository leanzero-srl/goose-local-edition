"""sb-7 seeded fixtures — the ONE home for everything the v3 mock, the harness, and the
scorer compare against (fixtures_v2's doctrine, re-based on the per-run seed).

DESIGN §2.3 / §5.5 contract: STRUCTURE IS FROZEN (N = 12,288; span = 96 Europe/Berlin days;
server page size 64 -> 192 pages; per-day cap 180; 4 statuses each >= 8%; currencies
EUR/USD/JPY/KWD with exponents 2/2/0/3; trap counts and classes; K = 8 partition commits;
3 workflow drafts) while every VALUE is seeded: which DST transition (4-item menu), the
per-day profile, every amount/currency/counterparty/timestamp, every schedule placement and
target, the tokens. `build(seed)` yields the dataset, the resolved fault schedule
(schedule_sb7 — the one derivation module), and ALL expectations as pure functions of the
seed: one derivation, three consumers (vendor serves it, harness renders the expectation
pack, scorer recomputes independently); drift is impossible by construction.

Nothing a check compares against is hand-written. EXPECTED_WEBHOOK_COUNTERS replays the
delivery script against a perfect consumer; OPTIMAL_REQUESTS replays the mock's own trigger
logic for a docs-perfect client; EXPECTED_LEDGER_FINAL applies the schedule to the base
collection; the scene digest is float64 recomputation of the §3.1 transform. The sb-6
assert battery (cells sum to N, DST discriminator live, walk non-degenerate) runs per-seed
inside build().

M2 ground truth starts TRUE: every initially-refunded payment has a reversal row, so
pair-conservation holds at boot; race/partition flips never touch or produce "refunded"
(only the refund txn does, atomically with its reversal), so M2 stays exact at every
observable instant of the run.
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta, timezone
from typing import Dict, List, Optional, Tuple
from zoneinfo import ZoneInfo

try:
    import schedule_sb7
except ImportError:                    # imported as a package module (bench.fixtures_v3)
    from . import schedule_sb7

# re-exported: one home, zero twins (bound off the module so the package path works too)
CLASS = schedule_sb7.CLASS
N_RECORDS = schedule_sb7.N_RECORDS
PAGE_BAND = schedule_sb7.PAGE_BAND
PAGE_COUNT = schedule_sb7.PAGE_COUNT
PAGE_SIZE = schedule_sb7.PAGE_SIZE
PARTITION_COMMITS_K = schedule_sb7.PARTITION_COMMITS_K
Rand = schedule_sb7.Rand
Schedule = schedule_sb7.Schedule

BERLIN = ZoneInfo("Europe/Berlin")
PHASE_MARKER = "__phase__"
DEFAULT_SEED = "5eedc0ffee5b7a11"

# ── frozen vocabulary and geometry (DESIGN §2.3 / §3.1 — binding) ────────────────────────────
STATUS_ORDER = ["settled", "pending", "refunded", "failed"]
CURRENCY_EXPONENTS = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
CURRENCIES = sorted(CURRENCY_EXPONENTS)
SPAN_DAYS = 96
PER_DAY_CAP = 180
STATUS_MIN_FRACTION = 0.08
DST_MENU = [date(2026, 3, 29), date(2026, 10, 25), date(2027, 3, 28), date(2025, 10, 26)]
DST_EDGE_MARGIN = 7                    # transition >= 7 days from both window ends
CELL_PITCH = 1.2
LABEL_CANDIDATE_COUNT = 12
_ELITE_COUNT = 14                      # planted top-amount rows (top-12 + margin)
HEIGHT_CASE_COUNT = 6                  # §3.1 height-pixel rung: 6 instances, >= 1 JPY + 1 KWD
READ_TARGET_COUNT = 2                  # §4.3 read stream polls 2 seeded /api/payments/<id>
BURST_COUNT = 24                       # frozen under-stream burst size (P tier; seeded targets)
RETRY_AFTER_SECS = 2                   # the B2 500 carries this Retry-After
DELIVERY_TIMEOUT_SECS = 10             # §4.3: delivery attempts time out at 10 s
_STRIDES = [5381, 6151, 7919, 9973, 11939]     # odd, not divisible by 3 -> coprime with 12288
_OFFSETS = ["+01:00", "Z", "-05:00", "+09:00", "+02:00", "-03:00"]
_NAMES = ["Aurora Freight", "Baltic Traders", "Cinder Works", "Delta Provisions",
          "Ember Logistics", "Fjord Supply", "Granite Metals", "Harbor Textiles",
          "Iris Components", "Juniper Foods", "Krait Marine", "Lumen Energy",
          "Meridian Paper", "Nordwind Tools", "Opal Ceramics", "Pinnacle Grain"]
_COUNTRIES = ["DE", "US", "JP", "KW", "FR", "GB", "NL"]
DRAFT_OUTCOMES = {"F1": "approve", "F2": "reject", "F3": "approve"}   # frozen workflow shape
NOTIFYING_EVENT_TYPES = ("draft.submitted", "draft.approved", "draft.rejected",
                         "reversal.created")   # §4.2 selective materialization (payment.sent
                                               # is processed but produces NO notification)


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def height_for(amount_minor: int, currency: str) -> float:
    """§3.1: h = clamp(0.9 + 0.55·log10(a_major), 0.2, 4.2), through the currency exponent."""
    a_major = amount_minor / (10 ** CURRENCY_EXPONENTS[currency])
    return clamp(0.9 + 0.55 * math.log10(a_major), 0.2, 4.2)


def a_major(amount_minor: int, currency: str) -> float:
    return amount_minor / (10 ** CURRENCY_EXPONENTS[currency])


def berlin_day(instant_iso: str) -> str:
    """UTC instant -> Europe/Berlin calendar date (the backend-owned bucketing rule)."""
    return (datetime.fromisoformat(instant_iso.replace("Z", "+00:00"))
            .astimezone(BERLIN).date().isoformat())


def _rfc3339_with_offset(instant_utc: datetime, suffix: str) -> str:
    if suffix == "Z":
        return instant_utc.strftime("%Y-%m-%dT%H:%M:%S") + "Z"
    sign = 1 if suffix[0] == "+" else -1
    hours, minutes = int(suffix[1:3]), int(suffix[4:6])
    local = instant_utc + sign * timedelta(hours=hours, minutes=minutes)
    return local.strftime("%Y-%m-%dT%H:%M:%S") + suffix


def compute_buckets(payments: List[Dict], tz=BERLIN, tz_name: str = "Europe/Berlin") -> Dict:
    """Full day×status grid from the payments' INSTANTS (fixtures_v2's shape, unchanged):
    every calendar day first→last with no gaps, one cell per (day, status) pair including
    zeros, day-major, frozen status order."""
    if not payments:
        return {"timezone": tz_name, "days": [], "statuses": list(STATUS_ORDER), "cells": []}
    def _day(p):
        return (datetime.fromisoformat(p["_instant"].replace("Z", "+00:00"))
                .astimezone(tz).date().isoformat())
    days_present = sorted({_day(p) for p in payments})
    first = date.fromisoformat(days_present[0])
    last = date.fromisoformat(days_present[-1])
    days = [(first + timedelta(days=k)).isoformat() for k in range((last - first).days + 1)]
    counts: Dict[tuple, int] = {}
    for p in payments:
        key = (_day(p), p["status"])
        counts[key] = counts.get(key, 0) + 1
    cells = [{"day": d, "status": s, "count": counts.get((d, s), 0)}
             for d in days for s in STATUS_ORDER]
    return {"timezone": tz_name, "days": days, "statuses": list(STATUS_ORDER), "cells": cells}


# ── seeded generation ────────────────────────────────────────────────────────────────────────

def choose_window(seed: str) -> Tuple[date, List[date], int]:
    """Seeded DST menu pick + transition placement: first day, the 96 days, transition index."""
    r = Rand(seed, "window")
    dst_day = r.choice(DST_MENU)
    t = r.rng(DST_EDGE_MARGIN, SPAN_DAYS - 1 - DST_EDGE_MARGIN)
    first = dst_day - timedelta(days=t)
    days = [first + timedelta(days=k) for k in range(SPAN_DAYS)]
    return first, days, t


def day_counts(seed: str) -> List[int]:
    """Seeded per-day profile: raw draws in [76, 180] (mean 128 = N/96), then a bounded
    deterministic repair walk to hit sum = N exactly; skew stays inside [40, 180]."""
    r = Rand(seed, "daycounts")
    counts = [76 + r.below(105) for _ in range(SPAN_DAYS)]
    diff = N_RECORDS - sum(counts)
    i = r.below(SPAN_DAYS)
    guard = 0
    while diff != 0:
        guard += 1
        assert guard < 200_000, "day-count repair walk degenerated"
        if diff > 0 and counts[i] < PER_DAY_CAP:
            counts[i] += 1
            diff -= 1
        elif diff < 0 and counts[i] > 40:
            counts[i] -= 1
            diff += 1
        i = (i + 1) % SPAN_DAYS
    assert sum(counts) == N_RECORDS and all(1 <= c <= PER_DAY_CAP for c in counts)
    return counts


def status_totals(seed: str) -> Dict[str, int]:
    """Each status >= 8% of N (floor 984), seeded split of the remainder."""
    floor = math.ceil(STATUS_MIN_FRACTION * N_RECORDS)
    r = Rand(seed, "statustotals")
    weights = [1 + r.below(100) for _ in STATUS_ORDER]
    leftover = N_RECORDS - floor * len(STATUS_ORDER)
    total_w = sum(weights)
    alloc = [leftover * w // total_w for w in weights]
    rem = leftover - sum(alloc)
    order = sorted(range(len(alloc)),
                   key=lambda i: (-(leftover * weights[i] % total_w), i))
    for i in order[:rem]:
        alloc[i] += 1
    out = {s: floor + a for s, a in zip(STATUS_ORDER, alloc)}
    assert sum(out.values()) == N_RECORDS and all(v >= floor for v in out.values())
    return out


@dataclass
class Fixture:
    seed: str
    first_day: date = None
    days: List[date] = field(default_factory=list)
    dst_index: int = 0
    day_counts: List[int] = field(default_factory=list)
    payments: List[Dict] = field(default_factory=list)        # chronological, ids follow rank
    reversals: List[Dict] = field(default_factory=list)       # initial (M2 boot truth)
    delivery_stride: int = 0
    schedule: Schedule = None
    value_slots: List[Dict] = field(default_factory=list)     # §2.3 value-dating queue
    tokens: Dict[str, str] = field(default_factory=dict)
    label_candidate_ids: List[str] = field(default_factory=list)
    # expectation pack (derived; see build())
    EXPECTED_TOTAL: int = 0
    EXPECTED_BUCKETS: Dict = field(default_factory=dict)
    EXPECTED_BY_CURRENCY: Dict = field(default_factory=dict)
    EXPECTED_BY_STATUS: Dict = field(default_factory=dict)
    EXPECTED_REVERSALS_BY_CURRENCY: Dict = field(default_factory=dict)
    EXPECTED_LEDGER_FINAL: Dict = field(default_factory=dict)
    EXPECTED_WEBHOOK_COUNTERS: Dict = field(default_factory=dict)
    EXPECTED_WORKFLOW_STATES: Dict = field(default_factory=dict)
    EXPECTED_NOTIFICATION_MULTISET: Dict = field(default_factory=dict)
    OPTIMAL_REQUESTS: int = 0
    CROSS_CURRENCY_SUM_FORBIDDEN: int = 0
    # score_sb7 pack surface (same truths under the names the scorer's contract prints):
    N: int = 0
    PAGE_SIZE: int = 0
    PAGES: int = 0
    LAYOUT: Dict = field(default_factory=dict)
    DIGEST: Dict = field(default_factory=dict)
    LABEL_CANDIDATES: List[Dict] = field(default_factory=list)
    HEIGHT_CASES: List[Dict] = field(default_factory=list)
    READ_TARGETS: List[str] = field(default_factory=list)
    PROBE_EXPECT: Dict = field(default_factory=dict)
    # post-schedule scripted targets (D1 corner, under-stream burst) — seeded, disjoint from
    # every §4.1 schedule target so no exercise invalidates another's expected versions:
    d1_target: Dict = field(default_factory=dict)
    burst_targets: List[Dict] = field(default_factory=list)

    # ── derived views ────────────────────────────────────────────────────────────────────────
    def index(self) -> Dict[str, Dict]:
        return {p["id"]: p for p in self.payments}

    def fresh_delivery_order(self) -> List[Dict]:
        """Deep-copied rows in the mock's scrambled delivery order — the mock mutates
        versions/notes/statuses, so it must never alias this fixture's canonical list."""
        n = len(self.payments)
        return [dict(self.payments[(k * self.delivery_stride) % n],
                     counterparty=dict(self.payments[(k * self.delivery_stride) % n]
                                       ["counterparty"]))
                for k in range(n)]

    def fresh_reversals(self) -> List[Dict]:
        return [dict(r) for r in self.reversals]

    def layout_basis(self) -> Dict:
        """§3.1 layout basis locked at page load: d0, D0, R0."""
        return {"d0": self.days[0].isoformat(), "D0": SPAN_DAYS, "R0": max(self.day_counts)}

    def scene_positions(self) -> List[Tuple[float, float, float]]:
        """(x, z, h) per chronological record under the §3.1 transform."""
        r0 = max(self.day_counts)
        out: List[Tuple[float, float, float]] = []
        rank_in_day: Dict[int, int] = {}
        for p in self.payments:
            d = p["_day_index"]
            r = rank_in_day.get(d, 0)
            rank_in_day[d] = r + 1
            x = (d - (SPAN_DAYS - 1) / 2) * CELL_PITCH
            z = (r - (r0 - 1) / 2) * CELL_PITCH
            out.append((x, z, height_for(p["amount_minor"], p["currency"])))
        return out

    def expected_scene_digest(self, extra: Optional[List[Dict]] = None) -> Dict:
        """§3.1 sceneDigest recomputed float64 from the seed. `extra` items are streamed
        creates: {"value_slot", "amount_minor", "currency"} — each appends at
        r = the slot day's load-time count (slots live on distinct days by construction)."""
        pos = self.scene_positions()
        r0 = max(self.day_counts)
        if extra:
            day_index = {d.isoformat(): i for i, d in enumerate(self.days)}
            for item in extra:
                slot = self.value_slots[item["value_slot"]]
                d = day_index[slot["day"]]
                r = self.day_counts[d]
                x = (d - (SPAN_DAYS - 1) / 2) * CELL_PITCH
                z = (r - (r0 - 1) / 2) * CELL_PITCH
                pos.append((x, z, height_for(item["amount_minor"], item["currency"])))
        sh = sum(h for _x, _z, h in pos)
        return {"count": len(pos),
                "Sh": round(sh, 4),
                "Sh2": round(sum(h * h for _x, _z, h in pos), 4),
                "Sx": round(sum(x for x, _z, _h in pos), 4),
                "Sz": round(sum(z for _x, z, _h in pos), 4),
                "Sxh": round(sum(x * h for x, _z, h in pos), 4),
                "Szh": round(sum(z * h for _x, z, h in pos), 4),
                "brushedCount": 0}

    def label_candidates(self) -> List[Dict]:
        """The 12 highest-a_major records (tie-break id ASC) — the §3.5 candidate set."""
        ranked = sorted(self.payments,
                        key=lambda p: (-a_major(p["amount_minor"], p["currency"]), p["id"]))
        return [{"id": p["id"], "amount_minor": p["amount_minor"],
                 "currency": p["currency"], "day": p["_day"]}
                for p in ranked[:LABEL_CANDIDATE_COUNT]]

    def expected_final_by_currency(self, approved_refs: Tuple[str, ...] = ("F1", "F3"),
                                   include_midwalk: bool = True) -> Dict:
        """M3 terminal conservation ground truth: fixture + scripted mutations (amounts are
        immutable in v3) + the midwalk create + one payment per approved draft."""
        out = {c: dict(v) for c, v in self.EXPECTED_BY_CURRENCY.items()}
        adds = []
        if include_midwalk:
            adds.append(self.schedule.midwalk_create)
        adds += [d for d in self.schedule.approval_fixture if d["ref"] in approved_refs]
        for item in adds:
            cur = item["currency"]
            out[cur]["count"] += 1
            out[cur]["total_minor"] += item["amount_minor"]
        return out


def scheduled_delivery_script(fx: "Fixture") -> List[Dict]:
    """The vendor's scheduled webhook deliveries in canonical fire order — trigger pages
    ascending; per race page: the 4 mutation events, then any extras riding that page
    (ooo pair delivered v+2-then-v+1, the byte-identical dup, the forged one); the refund
    txn pair at its own page; the midwalk create right after k2. This exact list is what
    vendor_service_v3 executes and what EXPECTED_WEBHOOK_COUNTERS replays."""
    sch = fx.schedule
    idx = fx.index()
    version: Dict[str, int] = {}
    note: Dict[str, str] = {}
    status: Dict[str, str] = {}

    def _bump(pid: str, kind: str, to_status: Optional[str] = None,
              new_note: Optional[str] = None) -> Dict:
        version[pid] = version.get(pid, idx[pid]["version"]) + 1
        if to_status is not None:
            status[pid] = to_status
        if new_note is not None:
            note[pid] = new_note
        return {"payment_id": pid, "version": version[pid],
                "status": status.get(pid, idx[pid]["status"]),
                "note": note.get(pid, idx[pid]["note"])}

    script: List[Dict] = []
    eid = 0

    def _evt(kind: str, snap: Dict, **kw) -> Dict:
        nonlocal eid
        eid += 1
        return {"event_id": f"evt_{eid:04d}", "kind": kind, **snap, **kw}

    pages = sorted(set(sch.race_pages) | {sch.refund_page})
    for page in pages:
        if page in sch.race_pages:
            for m in sch.race_mutations[page]:
                snap = _bump(m["payment_id"], m["kind"], m.get("to_status"), m.get("note"))
                script.append(_evt("apply", snap, type="payment.updated", page=page,
                                   sched=f"race_mutations[{page}]"))
            if sch.extras_page["ooo"] == page:
                pid = sch.ooo_pair["payment_id"]
                first = _bump(pid, "note", None, sch.ooo_pair["notes"][0])
                second = _bump(pid, "note", None, sch.ooo_pair["notes"][1])
                script.append(_evt("apply", second, type="payment.updated", page=page,
                                   sched="ooo_pair"))          # v+2 delivered first
                script.append(_evt("stale", first, type="payment.updated", page=page,
                                   sched="ooo_pair"))          # then v+1
            if sch.extras_page["dup"] == page:
                src = script[sch.dup_source_index]              # a k1-batch event, redelivered
                script.append(dict(src, kind="duplicate", page=page, sched="dup_event"))
            if sch.extras_page["forged"] == page:
                pid = sch.forged_event["payment_id"]
                fake = {"payment_id": pid, "version": idx[pid]["version"] + 1,
                        "status": idx[pid]["status"], "note": sch.forged_event["note"]}
                script.append(_evt("forged", fake, type="payment.updated", page=page,
                                   sched="forged_event"))
            if page == sch.race_pages[1]:                       # between k2 and k3
                mc = sch.midwalk_create
                slot = fx.value_slots[mc["value_slot"]]
                script.append(_evt("apply", {"payment_id": "__midwalk__", "version": 1,
                                             "status": "pending", "note": mc["note"]},
                                   type="payment.created", page=page, sched="midwalk_create",
                                   create=dict(mc, created_at=slot["created_at"],
                                               day=slot["day"])))
        if page == sch.refund_page:
            pid = sch.refund_txn["payment_id"]
            snap = _bump(pid, "status", "refunded", None)
            txn = {"id": sch.refund_txn["txn_id"], "of": 2}
            script.append(_evt("apply", snap, type="payment.updated", page=page,
                               sched="refund_txn", txn=dict(txn, part=1)))
            script.append(_evt("apply", {"payment_id": pid, "version": snap["version"],
                                         "status": "refunded", "note": snap["note"]},
                               type="reversal.created", page=page, sched="refund_txn",
                               txn=dict(txn, part=2),
                               reversal={"payment_id": pid,
                                         "amount_minor": idx[pid]["amount_minor"],
                                         "currency": idx[pid]["currency"]}))
    for c in sch.partition_commits:
        if c["kind"] != "vendor_flip":
            continue
        snap = _bump(c["payment_id"], "status", c["to_status"], None)
        script.append(_evt("apply", snap, type="payment.updated", page=None,
                           sched="partition_commits"))
    return script


def _replay_counters(script: List[Dict]) -> Dict[str, int]:
    """A perfect consumer replays the scheduled deliveries: the counter quad is computed,
    never hand-written. Stale (version <= applied) and duplicate (event id seen) are
    'ignored'; a bad signature is 'rejected'; everything else applies exactly once."""
    quad = {"received": 0, "applied": 0, "ignored": 0, "rejected": 0}
    seen: set = set()
    applied_version: Dict[str, int] = {}
    for ev in script:
        quad["received"] += 1
        if ev["kind"] == "forged":
            quad["rejected"] += 1
            continue
        if ev["event_id"] in seen:
            quad["ignored"] += 1
            continue
        seen.add(ev["event_id"])
        pid = ev["payment_id"]
        if ev["type"] == "payment.created":
            quad["applied"] += 1
            continue
        if ev["type"] == "reversal.created":
            quad["applied"] += 1
            continue
        if ev["version"] <= applied_version.get(pid, 1):
            quad["ignored"] += 1
            continue
        applied_version[pid] = ev["version"]
        quad["applied"] += 1
    return quad


def expected_webhook_counters(fx: "Fixture", app_creates: int = 0,
                              extra_applies: int = 0) -> Dict[str, int]:
    """Scheduled deliveries + one payment.created delivery per app-created payment (each
    approved draft loops back through the webhook like any other payment, §4.5).
    `extra_applies` covers post-schedule scripted deliveries the scorer fired itself (the D1
    mutation, the under-stream burst) — fresh single applies, counted from its own ledger."""
    quad = _replay_counters(scheduled_delivery_script(fx))
    quad["received"] += app_creates + extra_applies
    quad["applied"] += app_creates + extra_applies
    return quad


def _expected_ledger_final(fx: "Fixture") -> Dict:
    """Final vendor state after the full schedule fires: touched rows' version/status/note,
    the midwalk create, one reversal added by the refund txn, and the value-slot queue the
    app-created payments will consume in creation order."""
    touched: Dict[str, Dict] = {}
    idx = fx.index()
    for ev in scheduled_delivery_script(fx):
        if ev["kind"] in ("duplicate", "forged") or ev["type"] == "reversal.created":
            continue
        if ev["type"] == "payment.created":
            continue
        pid = ev["payment_id"]
        cur = touched.setdefault(pid, {"version": idx[pid]["version"],
                                       "status": idx[pid]["status"],
                                       "note": idx[pid]["note"]})
        if ev["version"] > cur["version"]:
            cur.update({"version": ev["version"], "status": ev["status"], "note": ev["note"]})
    mc = fx.schedule.midwalk_create
    slot = fx.value_slots[mc["value_slot"]]
    return {
        "touched": touched,
        "midwalk_create": dict(mc, created_at=slot["created_at"], day=slot["day"],
                               status="pending", version=1),
        "refund_reversal": {"payment_id": fx.schedule.refund_txn["payment_id"],
                            "txn_id": fx.schedule.refund_txn["txn_id"],
                            "reversal_id": fx.schedule.refund_txn["reversal_id"],
                            "amount_minor": idx[fx.schedule.refund_txn["payment_id"]]
                            ["amount_minor"],
                            "currency": idx[fx.schedule.refund_txn["payment_id"]]["currency"]},
        "value_slots": [dict(s) for s in fx.value_slots],
    }


def simulate_optimal_walk(sch: Schedule) -> int:
    """Docs-perfect client against the v3 walk: 192 fixed pages; the B1 drop costs exactly
    one aborted attempt (resume with the same cursor); the B2 500 costs exactly one retry
    after Retry-After. Replays the mock's trigger logic — never hand-written."""
    requests = 0
    served = 0
    drop_armed = False
    for page in range(1, PAGE_COUNT + 1):
        if drop_armed:
            requests += 1              # the dropped attempt (connection closed, no bytes)
            drop_armed = False
        if page == sch.http500_page:
            requests += 1              # the single 500 + Retry-After attempt
        requests += 1                  # the 200 that serves the page
        served += 1
        if served == sch.drop_after_page:
            drop_armed = True
    return requests


def _expected_notifications(fx: "Fixture") -> Dict[str, int]:
    """Computed from the run's committed ledger events, never hand-written: one row per
    notifying type instance — submits for every draft, approve/reject per frozen outcome,
    one reversal.created from the refund txn. payment.sent notifies nothing."""
    counts = {t: 0 for t in NOTIFYING_EVENT_TYPES}
    for ref, outcome in DRAFT_OUTCOMES.items():
        counts["draft.submitted"] += 1
        counts["draft.approved" if outcome == "approve" else "draft.rejected"] += 1
    counts["reversal.created"] += 1
    return counts


def _schedule_target_ids(sch: Schedule) -> set:
    """Every payment the §4.1 schedule mutates or forges against — the disjointness base."""
    ids = {m["payment_id"] for k in sch.race_pages for m in sch.race_mutations[k]}
    ids |= {sch.refund_txn["payment_id"], sch.ooo_pair["payment_id"],
            sch.forged_event["payment_id"]}
    ids |= {c["payment_id"] for c in sch.partition_commits if c["kind"] == "vendor_flip"}
    return ids


def _height_cases(fx: "Fixture") -> List[Dict]:
    """§3.1 (R9): 6 seeded instances for the height-pixel rung, >= 1 JPY and >= 1 KWD among
    them so a wrong currency exponent renders a measurably wrong top. `n` is the stable
    arrival index (initial serve order = chronological order = list position)."""
    r = Rand(fx.seed, "heightcases")
    pos = fx.scene_positions()
    by_cur = {c: [i for i, p in enumerate(fx.payments) if p["currency"] == c]
              for c in CURRENCIES}
    picked: List[int] = []
    for cur in ("JPY", "KWD", "EUR", "USD"):
        while True:
            i = by_cur[cur][r.below(len(by_cur[cur]))]
            if i not in picked:
                picked.append(i)
                break
    while len(picked) < HEIGHT_CASE_COUNT:
        i = r.below(N_RECORDS)
        if i not in picked:
            picked.append(i)
    return [{"id": fx.payments[i]["id"], "n": i,
             "amount_minor": fx.payments[i]["amount_minor"],
             "currency": fx.payments[i]["currency"], "day": fx.payments[i]["_day"],
             "x": round(pos[i][0], 4), "z": round(pos[i][1], 4), "h": round(pos[i][2], 4)}
            for i in picked]


def _read_targets(fx: "Fixture") -> List[str]:
    """§4.3 read stream: 2 seeded payment ids whose versions move during the racing window —
    status-kind race targets preferred so L3 monotonic-reads has real signal."""
    sch = fx.schedule
    status_ids = [m["payment_id"] for k in sch.race_pages for m in sch.race_mutations[k]
                  if m["kind"] == "status"]
    all_ids = [m["payment_id"] for k in sch.race_pages for m in sch.race_mutations[k]]
    pool = status_ids if len(status_ids) >= READ_TARGET_COUNT else all_ids
    r = Rand(fx.seed, "readtargets")
    out: List[str] = []
    while len(out) < READ_TARGET_COUNT:
        pid = r.choice(pool)
        if pid not in out:
            out.append(pid)
    return out


def _post_schedule_targets(fx: "Fixture") -> Tuple[Dict, List[Dict]]:
    """The D1 brushed-record mutation target and the under-stream burst targets: seeded,
    never refunded (M2), disjoint from every schedule target and from each other."""
    r = Rand(fx.seed, "postsched")
    excluded = set(_schedule_target_ids(fx.schedule))

    def _pick() -> Dict:
        while True:
            p = fx.payments[r.below(N_RECORDS)]
            if p["status"] != "refunded" and p["id"] not in excluded:
                excluded.add(p["id"])
                return {"payment_id": p["id"],
                        "to_status": r.choice(
                            [s for s in schedule_sb7.MUTABLE_STATUSES
                             if s != p["status"]])}

    d1 = _pick()
    burst = [_pick() for _ in range(BURST_COUNT)]
    return d1, burst


def _probe_expect(fx: "Fixture") -> Dict:
    """The JSON pack the harness hands the browser probe (SB7_EXPECT_FILE): every §3 number
    the probe compares against, derived here and nowhere else."""
    mc = fx.schedule.midwalk_create
    return {
        "seed": fx.seed, "n": N_RECORDS, "page_size": PAGE_SIZE, "pages": PAGE_COUNT,
        "window": [fx.days[0].isoformat(), fx.days[-1].isoformat()],
        "dst_day": fx.days[fx.dst_index].isoformat(),
        "layout": dict(fx.LAYOUT),
        "digest": dict(fx.DIGEST),
        "digest_post_midwalk": fx.expected_scene_digest(
            [{"value_slot": 0, "amount_minor": mc["amount_minor"],
              "currency": mc["currency"]}]),
        "label_candidates": [dict(c) for c in fx.LABEL_CANDIDATES],
        "height_cases": [dict(c) for c in fx.HEIGHT_CASES],
        "read_targets": list(fx.READ_TARGETS),
        "d1_target": dict(fx.d1_target),
        "burst_count": BURST_COUNT,
        "tokens": dict(fx.tokens),
    }


_CACHE: Dict[str, Fixture] = {}


def build(seed: str = DEFAULT_SEED) -> Fixture:
    if seed in _CACHE:
        return _CACHE[seed]
    fx = Fixture(seed=seed)
    fx.first_day, fx.days, fx.dst_index = choose_window(seed)
    fx.day_counts = day_counts(seed)
    totals = status_totals(seed)

    statuses: List[str] = []
    for s in STATUS_ORDER:
        statuses += [s] * totals[s]
    Rand(seed, "statusshuffle").shuffle(statuses)

    rcur = Rand(seed, "currency")
    ramt = Rand(seed, "amount")
    roff = Rand(seed, "offset")
    rsec = Rand(seed, "second")
    rows: List[Dict] = []
    used_minutes: List[set] = []
    i = 0
    for d, day in enumerate(fx.days):
        start_wall = datetime(day.year, day.month, day.day, tzinfo=BERLIN)
        nxt = day + timedelta(days=1)
        end_wall = datetime(nxt.year, nxt.month, nxt.day, tzinfo=BERLIN)
        start_utc = start_wall.astimezone(timezone.utc)
        duration_min = int((end_wall.astimezone(timezone.utc)
                            - start_utc).total_seconds()) // 60
        rmin = Rand(seed, f"minutes:{d}")
        minutes = rmin.sample_sorted_distinct(0, duration_min - 1, fx.day_counts[d])
        used_minutes.append(set(minutes))
        for m in minutes:
            instant = start_utc + timedelta(minutes=m, seconds=rsec.below(60))
            currency = CURRENCIES[rcur.below(4)]
            exp = CURRENCY_EXPONENTS[currency]
            dec = ramt.below(4)
            v100 = (ramt.rng(100, 299) * 1000) if dec == 3 else ramt.rng(100, 999) * 10 ** dec
            amount_minor = max(1, (v100 * 10 ** exp) // 100)
            rows.append({
                "id": f"pay_{i:05d}",
                "amount_minor": amount_minor,
                "currency": currency,
                "created_at": _rfc3339_with_offset(instant.replace(tzinfo=None),
                                                   _OFFSETS[roff.below(len(_OFFSETS))]),
                "status": statuses[i],
                "version": 1,
                "note": "",
                "counterparty": {"name": _NAMES[i % len(_NAMES)],
                                 "country": _COUNTRIES[i % len(_COUNTRIES)]},
                "_instant": instant.isoformat().replace("+00:00", "Z"),
                "_day": day.isoformat(),
                "_day_index": d,
            })
            i += 1
    assert i == N_RECORDS

    # Elite plant (§3.5 fixture guarantee): 14 rows on 14 distinct seeded days take the 14
    # largest amounts — strictly decreasing with gaps > jitter, so the top-12 label
    # candidates have distinct amounts and land on >= 6 distinct days by construction.
    relite = Rand(seed, "elite")
    day_starts = []
    acc = 0
    for c in fx.day_counts:
        day_starts.append(acc)
        acc += c
    elite_days = relite.sample_sorted_distinct(0, SPAN_DAYS - 1, _ELITE_COUNT)
    elite_rows = []
    for k, d in enumerate(elite_days):
        ri = day_starts[d] + relite.below(fx.day_counts[d])
        elite_rows.append(ri)
        row = rows[ri]
        amaj = 9000 - 260 * k + relite.below(100)
        row["amount_minor"] = amaj * 10 ** CURRENCY_EXPONENTS[row["currency"]]
    # amount-span floor plant: one non-elite row at exactly 1.00 major units
    while True:
        fi = relite.below(N_RECORDS)
        if fi not in elite_rows:
            rows[fi]["amount_minor"] = 10 ** CURRENCY_EXPONENTS[rows[fi]["currency"]]
            break

    fx.payments = rows
    fx.delivery_stride = _STRIDES[Rand(seed, "stride").below(len(_STRIDES))]

    rrev = Rand(seed, "reversal")
    k = 0
    for p in rows:
        if p["status"] != "refunded":
            continue
        inst = datetime.fromisoformat(p["_instant"].replace("Z", "+00:00"))
        rev_at = inst + timedelta(minutes=1 + rrev.below(1440))
        fx.reversals.append({
            "id": f"rev_{k:05d}", "payment_id": p["id"],
            "amount_minor": p["amount_minor"], "currency": p["currency"],
            "created_at": rev_at.strftime("%Y-%m-%dT%H:%M:%SZ"),
        })
        k += 1

    # value-slot queue (§2.3): 8 distinct in-span days, load-time count <= R0 - 2, on an
    # unused minute — the layout basis (d0, D0, R0) provably never moves for a create.
    r0 = max(fx.day_counts)
    rslot = Rand(seed, "valueslots")
    eligible = [d for d in range(SPAN_DAYS) if fx.day_counts[d] <= r0 - 2]
    assert len(eligible) >= schedule_sb7.VALUE_SLOTS, "value-slot headroom exhausted"
    slot_days: List[int] = []
    while len(slot_days) < schedule_sb7.VALUE_SLOTS:
        d = rslot.choice(eligible)
        if d not in slot_days:
            slot_days.append(d)
    for d in slot_days:
        day = fx.days[d]
        start_utc = datetime(day.year, day.month, day.day,
                             tzinfo=BERLIN).astimezone(timezone.utc)
        nxt = day + timedelta(days=1)
        duration_min = int((datetime(nxt.year, nxt.month, nxt.day, tzinfo=BERLIN)
                            .astimezone(timezone.utc) - start_utc).total_seconds()) // 60
        while True:
            m = rslot.below(duration_min)
            if m not in used_minutes[d]:
                used_minutes[d].add(m)
                break
        instant = start_utc + timedelta(minutes=m, seconds=rslot.below(60))
        fx.value_slots.append({
            "day": day.isoformat(), "day_index": d,
            "created_at": _rfc3339_with_offset(instant.replace(tzinfo=None),
                                               _OFFSETS[rslot.below(len(_OFFSETS))]),
            "_instant": instant.isoformat().replace("+00:00", "Z"),
        })

    sch = schedule_sb7.derive(seed)
    fx.schedule = schedule_sb7.resolve(seed, sch, fx.fresh_delivery_order(), CURRENCIES)
    fx.tokens = dict(sch.tokens)

    # ── expectation pack ─────────────────────────────────────────────────────────────────────
    fx.EXPECTED_TOTAL = N_RECORDS
    fx.EXPECTED_BUCKETS = compute_buckets(rows)
    fx.EXPECTED_BY_CURRENCY = {
        cur: {"count": sum(1 for p in rows if p["currency"] == cur),
              "total_minor": sum(p["amount_minor"] for p in rows if p["currency"] == cur)}
        for cur in CURRENCIES}
    fx.EXPECTED_BY_STATUS = {s: totals[s] for s in STATUS_ORDER}
    fx.EXPECTED_REVERSALS_BY_CURRENCY = {
        cur: {"count": sum(1 for r_ in fx.reversals if r_["currency"] == cur),
              "total_minor": sum(r_["amount_minor"] for r_ in fx.reversals
                                 if r_["currency"] == cur)}
        for cur in CURRENCIES
        if any(r_["currency"] == cur for r_ in fx.reversals)}
    fx.CROSS_CURRENCY_SUM_FORBIDDEN = sum(v["total_minor"]
                                          for v in fx.EXPECTED_BY_CURRENCY.values())
    fx.EXPECTED_LEDGER_FINAL = _expected_ledger_final(fx)
    fx.EXPECTED_WEBHOOK_COUNTERS = expected_webhook_counters(fx)
    fx.EXPECTED_WORKFLOW_STATES = {ref: ("sent" if outcome == "approve" else "rejected")
                                   for ref, outcome in DRAFT_OUTCOMES.items()}
    fx.EXPECTED_NOTIFICATION_MULTISET = _expected_notifications(fx)
    fx.OPTIMAL_REQUESTS = simulate_optimal_walk(fx.schedule)
    fx.label_candidate_ids = [c["id"] for c in fx.label_candidates()]

    fx.N, fx.PAGE_SIZE, fx.PAGES = N_RECORDS, PAGE_SIZE, PAGE_COUNT
    fx.LAYOUT = fx.layout_basis()
    fx.DIGEST = fx.expected_scene_digest()
    fx.LABEL_CANDIDATES = fx.label_candidates()
    fx.HEIGHT_CASES = _height_cases(fx)
    fx.READ_TARGETS = _read_targets(fx)
    fx.d1_target, fx.burst_targets = _post_schedule_targets(fx)
    fx.PROBE_EXPECT = _probe_expect(fx)

    _assert_battery(fx)
    _CACHE[seed] = fx
    return fx


def derive_tokens(seed: str) -> Dict[str, str]:
    return {role: schedule_sb7.hex_token(seed, role) for role in ("maker", "checker", "admin")}


# ── the per-seed import-time proof battery (§5.5: the traps are live, or refuse) ─────────────

def _assert_battery(fx: Fixture) -> None:
    rows = fx.payments
    assert len(rows) == N_RECORDS == PAGE_COUNT * PAGE_SIZE
    assert N_RECORDS % PAGE_SIZE == 0 and PAGE_COUNT == 192
    instants = [p["_instant"] for p in rows]
    assert instants == sorted(instants), "chronological ids broken"
    assert len(set(instants)) == N_RECORDS, "instants must be distinct"
    assert [p["id"] for p in rows] == sorted(p["id"] for p in rows)

    assert len(fx.days) == SPAN_DAYS and len(fx.day_counts) == SPAN_DAYS
    assert sum(fx.day_counts) == N_RECORDS
    assert all(1 <= c <= PER_DAY_CAP for c in fx.day_counts)
    offsets = [BERLIN.utcoffset(datetime(d.year, d.month, d.day, 12)) for d in fx.days]
    transitions = [k for k in range(1, SPAN_DAYS) if offsets[k] != offsets[k - 1]]
    assert transitions == [fx.dst_index], f"want exactly one DST transition, got {transitions}"
    assert DST_EDGE_MARGIN <= fx.dst_index <= SPAN_DAYS - 1 - DST_EDGE_MARGIN
    assert fx.days[fx.dst_index] in DST_MENU
    for p in rows:
        assert berlin_day(p["_instant"]) == p["_day"], "server-day vs instant drift"

    floor = math.ceil(STATUS_MIN_FRACTION * N_RECORDS)
    assert all(v >= floor for v in fx.EXPECTED_BY_STATUS.values())
    assert sum(fx.EXPECTED_BY_STATUS.values()) == N_RECORDS
    assert all(v["count"] > 0 for v in fx.EXPECTED_BY_CURRENCY.values())
    assert sum(v["count"] for v in fx.EXPECTED_BY_CURRENCY.values()) == N_RECORDS

    assert len(fx.EXPECTED_BUCKETS["days"]) == SPAN_DAYS
    assert len(fx.EXPECTED_BUCKETS["cells"]) == SPAN_DAYS * len(STATUS_ORDER)
    assert sum(c["count"] for c in fx.EXPECTED_BUCKETS["cells"]) == N_RECORDS
    utc_cells = {(c["day"], c["status"]): c["count"]
                 for c in compute_buckets(rows, tz=timezone.utc, tz_name="UTC")["cells"]}
    berlin_cells = {(c["day"], c["status"]): c["count"]
                    for c in fx.EXPECTED_BUCKETS["cells"]}
    differing = sum(1 for kk, v in berlin_cells.items() if utc_cells.get(kk, 0) != v)
    assert differing >= 8, f"UTC bucketing differs on only {differing} cells — trap is dead"

    majors = [a_major(p["amount_minor"], p["currency"]) for p in rows]
    assert max(majors) / min(majors) >= 1000, "amounts must span >= 3 decades in major units"
    cands = fx.label_candidates()
    assert len(cands) == LABEL_CANDIDATE_COUNT
    cand_majors = [a_major(c["amount_minor"], c["currency"]) for c in cands]
    assert len(set(cand_majors)) == LABEL_CANDIDATE_COUNT, \
        "label candidate amounts must be distinct"
    assert len({c["day"] for c in cands}) >= 6, "label candidates need >= 6 distinct days"

    refunded = [p for p in rows if p["status"] == "refunded"]
    assert len(fx.reversals) == len(refunded)
    for cur, agg in fx.EXPECTED_REVERSALS_BY_CURRENCY.items():
        want = sum(p["amount_minor"] for p in refunded if p["currency"] == cur)
        assert agg["total_minor"] == want, "M2 boot conservation broken in the fixture"

    r0 = max(fx.day_counts)
    slot_days = [s["day_index"] for s in fx.value_slots]
    assert len(fx.value_slots) == schedule_sb7.VALUE_SLOTS
    assert len(set(slot_days)) == schedule_sb7.VALUE_SLOTS, "value slots must use distinct days"
    for s in fx.value_slots:
        assert fx.day_counts[s["day_index"]] <= r0 - 2, "value slot lacks R0-2 headroom"
        assert fx.days[0].isoformat() <= s["day"] <= fx.days[-1].isoformat()
        assert berlin_day(s["_instant"]) == s["day"]

    sch = fx.schedule
    pages = sorted(sch.race_pages + [sch.drop_after_page, sch.http500_page, sch.refund_page])
    assert all(PAGE_BAND[0] <= p <= PAGE_BAND[1] for p in pages)
    assert all(b - a >= 2 for a, b in zip(pages, pages[1:])), "scheduled pages too close"
    assert sorted(sch.race_order.values()) == ["A", "A", "B", "B"]
    for k in sch.race_pages:
        muts = sch.race_mutations[k]
        assert len(muts) == 4 and sum(1 for m in muts if m["served"]) == 2
    all_targets = ([m["payment_id"] for k in sch.race_pages for m in sch.race_mutations[k]]
                   + [sch.refund_txn["payment_id"], sch.ooo_pair["payment_id"],
                      sch.forged_event["payment_id"]]
                   + [c["payment_id"] for c in sch.partition_commits
                      if c["kind"] == "vendor_flip"])
    assert len(all_targets) == len(set(all_targets)), "schedule targets must be disjoint"
    assert len(sch.partition_commits) == PARTITION_COMMITS_K
    idx = fx.index()
    for m in [m for k in sch.race_pages for m in sch.race_mutations[k]] + \
             [c for c in sch.partition_commits if c["kind"] == "vendor_flip"]:
        if m.get("kind") == "note":
            continue
        assert idx[m["payment_id"]]["status"] != "refunded"
        assert m["to_status"] != "refunded", "only the refund txn may produce refunded (M2)"
    assert idx[sch.refund_txn["payment_id"]]["status"] != "refunded"

    assert fx.OPTIMAL_REQUESTS == PAGE_COUNT + 2, "walk simulation degenerated"
    quad = fx.EXPECTED_WEBHOOK_COUNTERS
    assert quad["received"] == quad["applied"] + quad["ignored"] + quad["rejected"]
    assert quad["rejected"] == 1 and quad["ignored"] == 2
    digest_a = fx.expected_scene_digest()
    digest_b = fx.expected_scene_digest()
    assert digest_a == digest_b and digest_a["count"] == N_RECORDS
    assert set(fx.tokens) == {"maker", "checker", "admin"}
    assert all(len(t) == 32 for t in fx.tokens.values())
    notif = fx.EXPECTED_NOTIFICATION_MULTISET
    assert sum(notif.values()) == 7 and set(notif) == set(NOTIFYING_EVENT_TYPES)

    assert fx.N == N_RECORDS and fx.PAGE_SIZE == PAGE_SIZE and fx.PAGES == PAGE_COUNT == 192
    assert fx.LAYOUT == fx.layout_basis() and fx.DIGEST == digest_a
    assert len(fx.HEIGHT_CASES) == HEIGHT_CASE_COUNT
    assert len({c["id"] for c in fx.HEIGHT_CASES}) == HEIGHT_CASE_COUNT
    hc_currencies = {c["currency"] for c in fx.HEIGHT_CASES}
    assert "JPY" in hc_currencies and "KWD" in hc_currencies, "height cases need JPY + KWD"
    for c in fx.HEIGHT_CASES:
        assert fx.payments[c["n"]]["id"] == c["id"], "height case n/id drift"
        assert c["h"] == round(height_for(c["amount_minor"], c["currency"]), 4)
    assert len(fx.READ_TARGETS) == READ_TARGET_COUNT == len(set(fx.READ_TARGETS))
    race_ids = {m["payment_id"] for k in sch.race_pages for m in sch.race_mutations[k]}
    assert set(fx.READ_TARGETS) <= race_ids, "read targets must ride the racing window"
    sched_ids = _schedule_target_ids(sch)
    post_ids = {fx.d1_target["payment_id"]} | {b["payment_id"] for b in fx.burst_targets}
    assert len(post_ids) == 1 + BURST_COUNT and not (post_ids & sched_ids), \
        "post-schedule targets must be disjoint from the schedule's"
    for t in [fx.d1_target] + fx.burst_targets:
        row = idx[t["payment_id"]]
        assert row["status"] != "refunded" and t["to_status"] != "refunded"
        assert t["to_status"] != row["status"]
    assert sch.refund_txn["reversal_id"].startswith("rev_txn_")
    assert sch.refund_txn["amount_minor"] == idx[sch.refund_txn["payment_id"]]["amount_minor"]
    json.dumps(fx.PROBE_EXPECT)   # the probe pack must survive the JSON boundary


if __name__ == "__main__":
    import json
    import time
    t0 = time.time()
    fx = build(DEFAULT_SEED)
    print(f"built seed {DEFAULT_SEED} in {time.time() - t0:.2f}s — battery PASS")
    print(json.dumps({
        "window": [fx.days[0].isoformat(), fx.days[-1].isoformat()],
        "dst_day": fx.days[fx.dst_index].isoformat(),
        "by_status": fx.EXPECTED_BY_STATUS,
        "by_currency": fx.EXPECTED_BY_CURRENCY,
        "reversals": len(fx.reversals),
        "optimal_requests": fx.OPTIMAL_REQUESTS,
        "webhook_counters": fx.EXPECTED_WEBHOOK_COUNTERS,
        "scene_digest": fx.expected_scene_digest(),
        "race_pages": fx.schedule.race_pages,
        "race_order": fx.schedule.race_order,
        "j_drop": fx.schedule.drop_after_page, "j2_500": fx.schedule.http500_page,
        "refund_page": fx.schedule.refund_page,
        "tokens": fx.tokens,
    }, indent=2, default=str))
