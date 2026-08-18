"""sb-6 — grade a built `vspro` (VendorSync Pro) tree. THE HARD TIER.

Implements the sb-6 registry from sb6/SB6-PACKAGE.md Artifact 4 + sb6/SB6-SCORING-MATH.md,
with every red-team scorer amendment from sb6/SB6-REDTEAM.md applied (binding, not advisory):

  F4   e_frames_under_drag graded in the spec's unit (frames per scripted drag, not fps);
       rungs calibration-owned via sb6-thresholds.json.
  F7   G6 freeze gate = the `--reference` CLI mode below: scores a tree and REFUSES to emit
       freeze-marked output if any non-calibration-owned check < 0.95, any section was
       probe-unavailable, or the E gate fails. E-gate console cleanliness is scoped to the
       NOMINAL scenarios only (load/sync/empty/viz) — `error`/`viz-fallback` produce expected
       network noise.
  F8c  e_under_load_latency consumes an overlap fraction from the measurement and REFUSES
       (probe-unavailable, never an app zero) when the readers did not provably overlap the
       in-flight sync.
  F9   t_camera's reset anchor pays only when the probe observed the scene actually move
       (mid-drag sample consumed from the probe output; proj fractions as fallback).
  F11  vsdbg yaw compared as angular distance mod 360.
  F15  t_vsdbg_truth scene credit = matched / max(expected, claimed) (overclaims cost);
       request_efficiency requires all vendor traps passed in EVERY branch awarding >= 0.75,
       and the dead first line of the under-optimum branch is gone.
  F16  b_summary_currency caps on an actual cross-currency money SUM, never on a key name.
  F17/F18  harness-attributable absence (probe error, missing BENCH_VIZ_BUCKETS, section
       lost to a timeout, missing vendor-mock surface) is scored as PROBE UNAVAILABLE: the
       row is excluded from its tier mean and listed loudly in the verdict — never converted
       into an app zero. App-attributable absence (no canvas, no rows, no vsdbg) stays 0.
  F22  t_fallback's notice credit prefers the probe's differential assertion when present.
  F24  v_currency_rendered grades the minor-unit exponent from the rendered text — symbol,
       placement and separators are free.

Calibration-owned rungs (drag-frame floor, under-load ladder, k_P, gamma knobs, side-face
achievable fraction, overlap floor) load from bench/sb6-thresholds.json. Until the Bedrock
fit freezes them the file says "calibrated": false and every report carries a loud
UNCALIBRATED banner. A file claiming calibrated:true whose sha256 does not match the pin
baked below makes the scorer REFUSE to score (structural refusal, never a silent default).

Interface mirrors score_build.py exactly — gather(root, vendor_port, db, trace_path,
mark_phase=None) -> Ctx and evaluate(ctx) -> dict — so run_build.py swaps modules under
BENCH_SB6 with no flow changes. sb-5.x files are untouched; this module is additive.

Sibling surfaces this module consumes (guarded — a missing surface is a named harness
refusal, not a crash and not an app zero):
  fixtures_sb6 / vendor_service_v2:  EXPECTED_TOTAL, EXPECTED_BY_CURRENCY, EXPECTED_BUCKETS,
      OPTIMAL_REQUESTS, EXPECTED_WEBHOOK_COUNTERS
  vendor_service_v2 (module):  serve(port, trace), mark_phase(name), LIST_PATH,
      set_visible_cap(n)/clear_visible_cap()  [F2 half-seed], arm_conflict_once()/
      arm_conflict_second()/conflict_trace()  [412 dance], batch_trace(), AMOUNT_LIMIT,
      arm_stall(secs)  [F8b], trap_results(), run_delivery_script() or
      POST /admin/deliver-script  [F3], mutate_statuses()/restore_payments()
  product_probe_v2.mjs:  scenarios load|sync|error|empty|viz|viz-fallback per Artifact 3
      + red-team emit additions (sceneMoved, picks[].tableOk, noticeDifferentialOk,
      firstDrawMs, amountCells, optimistic causal proof, incremental emit with timedOut).
"""

from __future__ import annotations

import hashlib
import importlib
import json
import os
import re
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path
from typing import Callable, Dict, List, Optional

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

# Shared primitives — same helpers, same conventions, one definition (score_build is sb-5's
# module; importing it runs only registrations, no side effects on the sb-6 path).
from score_build import (  # noqa: E402
    _classes, _free_port, _get, _instant, _ladder, _pe, _post, _req, _rows_from_raw, g,
)
import perf_probe  # noqa: E402

# ── frozen vocabulary (spec-build-v3 §4/§5 — verbatim from the package) ──────────────────────

STATUS_ORDER = ["settled", "pending", "refunded", "failed"]
STATUS_HEX = {"settled": (22, 163, 74), "pending": (245, 158, 11),
              "refunded": (139, 92, 246), "failed": (220, 38, 38)}
CURRENCY_EXP = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
PAYMENT_KEYS_V3 = {"id", "amount_minor", "currency", "created_at", "settled_at", "status",
                   "version", "note", "counterparty_name", "country"}
WEB_FILES = ("web/index.html", "web/styles.css", "web/app.js", "web/viz.js")
ASSET_BUDGET_BYTES = 150 * 1024
LIST_LIMIT_DEFAULT, LIST_LIMIT_CAP = 50, 200

# ── calibration-owned thresholds (sb6-thresholds.json; UNCALIBRATED until the fit lands) ─────

THRESHOLDS_FILE = HERE / "sb6-thresholds.json"
CALIB_SHA256 = "723e71b5a029885103b2a4c61674bc8c8c6ca415ac397e20f3f7ecd1aa66490a"  # baked at freeze; a calibrated file must hash to this

_DEFAULT_THRESHOLDS = {
    "calibrated": False,
    "note": "UNCALIBRATED defaults — spec-traceable budgets only; the Bedrock fit has not run",
    "gamma_core": 1.0,
    "gamma_hard": 1.0,
    "k_p": 1.0,
    "gamma_cap": 4.0,
    # F4: frames per scripted 2-s drag (spec floor 24 per drag; unit is the spec's unit)
    "drag_frames_rungs": [[1.0, 24], [0.75, 18], [0.5, 12], [0.25, 6]],
    "underload_p95_rungs": [[1.0, 150], [0.75, 300], [0.5, 600], [0.25, 1200]],
    "underload_overlap_floor": 0.5,        # F8c: below this, load is unproven -> refusal
    "tooltip_ms_rungs": [[1.0, 150], [0.6, 400], [0.3, 1200]],
    "optimistic_paint_rungs": [[1.0, 100], [0.6, 250], [0.3, 800]],
    "side_face_achievable": 0.75,          # antialiased-edge misses, measured on the reference
    "vsdbg_proj_tol_px": 2.0,
}


def _load_thresholds() -> Dict:
    if not THRESHOLDS_FILE.is_file():
        return dict(_DEFAULT_THRESHOLDS)
    raw = THRESHOLDS_FILE.read_bytes()
    try:
        data = json.loads(raw)
    except Exception:
        raise SystemExit(f"sb6-thresholds.json is unparsable — refusing to score: {THRESHOLDS_FILE}")
    merged = {**_DEFAULT_THRESHOLDS, **data}
    if merged.get("calibrated"):
        digest = hashlib.sha256(raw).hexdigest()
        if CALIB_SHA256 == "TBD-AT-FREEZE" or digest != CALIB_SHA256:
            # A file CLAIMING calibration without a matching baked pin is tampered or stale.
            raise SystemExit(
                "sb6-thresholds.json claims calibrated=true but its sha256 does not match the "
                f"pin baked into score_sb6.py ({digest[:16]}… vs {CALIB_SHA256[:16]}…) — "
                "refusing to score. Re-freeze or restore the artifact.")
    return merged


TH = _load_thresholds()
CALIBRATED = bool(TH.get("calibrated"))
GAMMA = {"core": float(TH["gamma_core"]), "hard": float(TH["gamma_hard"])}
K_P = float(TH["k_p"])
GAMMA_CAP = float(TH.get("gamma_cap", 4.0))
assert GAMMA["core"] <= GAMMA_CAP and GAMMA["hard"] <= GAMMA_CAP, "gamma over cap"

UNCALIBRATED_BANNER = (
    "⚠⚠ SB-6 UNCALIBRATED — sb6-thresholds.json carries honest defaults; gamma/k_P/drag/"
    "side-face rungs are NOT frozen. Scores are rc-grade (sb-6.0-rc), never board-grade. ⚠⚠")

# sb-6.1 (2026-08-18 overnight): rendered-means-seen on the first-data stamp (hidden
# fallback rows no longer earn p_page_interactive), causal-evidence sync "completed"
# (observed /api/sync request / in-flight state / view change — never elapsed time alone),
# reachable j_empty_state middle rung (empty scenario now reports bodyTextLength),
# serves_page 0.2 rung requires an answering server, honest vendor-trap unavail wording.
# Thresholds file unchanged — CALIB_SHA256 pin carries over.
SCORER_VERSION = "sb-6.1" if CALIBRATED else "sb-6.1-rc"

# ── weights + the compression-proof asserts (deleting one is a loud diff) ────────────────────

TIER_WEIGHT_SB6 = {"A": 0.06, "B": 0.12, "C": 0.12, "D": 0.10,
                   "J": 0.12, "V": 0.08, "P": 0.06, "T": 0.14, "HARD": 0.20}
E_WEIGHT = 0.12
assert abs(sum(TIER_WEIGHT_SB6.values()) - 1.0) < 1e-9
assert TIER_WEIGHT_SB6["T"] + TIER_WEIGHT_SB6["HARD"] >= 0.34, "hard-axis weight floor"
assert E_WEIGHT >= 0.10, "excellence slice floor"

# Absorbed-by-compound checks: computed and reported, weight zero (anti-stacking).
DIAGNOSTIC = {"resync_idempotent", "update_propagation", "restart_persistence",
              "concurrent_sync_safe", "store_atomic_upsert",
              "j_loads_data", "j_console_clean", "local_pagination", "input_validation"}

# Checks whose rungs are calibration-owned — exempt from the --reference 0.95 sweep (their
# cut points are SET from the reference distribution, so grading the reference against them
# pre-freeze is circular). Everything else the reference must ace (F7 / G6).
CALIBRATION_OWNED = {"e_frames_under_drag", "e_under_load_latency", "e_optimistic_paint",
                     "t_tooltip", "p_list_latency", "p_buckets_latency", "p_summary_latency",
                     "p_page_interactive", "p_first_frame", "p_sync_wall"}

# ── sb-5-lineage change riding the sb-6 version: partial-sync attribution ────────────────────
# ROOT_BLOCKS extension (package §2 table row 8): sync_completeness < 1.0 — not only == 0 —
# attributes the dependents, so one partial-sync defect stops reading as eight failures.
ROOT_BLOCKS = {
    "sync_completeness": (
        "payment_row_shape", "total_field", "chronological_order", "b_summary_currency",
        "summary_bounds_utc", "b_buckets_dst", "row_integrity", "h_sync_discipline",
    ),
}


def attribute_root_causes(rows: List[Dict]) -> Dict[str, List[str]]:
    """Attribution only — no score changes. sb-6: the root fires on ANY shortfall (<1.0)."""
    by_name = {r["check"]: r for r in rows}
    out: Dict[str, List[str]] = {}
    for root, dependents in ROOT_BLOCKS.items():
        r = by_name.get(root)
        if r is None or r["score"] >= 1.0:
            continue
        blocked = [d for d in dependents if d in by_name and by_name[d]["score"] < 1.0]
        if blocked:
            out[root] = blocked
    return out


# ── sibling-surface access (guarded: missing surface = named refusal, never a crash) ─────────

_FX_CACHE: List = []


def _fx():
    """The sb-6 fixture constants module, or None. Never raises at import time."""
    if not _FX_CACHE:
        found = None
        for name in ("fixtures_v2", "fixtures_sb6", "vendor_service_v2"):
            try:
                m = importlib.import_module(name)
            except Exception:
                continue
            if all(hasattr(m, k) for k in ("EXPECTED_TOTAL", "EXPECTED_BY_CURRENCY",
                                           "EXPECTED_BUCKETS", "OPTIMAL_REQUESTS")):
                found = m
                break
        _FX_CACHE.append(found)
    return _FX_CACHE[0]


def _vendor():
    try:
        return importlib.import_module("vendor_service_v2")
    except Exception:
        return None


def unavail(reason: str) -> Dict:
    """F17/F18: harness-attributable absence. Excluded from the tier mean and listed in the
    verdict — an unproven measurement licenses nothing, in either direction."""
    return {"score": 0.0, "detail": f"PROBE UNAVAILABLE: {reason}"[:300],
            "consequence": "harness failure, not app evidence", "unavailable": True}


def _frac(d: Optional[Dict]) -> float:
    d = d or {}
    return (d.get("ok", 0) / d["total"]) if d.get("total") else 0.0


def compound(components: Dict[str, float], gates: Dict[str, bool]) -> tuple:
    """gate × min. Gates are binary preconditions (absence scores 0 — the vacuous rule);
    the weakest component bounds the whole, so one defect stops stacking partial credit."""
    if not all(gates.values()):
        return 0.0, {"gate_failed": [k for k, v in gates.items() if not v], **components}
    return min(components.values()), {**{f"gate:{k}": True for k in gates}, **components}


def _ang_dist(a: float, b: float) -> float:
    """F11: angular distance mod 360 — yaw normalization is render-equivalent."""
    d = abs(a - b) % 360.0
    return min(d, 360.0 - d)


def _kp_ladder(value, rungs) -> float:
    """P-tier value ladder with the top rung tightened by k_P (spec budgets stay printed)."""
    if not rungs:
        return 0.0
    scaled = [(s, (b / K_P if i == 0 else b)) for i, (s, b) in enumerate(rungs)]
    return _ladder(value, scaled)


def _money_ok(currency: str, minor, text: str) -> bool:
    """F24: exponent + digits, never presentation. Digits of the rendered number must equal
    the minor amount; decimal places must equal the currency's exponent (a trailing 3-digit
    group with exponent 0 reads as thousands grouping and passes)."""
    if not isinstance(minor, int) or not isinstance(text, str) or currency not in CURRENCY_EXP:
        return False
    digits = re.sub(r"\D", "", text)
    if digits != str(minor):
        return False
    e = CURRENCY_EXP[currency]
    m = re.search(r"[.,](\d+)\s*\D*$", text)
    dec_len = len(m.group(1)) if m else None
    if e == 0:
        return dec_len not in (1, 2)      # 1–2 trailing decimals on JPY is wrong money
    return dec_len == e or (e == 3 and dec_len == 3)


def _rgb_of(css: str) -> Optional[tuple]:
    m = re.match(r"rgba?\((\d+),\s*(\d+),\s*(\d+)", str(css or ""))
    return (int(m.group(1)), int(m.group(2)), int(m.group(3))) if m else None


def _near_rgb(a, b, tol=8) -> bool:
    return bool(a and b) and all(abs(x - y) <= tol for x, y in zip(a, b))


# ── probe runner (env passthrough for BENCH_VIZ_BUCKETS; viz gets the F18 long budget) ───────

def _probe(scenario: str, base: str, *flags: str, env: Optional[Dict] = None) -> Dict:
    node = os.environ.get("GOOSE_SWARM_RENDER_NODE", "node")
    script = HERE / "product_probe_v2.mjs"
    if not script.is_file():
        return {"_probe_error": f"{scenario}: product_probe_v2.mjs not present (sibling deliverable)"}
    cmd = [node, str(script), scenario, base, *flags]
    timeout = 200 if scenario.startswith("viz") else 120   # F18: viz budget 150s + margin
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout,
                           env={**os.environ, **(env or {})})
        try:
            return json.loads(r.stdout)
        except Exception as e:
            return {"_probe_error": f"{scenario}: {type(e).__name__} (exit {r.returncode}, "
                                    f"stderr: {(r.stderr or '')[:200]})"[:400]}
    except Exception as e:
        return {"_probe_error": f"{scenario}: {type(e).__name__}: {e}"[:300]}


# ── context ──────────────────────────────────────────────────────────────────────────────────

class Ctx:
    def __init__(self, root: Path):
        self.root = root
        self.pkg = root / "vspro"
        self.html = ""                      # index + all three assets, joined (source greps)
        self.asset_bytes: Dict[str, Optional[int]] = {}
        self.asset_ctypes: Dict[str, str] = {}
        self.harness_missing: List[str] = []   # vendor-mock surfaces gather could not find
        self.boot_error = ""
        self.boot_secs: Optional[float] = None
        self.port_bound = False
        self.proc_alive_5s = False
        self.health_boot: Dict = {}
        self.root_status: Optional[int] = None
        self.sync1: Dict = {}
        self.sync2: Dict = {}
        self.sync3: Dict = {}
        self.sync_fill: Dict = {}
        self.half_seeded = False
        self.payments: Dict = {}
        self.page5: Dict = {}
        self.capped: Dict = {}
        self.sorted_desc: Dict = {}
        self.filtered: Dict = {}
        self.summary: Dict = {}
        self.buckets: Dict = {}
        self.raw_pages: List = []
        self.api_ctype = ""
        self.json_probe: List = []
        self.val_matrix: List = []          # (path, status, status_ok) — status-code half only
        self.envelope_cases: List = []      # envelope-shape half (anti-stacking split)
        self.batch_result: Dict = {}
        self.batch_trace: Dict = {}
        self.conflict_trace: Dict = {}
        self.note_result: Dict = {}
        self.under_load: Dict = {}
        self.concurrent_total: Optional[int] = None
        self.update_changed = 0
        self.update_seen = 0
        self.rows_before_kill = 0
        self.rows_after_restart = -1
        self.health2: Dict = {}             # after restart + scripted webhook deliveries (F3)
        self.webhook_script: Dict = {}
        self.trap_results: Dict = {}
        self.trace: List[Dict] = []
        self.sync1_reqs = 0
        self.sync2_reqs = 0
        self.sync2_304 = 0
        self.sync2_cond = 0
        self.max_limit_used = 0
        self.probe_load: Dict = {}
        self.probe_sync: Dict = {}
        self.probe_error: Dict = {}
        self.probe_empty: Dict = {}
        self.probe_note: Dict = {}
        self.probe_viz: Dict = {}
        self.probe_viz_fallback: Dict = {}
        self.probe_viz_empty: Dict = {}
        self.perf: Dict = {}
        self._hard_rows: List[Dict] = []


def _viz(c: Ctx) -> Dict:
    return c.probe_viz if isinstance(getattr(c, "probe_viz", None), dict) else {}


def _viz_section(c: Ctx, key: str):
    """F18: incremental-emit awareness. Returns (section, refusal-or-None): a section absent
    from a timed-out emit is harness loss, not app evidence."""
    p = _viz(c)
    if _pe(p):
        return None, unavail(_pe(p))
    if key not in p:
        if p.get("timedOut"):
            return None, unavail(f"viz section '{key}' lost to the probe's hard cap (timedOut)")
        return None, None    # probe completed and the section is genuinely absent -> app truth
    return p.get(key), None


# ── registry ─────────────────────────────────────────────────────────────────────────────────

SB6_CHECKS: List[tuple] = []


def check(name: str, tier: str) -> Callable:
    def deco(fn):
        SB6_CHECKS.append((name, tier, fn))
        return fn
    return deco


product_check = check   # one registry; the alias keeps the score_build idiom readable


# ══ A: structure and runtime (ladders, no bare bits) ═════════════════════════════════════════

@check("modules_present", "A")
def _(c: Ctx):
    want = ["meridian.py", "store.py", "api.py", "__main__.py"] + list(WEB_FILES)
    have = [w for w in want if (c.pkg / w).is_file()]
    return g(len(have) / len(want), f"{len(have)}/{len(want)} named files",
             "files the spec names by path are missing",
             parts={w: (w in have) for w in want})


@check("interfaces_declared", "A")
def _(c: Ctx):
    client = set(_classes(c.pkg / "meridian.py").get("MeridianClient", []))
    store = set(_classes(c.pkg / "store.py").get("Store", []))
    want_c = {"fetch_all_payments", "get_payment", "total_count", "create_payment",
              "create_batch", "update_payment", "register_webhook"}
    want_s = {"upsert_many", "query", "get", "apply_event", "buckets", "count",
              "last_sync", "set_last_sync"}
    score = (len(client & want_c) + len(store & want_s)) / (len(want_c) + len(want_s))
    missing = sorted((want_c - client) | (want_s - store))
    return g(score, f"{len(client & want_c) + len(store & want_s)}/15 methods"
             + (f", missing {missing[:5]}" if missing else ""),
             "the named interface is not implemented as specified",
             parts={**{f"MeridianClient.{m}": (m in client) for m in sorted(want_c)},
                    **{f"Store.{m}": (m in store) for m in sorted(want_s)}})


@check("server_runs", "A")
def _(c: Ctx):
    if c.health_boot:
        s = 1.0 if (c.boot_secs or 99) <= 5 else 0.75
        return g(s, f"healthy in {c.boot_secs}s", "")
    if c.port_bound:
        return g(0.4, "port bound but /api/health non-200", "the app runs but cannot report health")
    if c.proc_alive_5s:
        return g(0.15, "process survives 5s without binding", "the tool does not serve")
    return g(0.0, f"crash at boot ({c.boot_error[:100]})", "the tool does not run at all")


@check("serves_page", "A")
def _(c: Ctx):
    if c.root_status != 200 or "<" not in c.html:
        exists = (c.pkg / "web/index.html").is_file()
        # sb-6.1: the file-on-disk 0.2 rung applies only when a server actually answered —
        # partial credit while the server is provably down graded a corpse as "serving".
        answered = c.root_status is not None
        return g(0.2 if (exists and answered) else 0.0, f"GET / -> {c.root_status}",
                 "the page is not served by the backend")
    assets = ["web/styles.css", "web/app.js", "web/viz.js"]
    ok = 0
    for a in assets:
        name = a.split("/")[-1]
        ctype = c.asset_ctypes.get(name, "")
        want = "css" if name.endswith(".css") else "javascript"
        if c.asset_bytes.get(name) and want in ctype:
            ok += 1
    s = 1.0 if ok == 3 else (0.7 if ok == 2 else 0.4)
    return g(s, f"page 200, {ok}/3 assets served with correct content types",
             "assets missing or mistyped break the page in a real browser",
             parts={a: bool(c.asset_bytes.get(a.split('/')[-1])) for a in assets})


@check("a_asset_budget", "A")
def _(c: Ctx):
    sizes = {w: (c.pkg / w).stat().st_size if (c.pkg / w).is_file() else None for w in WEB_FILES}
    known = [v for v in sizes.values() if v is not None]
    if not known:
        return g(0.0, "no web files on disk", "there is no frontend")
    total = sum(known)
    s = 1.0 if total <= ASSET_BUDGET_BYTES else (0.3 if total <= ASSET_BUDGET_BYTES * 1.2 else 0.0)
    return g(s, f"{total // 1024} KB of {ASSET_BUDGET_BYTES // 1024} KB budget "
             f"({len(known)}/4 files present)",
             "the budget exists so a vendored 3D library cannot",
             parts={k: v for k, v in sizes.items()})


@check("health_shape", "A")
def _(c: Ctx):
    top = sum(1 for k in ("status", "payments", "last_sync", "webhook") if k in c.health_boot)
    wh = c.health_boot.get("webhook") or {}
    sub = sum(1 for k in ("registered", "received", "applied", "ignored", "rejected") if k in wh)
    return g((top + sub) / 9, f"{top}/4 top keys, {sub}/5 webhook counters",
             "monitoring cannot read the documented shape")


@check("sync_shape", "A")
def _(c: Ctx):
    keys = {"fetched", "inserted", "updated", "total"}
    have = keys & set(c.sync1 or {})
    return g(len(have) / 4, f"{len(have)}/4 documented keys",
             "the caller cannot tell what a sync did")


# ══ B: behaviour ═════════════════════════════════════════════════════════════════════════════

@check("sync_completeness", "B")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing (fixtures_sb6/vendor_service_v2)")
    # The half-seed journey (F2) makes the FULL count live after the probe's sync click;
    # sync_fill covers a probe that never clicked. Grade the best evidenced full state.
    candidates = [c.health2.get("payments"), (c.sync_fill or {}).get("total"),
                  (c.sync2 or {}).get("total"), c.payments.get("total")]
    got = max((v for v in candidates if isinstance(v, int)), default=0)
    return g(min(got / fx.EXPECTED_TOTAL, 1.0),
             f"{got}/{fx.EXPECTED_TOTAL} payments after full sync",
             "an incomplete sync silently loses payments")


@check("total_field", "B")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    total = c.payments.get("total")
    if total == fx.EXPECTED_TOTAL:
        s = 1.0
    elif isinstance(total, int) and abs(total - fx.EXPECTED_TOTAL) <= 2:
        s = 0.5
    elif isinstance(total, int) and abs(total - fx.EXPECTED_TOTAL) <= fx.EXPECTED_TOTAL * 0.2:
        s = 0.25
    else:
        s = 0.0
    return g(s, f"total={total} (want {fx.EXPECTED_TOTAL})", "a wrong total breaks the caller's paging")


@check("payment_row_shape", "B")
def _(c: Ctx):
    data = c.payments.get("data") or []
    if not data:
        return g(0.0, "no rows", "n/a")
    keys = set(data[0])
    s = len(keys & PAYMENT_KEYS_V3) / len(PAYMENT_KEYS_V3)
    if keys - PAYMENT_KEYS_V3:
        s = max(0.0, s - 0.2)   # "exactly the keys" — extras cost
    return g(s, f"{len(keys & PAYMENT_KEYS_V3)}/10 documented keys"
             + (f", extras {sorted(keys - PAYMENT_KEYS_V3)[:3]}" if keys - PAYMENT_KEYS_V3 else ""),
             "the documented row contract is not met")


@check("chronological_order", "B")
def _(c: Ctx):
    data = c.payments.get("data") or []
    if len(data) < 2:
        return g(0.0, "too few rows", "n/a")
    times = [_instant(r.get("created_at")) for r in data]
    if any(t is None for t in times):
        return g(0.0, "unparseable created_at", "timestamps are not RFC3339")
    pairs = sum(1 for a, b in zip(times, times[1:]) if a <= b)
    return g(pairs / (len(times) - 1), f"{pairs}/{len(times) - 1} adjacent pairs ordered by instant",
             "mixed offsets make string ordering wrong; payments show out of sequence")


@check("summary_bounds_utc", "B")
def _(c: Ctx):
    oldest, newest = c.summary.get("oldest"), c.summary.get("newest")
    present = 0.4 if oldest and newest else 0.0
    utc = 0.3 if (oldest and newest and all(
        str(v).endswith("Z") or "+00:00" in str(v) for v in (oldest, newest))) else 0.0
    ordered = 0.3 if oldest and newest and (_instant(oldest) or 0) < (_instant(newest) or 0) else 0.0
    return g(present + utc + ordered, f"oldest={oldest} newest={newest}",
             "local-offset or missing bounds misstate the period covered")


@check("b_summary_currency", "B")
def _(c: Ctx):
    """k of 4 per-currency (count, total_minor) pairs exact -> k/4. F16: the cross-currency
    cap fires only on an actual money SUM across currencies, never on a key name."""
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    s = c.summary or {}
    by = {x.get("currency"): x for x in s.get("by_currency", []) if isinstance(x, dict)}
    exp = fx.EXPECTED_BY_CURRENCY
    k = sum(1 for cur, e in exp.items()
            if by.get(cur, {}).get("count") == e["count"]
            and by.get(cur, {}).get("total_minor") == e["total_minor"])
    score = k / len(exp)
    cross_sum = sum(e["total_minor"] for e in exp.values())
    singles = {e["total_minor"] for e in exp.values()}
    offending = [key for key, v in s.items()
                 if isinstance(v, int) and v == cross_sum and v not in singles]
    if offending:
        score = min(score, 0.25)
    order = [x.get("currency") for x in s.get("by_currency", []) if isinstance(x, dict)]
    ordered = order == sorted(order)
    return g(score if ordered else score * 0.9,
             f"{k}/{len(exp)} currency buckets exact"
             + (f"; CROSS-CURRENCY SUM in {offending}" if offending else "")
             + ("" if ordered else "; by_currency not sorted"),
             "money summed across currencies is wrong money",
             parts={"exact_buckets": k, "cross_sum_fields": offending, "sorted": ordered})


@check("b_buckets_dst", "B")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    got = {(x.get("day"), x.get("status")): x.get("count")
           for x in (c.buckets or {}).get("cells", []) if isinstance(x, dict)}
    want = {(x["day"], x["status"]): x["count"] for x in fx.EXPECTED_BUCKETS["cells"]}
    if not got:
        return g(0, "no cells returned", "the buckets endpoint is the 3D chart's data source")
    exact = sum(got.get(k) == v for k, v in want.items()) / len(want)
    shape = 1.0 if len(got) == len(want) else 0.5
    tz = 1.0 if (c.buckets or {}).get("timezone") == "Europe/Berlin" else 0.0
    return g(0.7 * exact + 0.2 * shape + 0.1 * tz,
             f"{sum(got.get(k) == v for k, v in want.items())}/{len(want)} cells exact, "
             f"shape {len(got)}/{len(want)}, tz={(c.buckets or {}).get('timezone')}",
             "Berlin-day bucketing across the DST switch is the data-correctness trap")


@check("b_error_envelope", "B")
def _(c: Ctx):
    """Envelope SHAPE half (the status-code half lives in c_api_depth's matrix — one
    measurement, one credit carrier each)."""
    cases = c.envelope_cases or []
    if not cases:
        return g(0, "no error responses observed", "an API that cannot say what went wrong")
    ok = sum(1 for x in cases if x.get("envelope_ok")) / len(cases)
    fe = [x for x in cases if x.get("expects_field_errors")]
    paths = (sum(1 for x in fe if x.get("field_paths_ok")) / len(fe)) if fe else 0.0
    return g(0.6 * ok + 0.4 * paths,
             f"envelope {ok:.2f}, field paths {paths:.2f} over {len(cases)} cases",
             "structured errors are the contract's error half",
             parts={"cases": [{k: x.get(k) for k in ('path', 'envelope_ok', 'field_paths_ok')}
                              for x in cases]})


@check("row_integrity", "B")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    all_rows, float_seen = [], False
    for _st, raw in c.raw_pages:
        rows, f = _rows_from_raw(raw)
        all_rows.extend(rows)
        float_seen = float_seen or f
    if not all_rows:
        return g(0.0, "no rows on the wire", "n/a")
    keys_ok = sum(1 for r in all_rows if isinstance(r, dict) and set(r) >= PAYMENT_KEYS_V3)
    ints_ok = 0 if float_seen else sum(
        1 for r in all_rows if isinstance(r.get("amount_minor"), int))
    times_ok = sum(1 for r in all_rows
                   if isinstance(r.get("created_at"), str) and len(r["created_at"]) >= 19)
    ids = {r.get("id") for r in all_rows if isinstance(r, dict)}
    n = len(all_rows)
    score = (0.35 * keys_ok / n + 0.25 * ints_ok / n + 0.15 * times_ok / n
             + 0.25 * min(len(ids) / fx.EXPECTED_TOTAL, 1.0))
    return g(score,
             f"{n} rows: keys {keys_ok}/{n}, int-amounts {ints_ok}/{n}"
             + (" (FLOAT ON WIRE)" if float_seen else "")
             + f", times {times_ok}/{n}, ids {len(ids)}/{fx.EXPECTED_TOTAL}",
             "malformed rows poison every consumer downstream of the API")


@check("json_everywhere", "B")
def _(c: Ctx):
    if not c.json_probe:
        return g(0.0, "no responses sampled", "n/a")
    total = sum((0.5 if parses else 0.0) + (0.5 if "json" in (ctype or "").lower() else 0.0)
                for _p, _s, parses, ctype in c.json_probe)
    return g(total / len(c.json_probe), f"{total:.1f}/{len(c.json_probe)} responses fully JSON",
             "an HTML error page mid-API breaks every client that trusted the contract")


@check("ui_offline", "B")
def _(c: Ctx):
    if not c.html.strip():
        return g(0.0, "no page to inspect", "there is no frontend")
    cdn = re.findall(r'(?:src|href)\s*=\s*["\']https?://[^"\']+', c.html)
    cdn += re.findall(r'\bimport\s*\(?["\']https?://[^"\']+', c.html)
    return g(1.0 if not cdn else 0.0, f"{len(cdn)} external asset ref(s)",
             "the spec requires zero external code; a CDN ref breaks offline operation")


# diagnostics absorbed by compounds (weight 0, still reported)

@check("local_pagination", "B")
def _(c: Ctx):
    parts = [
        len(c.payments.get("data") or []) == LIST_LIMIT_DEFAULT,
        c.capped.get("limit") == LIST_LIMIT_CAP and len(c.capped.get("data") or []) <= LIST_LIMIT_CAP,
        len(c.page5.get("data") or []) == 5,
    ]
    return g(sum(parts) / 3, f"default50/cap200/offset: {parts}",
             "the local API's own paging contract is not met")


@check("input_validation", "B")
def _(c: Ctx):
    if not c.val_matrix:
        return g(0.0, "matrix not exercised", "n/a")
    score = sum(cell for (_p, _s, cell) in c.val_matrix) / len(c.val_matrix)
    return g(score, f"{score * len(c.val_matrix):.1f}/{len(c.val_matrix)} status codes correct",
             "invalid input accepted or crashing instead of the documented rejection")


@check("resync_idempotent", "B")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    if not c.sync2:
        return g(0.0, "second sync failed", "re-running the tool is not safe")
    inserted, total = c.sync2.get("inserted"), c.sync2.get("total")
    ok = inserted == 0 and total == fx.EXPECTED_TOTAL
    return g(1.0 if ok else (0.5 if total == fx.EXPECTED_TOTAL else 0.0),
             f"second sync inserted={inserted} total={total}",
             "re-running duplicates rows and inflates the ledger")


# ══ C: vendor contract ═══════════════════════════════════════════════════════════════════════

def _list_events(c: Ctx, seg: List[Dict]) -> List[Dict]:
    v = _vendor()
    list_path = getattr(v, "LIST_PATH", "/v2/payments") if v else "/v2/payments"
    return [e for e in seg if e.get("path") == list_path and e.get("method") == "GET"]


@check("vendor_traps", "C")
def _(c: Ctx):
    """Retry-After (both forms), cursor expiry restart, stall survival — read from the v2
    mock's own trap ledger (F14: the stall is documented in vendor_docs before it is graded)."""
    t = c.trap_results
    if not t:
        # sb-6.1 wording: trap_results() exists in the harness — an empty ledger means the
        # harvest never ran because the app never exercised the vendor's trap surfaces
        # (usually a dead server), not that the harness lost the instrument.
        return unavail("vendor trap ledger empty — trap surfaces never exercised "
                       "(app/server never drove the vendor)")
    keys = ("retry_after", "cursor_expiry", "stall")
    vals = {k: float(t.get(k, 0.0)) for k in keys}
    return g(sum(vals.values()) / len(keys), f"{vals}",
             "a client that did not read the docs fails the vendor's documented behaviours",
             parts=vals)


@check("vendor_cursor_paging", "C")
def _(c: Ctx):
    """The client pages with the documented cursor and nothing else. Offenders are list
    requests carrying undocumented paging params (offset/page/start/skip); legitimate
    walk-starts (no params) are normal — gather triggers many walks, so a transitions
    fraction would punish the number of walks, not the protocol."""
    if not c.trace:
        return unavail("no vendor trace")
    lists = _list_events(c, c.trace)
    if not lists:
        return g(0.0, "no list requests at all", "the client never paged the collection")
    with_cursor = sum(1 for e in lists if (e.get("query") or {}).get("cursor"))
    offenders = sum(1 for e in lists
                    if any(k in (e.get("query") or {}) for k in ("offset", "page", "start", "skip")))
    if not with_cursor:
        return g(0.0, f"0/{len(lists)} list requests used a cursor",
                 "the collection cannot be walked without the documented cursor")
    return g(max(0.0, 1.0 - offenders / len(lists)),
             f"{with_cursor}/{len(lists)} cursor requests, {offenders} undocumented-param requests",
             "offset-guessing instead of the documented cursor protocol",
             parts={"with_cursor": with_cursor, "offenders": offenders, "lists": len(lists)})


@check("c_batch_partial", "C")
def _(c: Ctx):
    r, t = c.batch_result or {}, c.batch_trace or {}
    if not r and not t:
        return unavail("batch exercise not run (vendor v2 surface or endpoint absent)")
    results = r.get("results") if isinstance(r.get("results"), list) else []
    order_kept = bool(results) and all(x.get("index") == i for i, x in enumerate(results))
    failed = [x for x in results if x.get("status") == "error"]
    failed_reported = any((x.get("error") or {}).get("code") == "amount_over_limit" for x in failed)
    parts = {"order_kept": order_kept,
             "succeeded_once": t.get("duplicate_creates", 9) == 0,
             "failed_reported": failed_reported,
             "no_retry": t.get("retries_of_failed_item", 9) == 0
                         and t.get("fresh_key_retries", 9) == 0}
    return g(sum(parts.values()) / 4, f"{parts}",
             "partial failure is a normal outcome, not a rollback or a retry storm", parts=parts)


@check("c_api_depth", "C")
def _(c: Ctx):
    """COMPOUND gate × min(schema, pagination, validation). Absorbs local_pagination and
    input_validation (weight-0 diagnostics above)."""
    all_rows = []
    for _st, raw in c.raw_pages:
        rows, _f = _rows_from_raw(raw)
        all_rows.extend(rows)
    schema = (sum(1 for r in all_rows if isinstance(r, dict) and set(r) == PAYMENT_KEYS_V3)
              / len(all_rows)) if all_rows else 0.0
    pag_cells = [
        len(c.payments.get("data") or []) == LIST_LIMIT_DEFAULT,
        c.capped.get("limit") == LIST_LIMIT_CAP and len(c.capped.get("data") or []) <= LIST_LIMIT_CAP,
        len(c.page5.get("data") or []) == 5,
        bool(c.sorted_desc.get("data")) and _instant((c.sorted_desc["data"][0] or {}).get("created_at"))
        == max((_instant(r.get("created_at")) or 0) for r in c.sorted_desc["data"]),
        isinstance(c.filtered.get("total"), int)
        and all(r.get("status") == "refunded" for r in (c.filtered.get("data") or [])),
    ]
    pagination = sum(bool(x) for x in pag_cells) / len(pag_cells)
    validation = (sum(cell for (_p, _s, cell) in c.val_matrix) / len(c.val_matrix)) \
        if c.val_matrix else 0.0
    score, parts = compound(
        {"schema": round(schema, 3), "pagination": round(pagination, 3),
         "validation": round(validation, 3)},
        {"rows_on_wire": bool(all_rows)})
    return g(score, f"schema={schema:.2f} pagination={pagination:.2f} validation={validation:.2f}",
             "the API surface is the contract — depth, not the happy path", parts=parts)


@check("update_propagation", "C")
def _(c: Ctx):
    ran = isinstance(c.sync3, dict) and any(k in c.sync3 for k in ("inserted", "total", "fetched"))
    if not c.update_changed or not ran:
        return g(0.0, "mutation pass not exercised or sync3 absent", "n/a")
    return g(min(c.update_seen / c.update_changed, 1.0),
             f"{c.update_seen}/{c.update_changed} mutated statuses visible locally after sync3",
             "an app that only INSERTS shows stale statuses forever")


@check("restart_persistence", "C")
def _(c: Ctx):
    if not c.rows_before_kill:
        return g(0.0, "no rows before the kill — nothing to prove persistence with", "n/a")
    if c.rows_after_restart < 0:
        return g(0.0, f"app did not come back after SIGKILL (had {c.rows_before_kill} rows)",
                 "a crash loses the ledger")
    return g(min(c.rows_after_restart / c.rows_before_kill, 1.0),
             f"{c.rows_after_restart}/{c.rows_before_kill} rows survived kill+reboot",
             "rows that vanish on restart mean the SQLite file was decoration")


# ══ D: finesse ═══════════════════════════════════════════════════════════════════════════════

@check("api_content_type", "D")
def _(c: Ctx):
    if not c.json_probe:
        return g(0.0, "no responses sampled", "n/a")
    ok = sum(1 for _p, _s, _parses, ctype in c.json_probe if "json" in (ctype or "").lower())
    return g(ok / len(c.json_probe), f"{ok}/{len(c.json_probe)} JSON content types",
             "a wrong content type breaks strict clients")


@check("client_timeouts", "D")
def _(c: Ctx):
    """Behaviour first (the --stall trap: sync returned within bound), source grep residual
    0.4 (the check that used to decide the Opus–Sonnet order, demoted)."""
    t = c.trap_results
    if t and "stall" in t:
        if float(t.get("stall", 0.0)) >= 1.0:
            return g(1.0, "sync survived the vendor stall within bound", "")
    src = (c.pkg / "meridian.py").read_text(errors="replace") \
        if (c.pkg / "meridian.py").is_file() else ""
    hit = re.search(r"timeout\s*=", src)
    return g(0.4 if hit else 0.0,
             "stall not survived; timeout= present in source" if hit else "no request timeout",
             "one unresponsive vendor call hangs the sync with no recovery")


@check("uses_max_limit", "D")
def _(c: Ctx):
    return g(min(c.max_limit_used / 100, 1.0) if c.max_limit_used else 0.0,
             f"largest vendor page size requested: {c.max_limit_used or 'none'}",
             "not using the documented maximum page size costs avoidable round trips")


@check("store_indexed", "D")
def _(c: Ctx):
    src = (c.pkg / "store.py").read_text(errors="replace") if (c.pkg / "store.py").is_file() else ""
    hit = re.search(r"PRIMARY KEY|UNIQUE|CREATE\s+(UNIQUE\s+)?INDEX", src, re.I)
    return g(1.0 if hit else 0.0, "keyed" if hit else "no primary key or index",
             "every upsert becomes a full table scan")


@check("store_atomic_upsert", "D")
def _(c: Ctx):
    src = (c.pkg / "store.py").read_text(errors="replace") if (c.pkg / "store.py").is_file() else ""
    if not src:
        return g(0.0, "no store.py", "n/a")
    stmt = re.search(r"INSERT[^;\"']*?INTO\s+payments.*?(?=\"\"\"|;|\Z)", src, re.I | re.S)
    scope = stmt.group(0) if stmt else src
    atomic = re.search(r"ON CONFLICT[^;]*DO UPDATE", scope, re.I | re.S)
    weaker = re.search(r"INSERT OR REPLACE|INSERT OR IGNORE|REPLACE INTO", scope, re.I)
    racy = re.search(r"SELECT[^;]*WHERE[^;]*id", src, re.I) and re.search(r"INSERT INTO", src, re.I)
    return g(1.0 if atomic else (0.5 if weaker else (0.3 if racy else 0.0)),
             "ON CONFLICT DO UPDATE" if atomic else
             ("REPLACE/IGNORE — writes but does not merge" if weaker else
              ("select-then-insert" if racy else "no upsert found")),
             "read-then-write duplicates when two syncs overlap")


@check("concurrent_sync_safe", "D")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    if c.concurrent_total is None:
        return g(0.0, "concurrent sync not observed", "n/a")
    return g(max(0.0, 1 - abs(c.concurrent_total - fx.EXPECTED_TOTAL) / fx.EXPECTED_TOTAL),
             f"total after two overlapping syncs: {c.concurrent_total}",
             "two operators pressing Sync at once duplicate or lose rows")


@check("ui_error_actionable", "D")
def _(c: Ctx):
    p = c.probe_error
    if _pe(p):
        return unavail(_pe(p))
    text = str(p.get("actionableText") or "")
    actionable = bool(re.search(r"try again|retry|check|running|refresh", text, re.I))
    if "retryRecovered" in p:
        if p.get("retryRecovered"):
            return g(1.0, "retry affordance clicked and the view recovered", "")
        if actionable:
            return g(0.6, f"actionable text, retry did not recover: {text[:50]}", "")
    elif actionable:
        return g(1.0, f"actionable text: {text[:60]}", "")
    if p.get("errorStateVisible"):
        return g(0.3, "generic error indication", "the user cannot act on it")
    return g(0.0, "no error affordance", "backend failure leaves the user stranded")


# ══ J: journeys ══════════════════════════════════════════════════════════════════════════════

@check("j_loads_data", "J")
def _(c: Ctx):
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    fx = _fx()
    rows = p.get("renderedRowCount") or 0
    claimed = p.get("totalClaimedInDom")
    reconciles = bool(fx) and claimed == fx.EXPECTED_TOTAL
    return g((0.5 if rows > 0 else 0) + (0.5 if reconciles else 0),
             f"{rows} rows rendered, DOM claims {claimed}", "",
             parts={"rendered_rows": rows, "total_claimed": claimed})


@check("j_console_clean", "J")
def _(c: Ctx):
    errs = []
    for p in (c.probe_load, c.probe_sync):
        if isinstance(p, dict) and not _pe(p):
            ce = p.get("consoleErrors") or {}
            errs += ce.get("texts") or [""] * (ce.get("count") or 0)
    n = len(errs)
    return g(1.0 if n == 0 else (0.5 if n == 1 else 0.0),
             f"{n} console error(s) across load+sync" + (f": {errs[0][:100]}" if errs else ""),
             "JS errors in normal use" if n else "")


@check("j_first_use", "J")
def _(c: Ctx):
    """COMPOUND gate × min(ttfd ladder, reconciliation, console-clean). Absorbs j_loads_data
    and j_console_clean (diagnostics)."""
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    fx = _fx()
    rows = p.get("renderedRowCount") or 0
    ttfd = _ladder(p.get("timeToFirstDataMs"), [(1.0, 2000), (0.75, 3000), (0.5, 4500), (0.25, 8000)])
    reconcile = 1.0 if (fx and p.get("totalClaimedInDom") == fx.EXPECTED_TOTAL) else 0.0
    ce = (p.get("consoleErrors") or {}).get("count", 0)
    clean = 1.0 if ce == 0 else 0.0
    score, parts = compound(
        {"first_data": ttfd, "reconciles": reconcile, "console_clean": clean},
        {"rendered_rows": rows > 0})
    return g(score, f"ttfd={p.get('timeToFirstDataMs')}ms reconcile={reconcile} consoleErrs={ce}",
             "first use is one experience — slow, wrong, or noisy each bounds it", parts=parts)


@check("j_sync_journey", "J")
def _(c: Ctx):
    """Re-based per R13/F2: the app booted against the half-capped vendor, so the probe's
    #sync-now click observably changes the rendered row count."""
    p = c.probe_sync
    if _pe(p):
        return unavail(_pe(p))
    if not c.half_seeded:
        return unavail("half-seed phase unavailable (vendor v2 cap surface missing) — "
                       "view_refreshed would be unobservable, the sb-5 defect (R13)")
    parts = {"button_found": bool(p.get("found")),
             "in_flight_state": bool(p.get("disabledDuringSync") or p.get("inFlightState")),
             "completed": bool(p.get("completed")),
             "view_refreshed": bool(p.get("viewRefreshed"))}
    return g(sum(parts.values()) / 4,
             ", ".join(k for k, v in parts.items() if v) or "sync button never found",
             "the spec's headline interactive flow fails in a real browser"
             if not all(parts.values()) else "", parts=parts)


@check("j_error_state", "J")
def _(c: Ctx):
    """Ladder 1.0 visible+actionable(+working retry when the probe exercises it) · 0.3 any
    visible indication · 0. The retry rung is authoritative ONLY when the probe emitted the
    field — an unpassable top rung is the F7 defect class."""
    p = c.probe_error
    if _pe(p):
        return unavail(_pe(p))
    actionable = p.get("errorStateVisible") and re.search(
        r"try again|retry|check|running|refresh", str(p.get("actionableText") or ""), re.I)
    if "retryRecovered" in p:
        if actionable and p.get("retryRecovered"):
            return g(1.0, "visible + actionable + working retry", "")
        if actionable:
            return g(0.6, "visible + actionable, retry did not recover", "")
    elif actionable:
        return g(1.0, f"visible + actionable: {str(p.get('actionableText'))[:60]}", "")
    if p.get("errorStateVisible"):
        return g(0.3, "error indication only", "not actionable")
    return g(0.0, "no error state", "backend failure leaves the user with a blank page")


@check("j_empty_state", "J")
def _(c: Ctx):
    p = c.probe_empty
    if _pe(p):
        return unavail(_pe(p))
    rows = p.get("renderedRowCount") or 0
    if rows > 0:
        return g(0, f"{rows} rows rendered on an EMPTY db", "phantom data on first run")
    if "emptyCtaWorks" in p:      # CTA rung authoritative only when the probe exercised it
        if p.get("emptyCtaWorks"):
            return g(1.0, "empty text + working Sync CTA", "")
        if p.get("emptyStateVisible"):
            return g(0.6, f"empty text, CTA dead: {str(p.get('emptyStateText'))[:40]}", "")
    if p.get("emptyStateVisible"):
        return g(1.0, f"empty text: {str(p.get('emptyStateText'))[:50]}", "")
    if (p.get("bodyTextLength") or 0) > 0:
        return g(0.3, "page not blank but no empty state", "a fresh install reads as broken")
    return g(0.0, "blank page on empty db", "a fresh install shows nothing")


# ══ V: rendered visual truth ═════════════════════════════════════════════════════════════════

@check("v_dates_readable", "V")
def _(c: Ctx):
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    texts = [t for t in (p.get("dateTexts") or []) if t and t.strip()]
    if not texts:
        return g(0, "no date cells rendered", "the Date column is empty in a real browser")
    iso = [t for t in texts if re.search(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}", t)]
    if iso:
        return g(0, f"raw ISO-8601 shown to users: {iso[0][:60]}",
                 "the spec forbids machine timestamps in the rendered page")
    human = any(re.search(r"[A-Za-z]{3}", t) or re.search(r"\d{1,2}[./]\d{1,2}", t) for t in texts)
    return g(1.0 if human else 0.5, f"rendered dates like {texts[0][:50]}",
             "" if human else "formatted but not recognizably locale-readable")


_CUR_TOKENS = [("KWD", r"KWD|د\.ك"), ("JPY", r"JPY|¥"), ("USD", r"USD|\$"), ("EUR", r"EUR|€")]


def _detect_currency(text: str) -> Optional[str]:
    for cur, pat in _CUR_TOKENS:
        if re.search(pat, text):
            return cur
    return None


@check("v_currency_rendered", "V")
def _(c: Ctx):
    """F24: exponent + digits from the rendered text; symbol/placement/separators free.
    Probe v2 harvests raw `amountTexts` from the first rendered rows; the scorer pairs each
    against the fixture's first-page amounts for the detected currency — never a string
    match on the spec's examples."""
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    fx = _fx()
    if fx is None or not hasattr(fx, "build_payments"):
        return unavail("fixtures_v2.build_payments missing — no amount ground truth")
    texts = [t for t in (p.get("amountTexts") or []) if isinstance(t, str) and t.strip()]
    if not texts:
        return g(0, "no amount cells rendered", "money is the product")
    first_page = fx.build_payments()[:LIST_LIMIT_DEFAULT]
    by_cur: Dict[str, set] = {}
    for row in first_page:
        if isinstance(row, dict) and isinstance(row.get("amount_minor"), int):
            by_cur.setdefault(row.get("currency"), set()).add(row["amount_minor"])
    graded = []
    for t in texts:
        cur = _detect_currency(t)
        graded.append((cur, cur is not None
                       and any(_money_ok(cur, m, t) for m in by_cur.get(cur, ()))))
    ok = sum(1 for _cur, o in graded if o) / len(graded)
    trap_parts = []
    for cur in ("JPY", "KWD"):
        sub = [o for cc, o in graded if cc == cur]
        trap_parts.append(1.0 if sub and all(sub) else 0.0)
    trap = sum(trap_parts) / 2
    return g(0.6 * ok + 0.4 * trap,
             f"{ok:.2f} of {len(graded)} cells exponent-correct; JPY/KWD traps {trap:.2f}",
             "a yen with two decimals is wrong money",
             parts={"cells": len(graded), "ok_frac": round(ok, 3), "jpy_kwd": trap_parts})


@check("v_status_distinct", "V")
def _(c: Ctx):
    """Regraded on >=3 distinct styled pairs — the v3 fixture interleaves statuses so the
    default first page always carries >=3 (the sb-5 fixture ceiling is gone). The frozen
    hexes are graded: computed color or background must be the spec's own value."""
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    styles = p.get("statusStyles") or {}
    if not styles:
        return g(0, "no status cells rendered", "the Status column is empty")
    hex_ok = 0
    for name, s in styles.items():
        want = STATUS_HEX.get(str(name).lower())
        got = _rgb_of(s.get("backgroundColor")) or _rgb_of(s.get("color"))
        if want and _near_rgb(got, want):
            hex_ok += 1
    pairs = {(v.get("color"), v.get("backgroundColor")) for v in styles.values()}
    distinct = len(pairs) == len(styles) and len(styles) >= 3
    return g(0.6 * (hex_ok / max(3, len(styles))) + 0.4 * (1.0 if distinct else 0.0),
             f"{len(styles)} statuses on page 1, {hex_ok} at the frozen hex, "
             f"{len(pairs)} distinct styles",
             "statuses indistinguishable at a glance, or off the frozen palette",
             parts={"rendered": sorted(styles), "hex_ok": hex_ok, "distinct": distinct})


@check("v_pagination", "V")
def _(c: Ctx):
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    controls = bool(p.get("paginationControls"))
    rows = p.get("renderedRowCount") or 0
    bounded = 0 < rows <= LIST_LIMIT_DEFAULT
    return g((0.6 if controls else 0) + (0.4 if bounded else 0),
             f"controls={controls}, {rows} rows in one view",
             "" if controls and bounded else "the finance team scrolls an unpaginated dump")


@check("v_filter", "V")
def _(c: Ctx):
    """Probe v2 drives #status-filter (pick 'refunded', restore All) and emits RAW snapshots;
    the judgement lives here against the fixture's per-status totals (F21)."""
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    f = p.get("filterExercise")
    if isinstance(f, dict) and "after" in f:
        fx = _fx()
        picked = str(f.get("picked") or "")
        after = f.get("after") or {}
        restored = f.get("restored") or {}
        stats = [s for s in (after.get("statuses") or [])]
        rows_changed = bool(stats) and all(s == picked for s in stats) \
            and (after.get("rowCount") or 0) > 0
        want = getattr(fx, "EXPECTED_BY_STATUS", {}).get(picked) if fx else None
        readout_ok = (after.get("readoutTotal") == want) if want is not None \
            else isinstance(after.get("readoutTotal"), int)
        dv_ok = f.get("dataValueAfterPick") == picked
        restore_ok = f.get("dataValueAfterRestore") == "" and \
            (restored.get("readoutTotal") == fx.EXPECTED_TOTAL if fx else
             isinstance(restored.get("readoutTotal"), int))
        parts = {"rows_changed": rows_changed, "readout_ok": readout_ok,
                 "data_value_ok": dv_ok, "restore_ok": restore_ok,
                 "after_total": after.get("readoutTotal"), "want_total": want}
        if rows_changed and readout_ok and dv_ok and restore_ok:
            return g(1.0, f"picked {picked}: rows uniform, readout {after.get('readoutTotal')}"
                          f"=={want}, data-value tracks, restore ok", "", parts=parts)
        if rows_changed:
            return g(0.5, f"rows follow the filter but "
                          f"readout={after.get('readoutTotal')} (want {want}) "
                          f"dv={f.get('dataValueAfterPick')} restore={restore_ok}",
                     "the filter filters, but the readout/selection contract is not met",
                     parts=parts)
        if f.get("control"):
            return g(0.2, "control present but the pick changed nothing",
                     "a filter that filters nothing", parts=parts)
        return g(0.0, "no status filter control", "")
    if isinstance(f, dict):
        if f.get("control") and not f.get("optionFound"):
            return g(0.2, "filter control present; its options were not drivable",
                     "a dropdown a probe cannot open is a dropdown many users cannot open")
        if not f.get("control"):
            return g(0.0, "no status filter control", "")
    if "filterExercise" in p:      # legacy boolean shape
        fb = p.get("filterExercise") or {}
        if fb.get("rowsChanged") and fb.get("readoutUpdated") and fb.get("restored"):
            return g(1.0, "filter changes rows AND readout, and restores", "")
        if fb.get("rowsChanged"):
            return g(0.5, "rows change plausibly", "readout/restore unverified")
    if not p.get("filterControl"):
        return g(0.0, "no status filter control", "")
    # control exists but this probe never drove it — grading presence as full or as inert
    # would both be wrong; the missing exercise is a harness gap (F18).
    return unavail("probe lacks the filter exercise; presence alone cannot grade v_filter")


@check("v_responsive_375", "V")
def _(c: Ctx):
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    vp = p.get("viewport375") or {}
    rows = vp.get("renderedRowCount")
    rows = rows if isinstance(rows, int) else (p.get("renderedRowCount") or 0)
    no_scroll = vp.get("horizontalScroll") is False
    if not no_scroll:
        return g(0.0, f"375px horizontal scroll: {vp.get('horizontalScroll')}",
                 "the page breaks on a phone-width viewport")
    if rows <= 0:
        return g(0.0, "no h-scroll but zero rendered rows at 375px (vacuous)", "empty pages never scroll")
    if "tapTargetsOk" in vp:      # graded only when measured — an unpassable rung is F7's defect
        taps = bool(vp.get("tapTargetsOk"))
        return g(1.0 if taps else 0.6,
                 f"no h-scroll, {rows} rows, tap targets {'ok' if taps else 'small'}", "")
    return g(1.0, f"no h-scroll, {rows} rows (tap targets not measured by this probe)", "")


@check("v_styling", "V")
def _(c: Ctx):
    p = c.probe_load
    if _pe(p):
        return unavail(_pe(p))
    s = p.get("styling") or {}
    font = (s.get("bodyFontFamily") or "").strip()
    header = bool(s.get("appHeaderVisible")) if "appHeaderVisible" in s \
        else bool(re.search(r'id\s*=\s*["\']app-header["\']', c.html))
    parts = {"stylesheet": bool(s.get("hasStylesheet")),
             "layered_backgrounds": (s.get("distinctBackgroundCount") or 0) >= 3,
             "non_default_font": bool(font) and not font.lower().startswith("times"),
             "branded_header": header}
    return g(0.3 * parts["stylesheet"] + 0.25 * parts["layered_backgrounds"]
             + 0.25 * parts["non_default_font"] + 0.2 * parts["branded_header"],
             f"stylesheet={parts['stylesheet']}, {s.get('distinctBackgroundCount')} backgrounds, "
             f"font '{font[:30]}', header={parts['branded_header']}", "", parts=parts)


# ══ P: performance (k_P tightens the top rung; budgets are the spec's own numbers) ═══════════

@check("p_list_latency", "P")
def _(c: Ctx):
    m = c.perf.get("payments") or {}
    return g(_kp_ladder(m.get("p95_ms"), [(1.0, 150), (0.75, 300), (0.5, 600), (0.25, 1200)]),
             f"GET /api/payments limit=50 p95 {m.get('p95_ms')} ms (budget 150)",
             "", parts={"p95_ms": m.get("p95_ms")})


@check("p_buckets_latency", "P")
def _(c: Ctx):
    m = c.perf.get("buckets") or {}
    return g(_kp_ladder(m.get("p95_ms"), [(1.0, 200), (0.75, 400), (0.5, 800), (0.25, 1600)]),
             f"GET /api/buckets p95 {m.get('p95_ms')} ms (budget 200)",
             "", parts={"p95_ms": m.get("p95_ms")})


@check("p_summary_latency", "P")
def _(c: Ctx):
    m = c.perf.get("summary") or {}
    return g(_kp_ladder(m.get("p95_ms"), [(1.0, 150), (0.75, 300), (0.5, 600), (0.25, 1200)]),
             f"GET /api/summary p95 {m.get('p95_ms')} ms (budget 150)",
             "", parts={"p95_ms": m.get("p95_ms")})


@check("p_page_interactive", "P")
def _(c: Ctx):
    v = None if _pe(c.probe_load) else c.probe_load.get("timeToFirstDataMs")
    return g(_kp_ladder(v, [(1.0, 2000), (0.75, 3000), (0.5, 4500), (0.25, 8000)]),
             f"first data rows rendered at {v} ms (budget 2000)", "", parts={"ms": v})


@check("p_first_frame", "P")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    if "firstDrawMs" not in p:
        return unavail("probe emits no firstDrawMs — first-frame timing unmeasured "
                       "(probe gap; blocks the E gate until the probe adds it)")
    v = p.get("firstDrawMs")
    if v is None:
        if p.get("timedOut"):
            return unavail("firstDrawMs lost to probe timeout")
        return g(0.0, "no first 3D frame observed", "the canvas never drew")
    return g(_kp_ladder(v, [(1.0, 3000), (0.75, 5000), (0.5, 8000), (0.25, 15000)]),
             f"first non-background 3D frame at {v} ms (budget 3000)", "", parts={"ms": v})


@check("p_sync_wall", "P")
def _(c: Ctx):
    m = c.perf.get("sync") or {}
    return g(_kp_ladder(m.get("wall_ms"), [(1.0, 90000), (0.75, 135000), (0.5, 180000), (0.25, 270000)]),
             f"POST /api/sync wall {m.get('wall_ms')} ms (budget 90000, documented waits included)",
             "", parts={"wall_ms": m.get("wall_ms")})


# ══ T: the 3D contract, graded from analytically recomputed pixels ═══════════════════════════

@check("t_context_real", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    gl = p.get("gl") or {}
    ctxs = gl.get("contexts") or []
    ck, ref = _viz_section(c, "contextCheck")
    if ref:
        return ref
    ck = ck or {}
    corners = ck.get("corners") or {}
    parts = {
        "webgl_on_viz3d": any(x.get("canvasId") == "viz3d" for x in ctxs),
        "backing_matches_rect": bool(p.get("backingOk")),
        "draw_calls": (gl.get("drawCalls") or 0) >= 1,
        "coverage": (ck.get("gridNonBg") or 0) >= 3 and (ck.get("gridDistinct") or 0) >= 2,
        "corners_bg": corners.get("ok") == corners.get("total") and (corners.get("total") or 0) > 0,
    }
    s = (0.25 * parts["webgl_on_viz3d"] + 0.15 * parts["backing_matches_rect"]
         + 0.15 * parts["draw_calls"] + 0.25 * parts["coverage"] + 0.20 * parts["corners_bg"])
    return g(s, f"ctx={len(ctxs)} draws={gl.get('drawCalls')} nonBg={ck.get('gridNonBg')}/"
                f"{ck.get('gridTotal')} corners={corners.get('ok')}/{corners.get('total')}",
             "no real 3D surface — the panel is decoration or absent", parts=parts)


@check("t_scene_binding", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    pe_ = c.probe_viz_empty or {}
    if isinstance(pe_, dict) and pe_.get("emptyRun") and (pe_.get("vsdbgBarCount") or 0) > 0:
        return g(0, f"empty db but vsdbg claims {pe_.get('vsdbgBarCount')} bars",
                 "phantom data on an empty database")
    b, ref = _viz_section(c, "binding")
    if ref:
        return ref
    b = b or {}
    side_ach = float(TH["side_face_achievable"])
    # Weights renormalize over the classes that HAVE samples: a legal layout can project a
    # class empty (e.g. every sky probe lands above the canvas when the tallest bars fill it)
    # and an empty class proves nothing in either direction (F23 / the vacuous rule). A scene
    # with NO graded samples at all stays 0.
    classes = [
        (0.50, _frac(b.get("tops")), (b.get("tops") or {}).get("total", 0)),
        (0.25, _frac(b.get("above")), (b.get("above") or {}).get("total", 0)),
        (0.15, min(_frac(b.get("sides")) / side_ach, 1.0), (b.get("sides") or {}).get("total", 0)),
        (0.10, _frac(b.get("sky")), (b.get("sky") or {}).get("total", 0)),
    ]
    live = [(w, f) for w, f, total in classes if total]
    wsum = sum(w for w, _f in live)
    score = sum(w * f for w, f in live) / wsum if wsum else 0.0
    if (b.get("statusesExpected") or 0) >= 3 and (b.get("distinctTopColors") or 0) < 3:
        score = min(score, 0.25)   # mono-wash guard (fixture guarantees >=3 statuses)
    return g(score, f"tops {_frac(b.get('tops')):.2f} above {_frac(b.get('above')):.2f} "
                    f"sides {_frac(b.get('sides')):.2f} sky {_frac(b.get('sky')):.2f} "
                    f"colors={b.get('distinctTopColors')} skippedSmall={b.get('skippedSmall')}",
             "the scene does not encode the data — bars misplaced, mis-colored, or mis-heighted",
             parts={"tops": b.get("tops"), "above": b.get("above"), "sides": b.get("sides"),
                    "sky": b.get("sky"), "distinct_colors": b.get("distinctTopColors")})


@check("t_camera", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    d, ref = _viz_section(c, "drag")
    if ref:
        return ref
    d = d or {}
    w = p.get("wheel") or {}
    proj, projf = _frac(d.get("proj")), _frac(d.get("projFlipped"))
    proj_credit = proj if proj >= projf else 0.5 * projf   # sign flip: finesse, not a cliff
    cam = d.get("cameraAfter") or {}
    cam_ok = (cam.get("yaw") is not None and d.get("expectedYaw") is not None
              and _ang_dist(float(cam["yaw"]), float(d["expectedYaw"])) <= 1.5)   # F11
    # F9: reset pays only when the scene provably moved — the probe's mid-drag baseline
    # sample first (probe v2 emits `movedDuringDrag`), projection fractions as fallback.
    moved = (bool(d.get("movedDuringDrag")) or bool(d.get("sceneMoved"))
             or proj >= 0.5 or projf >= 0.5 or _frac(w.get("proj")) >= 0.5)
    reset = _frac(d.get("reset")) if moved else 0.0
    score = (0.20 * _frac(w.get("proj")) + 0.35 * proj_credit + 0.10 * cam_ok + 0.35 * reset)
    return g(score, f"wheel {_frac(w.get('proj')):.2f} drag {proj:.2f} (flip {projf:.2f}) "
                    f"camYaw={cam.get('yaw')} want {d.get('expectedYaw')} moved={moved} "
                    f"reset {reset:.2f}",
             "the orbit control does not implement the documented camera",
             parts={"wheel": w.get("proj"), "drag": d.get("proj"), "flip": d.get("projFlipped"),
                    "cam_ok": cam_ok, "moved": moved, "reset": d.get("reset")})


@check("t_picking", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    k, ref = _viz_section(c, "picking")
    if ref:
        return ref
    k = k or {}
    picks = k.get("picks") or []
    # F1c: a pick counts only when #status-filter took the value AND (when the probe measured
    # it) the TABLE actually refreshed — rows uniformly the picked status, readout total right
    # (probe v2 emits rowsUniform/totalOk per pick).
    def _ok(x):
        if not x.get("ok"):
            return False
        table = [x[f] for f in ("rowsUniform", "totalOk", "tableOk") if f in x]
        return all(table) if table else True
    correct = sum(1 for x in picks if _ok(x))
    frac_p = correct / len(picks) if picks else 0.0
    dp = k.get("depthPair") or {}
    depth = (1.0 if _ok(dp) else 0.0) if dp.get("available") else frac_p
    return g(0.65 * frac_p + 0.35 * depth,
             f"{correct}/{len(picks)} picks drive the product, depthPair="
             f"{'ok' if dp.get('ok') else dp.get('got')}",
             "clicking a bar does not drive the product — the 3D view is not wired to the data",
             parts={"picks": len(picks), "correct": correct, "depth_available": dp.get("available")})


@check("t_vsdbg_truth", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    v, ref = _viz_section(c, "vsdbg")
    if ref:
        return ref
    v = v or {}
    if not v.get("present"):
        return g(0, "window.vsdbg absent", "the graded instrumentation API is missing")
    sc = v.get("sceneOk") or {}
    denom = max(sc.get("expected") or 0, sc.get("claimed") or 0)   # F15a: overclaims cost
    scene = (sc.get("matched", 0) / denom) if denom else 0.0
    tol = float(TH["vsdbg_proj_tol_px"])
    proj = 1.0 if (v.get("projMaxErrPx") is not None and v["projMaxErrPx"] <= tol) else 0.0
    picks = _frac(v.get("picks"))
    ver = 1.0 if v.get("version") == 3 else 0.0
    return g(0.1 * ver + 0.4 * scene + 0.3 * proj + 0.2 * picks,
             f"scene {sc.get('matched')}/{denom} projErr={v.get('projMaxErrPx')}px "
             f"picks {v.get('picks')} version={v.get('version')}",
             "vsdbg reports a scene the canvas does not show",
             parts={"scene": scene, "proj_err_px": v.get("projMaxErrPx"), "version": v.get("version")})


@check("t_tooltip", "T")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    t, ref = _viz_section(c, "tooltip")
    if ref:
        return ref
    t = t or {}
    if not t.get("shown"):
        return g(0, "tooltip never appeared", "hover tells the user nothing")
    text_ok = bool(str(t.get("text", "")).startswith(str(t.get("wantPrefix", "\x00"))))
    latency = _ladder(t.get("ms"), [tuple(r) for r in TH["tooltip_ms_rungs"]])
    hides = 1.0 if t.get("hides") else 0.0
    return g(0.5 * text_ok + 0.3 * latency + 0.2 * hides,
             f"'{str(t.get('text'))[:40]}' in {t.get('ms')}ms (want '{t.get('wantPrefix')}…') "
             f"hides={t.get('hides')}",
             "the tooltip is absent, slow, or wrong about the data",
             parts={"ms": t.get("ms"), "text_ok": text_ok, "hides": t.get("hides")})


@check("t_fallback", "T")
def _(c: Ctx):
    p, q = _viz(c), c.probe_viz_fallback or {}
    if _pe(p):
        # the toggle half is unmeasured — scoring it 0 would convert harness loss into an
        # app zero (F18); the whole check refuses instead.
        return unavail(_pe(p))
    tg = p.get("toggle") or {}
    tg_frac = (tg.get("okCells", 0) / tg["totalCells"]) if tg.get("totalCells") else 0.0
    toggle = tg_frac if tg.get("visible") else (0.1 if tg.get("present") else 0.0)
    if _pe(q):
        grace, grace_note = None, f"no-webgl scenario unavailable ({_pe(q)})"
    else:
        errs = (q.get("consoleErrors") or {}).get("count", 99)
        ft = q.get("fallbackTable") or {}
        auto = (ft.get("okCells", 0) / ft["totalCells"]) \
            if ft.get("totalCells") and ft.get("visible") else 0.0
        # F22: the notice must be visible under glKill, live in/near the viz panel, AND be
        # absent in the normal viz run — probe v2 emits `noticeInViz` + `noticeNearViz` for
        # exactly this differential (a static "built with WebGL" footer fails both halves).
        if "noticeDifferentialOk" in q:
            notice_ok = bool(q.get("noticeDifferentialOk"))
        elif "noticeInViz" in p:
            notice_ok = (bool(q.get("notice")) and not p.get("noticeInViz")
                         and q.get("noticeNearViz", True))
        else:
            notice_ok = bool(q.get("notice")) and q.get("noticeNearViz", True)
        grace = (0.4 * (errs == 0) + 0.35 * auto + 0.15 * bool(notice_ok)
                 + 0.10 * ((q.get("renderedRowCount") or 0) > 0))
        grace_note = f"no-webgl errs={errs} auto {ft.get('okCells')}/{ft.get('totalCells')} " \
                     f"notice={notice_ok}"
    if grace is None:
        return unavail(grace_note) if not tg else \
            g(0.5 * toggle, f"toggle cells {tg.get('okCells')}/{tg.get('totalCells')}; {grace_note}",
              "the no-WebGL half is unproven", parts={"toggle": toggle})
    return g(0.5 * toggle + 0.5 * grace,
             f"toggle cells {tg.get('okCells')}/{tg.get('totalCells')}; {grace_note}",
             "a machine without WebGL gets a crash or a blank panel",
             parts={"toggle": toggle, "grace": grace})


# ══ HARD: compounds + the API-contract mechanisms ════════════════════════════════════════════

@check("h_sync_discipline", "HARD")
def _(c: Ctx):
    """gate × min(cheap, idempotent, propagates). 'cheap' is the CONTINUOUS second-sync
    formula (R9): 0.5·cond/reqs + 0.5·304/reqs — the dead 0.4 rung is gone."""
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    ran = isinstance(c.sync2, dict) and any(k in c.sync2 for k in ("inserted", "total", "fetched"))
    cheap = (0.5 * (c.sync2_cond / c.sync2_reqs) + 0.5 * (c.sync2_304 / c.sync2_reqs)) \
        if c.sync2_reqs else 0.0
    idem = (1.0 if c.sync2.get("inserted") == 0 and c.sync2.get("total") == fx.EXPECTED_TOTAL
            else 0.5 if c.sync2.get("total") == fx.EXPECTED_TOTAL else 0.0) if ran else 0.0
    prop = min(c.update_seen / c.update_changed, 1.0) if c.update_changed else 0.0
    score, parts = compound(
        {"cheap": round(cheap, 3), "idempotent": idem, "propagates": round(prop, 3)},
        {"sync2_ran": ran, "vendor_reqs_seen": c.sync2_reqs > 0})
    return g(score, f"cheap={cheap:.2f} ({c.sync2_cond}/{c.sync2_reqs} cond, "
                    f"{c.sync2_304}/{c.sync2_reqs} 304) idem={idem} prop={prop:.2f}",
             "sync must be cheap AND idempotent AND propagate updates — together, not severally",
             parts=parts)


@check("h_durability", "HARD")
def _(c: Ctx):
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing")
    persist = min(max(c.rows_after_restart, 0) / fx.EXPECTED_TOTAL, 1.0)
    conc = max(0.0, 1 - abs((c.concurrent_total or 0) - fx.EXPECTED_TOTAL) / fx.EXPECTED_TOTAL) \
        if c.concurrent_total is not None else 0.0
    src = (c.pkg / "store.py").read_text(errors="replace") if (c.pkg / "store.py").is_file() else ""
    stmt = re.search(r"INSERT[^;\"']*?INTO\s+payments.*?(?=\"\"\"|;|\Z)", src, re.I | re.S)
    scope = stmt.group(0) if stmt else src
    atomic = 1.0 if re.search(r"ON CONFLICT[^;]*DO UPDATE", scope, re.I | re.S) else \
        (0.5 if re.search(r"INSERT OR REPLACE|INSERT OR IGNORE|REPLACE INTO", scope, re.I) else 0.0)
    score, parts = compound(
        {"persists": round(persist, 3), "concurrent": round(conc, 3), "atomic": atomic},
        {"had_rows_before_kill": c.rows_before_kill > 0})
    return g(score, f"persist={persist:.2f} conc={conc:.2f} atomic={atomic}",
             "data that does not survive a restart or a race is not stored", parts=parts)


@check("h_webhook_ledger", "HARD")
def _(c: Ctx):
    exp = None
    for mod in (_fx(), _vendor()):
        exp = getattr(mod, "EXPECTED_WEBHOOK_COUNTERS", None) if mod else None
        if isinstance(exp, dict):
            break
    if not isinstance(exp, dict):
        return unavail("EXPECTED_WEBHOOK_COUNTERS missing (fixtures_v2/vendor_service_v2)")
    if not c.webhook_script and c.health2 == {}:
        return unavail("webhook delivery script never ran (F3 trigger surface missing)")
    w = (c.health2 or {}).get("webhook") or {}
    quad = ["received", "applied", "ignored", "rejected"]
    exact = sum(w.get(k) == exp.get(k) for k in quad) / 4
    forged = 1.0 if c.webhook_script.get("forged_status") == 401 else 0.0
    stale = 1.0 if c.webhook_script.get("stale_version_ok") else 0.0
    score, parts = compound(
        {"counters": exact, "forged_rejected": forged, "stale_ignored": stale},
        {"registered": bool(w.get("registered"))})
    return g(score, f"counters {[w.get(k) for k in quad]} want {[exp.get(k) for k in quad]} "
                    f"forged={c.webhook_script.get('forged_status')}",
             "the webhook ledger is the proof the app handled untrusted push traffic", parts=parts)


@check("h_conflict_dance", "HARD")
def _(c: Ctx):
    t = c.conflict_trace or {}
    if not t:
        return unavail("conflict trace missing (vendor v2 412 surface not exercised)")
    parts = {"if_match_always": t.get("writes_without_if_match", 1) == 0,
             "recovered_412": bool(t.get("retried_once_with_fresh_version")),
             "no_extra_retries": t.get("retries_after_412", 9) <= 1,
             "second_412_is_409": t.get("second_412_local_status") == 409
                                  and bool(t.get("second_412_row_unchanged"))}
    s = (0.25 * parts["if_match_always"] + 0.35 * parts["recovered_412"]
         + 0.15 * parts["no_extra_retries"] + 0.25 * parts["second_412_is_409"])
    return g(s, f"{ {k: v for k, v in t.items()} }"[:180],
             "optimistic concurrency done wrong silently loses someone's edit", parts=parts)


@check("request_efficiency", "HARD")
def _(c: Ctx):
    """1.0 iff reqs == OPTIMAL_REQUESTS (mock-exported, never hand-written) AND completeness
    == 1.0 AND all vendor traps passed. F15b: traps_ok required in EVERY branch >= 0.75;
    fewer-than-optimal scores 0 unless completeness is 1.0 and every trap passed."""
    fx = _fx()
    if fx is None:
        return unavail("sb-6 fixture constants missing (OPTIMAL_REQUESTS)")
    # F2: sync #1 ran against the half-capped collection, whose simulated optimum is its own
    # constant (13 for the capped walk vs 20 for the full one) — comparing the capped walk
    # against the full optimum would hand out free rungs.
    half_expected = getattr(c, "sync1_expected_total", None)
    opt = fx.OPTIMAL_REQUESTS_HALFSET if (
        half_expected and hasattr(fx, "OPTIMAL_REQUESTS_HALFSET")
        and half_expected == getattr(fx, "HALFSET_ROWS", None)) else fx.OPTIMAL_REQUESTS
    reqs = c.sync1_reqs
    total = max((v for v in ((c.sync1 or {}).get("total"), (c.sync1 or {}).get("fetched"))
                 if isinstance(v, int)), default=0)
    # sync1 ran against the half-capped collection (F2); completeness is measured against
    # what the vendor was serving at the time.
    denom = getattr(c, "sync1_expected_total", None) or fx.EXPECTED_TOTAL
    complete = min(total / denom, 1.0) if denom else 0.0
    traps_ok = c.trap_results and all(
        float(c.trap_results.get(k, 0.0)) >= 1.0 for k in ("retry_after", "cursor_expiry", "stall"))
    if not reqs:
        return g(0.0, "no vendor requests observed", "n/a")
    if reqs < opt:
        s = 1.0 if (complete >= 1.0 and traps_ok) else 0.0
    elif reqs == opt:
        s = 1.0 if (complete >= 1.0 and traps_ok) else 0.5 * complete
    elif reqs <= opt + 2:
        s = 0.75 * complete
    elif reqs <= opt + 7:
        s = 0.5 * complete
    else:
        s = (opt / reqs) * complete
    if not traps_ok:
        s = min(s, 0.5)   # F15b: no branch awards >= 0.75 with a failed trap
    return g(s, f"{reqs} vendor requests (optimum {opt}), completeness {complete:.2f}, "
                f"traps_ok={bool(traps_ok)}",
             "fetching nothing is not efficiency — credit requires the data actually arrived",
             parts={"reqs": reqs, "optimum": opt, "completeness": round(complete, 3),
                    "traps_ok": bool(traps_ok)})


# ══ E: excellence (gate-locked; the last 0.12) ═══════════════════════════════════════════════

@check("e_frames_under_drag", "E")
def _(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return unavail(_pe(p))
    d, ref = _viz_section(c, "drag")
    if ref:
        return ref
    d = d or {}
    fr = d.get("framesDelta")
    if fr is None or (d.get("drawCallsDelta") or 0) <= 0:
        return g(0, "frames not measurable or no draw calls", "an empty rAF loop earns nothing")
    # F4: the unit is the spec's unit — frames per scripted drag; rungs calibration-owned.
    rungs = [(s, -b) for s, b in (tuple(r) for r in TH["drag_frames_rungs"])]
    return g(_ladder(-fr, rungs),
             f"{fr} frames over the {d.get('wallMs')}ms drag "
             f"({d.get('drawCallsDelta')} draw calls)",
             "the 3D view must stay interactive under input",
             parts={"frames": fr, "wall_ms": d.get("wallMs"),
                    "draw_calls_delta": d.get("drawCallsDelta")})


@check("e_under_load_latency", "E")
def _(c: Ctx):
    m = c.under_load or {}
    if not m:
        return unavail("under-load measurement never ran")
    overlap = m.get("overlap_frac")
    floor = float(TH["underload_overlap_floor"])
    if overlap is None or overlap < floor:
        # F8c: an unproven negative must not license the credit — harness refusal, not app zero.
        return unavail(f"only {overlap if overlap is not None else '?'} of reader requests "
                       f"overlapped the in-flight sync (floor {floor}) — load unproven")
    if not m.get("correct"):
        return g(0, f"p95={m.get('p95_ms')}ms but responses wrong/empty under load",
                 "fast wrong answers are not performance")
    return g(_kp_ladder(m.get("p95_ms"), [tuple(r) for r in TH["underload_p95_rungs"]]),
             f"p95={m.get('p95_ms')}ms with 8 readers during sync (overlap {overlap:.2f})",
             "the product under real load",
             parts={"p95_ms": m.get("p95_ms"), "overlap_frac": overlap, "n": m.get("n")})


@check("e_optimistic_paint", "E")
def _(c: Ctx):
    """F10: the probe's `note` scenario HOLDS the POST 800 ms via page.route, so
    `paintedWhileHeld` is a causal fact, not a race; `paintMs` is page-side-stamped."""
    p = c.probe_note
    if _pe(p):
        return unavail(_pe(p))
    if not p.get("noteCellFound"):
        return g(0, "no Note cell found in the rendered table", "the note feature is absent")
    if not p.get("editorOpened"):
        return g(0, "the inline note editor never opened", "the edit affordance is dead")
    causal = bool(p.get("paintedWhileHeld"))
    if not causal:
        return g(0, "cell did not show the new value while the write was provably held",
                 "the note editor waits for the network — not optimistic")
    saving = 1.0 if p.get("savingWhileHeld") else 0.0
    ms = p.get("paintMs")
    lad = 0.0 if p.get("paintPreexisting") else \
        (_ladder(ms, [tuple(r) for r in TH["optimistic_paint_rungs"]]) if ms is not None else 0.0)
    saved = 1.0 if p.get("savedAfterRelease") else 0.0
    return g(0.5 + 0.15 * saving + 0.15 * lad + 0.2 * saved,
             f"painted while held ({ms}ms stamp), saving-state={bool(saving)}, "
             f"saved-after-release={bool(saved)}",
             "optimistic UI is the difference between an app and a form",
             parts={"paint_ms": ms, "saving_while_held": bool(saving), "saved": bool(saved)})


@check("e_hard_mastery", "E")
def _(c: Ctx):
    rows = [r for r in c._hard_rows if not r.get("unavailable")]
    if not rows:
        return unavail("no HARD rows available to master")
    mean = sum(r["score"] for r in rows) / len(rows)
    return g(1.0 if mean >= 0.90 else mean / 0.90 * 0.5, f"HARD mean {mean:.3f}",
             "excellence includes the mechanisms, not only the surface", parts={"hard_mean": mean})


# DIAGNOSTIC members must all be registered — a name that drifts would silently re-enter means.
_REGISTERED = {n for n, _t, _f in SB6_CHECKS}
assert DIAGNOSTIC <= _REGISTERED, f"DIAGNOSTIC names unregistered: {DIAGNOSTIC - _REGISTERED}"
assert len(_REGISTERED) == len(SB6_CHECKS), "duplicate check name in the sb-6 registry"


# ── excellence gate ──────────────────────────────────────────────────────────────────────────

def _nominal_console_errors(c: Ctx) -> Optional[int]:
    """F7: E-gate console cleanliness over NOMINAL scenarios only (load/sync/empty/viz);
    error/viz-fallback produce expected network noise. None = unproven (a probe was
    unavailable), which fails the gate honestly rather than passing it vacuously."""
    total = 0
    for p in (c.probe_load, c.probe_sync, c.probe_empty, c.probe_viz):
        if not isinstance(p, dict) or _pe(p):
            return None
        total += (p.get("consoleErrors") or {}).get("count", 0)
    return total


def excellence(rows: List[Dict], c: Ctx) -> tuple:
    by = {r["check"]: r for r in rows}
    console = _nominal_console_errors(c)
    p_names = [n for n in ("p_list_latency", "p_buckets_latency", "p_summary_latency",
                           "p_page_interactive", "p_first_frame", "p_sync_wall") if n in by]
    gate = (all(by[n]["score"] == 1.0 for n in
                ("j_first_use", "j_sync_journey", "j_error_state", "j_empty_state") if n in by)
            and console == 0
            and by.get("v_responsive_375", {}).get("score") == 1.0
            and by.get("v_dates_readable", {}).get("score") == 1.0
            and by.get("t_scene_binding", {}).get("score", 0) >= 1.0
            and all(by[n]["score"] == 1.0 for n in p_names))
    e_rows = [r for r in rows if r["tier"] == "E" and not r.get("unavailable")]
    e_mean = sum(r["score"] for r in e_rows) / len(e_rows) if e_rows else 0.0
    return (1.0 if gate else 0.0), e_mean


# ── evaluation ───────────────────────────────────────────────────────────────────────────────

def _gamma(x: float, tier: str) -> float:
    return max(0.0, min(1.0, x)) ** (GAMMA["hard"] if tier in ("T", "HARD") else GAMMA["core"])


def evaluate(c: Ctx) -> Dict:
    rows: List[Dict] = []
    for name, tier, fn in SB6_CHECKS:
        if tier == "E":
            continue
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})
    c._hard_rows = [r for r in rows if r["tier"] == "HARD"]
    for name, tier, fn in SB6_CHECKS:
        if tier != "E":
            continue
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})

    unavailable = [r["check"] for r in rows if r.get("unavailable")]
    tiers: Dict[str, Dict] = {}
    inner = 0.0
    for tier, w in TIER_WEIGHT_SB6.items():
        sub = [r for r in rows if r["tier"] == tier and r["check"] not in DIAGNOSTIC]
        avail = [r for r in sub if not r.get("unavailable")]
        # F17/F18: unavailable rows never become app zeros — the tier mean is over what was
        # actually measured, and the verdict names every excluded row out loud.
        mean = sum(_gamma(r["score"], tier) for r in avail) / len(avail) if avail else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(avail), "weight": w,
                       **({"unavailable": [r["check"] for r in sub if r.get("unavailable")]}
                          if len(avail) != len(sub) else {})}
        inner += mean * w
    gate, e_mean = excellence(rows, c)
    score = 0.88 * inner + E_WEIGHT * gate * e_mean
    tiers["E"] = {"mean": round(gate * e_mean, 4), "gate": bool(gate), "weight": E_WEIGHT}
    out = {"score": round(score, 4), "scorer_version": SCORER_VERSION,
           "inner": round(inner, 4), "excellence_gate": bool(gate),
           "calibration": ("frozen" if CALIBRATED else
                           "UNCALIBRATED — sb6-thresholds.json defaults; rc-grade only"),
           "gamma": GAMMA, "k_p": K_P,
           "tiers": tiers, "checks": rows,
           "probe_unavailable": unavailable,
           "harness_missing": list(getattr(c, "harness_missing", [])),
           "root_causes": attribute_root_causes(rows),
           "excellent": bool(gate) and score >= 0.88 and not unavailable,
           "solid": score >= 0.55}
    return out


def format_report(result: Dict, title: str = "") -> str:
    t = result["tiers"]
    head = (f"{title}  {100 * result['score']:.1f}%   " +
            " · ".join(f"{k} {100 * t[k]['mean']:.0f}%"
                       for k in ("A", "B", "C", "D", "J", "V", "P", "T", "HARD", "E") if k in t))
    lines = [head]
    if not CALIBRATED:
        lines.append(f"  {UNCALIBRATED_BANNER}")
    if result.get("probe_unavailable"):
        lines.append(f"  ⚠ {len(result['probe_unavailable'])} check(s) PROBE-UNAVAILABLE "
                     f"(excluded from means, NOT app zeros): "
                     f"{', '.join(result['probe_unavailable'][:8])}")
    if result.get("harness_missing"):
        lines.append(f"  ⚠ vendor-mock surfaces missing: {', '.join(result['harness_missing'][:8])}")
    lines.append("")
    for root, blocked in (result.get("root_causes") or {}).items():
        lines.append(f"  ⚠ {len(blocked)} further check(s) are downstream of `{root}` (<1.0) — "
                     f"ONE defect, not {len(blocked) + 1}: {', '.join(blocked)}")
        lines.append("")
    for row in sorted(result["checks"], key=lambda r: (r["tier"], r["check"])):
        pct = f"{100 * row['score']:>3.0f}%"
        flag = "◌" if row.get("unavailable") else " "
        diag = "·" if row["check"] in DIAGNOSTIC else " "
        lines.append(f" {flag}{diag}{row['tier']:<4} {pct} {row['check']:<24} "
                     f"{str(row.get('detail', ''))[:80]}")
        if row["score"] < 1.0 and row.get("consequence") not in ("n/a", "probe bug", "", None):
            lines.append(f"           └ {row['consequence']}")
    return "\n".join(lines)


# ── gathering ────────────────────────────────────────────────────────────────────────────────

def _trace_split(c: Ctx) -> tuple:
    v = _vendor()
    list_path = getattr(v, "LIST_PATH", "/v2/payments") if v else "/v2/payments"
    marks = [i for i, e in enumerate(c.trace) if str(e.get("__phase__", "")).startswith("sync")]
    names = [c.trace[i].get("__phase__") for i in marks]
    segs: Dict[str, List[Dict]] = {}
    for k, m in enumerate(marks):
        end = marks[k + 1] if k + 1 < len(marks) else len(c.trace)
        segs[names[k]] = c.trace[m + 1:end]
    lists = lambda seg: [e for e in seg if e.get("path") == list_path and e.get("method") == "GET"]
    first = lists(segs.get("sync1", []))
    second = lists(segs.get("sync2", []))
    limits = [int(e["query"]["limit"]) for e in lists(c.trace)
              if str((e.get("query") or {}).get("limit", "")).isdigit()]
    return (len(first), len(second),
            sum(1 for e in second if e.get("status") == 304),
            sum(1 for e in second if e.get("if_none_match")),
            max(limits or [0]))


def _measure_under_load(base: str, expected_total: int, vendor_mod) -> Dict:
    """F8: 8 readers during a live sync; a --stall window guarantees overlap when the vendor
    surface exists. Records the overlap fraction so the scorer can refuse an unproven load."""
    if vendor_mod and hasattr(vendor_mod, "arm_stall"):
        try:
            vendor_mod.arm_stall()   # re-arm the one-shot at the mock's frozen hold length
        except Exception:
            pass
    sync_window = {"t0": None, "t1": None}

    def _sync():
        sync_window["t0"] = time.time()
        _post(f"{base}/api/sync", timeout=180)
        sync_window["t1"] = time.time()

    st = threading.Thread(target=_sync)
    st.start()
    time.sleep(0.4)
    lock = threading.Lock()
    samples: List[Dict] = []

    def _reader():
        for _ in range(6):
            t0 = time.time()
            status, body, _raw, _h = _get(f"{base}/api/payments?limit=50", timeout=10)
            t1 = time.time()
            with lock:
                samples.append({"t0": t0, "t1": t1, "ms": (t1 - t0) * 1000,
                                "ok": status == 200 and isinstance(body, dict)
                                and isinstance(body.get("total"), int)
                                and len(body.get("data") or []) == LIST_LIMIT_DEFAULT})

    readers = [threading.Thread(target=_reader) for _ in range(8)]
    for r in readers:
        r.start()
    for r in readers:
        r.join(timeout=90)
    st.join(timeout=200)
    if not samples:
        return {}
    s0, s1 = sync_window["t0"], sync_window["t1"] or time.time()
    overlapped = [x for x in samples if s0 is not None and x["t1"] > s0 and x["t0"] < s1]
    ms = sorted(x["ms"] for x in samples)
    p95 = ms[min(len(ms) - 1, int(0.95 * len(ms)))]
    return {"p95_ms": round(p95, 1), "n": len(samples),
            "overlap_frac": round(len(overlapped) / len(samples), 3),
            "correct": all(x["ok"] for x in samples),
            "sync_wall_ms": round(((sync_window["t1"] or 0) - (sync_window["t0"] or 0)) * 1000, 1)}


def _spawn(c: Ctx, env: Dict, db: Path, port: int):
    return subprocess.Popen(
        [sys.executable, "-m", "vspro", "--db", str(db), "--port", str(port)],
        cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, start_new_session=True)


def gather(root: Path, vendor_port: int, db: Path, trace_path: Path,
           mark_phase=None) -> Ctx:
    # Absolute from here on: the app subprocess runs with cwd=root, so a relative db/tree
    # path would resolve INSIDE the tree (measured: --tree golden-vspro made the app open
    # golden-vspro/golden-vspro/graded-sb6.db and crash at boot).
    root = Path(root).resolve()
    db = Path(db).resolve()
    trace_path = Path(trace_path).resolve()
    c = Ctx(root)
    fx = _fx()
    V = _vendor()

    def _need(name: str) -> bool:
        if V is not None and hasattr(V, name):
            return True
        c.harness_missing.append(name)
        return False

    if not (root / "vspro").is_dir():
        nested = [p for p in root.iterdir() if p.is_dir() and (p / "vspro").is_dir()]
        if nested:
            c.root, c.pkg = nested[0], nested[0] / "vspro"
    for w in WEB_FILES:
        f = c.pkg / w
        c.asset_bytes[w.split("/")[-1]] = f.stat().st_size if f.is_file() else None
    html_path = c.pkg / "web/index.html"
    if html_path.is_file():
        c.html = html_path.read_text(errors="replace")

    port = _free_port()
    env = {**os.environ, "PYTHONPATH": str(c.root), "PYTHONDONTWRITEBYTECODE": "1",
           "MERIDIAN_BASE_URL": f"http://127.0.0.1:{vendor_port}",
           "MERIDIAN_API_KEY": getattr(V, "API_KEY", "sk_test_meridian") if V else "sk_test_meridian"}
    base = f"http://127.0.0.1:{port}"
    proc = None
    boot_t0 = time.time()
    try:
        proc = _spawn(c, env, db, port)
        deadline = time.time() + 25
        while time.time() < deadline:
            if proc.poll() is not None:
                c.boot_error = (proc.stdout.read() or "")[-800:]
                break
            status, body, _raw, _h = _get(f"{base}/api/health", timeout=2)
            if status is not None:
                c.port_bound = True
            if status == 200 and isinstance(body, dict):
                c.health_boot = body
                c.boot_secs = round(time.time() - boot_t0, 1)
                break
            if time.time() - boot_t0 > 5 and proc.poll() is None:
                c.proc_alive_5s = True
            time.sleep(0.4)

        if c.health_boot:
            c.root_status, _b, raw, _h = _get(f"{base}/")
            if raw:
                c.html = raw.decode(errors="replace") or c.html
            for asset in ("styles.css", "app.js", "viz.js"):
                for prefix in ("", "web/"):
                    a_st, _ab, araw, ah = _get(f"{base}/{prefix}{asset}")
                    if a_st == 200 and araw:
                        c.html += "\n" + araw.decode(errors="replace")
                        c.asset_ctypes[asset] = (ah or {}).get("Content-Type", "")
                        break

            # ── F2: half-seed, sync1 against the capped collection ─────────────────────────
            half = None
            if fx is not None and _need("set_visible_cap"):
                half = getattr(fx, "HALFSET_ROWS", fx.EXPECTED_TOTAL // 2)
                try:
                    V.set_visible_cap(half)
                    c.half_seeded = True
                    c.sync1_expected_total = half
                except Exception:
                    c.harness_missing.append("set_visible_cap:failed")
            if mark_phase:
                mark_phase("sync1")
            _s, c.sync1, _r, _h = _post(f"{base}/api/sync")

            # ── the journey sync: probe clicks #sync-now against the FULL collection ──────
            if c.half_seeded:
                try:
                    V.clear_visible_cap()
                except Exception:
                    pass
                if mark_phase:
                    mark_phase("syncfull")
            c.probe_sync = _probe("sync", base)
            _s, hs, _r, _h = _get(f"{base}/api/health")
            if fx is not None and isinstance(hs, dict) and hs.get("payments") != fx.EXPECTED_TOTAL:
                if mark_phase:
                    mark_phase("syncfill")   # the probe's click failed: fill so downstream is real
                _s, c.sync_fill, _r, _h = _post(f"{base}/api/sync")

            if mark_phase:
                mark_phase("sync2")
            _s, c.sync2, _r, _h = _post(f"{base}/api/sync")
            if mark_phase:
                # Close the graded sync2 window (reset=False: one-shots stay consumed) —
                # later phases (under-load's stall walk, perf, concurrency) must not dilute
                # the second sync's conditional/304 evidence.
                try:
                    mark_phase("sync2done", False)
                except TypeError:
                    pass

            # ── API reads ─────────────────────────────────────────────────────────────────
            _s, c.payments, _r, hdrs = _get(f"{base}/api/payments")
            c.api_ctype = (hdrs or {}).get("Content-Type", "")
            _s, c.page5, _r, _h = _get(f"{base}/api/payments?limit=5&offset=5")
            _s, c.capped, _r, _h = _get(f"{base}/api/payments?limit=500")
            _s, c.sorted_desc, _r, _h = _get(f"{base}/api/payments?sort=-created_at&limit=10")
            _s, c.filtered, _r, _h = _get(f"{base}/api/payments?status=refunded&limit=10")
            _s, c.summary, _r, _h = _get(f"{base}/api/summary")
            _s, c.buckets, _r, _h = _get(f"{base}/api/buckets")
            total_rows = fx.EXPECTED_TOTAL if fx else 1600
            for off in range(0, total_rows, LIST_LIMIT_CAP):
                st8, _b8, raw8, _h8 = _get(f"{base}/api/payments?limit={LIST_LIMIT_CAP}&offset={off}")
                c.raw_pages.append((st8, raw8))

            # validation matrix (status half) + envelope cases (shape half — split, F/anti-stack)
            def _envelope_ok(body) -> bool:
                e = (body or {}).get("error") if isinstance(body, dict) else None
                return isinstance(e, dict) and isinstance(e.get("code"), str) \
                    and isinstance(e.get("message"), str)

            for path, want_status, expects_fe in [
                ("/api/payments?limit=-1", 400, True),
                ("/api/payments?limit=abc", 400, True),
                ("/api/payments?offset=-5", 400, True),
                ("/api/payments?sort=bogus", 400, True),
                ("/api/payments?status=bogus", 400, True),
                ("/api/nope", 404, False),
            ]:
                st, body, _raw, _hh = _get(f"{base}{path}")
                c.val_matrix.append((path, st, 1.0 if st == want_status else 0.0))
                fe = ((body or {}).get("error") or {}).get("field_errors") \
                    if isinstance(body, dict) else None
                paths_ok = bool(fe) and all(
                    isinstance(x, dict) and isinstance(x.get("path"), str)
                    and isinstance(x.get("code"), str) for x in fe)
                c.envelope_cases.append({"path": path, "envelope_ok": _envelope_ok(body),
                                         "expects_field_errors": expects_fe,
                                         "field_paths_ok": paths_ok})

            # ── batch (c_batch_partial) ───────────────────────────────────────────────────
            limit_amt = None
            for mod in (V, fx):
                for name in ("AMOUNT_LIMIT", "AMOUNT_LIMIT_MINOR"):
                    if mod is not None and isinstance(getattr(mod, name, None), int):
                        limit_amt = getattr(mod, name)
                        break
                if limit_amt is not None:
                    break
            if limit_amt is None:
                c.harness_missing.append("AMOUNT_LIMIT_MINOR")
            else:
                items = [
                    {"amount": {"value_minor": 5000, "currency": "EUR"},
                     "counterparty": {"name": "Alpha GmbH", "country": "DE"},
                     "occurred_at": "2026-03-25T10:00:00Z", "idempotency_key": "sb6-b1"},
                    {"amount": {"value_minor": int(limit_amt) + 1, "currency": "USD"},
                     "counterparty": {"name": "Over Limit LLC", "country": "US"},
                     "occurred_at": "2026-03-25T11:00:00Z", "idempotency_key": "sb6-b2"},
                    {"amount": {"value_minor": 900, "currency": "JPY"},
                     "counterparty": {"name": "Kappa KK", "country": "JP"},
                     "occurred_at": "2026-03-25T12:00:00Z", "idempotency_key": "sb6-b3"},
                ]
                req = urllib.request.Request(
                    f"{base}/api/payments/batch",
                    data=json.dumps({"items": items}).encode(),
                    headers={"Content-Type": "application/json"}, method="POST")
                _s, body, _r, _h = _req(req, 30)
                c.batch_result = body if isinstance(body, dict) else {}
                if _need("batch_trace"):
                    try:
                        c.batch_trace = V.batch_trace() or {}
                    except Exception:
                        pass
                # invalid-item envelope case (b_error_envelope's third scripted case)
                bad = {"items": [{"amount": {"value_minor": "x", "currency": "EUR"},
                                  "counterparty": {"name": "", "country": "deu"},
                                  "occurred_at": "nope", "idempotency_key": ""}]}
                breq = urllib.request.Request(
                    f"{base}/api/payments/batch", data=json.dumps(bad).encode(),
                    headers={"Content-Type": "application/json"}, method="POST")
                bst, bbody, _r, _h = _req(breq, 15)
                c.val_matrix.append(("/api/payments/batch[invalid]", bst,
                                     1.0 if bst == 400 else 0.0))
                fe = ((bbody or {}).get("error") or {}).get("field_errors") \
                    if isinstance(bbody, dict) else None
                c.envelope_cases.append({
                    "path": "/api/payments/batch[invalid]", "envelope_ok": _envelope_ok(bbody),
                    "expects_field_errors": True,
                    "field_paths_ok": bool(fe) and any(
                        "items[" in str(x.get("path", "")) for x in fe if isinstance(x, dict))})

            # ── conflict dance (one-shot 412, then the scripted double-412) ───────────────
            pid = None
            data0 = c.payments.get("data") or []
            if data0 and isinstance(data0[0], dict):
                pid = data0[0].get("id")
            if pid and _need("arm_conflict_once"):
                try:
                    V.arm_conflict_once()
                    nreq = urllib.request.Request(
                        f"{base}/api/payments/{pid}/note",
                        data=json.dumps({"note": "sb6 conflict probe"}).encode(),
                        headers={"Content-Type": "application/json"}, method="POST")
                    n_st, n_body, _r, _h = _req(nreq, 30)
                    c.note_result = {"status": n_st, "body": n_body}
                    if hasattr(V, "arm_conflict_second"):
                        V.arm_conflict_second()
                        n2 = urllib.request.Request(
                            f"{base}/api/payments/{pid}/note",
                            data=json.dumps({"note": "sb6 double conflict"}).encode(),
                            headers={"Content-Type": "application/json"}, method="POST")
                        n2_st, _b2, _r2, _h2 = _req(n2, 30)
                        _s, row_after, _r, _h = _get(f"{base}/api/payments/{pid}")
                        second = {"second_412_local_status": n2_st,
                                  "second_412_row_unchanged":
                                      isinstance(row_after, dict)
                                      and row_after.get("note") != "sb6 double conflict"}
                    else:
                        second = {}
                    if hasattr(V, "conflict_trace"):
                        c.conflict_trace = {**(V.conflict_trace() or {}), **second}
                    elif second:
                        c.conflict_trace = second
                except Exception as e:
                    c.harness_missing.append(f"conflict:{type(e).__name__}")
                finally:
                    # the always-contended mode must not outlive its exercise: the note probe
                    # later writes through the same PATCH path and must see a normal vendor
                    if hasattr(V, "set_conflict_mode"):
                        try:
                            V.set_conflict_mode("off")
                        except Exception:
                            pass

            # ── under load (F8) ───────────────────────────────────────────────────────────
            if fx is not None:
                c.under_load = _measure_under_load(base, fx.EXPECTED_TOTAL, V)

            # ── traps ledger (F14: documented behaviours, read from the mock) ─────────────
            # AFTER under-load on purpose: the stall trap fires inside that phase, and
            # trap_results() blocks until the held handler resolves its verdict.
            if _need("trap_results"):
                try:
                    c.trap_results = V.trap_results() or {}
                except Exception:
                    pass

            # ── perf (idle) ───────────────────────────────────────────────────────────────
            c.perf = {
                "payments": perf_probe.measure_endpoint(base, f"/api/payments?limit={LIST_LIMIT_DEFAULT}"),
                "buckets": perf_probe.measure_endpoint(base, "/api/buckets"),
                "summary": perf_probe.measure_endpoint(base, "/api/summary"),
                "page": perf_probe.measure_page(base),
                "sync": perf_probe.measure_sync(base),
            }

            # ── concurrency ───────────────────────────────────────────────────────────────
            results: List = []
            threads = [threading.Thread(target=lambda: results.append(_post(f"{base}/api/sync")))
                       for _ in range(2)]
            for t in threads:
                t.start()
            for t in threads:
                t.join(timeout=240)
            _s, hc, _r, _h = _get(f"{base}/api/health")
            c.concurrent_total = (hc or {}).get("payments")

            # json_everywhere sample
            for jp in ("/api/health", "/api/payments", "/api/summary", "/api/buckets",
                       "/api/nope", "/api/payments?limit=-1"):
                stj, _bj, rawj, hj = _get(f"{base}{jp}")
                try:
                    json.loads(rawj if isinstance(rawj, str) else rawj.decode(errors="replace"))
                    parses = True
                except Exception:
                    parses = False
                c.json_probe.append((jp, stj, parses, (hj.get("Content-Type", "") if hj else "")))

            # ── mutation pass (update propagation): per-id truth, then un-mutate ──────────
            if mark_phase and V is not None and hasattr(V, "mutate_statuses"):
                try:
                    changed = V.mutate_statuses()
                    mark_phase("sync3")
                    _s, c.sync3, _r, _h = _post(f"{base}/api/sync")
                    if isinstance(changed, dict):
                        # The mock names exactly which rows changed to what; anything less
                        # precise is vacuously passable on a fixture that already contains
                        # rows of the target status.
                        c.update_changed = len(changed)
                        seen = 0
                        for pid, want_status in changed.items():
                            _s, row, _r, _h = _get(f"{base}/api/payments/{pid}")
                            if isinstance(row, dict) and row.get("status") == want_status:
                                seen += 1
                        c.update_seen = seen
                    else:                                  # legacy count-only mock surface
                        c.update_changed = int(changed or 0)
                        refunded = 0
                        for off in range(0, (fx.EXPECTED_TOTAL if fx else 1600), LIST_LIMIT_CAP):
                            _s, page, _r, _h = _get(
                                f"{base}/api/payments?limit={LIST_LIMIT_CAP}&offset={off}")
                            rows = (page.get("data") if isinstance(page, dict) else None) or []
                            refunded += sum(1 for r in rows if isinstance(r, dict)
                                            and r.get("status") == "refunded")
                        c.update_seen = refunded
                finally:
                    # Un-mutate with a FRESH version bump and sync once more, so the app's
                    # local state returns to the fixture truth the probes compare against.
                    # A raw restore_payments() here would reset vendor versions BELOW the
                    # app's rows and poison both the ETag ledger and the webhook end-state.
                    try:
                        if hasattr(V, "unmutate_statuses"):
                            V.unmutate_statuses()
                            mark_phase("sync4")
                            _post(f"{base}/api/sync")
                        else:
                            V.restore_payments()
                    except Exception:
                        pass
            elif V is not None and not hasattr(V, "mutate_statuses"):
                c.harness_missing.append("mutate_statuses")

            # ── product probes ────────────────────────────────────────────────────────────
            c.probe_load = _probe("load", base)
            c.probe_note = _probe("note", base)
            c.probe_error = _probe("error", base, "--block-api")
            viz_env = None
            if fx is not None:
                viz_env = {"BENCH_VIZ_BUCKETS": json.dumps(fx.EXPECTED_BUCKETS)}
            c.probe_viz = _probe("viz", base, env=viz_env) if viz_env else \
                {"_probe_error": "BENCH_VIZ_BUCKETS unavailable (fixtures missing) — F17 refusal"}
            c.probe_viz_fallback = _probe("viz-fallback", base, env=viz_env) if viz_env else \
                {"_probe_error": "BENCH_VIZ_BUCKETS unavailable (fixtures missing) — F17 refusal"}

            # empty instance: fresh db, probed for the empty state AND the phantom-data viz
            eport = _free_port()
            eproc = None
            try:
                eproc = _spawn(c, env, c.root / "empty-probe.db", eport)
                edeadline = time.time() + 15
                while time.time() < edeadline:
                    if eproc.poll() is not None:
                        c.probe_empty = {"_probe_error": "empty-instance died at boot"}
                        break
                    es, _eb, _er, _eh = _get(f"http://127.0.0.1:{eport}/api/health", timeout=2)
                    if es == 200:
                        ebase = f"http://127.0.0.1:{eport}"
                        c.probe_empty = _probe("empty", ebase)
                        empty_buckets = None
                        if fx is not None:
                            eb = fx.EXPECTED_BUCKETS
                            empty_buckets = {"days": [], "statuses": eb.get("statuses", STATUS_ORDER),
                                             "cells": []}
                            c.probe_viz_empty = _probe(
                                "viz", ebase, env={"BENCH_VIZ_BUCKETS": json.dumps(empty_buckets)})
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

            # ── restart persistence, then the webhook script (F3 ordering, frozen in code:
            #    restart → re-register (the app does it at boot) → trigger → read health2) ──
            _s, hb, _r, _h = _get(f"{base}/api/health")
            c.rows_before_kill = (hb or {}).get("payments") or 0
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                proc.kill()
            proc.wait(timeout=10)
            proc = _spawn(c, env, db, port)
            deadline2 = time.time() + 20
            c.rows_after_restart = -1
            while time.time() < deadline2:
                if proc.poll() is not None:
                    break
                _s, h3, _r, _h = _get(f"{base}/api/health", timeout=2)
                if _s == 200 and isinstance(h3, dict):
                    c.rows_after_restart = h3.get("payments") \
                        if isinstance(h3.get("payments"), int) else -1
                    break
                time.sleep(0.4)

            if c.rows_after_restart >= 0:
                time.sleep(1.0)   # give the app its post-boot re-registration window
                delivered = False
                if V is not None and hasattr(V, "run_delivery_script"):
                    try:
                        c.webhook_script = V.run_delivery_script() or {}
                        delivered = True
                    except Exception as e:
                        c.harness_missing.append(f"run_delivery_script:{type(e).__name__}")
                else:
                    a_st, a_body, _r, _h = _post(
                        f"http://127.0.0.1:{vendor_port}/admin/deliver-script", timeout=30)
                    if a_st == 200:
                        c.webhook_script = a_body if isinstance(a_body, dict) else {"ok": True}
                        delivered = True
                    else:
                        c.harness_missing.append("run_delivery_script")
                if delivered:
                    time.sleep(1.0)
                    _s, h4, _r, _h = _get(f"{base}/api/health", timeout=5)
                    c.health2 = h4 if isinstance(h4, dict) else {}
                    # h_webhook_ledger evidence: the forged delivery's HTTP status straight
                    # from the script result, and stale-not-applied proven on the APP's own
                    # rows against the fixture's simulated end-state (never a proxy).
                    results = (c.webhook_script or {}).get("results") or []
                    forged = next((r for r in results if r.get("kind") == "forged"), None)
                    if isinstance(c.webhook_script, dict) and forged is not None:
                        c.webhook_script["forged_status"] = forged.get("status")
                    exp_final = getattr(fx, "EXPECTED_WEBHOOK_FINAL", None) if fx else None
                    if isinstance(c.webhook_script, dict) and isinstance(exp_final, dict) \
                            and exp_final:
                        ok = True
                        for pid, want in exp_final.items():
                            _s, row, _r, _h = _get(f"{base}/api/payments/{pid}")
                            if not (isinstance(row, dict)
                                    and row.get("version") == want.get("version")
                                    and row.get("note") == want.get("note")):
                                ok = False
                        c.webhook_script["stale_version_ok"] = ok

            for attr in ("payments", "page5", "capped", "summary", "buckets",
                         "sync1", "sync2", "sorted_desc", "filtered"):
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
        (c.sync1_reqs, c.sync2_reqs, c.sync2_304, c.sync2_cond,
         c.max_limit_used) = _trace_split(c)
    return c


# ── CLI: score a tree; --reference is the F7/G6 freeze gate ──────────────────────────────────

def _reference_gate(result: Dict) -> List[str]:
    """Everything the reference must satisfy before ANY threshold is trusted. Returns the
    list of failures (empty = pass)."""
    fails: List[str] = []
    for r in result["checks"]:
        if r["check"] in CALIBRATION_OWNED or r["check"] in DIAGNOSTIC:
            continue
        if r.get("unavailable"):
            fails.append(f"{r['check']}: PROBE UNAVAILABLE — harness defect blocks freeze")
        elif r["score"] < 0.95:
            fails.append(f"{r['check']}: {r['score']:.2f} < 0.95 — {str(r.get('detail'))[:80]}")
    if not result.get("excellence_gate"):
        fails.append("E gate FAILED — a gate the reference cannot pass is locked shut for "
                     "harness reasons (the sb-5 view_refreshed class, F7)")
    return fails


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Score a vspro tree under sb-6.")
    ap.add_argument("--tree", type=Path, required=True, help="root of the built vspro tree")
    ap.add_argument("--port", type=int, default=8899, help="vendor mock port (8899+ policy)")
    ap.add_argument("--reference", action="store_true",
                    help="G6 freeze gate: refuse freeze-marked output unless every "
                         "non-calibration-owned check >= 0.95 and the E gate passes")
    ap.add_argument("--json-out", type=Path, help="write the verdict JSON here")
    args = ap.parse_args()
    args.tree = args.tree.resolve()

    V = _vendor()
    if V is None or not hasattr(V, "serve"):
        print("REFUSED: vendor_service_v2.serve is not available — the sb-6 mock is a "
              "sibling deliverable and scoring without it would grade against nothing.",
              file=sys.stderr)
        return 2
    trace = args.tree / "vendor-trace-sb6.jsonl"
    trace.unlink(missing_ok=True)
    server = V.serve(args.port, trace)
    try:
        ctx = gather(args.tree, args.port, args.tree / "graded-sb6.db", trace,
                     mark_phase=getattr(V, "mark_phase", None))
    finally:
        server.shutdown()
    result = evaluate(ctx)
    print(format_report(result, f"sb-6 {args.tree.name}"))
    if args.json_out:
        args.json_out.write_text(json.dumps(result, indent=2, default=str))

    if args.reference:
        fails = _reference_gate(result)
        if fails:
            print("\nREFERENCE GATE (G6): REFUSED — freeze is blocked. Harness defects, "
                  "not app defects, until proven otherwise:", file=sys.stderr)
            for f in fails:
                print(f"  ✗ {f}", file=sys.stderr)
            return 3
        marker = {"reference_pass": True, "scorer_version": SCORER_VERSION,
                  "score": result["score"], "date": time.strftime("%Y-%m-%d"),
                  "thresholds_calibrated": CALIBRATED,
                  "thresholds_sha256": hashlib.sha256(
                      THRESHOLDS_FILE.read_bytes()).hexdigest() if THRESHOLDS_FILE.is_file() else None}
        (args.tree / "sb6-reference-pass.json").write_text(json.dumps(marker, indent=2))
        print("\nREFERENCE GATE (G6): PASS — sb6-reference-pass.json written; thresholds may "
              "now be fitted against this tree's distribution.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
