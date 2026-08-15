"""Grade a built `vendorsync` tree. GRADED checks, four tiers, no cascading.

Two design rules drive everything here.

**Checks are graded, not binary.** "Kinda works" must be easy to reach; the distance from there to
excellent is what the instrument has to resolve. A binary sheet cannot express that — it collapses
"returned 3 of 47 payments" and "returned 46 of 47" into the same MISS. Every check therefore returns
a score in [0,1] and says what it measured.

**100 is not meant to be reachable.** A perfect score means the task has stopped measuring, so
Tier D grades operational finesse against an optimum that is deliberately demanding — request
efficiency against the theoretical minimum, conditional-refetch ratio, concurrency safety, UI polish.
Even a strong build should leave headroom. If something ever does reach 100, that is the signal to
deepen the task, not to celebrate.

Tiers, and what each is for:
  A  STRUCTURE + RUNTIME   the floor — a competent local 27b should reach most of this
  B  BEHAVIOUR             the documented contract of the app itself
  C  VENDOR CONTRACT       only reachable by reading the vendor docs; graded from the request trace
  D  FINESSE               efficiency, robustness, polish — where a good build separates from a great one
"""

from __future__ import annotations

import ast
import json
import os
import re
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path
from typing import Callable, Dict, List, Optional

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from probes import client_probe, vendor_trace  # noqa: E402

import vendor_service  # noqa: E402

# BENCH2 rank 1: the totals live in fixtures.py so probes/vendor_trace.py (which carried a
# stale 47) can never disagree with the scorer again. Imported by the same name ~10 checks use.
from fixtures import EXPECTED_SUM, EXPECTED_TOTAL  # noqa: E402

import perf_probe  # noqa: E402

PAYMENT_KEYS = {"id", "amount_minor", "currency", "created_at", "status"}
TIER_WEIGHT = {"A": 0.25, "B": 0.30, "C": 0.25, "D": 0.20}

# THE PRODUCT TIER (sb-5, Mihai 2026-08-15: "the benchmark should also review the end product…
# performs well, looks good"). Gated so the sb-4 path stays bit-identical: J (user journeys,
# browser truth), P (measured performance against SPEC-DOCUMENTED budgets), V (rendered visual
# quality). Visual outweighs perf deliberately: on localhost every stdlib server saturates the
# latency budgets, while the rendered page is where these builds actually fail as products.
PRODUCT = bool(os.environ.get("BENCH_PRODUCT"))
PRODUCT_WEIGHT = {"J": 0.15, "V": 0.10, "P": 0.05}
PRODUCT_CORE = 0.60  # core keeps the majority; hard-block keeps its 0.10

# Bump on ANY change to a check, a weight, or the fixture. A verdict carrying a different version is
# not comparable and the sweep will re-run it rather than reuse it. Without this, a stale verdict
# scored by an older, buggier grader sits silently in a table next to fresh ones — which is how a
# cheaper model appeared to beat a stronger one. The product gate IS a version change.
SCORER_VERSION = "sb-5" if PRODUCT else "sb-4"

# ── ROOT-CAUSE ATTRIBUTION ────────────────────────────────────────────────────────────────────
#
# WHY. Three cells of build 1786340680 reported Tier B = 0.3611 — "36% of twelve checks" — which
# reads as a broad weakness across twelve vendor-contract behaviours and sent a week of work at "the
# vendor contract" as though it were many things. It is not. Their own detail strings:
#
#     sync_completeness    0/247 payments after one sync
#     payment_row_shape    no rows
#     total_field          total=0
#     chronological_order  too few rows
#     summary_accuracy     total_minor=0 (want 4409197)
#     summary_bounds_utc   oldest=None newest=None
#     resync_idempotent    second sync inserted=0 total=0
#
# Six grammars for one sentence: the sync brought back nothing. Six of those checks CANNOT be
# evaluated at all without rows — they are not independent evidence, they are the same failure seen
# through six windows.
#
# ⚠️ THE SCORE IS DELIBERATELY UNCHANGED, and the first design here was wrong. Marking the blocked
# checks "unscored" was the obvious move and it is backwards: it lifts a totally broken app from
# 0.7226 to 0.8309 — an app whose headline feature returns zero rows would be rewarded for the
# checks it never got far enough to fail. "Absent input must score zero, never full marks" applies
# in both directions. The defect is real and its cost SHOULD be large; what was wrong was the
# REPORT, which implied breadth where there is depth. So this is attribution only — purely additive,
# no score, weight or detail string changes, and SCORER_VERSION deliberately NOT bumped.
#
# `local_pagination` is NOT here on purpose: it scores 0.33 rather than 0.00 on the same cells,
# because default/cap/offset semantics remain partly checkable against an empty collection. A
# partially-independent check does not belong in a blocked set.
ROOT_BLOCKS = {
    "sync_completeness": (
        "payment_row_shape", "total_field", "chronological_order",
        "summary_accuracy", "summary_bounds_utc", "resync_idempotent",
    ),
}


def attribute_root_causes(rows: List[Dict]) -> Dict[str, List[str]]:
    """Which zero checks are downstream of another zero check. Pure, so it is testable offline.

    A dependent is attributed ONLY when it scored 0 AND its prerequisite scored 0. A dependent that
    fails on its own while the prerequisite passed is a genuine independent defect and must keep its
    own name — attributing it would be the mirror of the error this function exists to correct.
    """
    by_name = {r["check"]: r for r in rows}
    out = {}
    for root, dependents in ROOT_BLOCKS.items():
        r = by_name.get(root)
        if r is None or r["score"] > 0:
            continue
        blocked = [d for d in dependents
                   if d in by_name and by_name[d]["score"] == 0]
        if blocked:
            out[root] = blocked
    return out


# ── http helpers ──────────────────────────────────────────────────────────────────────────────

def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _req(req, timeout: float):
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            try:
                return r.status, json.loads(raw), raw, dict(r.headers)
            except json.JSONDecodeError:
                return r.status, None, raw, dict(r.headers)
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw), raw, dict(e.headers)
        except json.JSONDecodeError:
            return e.code, None, raw, dict(e.headers)
    except Exception:
        return None, None, b"", {}


def _get(url: str, timeout: float = 10):
    return _req(urllib.request.Request(url), timeout)


def _post(url: str, timeout: float = 240):
    return _req(urllib.request.Request(url, data=b"", method="POST"), timeout)


def _product_probe(scenario: str, base: str, *flags: str) -> Dict:
    """Browser truth via product_probe.mjs (node + playwright). A returned dict carrying
    `_probe_error` is a HARNESS failure, not app evidence — the checks score it 0 with a loud
    detail, and the controls' HIGH gate catches any systemic probe breakage (the reference
    would zero too, and the grader refuses to be trusted)."""
    cmd = ["node", str(HERE / "product_probe.mjs"), scenario, base, *flags]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        return json.loads(r.stdout)
    except Exception as e:
        return {"_probe_error": f"{scenario}: {type(e).__name__}: {e}"[:300]}


def _ladder(value, budgets) -> float:
    """Quantized quarters against SPEC-DOCUMENTED budgets (never invented constants — the
    builder read the same numbers). Quantization keeps the determinism control honest:
    localhost margins are enormous, so a bucket boundary flip is vanishingly unlikely."""
    if value is None:
        return 0.0
    for score, bound in budgets:
        if value <= bound:
            return score
    return 0.0


def _classes(path: Path) -> Dict[str, List[str]]:
    out: Dict[str, List[str]] = {}
    try:
        tree = ast.parse(path.read_text(errors="replace"))
    except Exception:
        return out
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            out[node.name] = [n.name for n in node.body
                              if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
    return out


def _instant(value: str) -> Optional[float]:
    try:
        return datetime.fromisoformat(str(value).replace("Z", "+00:00")).timestamp()
    except Exception:
        return None


# ── context ───────────────────────────────────────────────────────────────────────────────────

class Ctx:
    def __init__(self, root: Path):
        self.root = root
        self.pkg = root / "vendorsync"
        self.html = ""
        self.health: Dict = {}
        self.payments: Dict = {}
        self.page5: Dict = {}
        self.capped: Dict = {}
        self.summary: Dict = {}
        self.sync1: Dict = {}
        self.sync2: Dict = {}
        self.client: Dict = {"results": {}, "trace": []}
        # BENCH3 (amend arms only): pre-fetched while the app lives — evaluate() runs after
        # the SIGKILL, so feature checks must read Ctx, never the wire.
        self.feat_by_status: Dict = {}
        self.feat_csv: bytes = b""
        self.sync3: Dict = {}
        self.update_changed: int = 0
        self.update_seen: int = 0
        self.restart_before: int = 0
        self.restart_after: int = -1
        self.val_matrix: List = []
        self.raw_pages: List = []
        self.json_probe: List = []
        self.health_post: Dict = {}
        self.concurrent_total: Optional[int] = None
        self.bad_limit: Optional[int] = None
        self.bad_offset: Optional[int] = None
        self.notfound: Optional[int] = None
        self.notfound_body: Dict = {}
        self.root_status: Optional[int] = None
        self.api_ctype = ""
        self.boot_error = ""
        self.trace: List[Dict] = []
        self.sync1_reqs = 0
        self.sync2_reqs = 0
        self.sync2_304 = 0
        self.max_limit_used = 0
        # PRODUCT tier (sb-5): browser scenarios + clock measurements, gathered app-alive.
        self.probe_load: Dict = {}
        self.probe_sync: Dict = {}
        self.probe_error: Dict = {}
        self.probe_empty: Dict = {}
        self.perf: Dict = {}


# ── check registry ────────────────────────────────────────────────────────────────────────────

CHECKS: List[tuple] = []

# BENCH3 (BENCH3-AMEND.md): the brownfield FEATURE half. Registered separately and evaluated
# ONLY when BENCH_AMEND is set at evaluate() time (env-at-import would miss the sweep's per-arm
# env), so greenfield scoring and the GRADER controls are byte-identical with the list present.
FEATURE_CHECKS: List[tuple] = []


def feature_check(name: str) -> Callable:
    def deco(fn):
        FEATURE_CHECKS.append((name, "F", fn))
        return fn

    return deco


def check(name: str, tier: str) -> Callable:
    def deco(fn):
        CHECKS.append((name, tier, fn))
        return fn
    return deco


# PRODUCT tier (sb-5): registered separately, evaluated only when BENCH_PRODUCT is set, same
# pattern as FEATURE_CHECKS so the sb-4 path stays byte-identical with the list present.
PRODUCT_CHECKS: List[tuple] = []


def product_check(name: str, tier: str) -> Callable:
    def deco(fn):
        PRODUCT_CHECKS.append((name, tier, fn))
        return fn
    return deco


def g(score: float, detail: str, consequence: str = "", parts: Dict = None) -> Dict:
    """`parts` is the PER-ITEM breakdown behind the aggregate in `detail`.

    Added because the aggregate alone made a whole class of question unanswerable. `modules_present`
    already computes exactly which of the five named files exist and then reports only "5/5 named
    files"; the breakdown was discarded one line later. That meant a file-level claim — e.g. "does a
    task_split child build its files worse than a normal task?" — could not be tested at all, and the
    only available substitute was a RUN-level score comparison, which answers a different question
    (and reversed under a corrected join when it was tried).

    PURELY ADDITIVE: no score, tier weight or `detail` string changes, so `SCORER_VERSION` is
    deliberately NOT bumped — a bump would force the entire archived corpus to be re-scored to
    acquire a field that changes no number. Absent when a check has no natural per-item split.
    """
    out = {"score": max(0.0, min(1.0, score)), "detail": detail, "consequence": consequence}
    if parts:
        out["parts"] = parts
    return out


def frac(have, want) -> float:
    want = set(want)
    return len(set(have) & want) / len(want) if want else 0.0


# ── A: structure and runtime (the floor) ──────────────────────────────────────────────────────

@check("modules_present", "A")
def _(c: Ctx):
    want = ["meridian.py", "store.py", "api.py", "__main__.py", "web/index.html"]
    have = [w for w in want if (c.pkg / w).is_file()]
    return g(len(have) / len(want), f"{len(have)}/{len(want)} named files",
             "files the spec names by path are missing",
             parts={w: (w in have) for w in want})


@check("interfaces_declared", "A")
def _(c: Ctx):
    client = set(_classes(c.pkg / "meridian.py").get("MeridianClient", []))
    store = set(_classes(c.pkg / "store.py").get("Store", []))
    want_c = {"fetch_all_payments", "total_count", "create_payment"}
    want_s = {"upsert_many", "all_payments", "count", "last_sync", "set_last_sync"}
    score = (len(client & want_c) + len(store & want_s)) / (len(want_c) + len(want_s))
    missing = sorted((want_c - client) | (want_s - store))
    return g(score, f"{int(score * 8)}/8 methods" + (f", missing {missing}" if missing else ""),
             "the named interface is not implemented as specified",
             parts={**{f"MeridianClient.{m}": (m in client) for m in sorted(want_c)},
                    **{f"Store.{m}": (m in store) for m in sorted(want_s)}})


@check("server_runs", "A")
def _(c: Ctx):
    return g(1.0 if c.health else 0.0,
             "healthy" if c.health else f"never bound ({c.boot_error[:100]})",
             "the tool does not run at all")


@check("serves_page", "A")
def _(c: Ctx):
    return g(1.0 if c.root_status == 200 and "<" in c.html else 0.0,
             f"GET / -> {c.root_status}", "the page is not served by the backend")


@check("health_shape", "A")
def _(c: Ctx):
    s = frac(c.health, {"status", "payments", "last_sync"})
    return g(s, f"{int(s * 3)}/3 documented keys", "monitoring cannot read the documented shape")


@check("sync_shape", "A")
def _(c: Ctx):
    s = frac(c.sync1, {"fetched", "inserted", "total"})
    return g(s, f"{int(s * 3)}/3 documented keys", "the caller cannot tell what a sync did")


# ── B: behaviour ──────────────────────────────────────────────────────────────────────────────

@check("sync_completeness", "B")
def _(c: Ctx):
    got = c.sync1.get("total") or c.sync1.get("fetched") or 0
    return g(min(got / EXPECTED_TOTAL, 1.0) if isinstance(got, int) else 0.0,
             f"{got}/{EXPECTED_TOTAL} payments after one sync",
             "an incomplete sync silently loses payments")


@check("resync_idempotent", "B")
def _(c: Ctx):
    if not c.sync2:
        return g(0.0, "second sync failed", "re-running the tool is not safe")
    inserted, total = c.sync2.get("inserted"), c.sync2.get("total")
    ok = inserted == 0 and total == EXPECTED_TOTAL
    near = total == EXPECTED_TOTAL
    return g(1.0 if ok else (0.5 if near else 0.0),
             f"second sync inserted={inserted} total={total}",
             "re-running duplicates rows and inflates the ledger")


@check("local_pagination", "B")
def _(c: Ctx):
    parts = [
        len(c.payments.get("data") or []) == 25,
        c.capped.get("limit") == 100 and len(c.capped.get("data") or []) <= 100,
        len(c.page5.get("data") or []) == 5,
    ]
    return g(sum(parts) / 3, f"default/cap/offset: {['no', 'yes'][parts[0]]}/"
             f"{['no', 'yes'][parts[1]]}/{['no', 'yes'][parts[2]]}",
             "the local API's own paging contract is not met")


@check("payment_row_shape", "B")
def _(c: Ctx):
    data = c.payments.get("data") or []
    if not data:
        return g(0.0, "no rows", "n/a")
    keys = set(data[0])
    s = frac(keys, PAYMENT_KEYS) - (0.2 if keys - PAYMENT_KEYS else 0.0)
    return g(s, f"{sorted(keys)}", "the documented row contract is not met")


@check("total_field", "B")
def _(c: Ctx):
    return g(1.0 if c.payments.get("total") == EXPECTED_TOTAL else 0.0,
             f"total={c.payments.get('total')}",
             "a wrong total breaks the caller's paging")


@check("chronological_order", "B")
def _(c: Ctx):
    data = c.payments.get("data") or []
    if len(data) < 2:
        return g(0.0, "too few rows", "n/a")
    times = [_instant(r.get("created_at")) for r in data]
    if any(t is None for t in times):
        return g(0.0, "unparseable created_at", "timestamps are not RFC3339")
    pairs = sum(1 for a, b in zip(times, times[1:]) if a <= b)
    return g(pairs / (len(times) - 1), f"{pairs}/{len(times) - 1} adjacent pairs ordered",
             "mixed offsets make string ordering wrong; payments show out of sequence")


@check("summary_accuracy", "B")
def _(c: Ctx):
    val = c.summary.get("total_minor")
    if not isinstance(val, int):
        return g(0.0, f"total_minor={val!r}", "the headline finance figure is missing or not an integer")
    err = abs(val - EXPECTED_SUM) / EXPECTED_SUM
    return g(1.0 if err == 0 else max(0.0, 1.0 - err * 10), f"total_minor={val} (want {EXPECTED_SUM})",
             "the headline figure the finance team reads is wrong")


@check("summary_bounds_utc", "B")
def _(c: Ctx):
    oldest, newest = c.summary.get("oldest"), c.summary.get("newest")
    present = 0.4 if oldest and newest else 0.0
    # `all()` over an empty generator is vacuously true, so absent bounds used to earn the UTC
    # credit — 30% for producing nothing. Both values must exist before either sub-score applies.
    utc = 0.3 if (oldest and newest and all(
        str(v).endswith("Z") or "+00:00" in str(v) for v in (oldest, newest))) else 0.0
    ordered = 0.3 if oldest and newest and (_instant(oldest) or 0) < (_instant(newest) or 0) else 0.0
    return g(present + utc + ordered, f"oldest={oldest} newest={newest}",
             "local-offset or missing bounds misstate the period covered")


@check("input_validation", "B")
def _(c: Ctx):
    """BENCH2 rank 7: 3 binary cells -> a 7-cell matrix, each cell half status + half the
    DOCUMENTED error body. A 500 on garbage input, a 400 with an empty body, and a correct
    rejection now score differently instead of collapsing into one bit."""
    if not c.val_matrix:
        return g(0.0, "matrix not exercised", "n/a")
    score = sum(cell for (_p, _s, cell) in c.val_matrix) / len(c.val_matrix)
    worst = [f"{p}={st}" for (p, st, cell) in c.val_matrix if cell < 1.0]
    return g(score, f"{score * len(c.val_matrix):.1f}/{len(c.val_matrix)} cells clean" +
             (f"; failing: {', '.join(worst[:3])}" if worst else ""),
             "invalid input accepted or crashing instead of the documented rejection")


@check("ui_states", "B")
def _(c: Ctx):
    pats = {"loading": r"loading|fetching|spinner", "empty": r"no payments|empty|never synced|nothing yet",
            "error": r"error|failed|unreachable|could not|retry"}
    hits = [k for k, p in pats.items() if re.search(p, c.html, re.I)]
    return g(len(hits) / 3, f"states present: {hits or 'none'}",
             "a user cannot tell the difference between loading, empty and broken")


@check("ui_currency", "B")
def _(c: Ctx):
    strong = re.search(r"toLocaleString|Intl\.NumberFormat", c.html)
    weak = re.search(r"toFixed\(2\)|/\s*100|€", c.html)
    return g(1.0 if strong else (0.5 if weak else 0.0),
             "Intl formatting" if strong else ("manual formatting" if weak else "none"),
             "minor units shown raw read as 100x the real amount")


@check("ui_offline", "B")
def _(c: Ctx):
    if not c.html.strip():
        # "no external assets" is vacuously true of no page at all. An absent frontend must score
        # zero here, not full marks — the same inverted-logic trap as the empty-bounds credit.
        return g(0.0, "no page to inspect", "there is no frontend")
    cdn = re.findall(r'(?:src|href)\s*=\s*["\']https?://[^"\']+', c.html)
    return g(1.0 if not cdn else 0.0, f"{len(cdn)} external asset(s)",
             "the spec requires offline operation; a CDN asset breaks it")


# ── C: vendor contract ────────────────────────────────────────────────────────────────────────

def _vendor_check(name: str, fn):
    @check(name, "C")
    def _(c: Ctx, _fn=fn):
        r = _fn(c.trace, {})
        return g(1.0 if r["ok"] else 0.0, r["detail"], r.get("consequence", ""))


for _n, _f in [
    ("vendor_read_docs", vendor_trace.check_read_the_docs),
    ("vendor_cursor_paging", vendor_trace.check_cursor_pagination),
    ("vendor_all_pages", vendor_trace.check_fetched_every_page),
    ("vendor_retry_secs", vendor_trace.check_retry_after_honoured),
    ("vendor_retry_date", vendor_trace.check_httpdate_retry_after),
    ("vendor_cursor_expiry", vendor_trace.check_cursor_expiry_recovery),
]:
    _vendor_check(_n, _f)


# BENCH2 rank 4: the client-contract checks — the dormant results-consuming vendor_trace checks,
# awake for the first time in campaign history, fed by the direct-invocation probe.
def _client_check(name: str, fn):
    @check(name, "C")
    def _(c: Ctx, _fn=fn):
        res = c.client.get("results") or {}
        r = _fn(c.client.get("trace") or [], res)
        detail = r["detail"]
        # A dead PROBE must not masquerade as a dead APP: "returned None" hid a whole
        # controls-failure class behind an uninformative detail. The probe's own error
        # report travels with the verdict so the next reader diagnoses instead of guessing.
        if not r["ok"] and res.get("_errors"):
            detail += f"  [probe errors: {str(res['_errors'])[:180]}]"
        return g(1.0 if r["ok"] else 0.0, detail, r.get("consequence", ""))


for _n, _f in [
    ("client_all_payments", vendor_trace.check_all_payments_returned),
    ("client_total_count", vendor_trace.check_total_semantics),
    ("client_true_order", vendor_trace.check_chronological_order),
    ("client_create_replay", vendor_trace.check_replay_treated_as_success),
    ("client_idempotency_key", vendor_trace.check_idempotency_key_sent),
    ("client_integer_amounts", vendor_trace.check_amounts_are_integer_minor),
]:
    _client_check(_n, _f)


@check("update_propagation", "C")
def _(c: Ctx):
    """BENCH2 rank 5: after the vendor changes 25 statuses, a third sync must bring them home.
    Graded as the fraction that propagated — 25 distinct rows of resolution. The guard is the
    vacuous rule: no sync3 response, or a mutation that never happened, scores 0, never 'no
    updates to miss'."""
    ran = isinstance(c.sync3, dict) and any(k in c.sync3 for k in ("inserted", "total", "fetched"))
    if not c.update_changed or not ran:
        return g(0.0, "mutation pass not exercised or sync3 absent", "n/a")
    return g(min(c.update_seen / c.update_changed, 1.0),
             f"{c.update_seen}/{c.update_changed} mutated statuses visible locally after sync3",
             "an app that only INSERTS shows stale statuses forever — refunds never appear")


@check("restart_persistence", "C")
def _(c: Ctx):
    """BENCH2 rank 6: SIGKILL the app, reboot it on the same --db, and the ledger must still be
    there — through the app's own API. The guard is the vacuous rule: no pre-kill rows means
    there was nothing to persist and the check reports that as 0, never as survival."""
    if not c.restart_before:
        return g(0.0, "no rows before the kill — nothing to prove persistence with", "n/a")
    if c.restart_after < 0:
        return g(0.0, f"app did not come back after SIGKILL (had {c.restart_before} rows)",
                 "a crash loses the ledger — the store was memory dressed as a database")
    return g(min(c.restart_after / c.restart_before, 1.0),
             f"{c.restart_after}/{c.restart_before} rows survived kill+reboot on the same db",
             "rows that vanish on restart mean the SQLite file was decoration, not persistence")


class _FloatSeen(float):
    pass


def _rows_from_raw(raw) -> tuple:
    """(rows, float_seen) parsed from wire bytes with a float sentinel — detection is a
    property of the BYTES, not of python's numeric view after the fact."""
    seen = {"f": False}

    def _pf(x):
        seen["f"] = True
        return float(x)

    try:
        body = json.loads(raw if isinstance(raw, str) else raw.decode(errors="replace"),
                          parse_float=_pf)
    except Exception:
        return [], False
    # The documented envelope key is "data" (every pre-existing check reads it); "payments"
    # and a bare list are tolerated shapes, scored by their content not their wrapper.
    rows = (body.get("data") or body.get("payments")) if isinstance(body, dict) else body
    return (rows if isinstance(rows, list) else []), seen["f"]


@check("row_integrity", "B")
def _(c: Ctx):
    """BENCH2 rank 8: every row of all three pages — exact documented keys 0.35, integer
    amounts ON THE WIRE 0.25 (parse_float sentinel), parseable timestamps 0.15, id coverage
    of the full collection 0.25."""
    all_rows, float_seen = [], False
    for st, raw in c.raw_pages:
        rows, f = _rows_from_raw(raw)
        all_rows.extend(rows)
        float_seen = float_seen or f
    if not all_rows:
        return g(0.0, "no rows on the wire", "n/a")
    keys_ok = sum(1 for r in all_rows if isinstance(r, dict) and set(r) >= PAYMENT_KEYS)
    ints_ok = 0 if float_seen else sum(
        1 for r in all_rows if isinstance(r.get("amount_minor"), int))
    times_ok = sum(1 for r in all_rows
                   if isinstance(r.get("created_at"), str) and len(r["created_at"]) >= 19)
    ids = {r.get("id") for r in all_rows if isinstance(r, dict)}
    n = len(all_rows)
    score = (0.35 * keys_ok / n + 0.25 * ints_ok / n + 0.15 * times_ok / n
             + 0.25 * min(len(ids) / EXPECTED_TOTAL, 1.0))
    return g(score,
             f"{n} rows: keys {keys_ok}/{n}, int-amounts {ints_ok}/{n}"
             + (" (FLOAT ON WIRE)" if float_seen else "")
             + f", times {times_ok}/{n}, ids {len(ids)}/{EXPECTED_TOTAL}",
             "malformed rows poison every consumer downstream of the API")


@check("chronological_order_full", "B")
def _(c: Ctx):
    """BENCH2 rank 8: ordered-pair fraction across the FULL collection against the vendor's
    single-source truth — 246 adjacent pairs of resolution where the old check had 24."""
    all_rows = []
    for st, raw in c.raw_pages:
        rows, _f = _rows_from_raw(raw)
        all_rows.extend(rows)
    ids = [r.get("id") for r in all_rows if isinstance(r, dict)]
    truth = {pid: i for i, pid in enumerate(vendor_service.true_order_ids())}
    known = [i for i in ids if i in truth]
    if len(known) < 2:
        return g(0.0, "too few known rows to order", "n/a")
    pairs = list(zip(known, known[1:]))
    ok = sum(1 for a, b2 in pairs if truth[a] < truth[b2])
    return g(ok / len(pairs), f"{ok}/{len(pairs)} adjacent pairs in true instant order",
             "mixed UTC offsets make lexicographic order wrong — payments appear out of sequence")


@check("json_everywhere", "B")
def _(c: Ctx):
    """BENCH2 rank 8: 'Every response is JSON' — the spec's own sentence, graded per response
    (success and error paths alike): half parses, half declares an application/json type."""
    if not c.json_probe:
        return g(0.0, "no responses sampled", "n/a")
    total = 0.0
    bad = []
    for path, st, parses, ctype in c.json_probe:
        cell = (0.5 if parses else 0.0) + (0.5 if "json" in (ctype or "").lower() else 0.0)
        total += cell
        if cell < 1.0:
            bad.append(f"{path}({'no-parse' if not parses else ''}{'/' if not parses and 'json' not in (ctype or '').lower() else ''}{'no-ctype' if 'json' not in (ctype or '').lower() else ''})")
    score = total / len(c.json_probe)
    return g(score, f"{total:.1f}/{len(c.json_probe)} responses fully JSON" +
             (f"; failing: {', '.join(bad[:3])}" if bad else ""),
             "an HTML error page mid-API breaks every client that trusted the contract")


@check("health_semantics", "B")
def _(c: Ctx):
    """BENCH2 rank 8: /api/health graded in quarters — ok status, a FRESH boot state (0 rows,
    null last_sync on a new db), the post-sync count, and a UTC-designated last_sync.
    Deliberately NOT root-blocked (the local_pagination partial-independence precedent)."""
    boot, post = c.health or {}, c.health_post or {}
    q1 = 0.25 if boot.get("status") == "ok" else 0.0
    q2 = 0.25 if boot.get("payments") == 0 and boot.get("last_sync") in (None, "") else 0.0
    q3 = 0.25 if post.get("payments") == EXPECTED_TOTAL else 0.0
    ls = str(post.get("last_sync") or "")
    q4 = 0.25 if ls and (ls.endswith("Z") or "+00:00" in ls) else 0.0
    return g(q1 + q2 + q3 + q4,
             f"ok={boot.get('status')} fresh={q2 > 0} post_count={post.get('payments')} last_sync={ls or None}",
             "a health endpoint that misreports state misleads every operator and monitor")


# ── D: finesse (deliberately hard to max) ─────────────────────────────────────────────────────

@check("request_efficiency", "D")
def _(c: Ctx):
    """247 rows at the documented max limit of 100 — three pages plus the trap chain is optimal.

    Graded against the theoretical optimum rather than a threshold, so there is always headroom: a
    client that pages at the default 25 is correct but not efficient, and says so.
    """
    if not c.sync1_reqs:
        return g(0.0, "no vendor requests observed", "n/a")
    # MEASURED by walking the chain optimally at limit=100: 3 pages + one 429 retry + the 410
    # restart + the HTTP-date retry = 7. Not a guess, and not 1 — the trap chain is unavoidable, so
    # charging a client for it would be a deduction no implementation could escape.
    optimum = 7
    ratio = optimum / c.sync1_reqs
    return g(ratio, f"{c.sync1_reqs} vendor requests for the first sync (optimum {optimum})",
             "paging at the default size multiplies request count and rate-limit exposure")


@check("uses_max_limit", "D")
def _(c: Ctx):
    return g(min(c.max_limit_used / 100, 1.0) if c.max_limit_used else 0.0,
             f"largest limit requested: {c.max_limit_used or 'none'} (max allowed 100)",
             "not using the documented maximum page size costs avoidable round trips")


@check("second_sync_cost", "C")
def _(c: Ctx):
    """BENCH2 rank 3 (sb-4): vendor_conditional (C) and resync_conditional_ratio (D) merged —
    the board's one CONFIRMED adjudication, the same mechanism double-counted as 16.3% of all
    remaining loss. A graded ladder replaces both. The GUARD is the vacuous-pass rule: a second
    sync that never ran, reported nothing, or issued no vendor requests is not cheap — it is
    absent, and absence scores zero."""
    ran = isinstance(c.sync2, dict) and any(k in c.sync2 for k in ("inserted", "total", "fetched"))
    if not ran or not c.sync2_reqs:
        return g(0.0, "second sync absent or reported nothing", "cheap-by-absence is the vacuous trap")
    if c.sync2_304 == c.sync2_reqs:
        score = 1.0  # every page answered 304 — the documented optimum
    elif c.sync2_cond == c.sync2_reqs:
        score = 0.7  # every request conditional, but stale tags cost re-downloads
    elif c.sync2_cond and c.sync2_304:
        score = 0.4  # the mechanism exists somewhere — today's easy 'kinda works' rung
    else:
        score = 0.0
    return g(score,
             f"{c.sync2_cond}/{c.sync2_reqs} conditional, {c.sync2_304}/{c.sync2_reqs} answered 304",
             "re-downloading unchanged pages is the documented top cause of quota exhaustion")


@check("concurrent_sync_safe", "D")
def _(c: Ctx):
    if c.concurrent_total is None:
        return g(0.0, "concurrent sync not observed", "n/a")
    return g(1.0 if c.concurrent_total == EXPECTED_TOTAL else 0.0,
             f"total after two overlapping syncs: {c.concurrent_total}",
             "two operators pressing Sync at once duplicate or lose rows")


@check("api_content_type", "D")
def _(c: Ctx):
    return g(1.0 if "application/json" in c.api_ctype else 0.0, f"Content-Type: {c.api_ctype!r}",
             "a wrong content type breaks strict clients and browser fetch handling")


@check("ui_polish", "D")
def _(c: Ctx):
    pats = {"last-sync shown": r"last[ _-]?sync", "disables while syncing": r"disabled",
            "row count": r"count|total", "retry affordance": r"retry|try again",
            "accessible table": r"<th\b|scope="}
    hits = [k for k, p in pats.items() if re.search(p, c.html, re.I)]
    return g(len(hits) / len(pats), f"{len(hits)}/{len(pats)}: {hits}",
             "the page works but gives the operator little to act on")


@check("store_atomic_upsert", "D")
def _(c: Ctx):
    """A single atomic upsert, or select-then-insert? Traceable to the spec's "must be updated, not
    duplicated" plus "run repeatedly" — the read-then-write shape loses that race under concurrency."""
    src = (c.pkg / "store.py").read_text(errors="replace") if (c.pkg / "store.py").is_file() else ""
    if not src:
        return g(0.0, "no store.py", "n/a")
    # ON CONFLICT ... DO UPDATE is the only form that MERGES. INSERT OR REPLACE deletes the old row
    # and inserts a new one, silently dropping columns the write omits and firing delete triggers;
    # INSERT OR IGNORE keeps stale data instead of updating it. Both write, neither upserts.
    # Scope to the PAYMENTS write. A whole-file scan matched an unrelated ON CONFLICT in the
    # last_sync/meta write and reported a non-atomic payments upsert as atomic — the check passed a
    # build whose sync path had genuinely lost its merge clause.
    stmt = re.search(r"INSERT[^;\"']*?INTO\s+payments.*?(?=\"\"\"|;|\Z)", src, re.I | re.S)
    scope = stmt.group(0) if stmt else src
    atomic = re.search(r"ON CONFLICT[^;]*DO UPDATE", scope, re.I | re.S)
    weaker = re.search(r"INSERT OR REPLACE|INSERT OR IGNORE|REPLACE INTO", scope, re.I)
    racy = re.search(r"SELECT[^;]*WHERE[^;]*id", src, re.I) and re.search(r"INSERT INTO", src, re.I)
    return g(1.0 if atomic else (0.5 if weaker else (0.3 if racy else 0.0)),
             "ON CONFLICT DO UPDATE (true merge)" if atomic else
             ("INSERT OR REPLACE/IGNORE — writes but does not merge" if weaker else
              ("select-then-insert" if racy else "no upsert found")),
             "REPLACE deletes and reinserts, dropping omitted columns; IGNORE keeps stale rows; "
             "read-then-write duplicates when two syncs overlap")


@check("store_indexed", "D")
def _(c: Ctx):
    """247 rows upserted per sync: without a key on id every upsert is a full scan."""
    src = (c.pkg / "store.py").read_text(errors="replace") if (c.pkg / "store.py").is_file() else ""
    hit = re.search(r"PRIMARY KEY|UNIQUE|CREATE\s+(UNIQUE\s+)?INDEX", src, re.I)
    return g(1.0 if hit else 0.0, "keyed" if hit else "no primary key or index on the payment id",
             "every upsert becomes a full table scan; sync cost grows quadratically")


@check("client_timeouts", "D")
def _(c: Ctx):
    """A vendor that accepts the connection and never answers must not hang the sync forever."""
    src = (c.pkg / "meridian.py").read_text(errors="replace") if (c.pkg / "meridian.py").is_file() else ""
    hit = re.search(r"timeout\s*=", src)
    return g(1.0 if hit else 0.0, "timeout set" if hit else "no request timeout",
             "one unresponsive vendor call hangs the sync with no recovery")


@check("ui_error_actionable", "D")
def _(c: Ctx):
    """The spec asks for text a user can ACT on, not merely the word "error"."""
    if not c.html.strip():
        return g(0.0, "no page", "n/a")
    actionable = re.search(r"try again|retry|check your connection|is the server running|refresh",
                           c.html, re.I)
    generic = re.search(r"error|failed", c.html, re.I)
    return g(1.0 if actionable else (0.4 if generic else 0.0),
             "actionable guidance" if actionable else ("generic error text" if generic else "none"),
             "an error the user cannot act on is only marginally better than a blank screen")


# REMOVED: error_detail_quality. It scored a 404 body of {"error": "not_found"} at 60% and wanted
# extra keys — but that body is EXACTLY what spec-build.md specifies. A check that penalises precise
# compliance with the stated contract is a preference dressed as a defect, and fails the traceability
# test in the legitimacy rule. Deleted rather than rebased: there is no defect here to measure.

# ── gathering ─────────────────────────────────────────────────────────────────────────────────

def _trace_split(trace: List[Dict]) -> tuple:
    """(first-sync requests, second-sync requests, second-sync 304s, largest limit seen)."""
    marks = [i for i, e in enumerate(trace) if e.get("__phase__", "").startswith("sync")]
    lists = lambda seg: [e for e in seg if e.get("path") == "/v1/payments" and e.get("method") == "GET"]
    # BENCH2 rank 1: MARKER-BOUNDED windows. sync2's window used to run to end-of-trace, so any
    # LATER phase (a sync3 mutation pass, a probe's own traffic) would be silently credited to
    # sync2. Each phase now ends at the next marker; byte-identical for today's two-marker
    # traces and correct the day a third phase exists.
    segs = []
    for k, m in enumerate(marks):
        end = marks[k + 1] if k + 1 < len(marks) else len(trace)
        segs.append(trace[m + 1:end])
    first = segs[0] if segs else trace
    second = segs[1] if len(segs) > 1 else []
    limits = [int(e["query"]["limit"]) for e in lists(trace)
              if str(e.get("query", {}).get("limit", "")).isdigit()]
    return (len(lists(first)), len(lists(second)),
            sum(1 for e in lists(second) if e.get("status") == 304),
            sum(1 for e in lists(second) if e.get("if_none_match")), max(limits or [0]))


def gather(root: Path, vendor_port: int, db: Path, trace_path: Path,
           mark_phase=None) -> Ctx:
    c = Ctx(root)
    if not (root / "vendorsync").is_dir():
        nested = [p for p in root.iterdir() if p.is_dir() and (p / "vendorsync").is_dir()]
        if nested:
            c.root, c.pkg = nested[0], nested[0] / "vendorsync"
    html_path = c.pkg / "web/index.html"
    if html_path.is_file():
        c.html = html_path.read_text(errors="replace")

    port = _free_port()
    env = {**os.environ, "PYTHONPATH": str(c.root), "PYTHONDONTWRITEBYTECODE": "1",
           "MERIDIAN_BASE_URL": f"http://127.0.0.1:{vendor_port}",
           "MERIDIAN_API_KEY": "sk_test_meridian"}
    proc = None
    try:
        proc = subprocess.Popen(
            [sys.executable, "-m", "vendorsync", "--db", str(db), "--port", str(port)],
            cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, start_new_session=True)
        base = f"http://127.0.0.1:{port}"
        deadline = time.time() + 25
        while time.time() < deadline:
            if proc.poll() is not None:
                c.boot_error = (proc.stdout.read() or "")[-800:]
                break
            status, body, _raw, _h = _get(f"{base}/api/health", timeout=2)
            if status == 200 and isinstance(body, dict):
                c.health = body
                break
            time.sleep(0.4)

        if c.health:
            c.root_status, _b, raw, _h = _get(f"{base}/")
            if raw:
                c.html = raw.decode(errors="replace") or c.html

            if mark_phase:
                mark_phase("sync1")
            _s, c.sync1, _r, _h = _post(f"{base}/api/sync")
            if mark_phase:
                mark_phase("sync2")
            _s, c.sync2, _r, _h = _post(f"{base}/api/sync")

            _s, c.payments, _r, hdrs = _get(f"{base}/api/payments")
            c.api_ctype = hdrs.get("Content-Type", "")
            _s, c.page5, _r, _h = _get(f"{base}/api/payments?limit=5&offset=5")
            _s, c.capped, _r, _h = _get(f"{base}/api/payments?limit=500")
            _s, c.summary, _r, _h = _get(f"{base}/api/summary")
            # BENCH2 rank 8: three RAW pages for wire-byte inspection (row_integrity's float
            # sentinel must see the bytes, not python's post-parse floats-become-ints view).
            for off in (0, 100, 200):
                st8, _b8, raw8, _h8 = _get(f"{base}/api/payments?limit=100&offset={off}")
                c.raw_pages.append((st8, raw8))
            c.bad_limit, _b, _r, _h = _get(f"{base}/api/payments?limit=-1")
            c.bad_offset, _b, _r, _h = _get(f"{base}/api/payments?offset=abc")
            c.notfound, c.notfound_body, _r, _h = _get(f"{base}/api/nope")
            # BENCH2 rank 7: the 7-cell validation matrix — each cell is (status is 400/404) +
            # (body is the DOCUMENTED error JSON), graded half each. The spec's own sentence:
            # non-numeric or negative limit/offset -> 400 {"error":"bad_request"}; unknown path
            # -> 404 {"error":"not_found"}. Every response JSON.
            for path, want_status, want_err in [
                ("/api/payments?limit=-1", 400, "bad_request"),
                ("/api/payments?limit=abc", 400, "bad_request"),
                ("/api/payments?offset=-5", 400, "bad_request"),
                ("/api/payments?offset=abc", 400, "bad_request"),
                ("/api/nope", 404, "not_found"),
                ("/api/payments/deep/unknown", 404, "not_found"),
            ]:
                st, body, _raw, _hh = _get(f"{base}{path}")
                cell = 0.0
                if st == want_status:
                    cell += 0.5
                if isinstance(body, dict) and body.get("error") == want_err:
                    cell += 0.5
                c.val_matrix.append((path, st, cell))

            # two overlapping syncs must not duplicate or lose rows
            results: List = []
            threads = [threading.Thread(target=lambda: results.append(_post(f"{base}/api/sync")))
                       for _ in range(2)]
            for t in threads:
                t.start()
            for t in threads:
                t.join(timeout=240)
            _s, health2, _r, _h = _get(f"{base}/api/health")
            c.concurrent_total = (health2 or {}).get("payments")
            c.health_post = health2 if isinstance(health2, dict) else {}
            # BENCH2 rank 8: json_everywhere sample — the spec's "Every response is JSON"
            # graded across success AND error paths: half parses-as-JSON, half declares a JSON
            # content type. Issued-but-unanswered counts 0.
            for jp in ("/api/health", "/api/payments", "/api/payments?limit=5&offset=5",
                       "/api/summary", "/api/nope", "/api/payments?limit=-1"):
                stj, _bj, rawj, hj = _get(f"{base}{jp}")
                try:
                    json.loads(rawj if isinstance(rawj, str) else rawj.decode(errors="replace"))
                    parses = True
                except Exception:
                    parses = False
                c.json_probe.append((jp, stj, parses,
                                     (hj.get("Content-Type", "") if hj else "")))

            # BENCH2 rank 5: UPDATE PROPAGATION. The spec's own sentence — "a payment already
            # present must be UPDATED" — has never been exercised: no run ever changed vendor
            # data. mutate_statuses flips 25 rows (length unchanged, so every total-based check
            # upstream stays valid), sync3 runs with continuation semantics (traps stay spent,
            # the marker-bounded windows attribute it correctly), and the LOCAL store is paged
            # to count how many of the 25 arrived. restore_payments() in the finally puts the
            # module state back for the next rep in this process.
            if mark_phase:
                try:
                    c.update_changed = vendor_service.mutate_statuses()
                    mark_phase("sync3")
                    _s, c.sync3, _r, _h = _post(f"{base}/api/sync")
                    refunded = 0
                    for off in (0, 100, 200):
                        _s, page, _r, _h = _get(f"{base}/api/payments?limit=100&offset={off}")
                        rows = ((page.get("data") or page.get("payments"))
                                if isinstance(page, dict) else page)
                        if isinstance(rows, list):
                            refunded += sum(1 for r in rows
                                            if isinstance(r, dict) and r.get("status") == "refunded")
                    c.update_seen = refunded
                finally:
                    vendor_service.restore_payments()

            # BENCH3: FEATURE PRE-FETCH (BENCH_AMEND arms only) — grabbed while the app is
            # still alive; greenfield runs skip both requests entirely.
            if os.environ.get("BENCH_AMEND"):
                st, body, _r, _h = _req(
                    urllib.request.Request(
                        f"http://127.0.0.1:{port}/api/summary/by-status"
                    ),
                    10,
                )
                c.feat_by_status = (
                    body if st == 200 and isinstance(body, dict) else {"_code": st}
                )
                st2, _b, raw2, _h2 = _req(
                    urllib.request.Request(f"http://127.0.0.1:{port}/api/export.csv"), 15
                )
                c.feat_csv = raw2 if st2 == 200 else b""

            # PRODUCT TIER (sb-5): browser truth + clock truth, gathered while the app is
            # alive. Placed AFTER every trace-window-dependent gather (sync1/sync2 markers,
            # update_propagation's sync3) so the browser's own Sync click and the timed sync
            # cannot contaminate a window — and BEFORE the SIGKILL below.
            if PRODUCT:
                c.perf = {
                    "payments": perf_probe.measure_endpoint(base, "/api/payments?limit=25"),
                    "page": perf_probe.measure_page(base),
                    "sync": perf_probe.measure_sync(base),
                }
                c.probe_load = _product_probe("load", base)
                c.probe_sync = _product_probe("sync", base)
                c.probe_error = _product_probe("error", base, "--block-api")
                # EMPTY state wants a fresh db: a second instance, probed, then killed.
                eport = _free_port()
                eproc = None
                try:
                    eproc = subprocess.Popen(
                        [sys.executable, "-m", "vendorsync", "--db",
                         str(c.root / "empty-probe.db"), "--port", str(eport)],
                        cwd=c.root, env=env, stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT, text=True, start_new_session=True)
                    edeadline = time.time() + 15
                    while time.time() < edeadline:
                        if eproc.poll() is not None:
                            c.probe_empty = {"_probe_error": "empty-instance died at boot"}
                            break
                        es, _eb, _er, _eh = _get(f"http://127.0.0.1:{eport}/api/health",
                                                 timeout=2)
                        if es == 200:
                            c.probe_empty = _product_probe(
                                "empty", f"http://127.0.0.1:{eport}")
                            break
                        time.sleep(0.4)
                    else:
                        c.probe_empty = {"_probe_error": "empty-instance never became healthy"}
                finally:
                    if eproc and eproc.poll() is None:
                        try:
                            os.killpg(os.getpgid(eproc.pid), signal.SIGKILL)
                        except Exception:
                            eproc.kill()
                    (c.root / "empty-probe.db").unlink(missing_ok=True)

            # BENCH2 rank 6: RESTART PERSISTENCE. The spec's "run repeatedly against the same
            # database" has only ever been tested within one process lifetime. Kill the app
            # (SIGKILL — a crash, not a courtesy shutdown), respawn on the SAME db, and count
            # rows through the app's OWN API (never a raw sqlite open — the judge's WAL
            # warning: a read-only URI open after SIGKILL can miss WAL-committed rows and
            # false-flag a correct app). An in-memory store dies here by construction.
            _s, before_page, _r, _h = _get(f"{base}/api/health")
            c.restart_before = (before_page or {}).get("payments") or 0
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                proc.kill()
            proc.wait(timeout=10)
            proc = subprocess.Popen(
                [sys.executable, "-m", "vendorsync", "--db", str(db), "--port", str(port)],
                cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, start_new_session=True)
            deadline2 = time.time() + 20
            c.restart_after = -1
            while time.time() < deadline2:
                if proc.poll() is not None:
                    break
                _s, h3, _r, _h = _get(f"{base}/api/health", timeout=2)
                if _s == 200 and isinstance(h3, dict):
                    c.restart_after = h3.get("payments") if isinstance(h3.get("payments"), int) else -1
                    break
                time.sleep(0.4)

            for attr in ("payments", "page5", "capped", "summary", "sync1", "sync2"):
                if getattr(c, attr) is None:
                    setattr(c, attr, {})
    finally:
        if proc and proc.poll() is None:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                proc.kill()

    if trace_path.is_file():
        c.trace = [json.loads(l) for l in trace_path.read_text().splitlines() if l.strip()]
        c.sync1_reqs, c.sync2_reqs, c.sync2_304, c.sync2_cond, c.max_limit_used = _trace_split(c.trace)
    # BENCH2 rank 4: the direct-invocation client probe — its OWN vendor instance, so it can
    # never contaminate the main exercise's trace or trap state. create_payment is REQ-1's
    # previously-dead surface; a probe failure is an honest per-step error, never an exception.
    try:
        c.client = client_probe.run_client_probe(c.root, vendor_service)
    except Exception as e:
        c.client = {"results": {"_errors": {"probe": str(e)[:200]}}, "trace": []}
    return c


# ── PRODUCT TIER (sb-5): J journeys / P performance / V visual — browser and clock truth ─────
# Every check reads Ctx fields the gather phase filled from product_probe.mjs (a real headless
# chromium over the RENDERED page) and perf_probe (wall-clock measurements). A `_probe_error`
# dict is a harness failure: scored 0 with a loud detail, and systemic breakage is caught by the
# controls' HIGH gate (the reference zeros too and the grader refuses trust).


def _pe(p: Dict):
    return p.get("_probe_error") if isinstance(p, dict) else "probe returned nothing"


@product_check("j_loads_data", "J")
def j_loads_data(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    rows = p.get("renderedRowCount") or 0
    claimed = p.get("totalClaimedInDom")
    reconciles = claimed == EXPECTED_TOTAL or rows == EXPECTED_TOTAL
    return g((0.5 if rows > 0 else 0) + (0.5 if reconciles else 0),
             f"{rows} rows rendered, DOM claims {claimed} of {EXPECTED_TOTAL}",
             "the page under-reports the collection" if not reconciles else "",
             parts={"rendered_rows": rows > 0, "total_reconciles": reconciles})


@product_check("j_console_clean", "J")
def j_console_clean(c):
    errs = []
    for p in (c.probe_load, c.probe_sync):
        if isinstance(p, dict) and not _pe(p):
            ce = p.get("consoleErrors") or {}
            errs += ce.get("texts") or [""] * (ce.get("count") or 0)
    n = len(errs)
    return g(1.0 if n == 0 else (0.5 if n == 1 else 0.0),
             f"{n} console error(s) across load+sync" + (f": {errs[0][:120]}" if errs else ""),
             "JS errors in normal use" if n else "")


@product_check("j_sync_journey", "J")
def j_sync_journey(c):
    p = c.probe_sync
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    parts = {"button_found": bool(p.get("found")),
             "disabled_while_syncing": bool(p.get("disabledDuringSync")),
             "completed": bool(p.get("completed")),
             "view_refreshed": bool(p.get("viewRefreshed"))}
    return g(sum(parts.values()) / 4,
             ", ".join(k for k, v in parts.items() if v) or "sync button never found",
             "the spec's one interactive flow fails in a real browser" if not all(parts.values()) else "",
             parts=parts)


@product_check("j_error_state", "J")
def j_error_state(c):
    p = c.probe_error
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    visible = bool(p.get("errorStateVisible"))
    return g(1.0 if visible else 0.0,
             f"error state {'shown: ' + str(p.get('actionableText'))[:80] if visible else 'NOT shown'}"
             f" (api blocked, bodyText {p.get('bodyTextLength')})",
             "" if visible else "backend failure leaves the user with no actionable message")


@product_check("j_empty_state", "J")
def j_empty_state(c):
    p = c.probe_empty
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    visible = bool(p.get("emptyStateVisible"))
    rows = p.get("renderedRowCount") or 0
    if rows > 0:
        return g(0, f"{rows} rows rendered on an EMPTY db", "phantom data on first run")
    return g(1.0 if visible else 0.0,
             f"empty state {'shown: ' + str(p.get('emptyStateText'))[:60] if visible else 'NOT shown'}",
             "" if visible else "a fresh install shows a blank page")


@product_check("p_list_latency", "P")
def p_list_latency(c):
    m = (c.perf.get("payments") or {})
    return g(_ladder(m.get("p95_ms"), [(1.0, 150), (0.75, 300), (0.5, 600), (0.25, 1200)]),
             f"GET /api/payments limit=25 p95 {m.get('p95_ms')} ms (budget 150)",
             "", parts={"p95_ms": m.get("p95_ms"), "errors": len(m.get("errors") or [])})


@product_check("p_page_interactive", "P")
def p_page_interactive(c):
    v = None if _pe(c.probe_load) else c.probe_load.get("timeToFirstDataMs")
    return g(_ladder(v, [(1.0, 2000), (0.75, 3000), (0.5, 4500), (0.25, 8000)]),
             f"first data rows rendered at {v} ms (budget 2000)")


@product_check("p_sync_wall", "P")
def p_sync_wall(c):
    m = (c.perf.get("sync") or {})
    return g(_ladder(m.get("wall_ms"), [(1.0, 60000), (0.75, 90000), (0.5, 120000), (0.25, 180000)]),
             f"POST /api/sync wall {m.get('wall_ms')} ms (budget 60000)")


@product_check("v_dates_readable", "V")
def v_dates_readable(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    texts = [t for t in (p.get("dateTexts") or []) if t and t.strip()]
    if not texts:
        return g(0, "no date cells rendered", "the Date column is empty in a real browser")
    iso = [t for t in texts if re.search(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}", t)]
    if iso:
        return g(0, f"raw ISO-8601 shown to users: {iso[0][:60]}",
                 "the spec forbids machine timestamps in the rendered page")
    human = any(re.search(r"[A-Za-z]{3}", t) or re.search(r"\d{1,2}[./]\d{1,2}", t) for t in texts)
    return g(1.0 if human else 0.5, f"rendered dates like {texts[0][:60]}",
             "" if human else "formatted but not recognizably locale-readable")


@product_check("v_status_distinct", "V")
def v_status_distinct(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    styles = p.get("statusStyles") or {}
    if len(styles) >= 2:
        pairs = {(v.get("color"), v.get("backgroundColor")) for v in styles.values()}
        ok = len(pairs) == len(styles)
        return g(1.0 if ok else 0.0,
                 f"{len(styles)} statuses rendered, {len(pairs)} distinct style(s): {sorted(styles)}",
                 "" if ok else "statuses are indistinguishable at a glance")
    if len(styles) == 1:
        (name, s), = styles.items()
        styled = (s.get("backgroundColor") or "rgba(0, 0, 0, 0)") not in ("rgba(0, 0, 0, 0)", "transparent")
        return g(0.5 if styled else 0.0,
                 f"only '{name}' rendered on page 1; {'badge-styled' if styled else 'plain text'}",
                 "cannot verify distinctness with one status; plain text scores 0")
    return g(0, "no status cells rendered", "the Status column is empty in a real browser")


@product_check("v_pagination", "V")
def v_pagination(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    controls = bool(p.get("paginationControls"))
    rows = p.get("renderedRowCount") or 0
    bounded = 0 < rows <= 50
    return g((0.6 if controls else 0) + (0.4 if bounded else 0),
             f"controls={controls}, {rows} rows in one view (spec: never all {EXPECTED_TOTAL})",
             "" if controls and bounded else "the finance team scrolls an unpaginated dump",
             parts={"controls": controls, "bounded_view": bounded})


@product_check("v_filter", "V")
def v_filter(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    return g(1.0 if p.get("filterControl") else 0.0,
             "status filter control present" if p.get("filterControl") else "no status filter control")


@product_check("v_responsive_375", "V")
def v_responsive_375(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    vp = p.get("viewport375") or {}
    ok = vp.get("horizontalScroll") is False
    return g(1.0 if ok else 0.0,
             f"375px horizontal scroll: {vp.get('horizontalScroll')}",
             "" if ok else "the page breaks on a phone-width viewport")


@product_check("v_styling", "V")
def v_styling(c):
    p = c.probe_load
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    s = p.get("styling") or {}
    font = (s.get("bodyFontFamily") or "").strip()
    parts = {"stylesheet": bool(s.get("hasStylesheet")),
             "layered_backgrounds": (s.get("distinctBackgroundCount") or 0) >= 3,
             "non_default_font": bool(font) and not font.lower().startswith("times")}
    return g(0.4 * parts["stylesheet"] + 0.3 * parts["layered_backgrounds"]
             + 0.3 * parts["non_default_font"],
             f"stylesheet={parts['stylesheet']}, {s.get('distinctBackgroundCount')} distinct "
             f"backgrounds, font '{font[:40]}'", "", parts=parts)


# BENCH2 rank 9: THE HARD BLOCK. These checks are the last 0.10 of any score — a core-perfect
# build caps at 0.90, so 0.996-by-saturation is structurally impossible: the final tenth is
# earnable only from the checks that stay hard by construction (a measured request optimum, a
# graded conditional-refetch ladder, a surface no run had ever exercised, an update pass, a
# crash-restart). Membership per BENCH2-DESIGN #9: request_efficiency (formula unchanged,
# optimum still the measured 7), second_sync_cost, the create-replay PAIR (both halves of the
# double-charge defense), update_propagation, restart_persistence.
HARD_BLOCK = {
    "request_efficiency",
    "second_sync_cost",
    "client_create_replay",
    "client_idempotency_key",
    "update_propagation",
    "restart_persistence",
}


@feature_check("feat_summary_by_status")
def feat_summary_by_status(c: Ctx):
    body = c.feat_by_status
    if not body or "_code" in body:
        return {"score": 0.0,
                "detail": f"GET /api/summary/by-status -> {body.get('_code', 'no fetch')}",
                "consequence": "the new feature endpoint is absent or broken"}
    buckets = body.get("by_status") if isinstance(body.get("by_status"), dict) else body
    if not isinstance(buckets, dict) or not buckets:
        return {"score": 0.0, "detail": "no status buckets in the response",
                "consequence": "feature shape wrong"}
    shaped = all(isinstance(v, dict) and "count" in v and "total_minor" in v
                 for v in buckets.values())
    count_sum = sum(v.get("count", 0) for v in buckets.values() if isinstance(v, dict))
    total = c.payments.get("total") or c.payments.get("total_count") or EXPECTED_TOTAL
    accurate = shaped and count_sum == total
    return {"score": 1.0 if accurate else (0.33 if shaped else 0.0),
            "detail": f"buckets={list(buckets)[:4]} count_sum={count_sum} vs total={total}",
            "consequence": "" if accurate else "bucket counts do not reconcile with the collection"}


@feature_check("feat_export_csv")
def feat_export_csv(c: Ctx):
    raw = c.feat_csv
    if not raw:
        return {"score": 0.0, "detail": "GET /api/export.csv returned nothing",
                "consequence": "the export endpoint is absent or broken"}
    lines = [l for l in raw.decode(errors="replace").strip().splitlines() if l.strip()]
    if not lines:
        return {"score": 0.0, "detail": "empty csv", "consequence": "no export"}
    header = lines[0].lower()
    has_header = "id" in header and "amount" in header and "status" in header
    total = c.payments.get("total") or c.payments.get("total_count") or EXPECTED_TOTAL
    rows_ok = len(lines) - 1 == total
    score = 1.0 if (has_header and rows_ok) else (0.33 if has_header or rows_ok else 0.0)
    return {"score": score,
            "detail": f"rows={len(lines) - 1} vs total={total}; header_ok={has_header}",
            "consequence": "" if score == 1.0 else "export incomplete or unlabeled"}


@feature_check("feat_ui_by_status")
def feat_ui_by_status(c: Ctx):
    page = c.html.lower()
    present = "by-status" in page or "by status" in page
    return {"score": 1.0 if present else 0.0,
            "detail": "page carries a by-status section" if present else "no by-status section",
            "consequence": "" if present else "the feature is invisible in the UI"}


def evaluate(c: Ctx) -> Dict:
    rows = []
    for name, tier, fn in CHECKS:
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})

    tiers = {}
    core_weighted = 0.0
    for tier, weight in TIER_WEIGHT.items():
        # Core = the tier-weighted mean over everything OUTSIDE the hard block, renormalized
        # into 0.90 of the total. Hard-block members are excluded from their home tier's mean
        # so their difficulty cannot be diluted by tier-mates.
        sub = [r for r in rows if r["tier"] == tier and r["check"] not in HARD_BLOCK]
        mean = sum(r["score"] for r in sub) / len(sub) if sub else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(sub), "weight": weight}
        core_weighted += mean * weight
    hard_rows = [r for r in rows if r["check"] in HARD_BLOCK]
    hard_mean = sum(r["score"] for r in hard_rows) / len(hard_rows) if hard_rows else 0.0
    tiers["HARD"] = {"mean": round(hard_mean, 4), "checks": len(hard_rows), "weight": 0.10}
    if PRODUCT:
        # sb-5: core renormalizes to 0.60 and the product tiers carry 0.30 — the functional
        # majority holds, and the product axes are big enough to move a build.
        prows = []
        for name, tier, fn in PRODUCT_CHECKS:
            try:
                outcome = fn(c)
            except Exception as exc:
                outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}",
                           "consequence": "probe bug"}
            prows.append({"check": name, "tier": tier, **outcome})
        rows += prows
        product_weighted = 0.0
        for tier, weight in PRODUCT_WEIGHT.items():
            sub = [r for r in prows if r["tier"] == tier]
            mean = sum(r["score"] for r in sub) / len(sub) if sub else 0.0
            tiers[tier] = {"mean": round(mean, 4), "checks": len(sub), "weight": weight}
            product_weighted += mean * weight
        score = PRODUCT_CORE * core_weighted + product_weighted + 0.10 * hard_mean
    else:
        score = 0.90 * core_weighted + 0.10 * hard_mean
    # BENCH3: the amend arms carry a FEATURE half alongside the unchanged sb-4 score (which IS
    # the regression half — the amended tree must hold the known-good's level). Env-gated at
    # call time; greenfield results carry no amend fields at all.
    amend: Dict = {}
    if os.environ.get("BENCH_AMEND"):
        feat_rows = []
        for name, tier, fn in FEATURE_CHECKS:
            try:
                outcome = fn(c)
            except Exception as exc:
                outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}",
                           "consequence": "probe bug"}
            feat_rows.append({"check": name, "tier": tier, **outcome})
        feat_mean = (sum(r["score"] for r in feat_rows) / len(feat_rows)
                     if feat_rows else 0.0)
        amend = {"feature": round(feat_mean, 4), "feature_checks": feat_rows,
                 # One number for the arm's [done] row: regression and feature weighted
                 # evenly — breaking the base is as disqualifying as not landing the feature.
                 "amend_score": round(0.5 * score + 0.5 * feat_mean, 4)}
    return {"score": round(score, 4), "scorer_version": SCORER_VERSION, **amend,
            "core": round(core_weighted, 4), "hard": round(hard_mean, 4),
            # Per-run quality bands — the ARM-level k/n rates aggregate these in the reporter:
            # consistency, not any single score, is the campaign's real battleground.
            "excellent": score >= 0.90, "solid": score >= 0.75,
            "tiers": tiers, "checks": rows,
            "root_causes": attribute_root_causes(rows)}


def format_report(result: Dict, title: str = "") -> str:
    t = result["tiers"]
    head = (f"{title}  {100 * result['score']:.1f}%   " +
            " · ".join(f"{k} {100 * t[k]['mean']:.0f}%"
                       for k in ("A", "B", "C", "D", "J", "P", "V", "HARD") if k in t))
    lines = [head, ""]
    # THE LINE THAT STOPS A TIER MEAN BEING READ AS BREADTH. Without it, "B 36%" is indistinguishable
    # from twelve half-working behaviours, and that reading cost this campaign real work.
    for root, blocked in (result.get("root_causes") or {}).items():
        lines.append(f"  ⚠ {len(blocked)} further check(s) are downstream of `{root}`, which scored "
                     f"0 — this is ONE defect, not {len(blocked) + 1}: {', '.join(blocked)}")
        lines.append("")
    for row in sorted(result["checks"], key=lambda r: (r["tier"], r["check"])):
        pct = f"{100 * row['score']:>3.0f}%"
        lines.append(f"  {row['tier']} {pct} {row['check']:<26} {str(row.get('detail', ''))[:82]}")
        if row["score"] < 1.0 and row.get("consequence") not in ("n/a", "probe bug", "", None):
            lines.append(f"           └ {row['consequence']}")
    return "\n".join(lines)
