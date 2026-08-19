"""sb-7 determinism spine (DESIGN §4.1) — the ONE module every scheduled thing derives from.

`derive(seed)` is the single derivation function; the vendor mock, the harness driver, and
the scorer all import it from here — never three copies. Cardinalities are FROZEN (fraction
denominators must not vary by seed, red-team F4a); only placements, targets, orders, and
values are seeded. Randomness is D(label, i) = HMAC_SHA256(seed, f"{label}:{i}") consumed as
4-byte words with rejection sampling (no modulo bias), so every consumer that replays the
same label stream gets the same choices.

Two-phase on purpose: `derive(seed)` is pure in the seed (pages, orders, bands, tokens);
target RESOLUTION (which payment ids, which value dates) needs the built collection, so
`resolve(seed, schedule, payments_delivery_order, ...)` lives here too and is called by
`fixtures_v3.build(seed)` — one derivation chain, one module, zero twins.

[U]/[A] classification per red-team R1 is carried on the schedule itself (`CLASS`):
[U] = unconditionally firable (vendor-side commits/arms, driver kills by trace-tail or
state-poll) — unfired [U] is a structural refusal, a harness failure, never an app zero.
[A] = app-dependent (needs the walk to reach a page, a conditional request, sync #2 to
start) — unfired [A] makes the driver log `sched-unreached` and zeroes exactly the
dependent rungs.
"""

from __future__ import annotations

import hashlib
import hmac
from dataclasses import dataclass, field
from typing import Dict, List, Optional

# ── frozen cardinalities and bands (DESIGN §2.3 / §4.1 — binding) ────────────────────────────
N_RECORDS = 12_288                 # frozen; every "full count" budget means this number
PAGE_SIZE = 64                     # server-fixed; v3 has no limit param (red-team R3)
PAGE_COUNT = N_RECORDS // PAGE_SIZE            # 192 — a fixture constant both sides compute
PAGE_BAND = (2, 191)               # every scheduled page lives in [2, 191]
RACE_PAGE_COUNT = 4                # k1 < k2 < k3 < k4
RACE_MUTATIONS_PER_PAGE = 4        # 2 already-served + 2 not-yet-served targets
RACE_ORDERS = ("A", "A", "B", "B")             # 2 webhook-first + 2 page-first, seeded placing
PARTITION_COMMITS_K = 8            # frozen count; seeded contents; >= 3 outbox-crossing
PARTITION_VENDOR_FLIPS = 5         # the non-crossing 5 of the 8 (3 crossing ride workflow F3)
SIGKILL_AFTER_LIST_BAND = (2, 5)   # n-th list response of sync #2
VENDOR_DOWN_BOOT_BAND = (3, 8)     # w seconds
STALE_304_SYNC_CHOICES = (3, 4)    # s: the (s-2)-th sync that PRESENTS a stale conditional
                                   # gets the armed lying 304 (ordinals 3/4 map to the 1st/2nd
                                   # stale-presenting sync — staleness, not wall order, arms it)
PARTITION_AFTER_EVENT_BAND = (2, 64)  # ledger-seq threshold X; any durable outbox-crossing
                                      # delivery satisfies it (seqs run ~N), and kill-placement
                                      # honesty (§4.1) makes grading placement-independent
DRAFT_FIXTURES = ("F1", "F2", "F3")            # UI-approve / UI-reject / API+kills
VALUE_SLOTS = 8                    # seeded in-span value dates for created payments (§2.3);
                                   # slot 0 is the mid-walk create, 1.. serve app creates
TOKEN_HEX_LEN = 32
MUTABLE_STATUSES = ("settled", "pending", "failed")   # race/partition flips never touch
                                                      # "refunded" — only the refund txn
                                                      # does, paired with its reversal (M2)

# ooo_pair/dup_event/forged_event/midwalk_create ride seeded race pages of sync #1 — their
# firing needs the app's walk to reach those pages, so they are [A], not [U] (an app that
# never syncs must zero the dependent rungs, never convert into a structural refusal).
CLASS: Dict[str, str] = {
    "race_pages": "A", "race_mutations": "A", "race_order": "A", "refund_txn": "A",
    "ooo_pair": "A", "dup_event": "A", "forged_event": "A", "midwalk_create": "A",
    "drop_after_page": "A", "http500_page": "A", "sigkill_after_list": "A",
    "vendor_down_boot_secs": "U", "stale_304_sync": "A", "partition_after_event": "U",
    "partition_commits": "U", "approval_fixture": "U", "tokens": "U",
}


class Rand:
    """Deterministic word stream: block j = HMAC_SHA256(seed, f"{label}:{j}")."""

    def __init__(self, seed: str, label: str):
        self._key = seed.encode()
        self._label = label
        self._block = 0
        self._buf = b""

    def _refill(self) -> None:
        msg = f"{self._label}:{self._block}".encode()
        self._buf += hmac.new(self._key, msg, hashlib.sha256).digest()
        self._block += 1

    def u32(self) -> int:
        while len(self._buf) < 4:
            self._refill()
        word, self._buf = self._buf[:4], self._buf[4:]
        return int.from_bytes(word, "big")

    def below(self, n: int) -> int:
        assert n > 0
        limit = (1 << 32) - ((1 << 32) % n)
        while True:
            w = self.u32()
            if w < limit:
                return w % n

    def rng(self, lo: int, hi: int) -> int:
        """Inclusive band [lo, hi]."""
        return lo + self.below(hi - lo + 1)

    def choice(self, seq):
        return seq[self.below(len(seq))]

    def shuffle(self, items: list) -> list:
        for i in range(len(items) - 1, 0, -1):
            j = self.below(i + 1)
            items[i], items[j] = items[j], items[i]
        return items

    def sample_sorted_distinct(self, lo: int, hi: int, k: int) -> List[int]:
        """k distinct sorted ints from [lo, hi], rejection-free via partial pool walk."""
        assert hi - lo + 1 >= k
        picked: set = set()
        while len(picked) < k:
            picked.add(self.rng(lo, hi))
        return sorted(picked)


def hex_token(seed: str, label: str) -> str:
    return hmac.new(seed.encode(), f"token:{label}".encode(),
                    hashlib.sha256).hexdigest()[:TOKEN_HEX_LEN]


@dataclass
class Schedule:
    seed: str
    race_pages: List[int] = field(default_factory=list)           # k1 < k2 < k3 < k4
    race_order: Dict[int, str] = field(default_factory=dict)      # page -> "A" | "B"
    race_mutation_kinds: Dict[int, List[str]] = field(default_factory=dict)
    drop_after_page: int = 0                                      # j
    http500_page: int = 0                                         # j2
    refund_page: int = 0                                          # refund txn trigger page
    extras_page: Dict[str, int] = field(default_factory=dict)     # ooo/dup/forged -> race page
    dup_source_index: int = 0                                     # which k1 event is redelivered
    sigkill_after_list: int = 0                                   # n, sync #2
    vendor_down_boot_secs: int = 0                                # w
    stale_304_sync: int = 0                                       # s in {3, 4}
    partition_after_event: int = 0                                # X
    tokens: Dict[str, str] = field(default_factory=dict)
    # resolved by resolve() against the built collection:
    race_mutations: Dict[int, List[Dict]] = field(default_factory=dict)
    refund_txn: Dict = field(default_factory=dict)
    ooo_pair: Dict = field(default_factory=dict)
    forged_event: Dict = field(default_factory=dict)
    midwalk_create: Dict = field(default_factory=dict)
    partition_commits: List[Dict] = field(default_factory=list)
    approval_fixture: List[Dict] = field(default_factory=list)
    resolved: bool = False

    def classes(self) -> Dict[str, str]:
        return dict(CLASS)


def derive(seed: str) -> Schedule:
    """The pure-seed half: pages, orders, bands, tokens. Frozen cardinalities throughout."""
    assert isinstance(seed, str) and len(seed) == 16 and all(
        c in "0123456789abcdef" for c in seed.lower()), f"seed must be 16 hex chars: {seed!r}"
    sch = Schedule(seed=seed)

    # 7 scheduled pages, pairwise >= 2 apart inside PAGE_BAND: sample 7 sorted distinct from
    # a compressed band, then re-inflate (+i per index) so consecutive gaps are >= 2.
    r = Rand(seed, "pages")
    base = r.sample_sorted_distinct(PAGE_BAND[0], PAGE_BAND[1] - 6, 7)
    pages = [v + i for i, v in enumerate(base)]
    assert all(b - a >= 2 for a, b in zip(pages, pages[1:]))
    roles = ["race", "race", "race", "race", "j", "j2", "refund"]
    Rand(seed, "page-roles").shuffle(roles)
    for page, role in zip(pages, roles):
        if role == "race":
            sch.race_pages.append(page)
        elif role == "j":
            sch.drop_after_page = page
        elif role == "j2":
            sch.http500_page = page
        else:
            sch.refund_page = page
    sch.race_pages.sort()

    orders = list(RACE_ORDERS)
    Rand(seed, "race-order").shuffle(orders)
    sch.race_order = dict(zip(sch.race_pages, orders))
    assert sorted(sch.race_order.values()) == ["A", "A", "B", "B"]

    rk = Rand(seed, "race-kinds")
    sch.race_mutation_kinds = {
        k: [("status" if rk.below(2) == 0 else "note")
            for _ in range(RACE_MUTATIONS_PER_PAGE)]
        for k in sch.race_pages
    }

    re_ = Rand(seed, "extras")
    sch.extras_page = {
        "ooo": re_.choice(sch.race_pages),
        "dup": re_.choice(sch.race_pages[1:]),   # needs an earlier delivery to duplicate
        "forged": re_.choice(sch.race_pages),
    }
    sch.dup_source_index = re_.below(RACE_MUTATIONS_PER_PAGE)

    rb = Rand(seed, "bands")
    sch.sigkill_after_list = rb.rng(*SIGKILL_AFTER_LIST_BAND)
    sch.vendor_down_boot_secs = rb.rng(*VENDOR_DOWN_BOOT_BAND)
    sch.stale_304_sync = rb.choice(STALE_304_SYNC_CHOICES)
    sch.partition_after_event = rb.rng(*PARTITION_AFTER_EVENT_BAND)

    sch.tokens = {role: hex_token(seed, role) for role in ("maker", "checker", "admin")}
    assert len(set(sch.tokens.values())) == 3
    return sch


def _pick_target(r: Rand, pool: List[int], used: set) -> int:
    """Seeded pick from an eligible delivery-index pool, never reusing a target."""
    for _ in range(10_000):
        idx = r.choice(pool)
        if idx not in used:
            used.add(idx)
            return idx
    raise AssertionError("target pool exhausted — fixture too small for the schedule")


def resolve(seed: str, sch: Schedule, delivery: List[Dict],
            currencies: List[str]) -> Schedule:
    """Resolve seeded targets against the built collection (delivery-order rows).

    Mutation targets are globally DISJOINT (no exercise invalidates another's expected
    versions — sb-6 house rule) and status flips never touch or produce "refunded" outside
    the refund txn, so M2 pair-conservation stays exact at every observable instant.
    """
    assert not sch.resolved
    used: set = set()
    rt = Rand(seed, "targets")

    for k in sch.race_pages:
        served_pool = list(range(0, (k - 1) * PAGE_SIZE))
        unserved_pool = list(range(k * PAGE_SIZE, len(delivery)))
        muts: List[Dict] = []
        for slot, kind in enumerate(sch.race_mutation_kinds[k]):
            pool = served_pool if slot < 2 else unserved_pool
            if kind == "status":
                pool = [i for i in pool if delivery[i]["status"] != "refunded"]
            idx = _pick_target(rt, pool, used)
            row = delivery[idx]
            mut = {"payment_id": row["id"], "delivery_index": idx, "kind": kind,
                   "served": slot < 2}
            if kind == "status":
                mut["to_status"] = rt.choice(
                    [s for s in MUTABLE_STATUSES if s != row["status"]])
            else:
                mut["note"] = f"race-{k}-{slot}"
            muts.append(mut)
        sch.race_mutations[k] = muts

    refund_pool = [i for i in range(0, (sch.refund_page - 1) * PAGE_SIZE)
                   if delivery[i]["status"] != "refunded"]
    ridx = _pick_target(rt, refund_pool, used)
    txn_id = f"txn_{Rand(seed, 'txn').below(9000) + 1000}"
    sch.refund_txn = {"payment_id": delivery[ridx]["id"], "delivery_index": ridx,
                      "amount_minor": delivery[ridx]["amount_minor"],
                      "currency": delivery[ridx]["currency"],
                      "txn_id": txn_id, "reversal_id": f"rev_{txn_id}"}

    oidx = _pick_target(rt, list(range(len(delivery))), used)
    sch.ooo_pair = {"payment_id": delivery[oidx]["id"], "delivery_index": oidx,
                    "notes": ["ooo-1", "ooo-2"], "page": sch.extras_page["ooo"]}

    fidx = _pick_target(rt, list(range(len(delivery))), used)
    sch.forged_event = {"payment_id": delivery[fidx]["id"], "delivery_index": fidx,
                        "note": "forged-x", "page": sch.extras_page["forged"]}

    rm = Rand(seed, "midwalk")
    exp_pool = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
    cur = rm.choice(currencies)
    major = 10 + rm.below(2_990)
    sch.midwalk_create = {
        "currency": cur,
        "amount_minor": major * (10 ** exp_pool[cur]) + (rm.below(10 ** exp_pool[cur])
                                                         if exp_pool[cur] else 0),
        "counterparty": {"name": "Midwalk Systems", "country": "DE"},
        "note": "midwalk", "value_slot": 0,
    }

    rp = Rand(seed, "partition")
    flip_pool = [i for i in range(len(delivery)) if delivery[i]["status"] != "refunded"]
    commits: List[Dict] = []
    for _ in range(PARTITION_VENDOR_FLIPS):
        idx = _pick_target(rp, flip_pool, used)
        row = delivery[idx]
        commits.append({"kind": "vendor_flip", "payment_id": row["id"],
                        "delivery_index": idx,
                        "to_status": rp.choice(
                            [s for s in MUTABLE_STATUSES if s != row["status"]])})
    for op in ("submit", "approve", "send"):
        commits.append({"kind": "workflow", "ref": "F3", "op": op, "outbox_crossing": True})
    assert len(commits) == PARTITION_COMMITS_K
    assert sum(1 for c in commits if c.get("outbox_crossing")) >= 3
    sch.partition_commits = commits

    ra = Rand(seed, "approval")
    for i, ref in enumerate(DRAFT_FIXTURES):
        cur = ra.choice(currencies)
        major = 10 + ra.below(490)
        sch.approval_fixture.append({
            "ref": ref, "value_slot": i + 1,
            "amount_minor": major * (10 ** exp_pool[cur]) + (ra.below(10 ** exp_pool[cur])
                                                             if exp_pool[cur] else 0),
            "currency": cur,
            "counterparty": {"name": f"Draft Counterparty {ref}", "country": "DE"},
            "note": f"draft-{ref}",
            "kills": {"A1": "after_submit_200_inside_partition",
                      "A2": "after_approve_200_inside_send_window"} if ref == "F3" else {},
        })

    sch.resolved = True
    return sch
