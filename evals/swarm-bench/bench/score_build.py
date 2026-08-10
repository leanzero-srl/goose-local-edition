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
from probes import vendor_trace  # noqa: E402

EXPECTED_TOTAL = 247
PAYMENT_KEYS = {"id", "amount_minor", "currency", "created_at", "status"}
EXPECTED_SUM = sum(1000 + i * 137 for i in range(EXPECTED_TOTAL))
TIER_WEIGHT = {"A": 0.25, "B": 0.30, "C": 0.25, "D": 0.20}

# Bump on ANY change to a check, a weight, or the fixture. A verdict carrying a different version is
# not comparable and the sweep will re-run it rather than reuse it. Without this, a stale verdict
# scored by an older, buggier grader sits silently in a table next to fresh ones — which is how a
# cheaper model appeared to beat a stronger one.
SCORER_VERSION = "sb-3"

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


# ── check registry ────────────────────────────────────────────────────────────────────────────

CHECKS: List[tuple] = []


def check(name: str, tier: str) -> Callable:
    def deco(fn):
        CHECKS.append((name, tier, fn))
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
    parts = [c.bad_limit == 400, c.bad_offset == 400,
             c.notfound == 404 and (c.notfound_body or {}).get("error") == "not_found"]
    return g(sum(parts) / 3, f"bad-limit={c.bad_limit} bad-offset={c.bad_offset} 404={c.notfound}",
             "invalid input is accepted or crashes instead of being rejected cleanly")


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
    ("vendor_conditional", vendor_trace.check_conditional_request),
]:
    _vendor_check(_n, _f)


# ── D: finesse (deliberately hard to max) ─────────────────────────────────────────────────────

@check("request_efficiency", "D")
def _(c: Ctx):
    """The collection is 47 rows and the documented max limit is 100 — one page is achievable.

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


@check("resync_conditional_ratio", "D")
def _(c: Ctx):
    if not c.sync2_reqs:
        return g(0.0, "no second-sync requests", "n/a")
    return g(c.sync2_304 / c.sync2_reqs,
             f"{c.sync2_304}/{c.sync2_reqs} of the re-sync answered 304",
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
    if len(marks) >= 2:
        first, second = trace[marks[0] + 1:marks[1]], trace[marks[1] + 1:]
    elif len(marks) == 1:
        first, second = trace[marks[0] + 1:], []
    else:
        first, second = trace, []
    limits = [int(e["query"]["limit"]) for e in lists(trace)
              if str(e.get("query", {}).get("limit", "")).isdigit()]
    return (len(lists(first)), len(lists(second)),
            sum(1 for e in lists(second) if e.get("status") == 304), max(limits or [0]))


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
            c.bad_limit, _b, _r, _h = _get(f"{base}/api/payments?limit=-1")
            c.bad_offset, _b, _r, _h = _get(f"{base}/api/payments?offset=abc")
            c.notfound, c.notfound_body, _r, _h = _get(f"{base}/api/nope")

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
        c.sync1_reqs, c.sync2_reqs, c.sync2_304, c.max_limit_used = _trace_split(c.trace)
    return c


def evaluate(c: Ctx) -> Dict:
    rows = []
    for name, tier, fn in CHECKS:
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})

    tiers = {}
    weighted = 0.0
    for tier, weight in TIER_WEIGHT.items():
        sub = [r for r in rows if r["tier"] == tier]
        mean = sum(r["score"] for r in sub) / len(sub) if sub else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(sub), "weight": weight}
        weighted += mean * weight
    return {"score": round(weighted, 4), "scorer_version": SCORER_VERSION,
            "tiers": tiers, "checks": rows,
            "root_causes": attribute_root_causes(rows)}


def format_report(result: Dict, title: str = "") -> str:
    t = result["tiers"]
    head = (f"{title}  {100 * result['score']:.1f}%   " +
            " · ".join(f"{k} {100 * t[k]['mean']:.0f}%" for k in ("A", "B", "C", "D")))
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
