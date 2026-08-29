"""sb-7 — grade a built Meridian-v3 payments-console tree (`app/` package: ledgerd +
notifierd + web frontend) against sb7/DESIGN.md. THE PROPERTIES-OVER-SCHEDULES TIER.

Implements DESIGN §5 exactly: the check registry below (tiers A/B/C/D/J/V/P/T/X/R + the E
slice), pure `compose_from_rows`, the 12-member CRITICAL severity registry with measured
severity inputs, the multiplier `factor = m + (1-m)·severity_input` (m =
critical_multiplier_floor, thresholds-owned, default 0.6), the three red-team severity
amendments (F1 evidence-disjoint residual conservation, F2 severity-input transforms with the
data-loss ≤0.5 cliff and the DST-cell leg, F3 ROOT_BLOCKS multiplier dedup + vacuous-leg
attribution), the F9 unavailability doctrine (positive-control-gated `unavail`; absent app
surfaces are g(0) + root-block + severity input 0, never refusals), and
`severity_selftest()` — wired into `--reference` — proving the §5.4 monotonicity chain, the
F1/F2/F3/F9 scenarios, dominance, and gradient sanity through the REAL composition.

Scoring is SERIAL and HERMETIC at the tree (F895-F897 lineage): every invocation wipes the
graded db-dir, trace, tokens and probe artifacts before booting anything, and every graded
verdict records its `fixture_seed`.

Architecture mirrors score_sb6.py: `@check(name, tier)` registry, `g()` results, `unavail()`
refusals, `compound(gate × min)`, DIAGNOSTIC anti-stacking, CALIBRATION_OWNED rungs behind
sb7-thresholds.json + CALIB_SHA256 pin (UNCALIBRATED banner until the freeze lands),
`gather(root, vendor_port, db_dir, trace_path, mark_phase, seed) -> Ctx`,
`evaluate(ctx) -> dict`, CLI `--tree/--port/--reference/--json-out` (+ `--seed`).

Sibling surfaces this module consumes (guarded — a missing surface is a named harness
refusal at the CLI or a `harness_missing` entry, never a crash and never an app zero):

  schedule_sb7.derive(seed) -> Schedule (attrs or mapping; §4.1 fields):
      race_pages, race_mutations, race_order, refund_txn {payment_id, reversal_id,
      amount_minor, currency}, ooo_pair, dup_event, forged_event, midwalk_create,
      drop_after_page, http500_page, sigkill_after_list, vendor_down_boot_secs,
      stale_304_sync, partition_after_event, partition_commits, approval_fixture,
      tokens {maker, checker, admin}
  fixtures_v3.build(seed) -> pack (attrs or mapping):
      N (=12288), PAGE_SIZE (=64), PAGES (=192), payments, EXPECTED_TOTAL,
      EXPECTED_BUCKETS, EXPECTED_BY_CURRENCY, EXPECTED_LEDGER_FINAL, OPTIMAL_REQUESTS,
      EXPECTED_WEBHOOK_COUNTERS, EXPECTED_WORKFLOW_STATES, EXPECTED_NOTIFICATION_MULTISET,
      LAYOUT {d0, D0, R0}, DIGEST {count, Sh, Sh2, Sx, Sz, Sxh, Szh}, LABEL_CANDIDATES,
      HEIGHT_CASES, READ_TARGETS (2 payment ids), PROBE_EXPECT (JSON-serializable pack for
      the browser probe)
  vendor_service_v3 (module):  serve(port, trace, seed=...), mark_phase(name, reset=None),
      LIST_PATH ("/v3/payments"), and the control surface (each _need-guarded):
      arm_boot_refusal(secs) [B4], quiescent() -> bool, commit_ledger() -> {payment_id:
      {version, status, amount_minor, currency}}, commit_versions() -> {(payment_id,
      version), ...} committed set, set_down(bool) [error journey], fire_partition_commits()
      [B7 K=8], arm_hold_create() [A2 window], fire_d1_mutation() [D1 brushed-record
      mutation], fire_burst() -> {t0, t1} [under-stream load], vendor_payment_count() and
      idempotency_replays() [B8/A2 dupe truth]
  product_probe_v3.mjs — the probe's SCENARIOS list is the single scenario authority:
      boot | load | sync | flow | error | viz | feed
      gather invokes load, sync, flow, viz, error, and boot (the boot emit is stored under
      the "empty" evidence key — j_empty_state / D3 read its preSyncState). viz consumes env
      BENCH_SB7_EXPECT (path to the expectation-pack JSON the harness writes from
      fixtures_v3; SB7_EXPECT_FILE is the same path as a compat alias) and emits the §3
      evidence in ONE session: contextReal, layout, digest, heightPixels (R9), gl (wrapper
      v2 counters incl. FBO classification and buffer bytes), dragBudget (pinned window,
      deltaDefaultDrawsAtRaf/deltaFrames, dblclickReset), idle.windows, picks.results +
      pickCounters (sinceInvalidation/duringPickCalls), cameraMath (defaults/wheel/
      pitchClamp/projMaxErrPx), labels + labelSetExact, brush + brushHighlight + brushClear,
      coast {flick, cancel, slowRelease, cadence}, stream {perBatch, changedPixel,
      layoutAfter}, streamApplied, d1 {survived, survivedInBrush}, vs7dbgTruth — there are
      NO separate coast/stream scenarios; the T/P checks read the viz emit. flow emits the
      F1/F2 workflow journey evidence (roleTokenAccepted, draftCreated, submitCausal,
      approveCausal, rejectCausal, draftListStates, top-level notificationSeen +
      paymentInTable, optimistic {paintMs, savedAfterRelease}, d2Resubmit); load emits the
      notifications-feed state read during the partition and after the heal. All scenarios
      emit consoleErrors {count} and incremental sections with timedOut.

Every number graded here is either printed in the spec (spec-derived) or recomputed from the
run's `fixture_seed` via fixtures_v3/schedule_sb7 — never hand-written expectations.
"""

from __future__ import annotations

import hashlib
import importlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

# Shared primitives — same helpers, same conventions, one definition (score_build is sb-5's
# module; importing it runs only registrations, no side effects on the sb-7 path).
from score_build import (  # noqa: E402
    _free_port, _get, _instant, _ladder, _post, _req, g,
)
from score_build import _pe as _pe_raw  # noqa: E402


def _pe(p):
    """Probe-error detector covering BOTH error spellings: the runner's `_probe_error`
    (subprocess/JSON failure) and the probe's own `probeError` refusal emit (F17: a missing
    expectation pack must refuse, never zero the app)."""
    e = _pe_raw(p)
    if e:
        return e
    if isinstance(p, dict) and p.get("probeError"):
        return str(p["probeError"])
    return None

# ── frozen vocabulary (DESIGN §2.3 / §3.1 — verbatim) ────────────────────────────────────────

STATUS_ORDER = ["settled", "pending", "refunded", "failed"]
STATUS_HEX = {"settled": (5, 150, 105), "pending": (217, 119, 6),
              "refunded": (124, 58, 237), "failed": (185, 28, 28)}
BACKGROUND_RGB = (16, 24, 40)
CURRENCY_EXP = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
PAYMENT_KEYS_V3 = {"id", "amount_minor", "currency", "status", "created_at", "day", "version"}
# the spec's /api/payments row contract: exactly these ten keys (day is viz-records-only)
PAYMENT_ROW_KEYS = {"id", "amount_minor", "currency", "created_at", "settled_at", "status",
                    "version", "note", "counterparty_name", "country"}
VIZ_COLUMNS = ("id", "amount_minor", "currency", "status", "created_at", "day", "version")
EVENT_TYPES = {"payment.created", "payment.updated", "reversal.created", "draft.created",
               "draft.submitted", "draft.approved", "draft.rejected", "payment.sent"}
EVENT_SOURCES = {"sync", "webhook", "local", "approval"}
OUTBOX_TYPES = {"draft.submitted", "draft.approved", "draft.rejected", "reversal.created",
                "payment.sent"}
NOTIFY_TYPES = {"draft.submitted", "draft.approved", "draft.rejected", "reversal.created"}
WEB_FILES = ("web/index.html", "web/styles.css", "web/app.js", "web/viz.js")
ASSET_BUDGET_BYTES = 150 * 1024
N_FROZEN = 12_288
PAGE_SIZE = 64
PAGES = N_FROZEN // PAGE_SIZE            # 192 — a fixture constant both sides can compute
DELTA = 1.2                              # cell pitch (§3.1)
TAU = 0.4                                # coast decay (§3.4)
DRAW_BUDGET = 8                          # default-FBO draws per rendered frame (§3.2)
DRAG_MOVES = 40                          # scripted budget drag M (§3.2)
LABEL_W, LABEL_H = 110, 18               # label border-box (§3.5)
PARTITION_K = 8                          # frozen partition commit count (§4.1)

# ── calibration-owned thresholds (sb7-thresholds.json; UNCALIBRATED until the freeze) ────────

THRESHOLDS_FILE = HERE / "sb7-thresholds.json"
CALIB_SHA256 = "TBD-AT-FREEZE"  # baked at freeze; a calibrated file must hash to this

_DEFAULT_THRESHOLDS = {
    "calibrated": False,
    "note": "UNCALIBRATED defaults — spec-traceable budgets only; the golden worst-of-5 fit "
            "has not run (DESIGN §5.6/§5.8)",
    "gamma_core": 1.0,
    "gamma_hard": 1.0,
    "k_p": 1.0,
    "gamma_cap": 4.0,
    "critical_multiplier_floor": 0.6,
    # frames observed over the scripted 40-move budget drag (spec floor: the drag renders)
    "drag_frames_rungs": [[1.0, 32], [0.75, 24], [0.5, 16], [0.25, 8]],
    # pre-freeze top rungs sit at the PRINTED spec budgets (§ perf: 250 ms apply, 150 ms api
    # p95, 120 s sync wall); the golden worst-of-5 fit tightens them at calibration.
    "stream_apply_ms_rungs": [[1.0, 250], [0.75, 500], [0.5, 1000], [0.25, 2000]],
    "underload_p95_rungs": [[1.0, 150], [0.75, 300], [0.5, 600], [0.25, 1200]],
    "underload_overlap_floor": 0.5,       # F8c lineage: below this, load is unproven -> refusal
    "api_p95_ms_rungs": [[1.0, 150], [0.75, 300], [0.5, 600], [0.25, 1500]],
    "sync_wall_ms_rungs": [[1.0, 120000], [0.75, 240000], [0.5, 420000], [0.25, 600000]],
    "optimistic_paint_rungs": [[1.0, 100], [0.6, 250], [0.3, 800]],
    "e_drag_frames_rungs": [[1.0, 40], [0.75, 32], [0.5, 24], [0.25, 12]],
    "e_stream_apply_ms_rungs": [[1.0, 100], [0.75, 200], [0.5, 400], [0.25, 800]],
    "vs7dbg_proj_tol_px": 2.0,
}


def _load_thresholds() -> Dict:
    if not THRESHOLDS_FILE.is_file():
        return dict(_DEFAULT_THRESHOLDS)
    raw = THRESHOLDS_FILE.read_bytes()
    try:
        data = json.loads(raw)
    except Exception:
        raise SystemExit(f"sb7-thresholds.json is unparsable — refusing to score: {THRESHOLDS_FILE}")
    merged = {**_DEFAULT_THRESHOLDS, **data}
    if merged.get("calibrated"):
        digest = hashlib.sha256(raw).hexdigest()
        if CALIB_SHA256 == "TBD-AT-FREEZE" or digest != CALIB_SHA256:
            raise SystemExit(
                "sb7-thresholds.json claims calibrated=true but its sha256 does not match the "
                f"pin baked into score_sb7.py ({digest[:16]}… vs {CALIB_SHA256[:16]}…) — "
                "refusing to score. Re-freeze or restore the artifact.")
    return merged


TH = _load_thresholds()
CALIBRATED = bool(TH.get("calibrated"))
GAMMA = {"core": float(TH["gamma_core"]), "hard": float(TH["gamma_hard"])}
K_P = float(TH["k_p"])
GAMMA_CAP = float(TH.get("gamma_cap", 4.0))
assert GAMMA["core"] <= GAMMA_CAP and GAMMA["hard"] <= GAMMA_CAP, "gamma over cap"

UNCALIBRATED_BANNER = (
    "⚠⚠ SB-7 UNCALIBRATED — sb7-thresholds.json carries honest defaults; CAL rungs are NOT "
    "frozen (no CALIB pin). Scores are rc-grade (sb-7.0-rc), never board-grade. ⚠⚠")

SCORER_VERSION = "sb-7.0" if CALIBRATED else "sb-7.0-rc"

# ── weights + the compression-proof asserts (DESIGN §5.2 verbatim) ───────────────────────────

TIER_WEIGHT_SB7 = {"A": 0.04, "B": 0.09, "C": 0.09, "D": 0.06, "J": 0.12,
                   "V": 0.06, "P": 0.08, "T": 0.14, "X": 0.16, "R": 0.16}
E_WEIGHT = 0.12
assert abs(sum(TIER_WEIGHT_SB7.values()) - 1.0) < 1e-9
assert TIER_WEIGHT_SB7["T"] + TIER_WEIGHT_SB7["X"] + TIER_WEIGHT_SB7["R"] >= 0.46
assert E_WEIGHT >= 0.10

# Absorbed-by-compound checks: computed and reported, weight zero (anti-stacking).
# j_loads_data is DIAGNOSTIC + CRITICAL: multiplier-only, per the §5.4 registry table.
DIAGNOSTIC = {"j_loads_data", "j_console_clean"}

# Checks whose rungs are calibration-owned — exempt from the --reference ace sweep (their cut
# points are SET from the golden's worst-of-5 distribution; grading the golden against them
# pre-freeze is circular). Everything else the reference must ace. §5.6: p_idle_flatness is
# NOT here — the 0-draws/500 ms demand-render rule is frozen in §3.2, not calibrated.
CALIBRATION_OWNED = {"p_drag_frames", "p_stream_apply", "p_under_stream", "p_sync_wall",
                     "p_api_latency", "e_frames_under_drag", "e_stream_apply_latency",
                     "e_under_load_latency", "e_optimistic_paint"}

# ── ROOT_BLOCKS: attribution + F3 multiplier dedup ───────────────────────────────────────────
# Attribution (report-side): a root < 1.0 explains its dependents' shortfalls — one defect,
# not N. Multiplier dedup (compose-side, F3): a CRITICAL root that already multiplied
# suppresses its critical dependents' multipliers; rows carrying parts["vacuous_root"]
# (nothing to lose because nothing loaded) attribute to that root, price zero through their
# tier, and fire no multiplier of their own.
ROOT_BLOCKS = {
    "server_runs": (
        "sync_completeness", "b_money_rendered", "b_buckets_dst", "x_conservation_residual",
        "x_no_lost_write", "r_no_row_loss", "r_no_dupe_effect", "r_cache_truth",
        "r_workflow_durability", "j_loads_data", "j_workflow_journey", "j_first_use",
        "j_sync_journey", "t_vs7dbg_truth",
    ),
    "sync_completeness": (
        "b_total_field", "b_row_shape", "b_chronological_order", "b_summary_shape",
        "b_money_rendered", "b_buckets_dst", "b_viz_records", "j_loads_data",
        "j_first_use", "j_sync_journey", "t_scene_binding", "t_height_pixels",
        "x_l4_convergence", "x_m3_terminal_conservation", "x_conservation_residual",
        "x_no_lost_write", "r_no_row_loss", "r_cache_truth", "c_paged_walk",
        "c_b1_drop_resume", "c_b2_retry_after", "c_b5_generation_304",
        "c_conditional_resync",
    ),
    "t_vs7dbg_truth": (
        "t_scene_binding", "t_layout_basis", "t_draw_budget", "t_pick_buffer",
        "t_pick_real_pass", "t_click_semantics", "t_camera_math", "t_coast_identity",
        "t_coast_reality", "t_labels_culling", "t_brush_link", "t_stream_diff",
    ),
}


def attribute_root_causes(rows: List[Dict]) -> Dict[str, List[str]]:
    """Attribution only — no score changes. The root fires on ANY shortfall (<1.0)."""
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

_MOD_CACHE: Dict[str, object] = {}


def _mod(name: str):
    if name not in _MOD_CACHE:
        try:
            _MOD_CACHE[name] = importlib.import_module(name)
        except Exception:
            _MOD_CACHE[name] = None
    return _MOD_CACHE[name]


def _vendor():
    return _mod("vendor_service_v3")


def _fixtures():
    return _mod("fixtures_v3")


def _schedule_mod():
    return _mod("schedule_sb7")


def _field(obj, key, default=None):
    """Schedule/fixture packs may be dataclasses, plain objects, or dicts — one accessor."""
    if obj is None:
        return default
    if isinstance(obj, dict):
        return obj.get(key, default)
    return getattr(obj, key, default)


def unavail(reason: str) -> Dict:
    """F9 doctrine: legal ONLY for harness-attributable absence (probe error, missing
    vendor-mock surface, section lost to a timeout) with the positive control being the
    harness's own recorded evidence path. Excluded from the tier mean and listed in the
    verdict. An absent APP surface is never this — it is g(0) with ROOT_BLOCKS attribution
    and severity input 0 (see _critical_severity_input)."""
    return {"score": 0.0, "detail": f"PROBE UNAVAILABLE: {reason}"[:300],
            "consequence": "harness failure, not app evidence", "unavailable": True}


def compound(components: Dict[str, float], gates: Dict[str, bool]) -> tuple:
    """gate × min. Gates are binary preconditions (absence scores 0 — the vacuous rule);
    the weakest component bounds the whole, so one defect stops stacking partial credit."""
    if not all(gates.values()):
        return 0.0, {"gate_failed": [k for k, v in gates.items() if not v], **components}
    return min(components.values()), {**{f"gate:{k}": True for k in gates}, **components}


def _ang_dist(a: float, b: float) -> float:
    d = abs(a - b) % 360.0
    return min(d, 360.0 - d)


def _kp_ladder(value, rungs) -> float:
    """CAL value ladder with the top rung tightened by k_P (spec budgets stay printed)."""
    if not rungs:
        return 0.0
    scaled = [(s, (b / K_P if i == 0 else b)) for i, (s, b) in enumerate(rungs)]
    return _ladder(value, scaled)


def _money_ok(currency: str, minor, text: str) -> bool:
    """Exponent + digits from the rendered text, never presentation (sb-6 F24 carried):
    digits must equal the minor amount; decimal places must equal the currency exponent
    (a trailing 3-digit group with exponent 0 reads as thousands grouping and passes)."""
    if not isinstance(minor, int) or not isinstance(text, str) or currency not in CURRENCY_EXP:
        return False
    digits = re.sub(r"\D", "", text)
    if digits != str(minor):
        return False
    e = CURRENCY_EXP[currency]
    m = re.search(r"[.,](\d+)\s*\D*$", text)
    dec_len = len(m.group(1)) if m else None
    if e == 0:
        return dec_len not in (1, 2)
    return dec_len == e or (e == 3 and dec_len == 3)


def _near_rgb(a, b, tol=8) -> bool:
    return bool(a and b) and all(abs(x - y) <= tol for x, y in zip(a, b))


def _dim(rgb: tuple) -> tuple:
    return tuple(round(0.30 * v) for v in rgb)


# ── probe runner ─────────────────────────────────────────────────────────────────────────────

PROBE_SCRIPT = HERE / "product_probe_v3.mjs"


def _probe(scenario: str, base: str, *flags: str, env: Optional[Dict] = None,
           timeout: Optional[int] = None) -> Dict:
    node = os.environ.get("GOOSE_SWARM_RENDER_NODE", "node")
    if not PROBE_SCRIPT.is_file():
        return {"_probe_error": f"{scenario}: product_probe_v3.mjs not present (sibling deliverable)"}
    cmd = [node, str(PROBE_SCRIPT), scenario, base, *flags]
    budget = timeout or (240 if scenario == "viz" else 120)
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=budget,
                           env={**os.environ, **(env or {})})
        try:
            return json.loads(r.stdout)
        except Exception as e:
            return {"_probe_error": f"{scenario}: {type(e).__name__} (exit {r.returncode}, "
                                    f"stderr: {(r.stderr or '')[:200]})"[:400]}
    except Exception as e:
        return {"_probe_error": f"{scenario}: {type(e).__name__}: {e}"[:300]}


class _ProbeThread(threading.Thread):
    """Runs a probe scenario concurrently with vendor pokes (the D1 mutation, the stream
    batch, the feed heal) — result lands in .result after join."""

    def __init__(self, scenario: str, base: str, *flags: str, env: Optional[Dict] = None,
                 timeout: Optional[int] = None):
        super().__init__(daemon=True)
        self.args = (scenario, base, flags, env, timeout)
        self.result: Dict = {}

    def run(self):
        scenario, base, flags, env, timeout = self.args
        self.result = _probe(scenario, base, *flags, env=env, timeout=timeout)


# ── context ──────────────────────────────────────────────────────────────────────────────────

class Ctx:
    def __init__(self, root: Path):
        self.root = root
        self.pkg = root / "app"
        self.web = root / "web"
        self.fixture_seed: Optional[str] = None
        self.schedule = None                 # schedule_sb7.derive(seed)
        self.pack = None                     # fixtures_v3.build(seed)
        self.html = ""                       # index + assets, joined (source greps)
        self.asset_bytes: Dict[str, Optional[int]] = {}
        self.asset_ctypes: Dict[str, str] = {}
        self.decisions_text = ""
        self.harness_missing: List[str] = []
        self.sched_unreached: List[Dict] = []
        # boot
        self.boot_error = ""
        self.boot_secs: Optional[float] = None
        self.port_bound = False
        self.proc_alive_5s = False
        self.root_status: Optional[int] = None
        self.boot_vendor_down_served = False   # ledgerd answered while the vendor refused
        self.notifier_health_boot: Dict = {}
        self.combined_boot: Dict = {}          # the `python -m app` entrypoint check
        self.peer_down_ok: Optional[bool] = None
        # API surfaces
        self.payments: Dict = {}
        self.page_deep: Dict = {}
        self.filtered: Dict = {}
        self.sorted_desc: Dict = {}
        self.summary: Dict = {}
        self.summary_final: Dict = {}
        self.buckets: Dict = {}
        self.events: List[Dict] = []
        self.events_shape: Dict = {}
        self.outbox_status: Dict = {}
        self.outbox_during_partition: Dict = {}
        self.outbox_after_heal: Dict = {}
        self.notifications_proxy: Dict = {}
        self.notifications_proxy_down: Tuple = (None, None)   # (status, body) while notifier down
        self.viz_records: Dict = {}
        self.stream_head: Dict = {}            # SSE content-type + first-bytes shape
        self.api_ctype = ""
        self.json_probe: List = []
        self.val_matrix: List = []
        self.envelope_cases: List = []
        self.auth_matrix: List = []            # (case, want, got, ok)
        self.drafts_admin: Dict = {}
        # sync / trace
        self.trace: List[Dict] = []
        self.trace_path: Optional[Path] = None
        self.sync1_done = False
        self.sync1_wall_ms: Optional[float] = None
        self.sync1_reqs = 0
        self.resync_reqs = 0
        self.resync_304 = 0
        self.resync_cond = 0
        self.read_stream: List[Dict] = []      # {"t", "summary", "rows": {pid: row}}
        self.vendor_committed: Dict = {}       # payment_id -> final {version, status, ...}
        self.vendor_versions: set = set()      # {(payment_id, version)} committed
        self.vendor_payment_count: Optional[int] = None
        self.idempotency_replays: Optional[Dict] = None
        # kills / faults
        self.rows_before_kill = 0
        self.rows_after_restart = -1
        self.b3_result: Dict = {}
        self.b5_result: Dict = {}
        self.b7_result: Dict = {}
        self.workflow: Dict = {}               # F3 API journey + A1/A2 evidence
        self.d2_resubmit: Dict = {}
        self.notifier_processed: List[Dict] = []
        self.notifier_processed_prekill: List[Dict] = []
        self.notifier_notifications: Dict = {}
        self.notifier_health_final: Dict = {}
        self.under_stream: Dict = {}
        self.api_lat: Dict = {}
        self.final_rows: Dict = {}             # payment_id -> row (post-everything spot reads)
        self.final_total: Optional[int] = None
        # decisions / corners
        self.decisions: Dict[str, str] = {}    # "D1"/"D2"/"D3" -> documented text
        # fault evidence (B1..B8 + kill matrix)
        self.b1_result: Dict = {}
        self.b2_result: Dict = {}
        self.b4_result: Dict = {}
        self.b6_result: Dict = {}
        self.kill_ledger: List[Dict] = []      # {"label", "before", "after", "restarted"}
        self.notifier_alive_during_kill: Optional[bool] = None
        self.walk_stats: Dict = {}             # trace-derived sync-#1 walk facts
        self.read_targets: List[str] = []      # [refund pid, race-k1 served pid]
        self.notif_partition: Dict = {}        # load-probe emit during the partition
        self.notif_heal: Dict = {}             # load-probe emit after the heal
        self.heal_live_secs: Optional[float] = None
        self.quiesced = False
        self.expected_total_at_load: Optional[int] = None
        self.expected_total_at_probe: Optional[int] = None   # N + creates committed by then
        self.expected_final_bycur: Dict = {}
        self.viz_digest_expected: Dict = {}
        self.draft_ids: Dict[str, str] = {}    # "F1"/"F2"/"F3" -> draft id
        self.app_created_count = 0             # midwalk + vendor payments from approved drafts
        # probes
        self.probes: Dict[str, Dict] = {}      # scenario -> emit
        self._txr_rows: List[Dict] = []


def _pv(c: Ctx, scenario: str) -> Dict:
    p = c.probes.get(scenario)
    return p if isinstance(p, dict) else {}


def _viz_section(c: Ctx, key: str):
    """Incremental-emit awareness (sb-6 F18 carried): a section absent from a timed-out viz
    emit is harness loss, not app evidence. Returns (section, refusal-or-None)."""
    p = _pv(c, "viz")
    if _pe(p):
        return None, unavail(_pe(p))
    if key not in p:
        if p.get("timedOut"):
            return None, unavail(f"viz section '{key}' lost to the probe's hard cap (timedOut)")
        return None, None
    return p.get(key), None


# ── registry ─────────────────────────────────────────────────────────────────────────────────

SB7_CHECKS: List[tuple] = []


def check(name: str, tier: str) -> Callable:
    def deco(fn):
        SB7_CHECKS.append((name, tier, fn))
        return fn
    return deco


# ── SEVERITY REGISTRY (DESIGN §5.4 — 12 members, every one golden-1.0-provable) ──────────────
# Severity is a property of the CONSEQUENCE, not the tier. Cliffs live inside the checks
# (1.0-or-collapse with vacuous-pass gates); the multiplier reads the measured severity input
# through _critical_severity_input. Admission rule: a check enters only if the golden provably
# scores exactly 1.0 on it under every calibration seed — criticals are facts a correct app
# achieves, never budgets (enforced at the freeze gate: golden == 1.0, not >= 0.95).
CRITICAL_CHECKS = {
    "server_runs":              "crash — the tool does not run",
    "sync_completeness":        "data loss — silently missing payments",
    "b_money_rendered":         "wrong money — wrong exponent/digits or a cross-currency sum",
    "b_buckets_dst":            "wrong money — mis-bucketed days",
    "x_conservation_residual":  "wrong money — unexplained minor units created/destroyed "
                                "after dupe/loss attribution",
    "x_no_lost_write":          "wrong money — an acknowledged mutation absent from final state",
    "r_no_row_loss":            "data loss — a committed row missing after any seeded kill",
    "r_no_dupe_effect":         "wrong money — a ledger effect applied twice",
    "r_cache_truth":            "data loss — 304-vs-cache mismatch served as fresh",
    "r_workflow_durability":    "data loss — submitted/approved state reverting after SIGKILL",
    "j_loads_data":             "dead primary flow — no data visible",
    "j_workflow_journey":       "dead primary flow — approval cannot complete through the UI",
}
assert not (set(CRITICAL_CHECKS) & CALIBRATION_OWNED), "criticals must never be CAL-owned"

# ── the coverage ledger (DESIGN §5.3) ────────────────────────────────────────────────────────
# spec anchor -> (probe scenario, evidence key, check). Import-time: every roster check is
# registered (asserted after the registry closes). Freeze-time: every row must have produced
# non-vacuous causal evidence on the golden (probe-emitted, click-driven, rendered-means-seen).
AFFORDANCE_ROSTER = {
    "#payments-table":  ("load", "tableRendered",     "j_first_use"),
    "#payments-table@boot": ("empty", "preSyncState", "j_empty_state"),
    "#sync-now":        ("sync", "syncCausal",        "j_sync_journey"),
    "#role-token":      ("flow", "roleTokenAccepted", "j_workflow_journey"),
    "#draft-form":      ("flow", "draftCreated",      "j_workflow_journey"),
    "#draft-list":      ("flow", "draftListStates",   "j_workflow_journey"),
    "#approve-btn":     ("flow", "approveCausal",     "j_workflow_journey"),
    "#reject-btn":      ("flow", "rejectCausal",      "j_workflow_reject"),
    "#notifications":   ("load", "notifications",     "j_notifications_feed"),
    "#viz3d":           ("viz",  "contextReal",       "t_context_real"),
    "#viz3d.pick":      ("viz",  "picks",             "t_pick_buffer"),
    "#viz-labels":      ("viz",  "labels",            "t_labels_culling"),
    "#brush-link":      ("viz",  "brush",             "t_brush_link"),
    "#brush-count":     ("viz",  "brush",             "t_brush_link"),
    "#stream-apply":    ("viz",  "streamApplied",     "t_stream_diff"),
}

# ── chunk-2 helpers ──────────────────────────────────────────────────────────────────────────

import perf_probe  # noqa: E402


def _rgb_of(css: str) -> Optional[tuple]:
    m = re.match(r"rgba?\((\d+),\s*(\d+),\s*(\d+)", str(css or ""))
    return (int(m.group(1)), int(m.group(2)), int(m.group(3))) if m else None


def _post_json(url: str, payload, headers: Optional[Dict] = None, timeout: float = 30):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), method="POST",
                                 headers={"Content-Type": "application/json",
                                          **(headers or {})})
    return _req(req, timeout)


def _get_auth(url: str, token: Optional[str], timeout: float = 10):
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    return _req(urllib.request.Request(url, headers=headers), timeout)


def _post_auth(url: str, payload, token: Optional[str], timeout: float = 30):
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    return _post_json(url, payload if payload is not None else {}, headers, timeout)


def _parse_decisions(text: str) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for m in re.finditer(r"^##\s*(D[123])\b[^\n]*\n(.*?)(?=^##\s*D[123]\b|\Z)",
                         text or "", re.M | re.S):
        out[m.group(1)] = m.group(2).strip()
    return out


_CUR_TOKENS = [("KWD", r"KWD|د\.ك"), ("JPY", r"JPY|¥"), ("USD", r"USD|\$"), ("EUR", r"EUR|€")]


def _detect_currency(text: str) -> Optional[str]:
    for cur, pat in _CUR_TOKENS:
        if re.search(pat, text):
            return cur
    return None


def _list_path() -> str:
    v = _vendor()
    return getattr(v, "LIST_PATH", "/v3/payments") if v else "/v3/payments"


def _segments(trace: List[Dict]) -> Dict[str, List[Dict]]:
    """Phase-marked trace segments ({"__phase__": name} boundary lines, v2 house style)."""
    segs: Dict[str, List[Dict]] = {}
    cur = "preboot"
    for e in trace:
        ph = e.get("__phase__")
        if ph is not None:
            cur = str(ph)
            segs.setdefault(cur, [])
            continue
        segs.setdefault(cur, []).append(e)
    return segs


def _list_events(seg: List[Dict]) -> List[Dict]:
    lp = _list_path()
    return [e for e in seg if e.get("path") == lp and e.get("method") == "GET"]


def _notif_state(p: Dict) -> Optional[str]:
    """The #notifications data-state from a probe emit, across the emit-shape candidates."""
    for k in ("notifications", "notificationsFeed", "feed"):
        v = p.get(k)
        if isinstance(v, dict):
            s = v.get("state") or v.get("dataState")
            if s:
                return str(s)
        elif isinstance(v, str):
            return v
    s = p.get("notificationsState")
    return str(s) if s else None


def _vacuous(root: str) -> Dict:
    """A CRITICAL leg with nothing to prove because a named root already failed (F3): the
    row attributes to the root, prices zero weight (excluded from its tier mean), and fires
    no multiplier of its own — the root already multiplied."""
    return {"score": 0.0,
            "detail": f"vacuous — nothing to lose because `{root}` already failed",
            "consequence": "attributed to the root defect, not double-priced",
            "parts": {"vacuous_root": root}}


def _sync_dead(c: Ctx) -> bool:
    total = (c.payments or {}).get("total")
    return not (isinstance(total, int) and total >= N_FROZEN // 2)


def _absent(surface: str, root: Optional[str] = None) -> Dict:
    """F9: a REQUIRED app surface is missing — g(0) with attribution, never unavail(); the
    severity input for a critical riding it is 0 (an invariant that cannot be evidenced is
    unproven; non-implementation must never outscore implemented-with-one-violation)."""
    parts = {"absent_surface": surface}
    if root:
        parts["vacuous_root"] = root
    return {"score": 0.0, "detail": f"required app surface absent: {surface}",
            "consequence": "the graded surface is not implemented", "parts": parts}


# ══ A: structure and runtime ═════════════════════════════════════════════════════════════════

@check("a_package_layout", "A")
def _(c: Ctx):
    want = ["app/__main__.py", "DECISIONS.md"] + list(WEB_FILES)
    have = [w for w in want if (c.root / w).is_file()]
    mods = []
    for m in ("ledgerd", "notifierd"):
        ok = (c.pkg / f"{m}.py").is_file() or (c.pkg / m / "__main__.py").is_file()
        mods.append((f"app.{m}", ok))
    score = (len(have) + sum(1 for _n, ok in mods if ok)) / (len(want) + 2)
    return g(score, f"{len(have)}/{len(want)} named files, "
             f"{sum(1 for _n, ok in mods if ok)}/2 bootable service modules",
             "files the spec names by path are missing",
             parts={**{w: (w in have) for w in want}, **dict(mods)})


@check("server_runs", "A")
def _(c: Ctx):
    if c.root_status == 200 and c.notifier_health_boot:
        s = 1.0 if (c.boot_secs or 99) <= 10 else 0.75
        return g(s, f"both services healthy, ledgerd in {c.boot_secs}s", "")
    if c.root_status == 200:
        return g(0.6, "ledgerd serves; notifierd never reported health",
                 "half the tool runs")
    if c.port_bound:
        return g(0.4, "port bound but GET / non-200", "the app runs but cannot serve")
    if c.proc_alive_5s:
        return g(0.15, "process survives 5s without binding", "the tool does not serve")
    return g(0.0, f"crash at boot ({c.boot_error[:100]})", "the tool does not run at all")


@check("a_combined_entrypoint", "A")
def _(c: Ctx):
    b = c.combined_boot or {}
    if not b:
        return unavail("combined-entrypoint smoke never ran (harness phase skipped)")
    parts = {"booted": bool(b.get("booted")), "ledger_healthy": bool(b.get("ledger_ok")),
             "notifier_healthy": bool(b.get("notifier_ok"))}
    return g(sum(parts.values()) / 3,
             f"python -m app: {b.get('detail', parts)}",
             "the documented single-command entrypoint does not boot both services",
             parts=parts)


@check("serves_page", "A")
def _(c: Ctx):
    if c.root_status != 200 or "<" not in c.html:
        exists = (c.root / "web/index.html").is_file()
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
    sizes = {w: (c.root / w).stat().st_size if (c.root / w).is_file() else None
             for w in WEB_FILES}
    known = [v for v in sizes.values() if v is not None]
    if not known:
        return g(0.0, "no web files on disk", "there is no frontend")
    total = sum(known)
    size_leg = 1.0 if total <= ASSET_BUDGET_BYTES else \
        (0.3 if total <= ASSET_BUDGET_BYTES * 1.2 else 0.0)
    cdn = re.findall(r'(?:src|href)\s*=\s*["\']https?://[^"\']+', c.html)
    cdn += re.findall(r'\bimport\s*\(?\s*["\']https?://[^"\']+', c.html)
    cdn += re.findall(r'\bfetch\s*\(\s*["\']https?://[^"\']+', c.html)
    offline_leg = 1.0 if not cdn else 0.0
    return g(min(size_leg, offline_leg),
             f"{total // 1024} KB of {ASSET_BUDGET_BYTES // 1024} KB "
             f"({len(known)}/4 files), {len(cdn)} external ref(s)",
             "the budget exists so a vendored 3D library cannot; external refs break the "
             "self-contained contract",
             parts={"bytes": total, "external_refs": len(cdn), **sizes})


@check("a_db_ownership", "A")
def _(c: Ctx):
    db_dir = getattr(c, "db_dir", None)
    if db_dir is None:
        return unavail("db-dir never recorded by the harness")
    ledger = (Path(db_dir) / "ledger.db").is_file()
    notifier = (Path(db_dir) / "notifier.db").is_file()
    extras = [p.name for p in Path(db_dir).glob("*.db")
              if p.name not in ("ledger.db", "notifier.db")]
    s = (0.5 * ledger + 0.5 * notifier) * (1.0 if not extras else 0.8)
    return g(s, f"ledger.db={ledger} notifier.db={notifier}"
             + (f", extras {extras[:3]}" if extras else ""),
             "one SQLite file per service under --db-dir is the §2.1 contract",
             parts={"ledger.db": ledger, "notifier.db": notifier, "extras": extras})


# ══ B: API correctness ═══════════════════════════════════════════════════════════════════════

@check("sync_completeness", "B")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    candidates = [(c.payments or {}).get("total"), c.final_total,
                  (c.summary or {}).get("total")]
    got = max((v for v in candidates if isinstance(v, int)), default=0)
    got_base = min(got, N_FROZEN)          # creates never mask a shortfall of base rows
    return g(min(got_base / N_FROZEN, 1.0),
             f"{got}/{N_FROZEN} payments after the self-driven full sync",
             "an incomplete sync silently loses payments",
             parts={"evidenced_total": got})


@check("b_total_field", "B")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no full state to grade the total against",
                 "a wrong total breaks the caller's paging",
                 parts={"vacuous_root": "sync_completeness"})
    want = c.expected_total_at_load if isinstance(c.expected_total_at_load, int) \
        else N_FROZEN
    total = (c.payments or {}).get("total")
    if total in (want, N_FROZEN, want + 1):
        s = 1.0 if total == want else 0.75    # off by an in-flight create only
    elif isinstance(total, int) and abs(total - want) <= 4:
        s = 0.5
    else:
        s = 0.0
    return g(s, f"total={total} (want {want})", "a wrong total breaks the caller's paging")


@check("b_row_shape", "B")
def _(c: Ctx):
    """The spec's ten /api/payments row keys exactly (day is viz-records-only; the vendor's
    nested counterparty is flattened into counterparty_name/country)."""
    data = (c.payments or {}).get("data") or []
    if not data:
        if _sync_dead(c):
            return g(0.0, "no rows", "n/a", parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "no rows", "n/a")
    keys = set(data[0]) if isinstance(data[0], dict) else set()
    s = len(keys & PAYMENT_ROW_KEYS) / len(PAYMENT_ROW_KEYS)
    stray = keys - PAYMENT_ROW_KEYS
    if stray:
        s = max(0.0, s - 0.1)
    return g(s, f"{len(keys & PAYMENT_ROW_KEYS)}/{len(PAYMENT_ROW_KEYS)} documented keys"
             + (f", stray {sorted(stray)[:3]}" if stray else ""),
             "the documented row contract is not met")


@check("b_chronological_order", "B")
def _(c: Ctx):
    data = (c.payments or {}).get("data") or []
    if len(data) < 2:
        if _sync_dead(c):
            return g(0.0, "too few rows", "n/a", parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "too few rows", "n/a")
    times = [_instant(r.get("created_at")) for r in data if isinstance(r, dict)]
    if any(t is None for t in times):
        return g(0.0, "unparseable created_at", "timestamps are not RFC3339")
    pairs = sum(1 for a, b in zip(times, times[1:]) if a <= b)
    return g(pairs / (len(times) - 1),
             f"{pairs}/{len(times) - 1} adjacent pairs ordered by instant",
             "mixed offsets make string ordering wrong; payments show out of sequence")


@check("b_summary_shape", "B")
def _(c: Ctx):
    s = c.summary or {}
    if not s:
        if _sync_dead(c):
            return g(0.0, "no summary", "n/a", parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "no summary served", "the summary is the money surface")
    by = [x for x in (s.get("by_currency") or []) if isinstance(x, dict)]
    by_keys = bool(by) and all({"currency", "count", "total_minor"} <= set(x) for x in by)
    by_sorted = [x.get("currency") for x in by] == sorted(x.get("currency") for x in by)
    rev = s.get("reversals")
    rev_present = isinstance(rev, list)
    rev_rows = [x for x in (rev or []) if isinstance(x, dict)]
    rev_keys = (not rev_rows) or all({"currency", "count", "total_minor"} <= set(x)
                                     for x in rev_rows)
    rev_sorted = [x.get("currency") for x in rev_rows] == \
        sorted(x.get("currency") for x in rev_rows)
    parts = {"by_currency_keys": by_keys, "by_currency_sorted": by_sorted,
             "reversals_block": rev_present, "reversals_keys": rev_keys,
             "reversals_sorted": rev_sorted}
    return g(sum(parts.values()) / 5,
             f"by_currency {len(by)} rows (sorted={by_sorted}), reversals "
             f"{'present' if rev_present else 'MISSING'} ({len(rev_rows)} rows)",
             "the §4.3 summary contract (per-currency blocks, ascending, reversals) is not met",
             parts=parts)


@check("b_money_rendered", "B")
def _(c: Ctx):
    """CRITICAL. Exponent + digits from the rendered text (F24 lineage) plus the actual
    cross-currency SUM scan on the API (F16: never a key name). Cross-sum -> leg 0."""
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    p = _pv(c, "load")
    if _pe(p):
        return unavail(_pe(p))
    if c.pack is None:
        return unavail("fixtures_v3 pack missing — no amount ground truth")
    texts = [t for t in (p.get("amountTexts") or []) if isinstance(t, str) and t.strip()]
    if not texts:
        return g(0, "no amount cells rendered", "money is the product")
    first_page = c.pack.payments[:50]
    by_cur: Dict[str, set] = {}
    for row in first_page:
        by_cur.setdefault(row["currency"], set()).add(row["amount_minor"])
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
    offending: List[str] = []
    for summ in (c.summary, c.summary_final):
        if not isinstance(summ, dict):
            continue
        singles = {x.get("total_minor") for x in (summ.get("by_currency") or [])
                   if isinstance(x, dict)}
        cross = sum(v for v in singles if isinstance(v, int))
        if not singles or cross in singles:
            continue
        for key, v in summ.items():
            if isinstance(v, int) and v == cross and v not in singles:
                offending.append(key)
    score = 0.6 * ok + 0.4 * trap
    if offending:
        score = 0.0
    return g(score,
             f"{ok:.2f} of {len(graded)} cells exponent-correct; JPY/KWD traps {trap:.2f}"
             + (f"; CROSS-CURRENCY SUM in {sorted(set(offending))}" if offending else ""),
             "a yen with two decimals — or money summed across currencies — is wrong money",
             parts={"cells": len(graded), "ok_frac": round(ok, 3), "jpy_kwd": trap_parts,
                    "cross_currency_sum": sorted(set(offending))})


@check("b_buckets_dst", "B")
def _(c: Ctx):
    """CRITICAL. The severity leg is the DST-window cells specifically (F2); the fraction
    still prices the tier weight."""
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    got = {(x.get("day"), x.get("status")): x.get("count")
           for x in (c.buckets or {}).get("cells", []) if isinstance(x, dict)}
    want = dict(getattr(c, "buckets_expected", None)
                or {(x["day"], x["status"]): x["count"]
                    for x in c.pack.EXPECTED_BUCKETS["cells"]})
    if not got:
        return g(0, "no cells returned", "the buckets endpoint misstates daily money")
    exact = sum(got.get(k) == v for k, v in want.items()) / len(want)
    dst_day = c.pack.days[c.pack.dst_index].isoformat()
    window = [k for k in want if abs((_instant(k[0] + "T12:00:00Z") or 0)
                                     - (_instant(dst_day + "T12:00:00Z") or 0)) <= 2 * 86400]
    dst_ok = (sum(got.get(k) == want[k] for k in window) / len(window)) if window else 0.0
    shape = 1.0 if len(got) == len(want) else 0.5
    tz = 1.0 if (c.buckets or {}).get("timezone") == "Europe/Berlin" else 0.0
    return g(0.7 * exact + 0.2 * shape + 0.1 * tz,
             f"{sum(got.get(k) == v for k, v in want.items())}/{len(want)} cells exact, "
             f"DST-window {dst_ok:.2f}, tz={(c.buckets or {}).get('timezone')}",
             "Berlin-day bucketing across the DST switch is the data-correctness trap",
             parts={"exact_frac": round(exact, 4), "dst_window_ok": round(dst_ok, 4),
                    "shape": shape, "tz": tz})


@check("b_viz_records", "B")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    v = c.viz_records or {}
    if not v:
        if _sync_dead(c):
            return g(0.0, "no /api/viz/records", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "/api/viz/records absent or empty", "the 3D field has no data source")
    cols = [k for k in VIZ_COLUMNS if isinstance(v.get(k), list)]
    count = v.get("count")
    lens = {len(v[k]) for k in cols} if cols else set()
    equal_len = len(lens) == 1 and (count in lens)
    n = lens.pop() if len(lens) == 1 else 0
    order_ok, day_ok = 0.0, 0.0
    if n >= 2 and {"created_at", "id", "day"} <= set(cols):
        sample = range(0, min(n, 600))
        insts = [_instant(v["created_at"][i]) for i in sample]
        pairs = [(a, b) for a, b in zip(sample, list(sample)[1:])]
        ok_pairs = sum(1 for a, b in pairs
                       if (insts[a], v["id"][a]) <= (insts[b], v["id"][b]))
        order_ok = ok_pairs / len(pairs) if pairs else 0.0
        idx = c.pack.index()
        checked = ok_cnt = 0
        for i in sample:
            row = idx.get(v["id"][i])
            if row is None:
                continue
            checked += 1
            ok_cnt += 1 if v["day"][i] == row["_day"] else 0
        day_ok = (ok_cnt / checked) if checked else 0.0
    parts = {"columns": len(cols), "equal_len": equal_len, "count": count,
             "order_ok": round(order_ok, 3), "berlin_day_ok": round(day_ok, 3)}
    score = (0.3 * (len(cols) / len(VIZ_COLUMNS)) + 0.2 * equal_len
             + 0.2 * order_ok + 0.3 * day_ok)
    return g(score, f"{len(cols)}/{len(VIZ_COLUMNS)} columns, count={count}, "
             f"order {order_ok:.2f}, server Berlin day {day_ok:.2f}",
             "a frontend recomputing days in UTC produces probeably wrong x positions",
             parts=parts)


@check("b_events_log", "B")
def _(c: Ctx):
    ev = c.events or []
    if not ev:
        shape = c.events_shape or {}
        if shape.get("status") == 404:
            return _absent("/api/events")
        return g(0.0, f"no events returned (status {shape.get('status')})",
                 "the append-only ledger is the systems tier's spine")
    seqs = [e.get("seq") for e in ev if isinstance(e, dict)]
    contiguous = seqs == list(range(1, len(seqs) + 1))
    types_ok = sum(1 for e in ev if e.get("type") in EVENT_TYPES) / len(ev)
    src_ok = sum(1 for e in ev if e.get("source") in EVENT_SOURCES) / len(ev)
    keys_ok = sum(1 for e in ev if {"seq", "type", "at"} <= set(e)) / len(ev)
    parts = {"count": len(ev), "contiguous_from_1": contiguous,
             "types_ok": round(types_ok, 3), "sources_ok": round(src_ok, 3),
             "keys_ok": round(keys_ok, 3)}
    return g(0.4 * contiguous + 0.2 * types_ok + 0.2 * src_ok + 0.2 * keys_ok,
             f"{len(ev)} events, contiguous={contiguous}, vocab {types_ok:.2f}/{src_ok:.2f}",
             "a seq gap is evidence of a lost write and is graded as one", parts=parts)


@check("b_error_envelope", "B")
def _(c: Ctx):
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


@check("b_json_shapes", "B")
def _(c: Ctx):
    if not c.json_probe:
        return g(0.0, "no responses sampled", "n/a")
    total = sum(1.0 if parses else 0.0 for _p, _s, parses, _ctype in c.json_probe)
    return g(total / len(c.json_probe),
             f"{total:.0f}/{len(c.json_probe)} responses parse as JSON",
             "an HTML error page mid-API breaks every client that trusted the contract")


# ══ C: vendor discipline ═════════════════════════════════════════════════════════════════════

@check("c_paged_walk", "C")
def _(c: Ctx):
    w = c.walk_stats or {}
    if not c.trace:
        return unavail("no vendor trace")
    lists = w.get("sync1_lists", 0)
    if not lists:
        if _sync_dead(c):
            return g(0.0, "no list requests in sync #1", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "no list requests at all", "the client never paged the collection")
    served = w.get("sync1_200", 0)
    offenders = w.get("undocumented_params", 0)
    dup_pages = w.get("dup_served_pages", 0)
    complete = min(served / PAGES, 1.0)
    clean = max(0.0, 1.0 - offenders / lists)
    no_dupes = 1.0 if dup_pages == 0 else max(0.0, 1.0 - dup_pages / PAGES * 10)
    score, parts = compound(
        {"complete_192": round(complete, 3), "documented_params": round(clean, 3),
         "no_duplicate_pages": round(no_dupes, 3)},
        {"walked": served > 0})
    return g(score, f"{served}/{PAGES} pages served, {offenders} undocumented-param "
             f"requests, {dup_pages} duplicate pages",
             "the server-fixed 64-page protocol is the only documented walk", parts=parts)


@check("c_b1_drop_resume", "C")
def _(c: Ctx):
    b = c.b1_result or {}
    if not c.trace:
        return unavail("no vendor trace")
    if not b.get("armed"):
        if _sync_dead(c):
            return g(0.0, "walk never reached the drop page", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return unavail("B1 drop never fired (sched-unreached or vendor surface missing)")
    parts = {"resumed": bool(b.get("resumed")),
             "no_full_restart": bool(b.get("no_full_restart")),
             "walk_completed": bool(b.get("walk_completed"))}
    return g(sum(parts.values()) / 3, f"{b}",
             "a dropped connection must cost a resume, not a restart or a hole",
             parts=parts)


@check("c_b2_retry_after", "C")
def _(c: Ctx):
    b = c.b2_result or {}
    if not c.trace:
        return unavail("no vendor trace")
    if not b.get("armed"):
        if _sync_dead(c):
            return g(0.0, "walk never reached the 500 page", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return unavail("B2 500 never fired (sched-unreached or vendor surface missing)")
    parts = {"single_retry": bool(b.get("single_retry")),
             "waited_retry_after": bool(b.get("waited", True)),
             "continued": bool(b.get("continued"))}
    return g(sum(parts.values()) / 3, f"{b}",
             "one 500 must cost one documented retry, never a fresh restart of committed work",
             parts=parts)


@check("c_b5_generation_304", "C")
def _(c: Ctx):
    b = c.b5_result or {}
    if not c.trace:
        return unavail("no vendor trace")
    if not b.get("armed"):
        if _sync_dead(c):
            return g(0.0, "the armed sync never ran", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return unavail("B5 lying-304 never armed/fired (vendor surface missing or "
                       "sched-unreached)")
    conds = b.get("conditional_repeats", 99)
    parts = {"one_unconditional_refetch": b.get("unconditional_refetches") == 1,
             "no_loop": conds <= 3,
             "propagated": bool(b.get("propagated"))}
    return g(sum(parts.values()) / 3,
             f"unconditional={b.get('unconditional_refetches')} conditional-repeats={conds} "
             f"propagated={b.get('propagated')}",
             "> 3 identical conditional requests is the documented infinite-loop bug",
             parts=parts)


@check("c_conditional_resync", "C")
def _(c: Ctx):
    if not c.trace:
        return unavail("no vendor trace")
    reqs = c.resync_reqs
    if not reqs:
        if _sync_dead(c):
            return g(0.0, "no re-sync observed", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "no post-mutation re-sync requests observed",
                 "never sending conditional requests is itself a graded failure (§4.1)")
    # Harness fix: the 304 FRACTION is vendor-controlled (the global collection
    # generation moves with every commit, so a re-sync after real mutations gets
    # honest 200s on every page no matter how disciplined the client), which made the
    # old 0.5·cond + 0.5·304 formula structurally unreachable. What the CLIENT owns is
    # conditional discipline: every request carries a validator except the documented
    # B5 unconditional refetch and the first visit to a page that did not exist at
    # fixture load. 304s stay reported as evidence.
    allowed = getattr(c, "resync_allowed_uncond", 0)
    cheap = min(1.0, (c.resync_cond + allowed) / reqs)
    return g(cheap, f"{c.resync_cond}/{reqs} conditional ({allowed} documented "
             f"unconditional), {c.resync_304} x 304 across post-mutation syncs",
             "an unconditional full re-walk on every sync is the expensive-client defect")


@check("c_webhook_discipline", "C")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    w = c.walk_stats or {}
    deliveries = w.get("webhook_deliveries")
    if deliveries is None:
        return unavail("vendor trace carries no webhook delivery records "
                       "(vendor_service_v3 surface missing)")
    if not deliveries:
        if _sync_dead(c):
            return g(0.0, "no webhook deliveries observed", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return g(0.0, "vendor delivered no webhooks the app accepted", "n/a")
    acked = w.get("webhook_2xx", 0)
    forged_status = w.get("forged_status")
    parts = {"deliveries_acked": round(acked / deliveries, 3),
             "forged_401": forged_status == 401}
    return g(0.6 * parts["deliveries_acked"] + 0.4 * parts["forged_401"],
             f"{acked}/{deliveries} deliveries 2xx-acked, forged -> {forged_status}",
             "push traffic is untrusted input; a forged signature must bounce with 401",
             parts=parts)


@check("c_send_idempotency", "C")
def _(c: Ctx):
    wf = c.workflow or {}
    sends = wf.get("send_requests")
    if sends is None:
        return unavail("vendor trace carries no POST /v3/payments records")
    if not sends:
        return g(0.0, "the app never sent an approved draft to the vendor",
                 "the approve step's SEND half is missing")
    keyed = wf.get("send_with_key", 0)
    reused = wf.get("retry_reused_key")
    parts = {"all_keyed": keyed == sends,
             "retry_reused_key": bool(reused) if reused is not None else (keyed == sends)}
    return g(sum(parts.values()) / 2,
             f"{keyed}/{sends} sends carried Idempotency-Key, retry_reused={reused}",
             "a fresh key per retry is the seeded dupe r_no_dupe_effect catches",
             parts=parts)


# ══ D: robustness / judgment ═════════════════════════════════════════════════════════════════

@check("d_content_types", "D")
def _(c: Ctx):
    cells = []
    if c.json_probe:
        ok = sum(1 for _p, _s, _parses, ctype in c.json_probe
                 if "json" in (ctype or "").lower())
        cells.append(ok / len(c.json_probe))
    sse = (c.stream_head or {}).get("ctype", "")
    cells.append(1.0 if "text/event-stream" in sse else 0.0)
    if not cells:
        return g(0.0, "no responses sampled", "n/a")
    return g(sum(cells) / len(cells),
             f"api json {cells[0]:.2f}, /api/stream ctype '{sse[:40]}'",
             "a wrong content type breaks strict clients and kills EventSource")


@check("d_validation", "D")
def _(c: Ctx):
    matrix = list(c.val_matrix) + list(c.auth_matrix)
    if not matrix:
        return g(0.0, "matrix not exercised", "n/a")
    score = sum(1.0 if row[-1] else 0.0 for row in matrix) / len(matrix)
    bad = [row[0] for row in matrix if not row[-1]]
    return g(score, f"{sum(1 for r in matrix if r[-1])}/{len(matrix)} status codes correct"
             + (f"; wrong: {bad[:4]}" if bad else ""),
             "invalid input or a wrong role accepted instead of the documented rejection",
             parts={"wrong_cells": bad})


@check("d_client_timeouts", "D")
def _(c: Ctx):
    b4 = c.b4_result or {}
    if b4.get("served_while_down"):
        return g(1.0, "ledgerd bound and served local state while the vendor refused "
                 f"connections ({b4.get('refusal_secs')}s window)", "")
    src = ""
    for name in ("ledgerd.py", "meridian.py", "vendor.py", "sync.py"):
        f = c.pkg / name
        if f.is_file():
            src += f.read_text(errors="replace")
    hit = re.search(r"timeout\s*=", src)
    return g(0.4 if hit else 0.0,
             "vendor-down boot not proven; timeout= present in source" if hit
             else "no request timeout and no vendor-down evidence",
             "one unresponsive vendor call hangs the boot with no recovery")



def _error_obj(body) -> dict:
    """The `error` envelope of a response body, or {} when there is none.

    An app may answer {"error": "Not found"} -- a STRING, not an envelope. Every site that did
    `(body.get("error") or {}).get(...)` crashed on that shape with AttributeError, and a scorer that
    crashes reports nothing. r0 hit it twice in one run (the validation matrix and d_peer_absence)
    after one site had already been fixed in isolation. This is the single rule; use it everywhere.
    """
    if not isinstance(body, dict):
        return {}
    err = body.get("error")
    return err if isinstance(err, dict) else {}

@check("d_peer_absence", "D")
def _(c: Ctx):
    st, body = c.notifications_proxy_down
    parts = {
        "proxy_502": st == 502,
        "envelope_code": _error_obj(body).get("code") == "notifier_unreachable",
        "ledgerd_alive_through_partition": bool((c.b7_result or {}).get("ledger_alive")),
        "notifierd_alive_through_ledger_kill": bool(c.notifier_alive_during_kill),
    }
    if st is None and not (c.b7_result or {}):
        return unavail("peer-absence window never orchestrated (partition phase missing)")
    return g(sum(bool(v) for v in parts.values()) / len(parts),
             f"proxy -> {st} code={parts['envelope_code']}, ledgerd-alive="
             f"{parts['ledgerd_alive_through_partition']}, notifierd-alive="
             f"{parts['notifierd_alive_through_ledger_kill']}",
             "neither service may crash on the other's absence (§2.2)", parts=parts)


@check("d_decisions_doc", "D")
def _(c: Ctx):
    d = c.decisions or {}
    if not d:
        return g(0.0, "DECISIONS.md absent or carries none of the frozen ## D1/D2/D3 "
                 "headings", "the documented-corner register is a named deliverable")

    def _stance(text: str, yes: List[str], no: List[str]) -> Optional[bool]:
        t = (text or "").lower()
        y = any(w in t for w in yes)
        n = any(w in t for w in no)
        if y == n:
            return None
        return y

    corners = []
    d1_obs = ((_pv(c, "viz").get("d1") or {}).get("survived")
              if isinstance(_pv(c, "viz").get("d1"), dict) else None)
    d1_doc = _stance(d.get("D1", ""), ["surviv", "keep", "remain", "stay", "persist"],
                     ["clear", "remove", "drop", "reset", "evict", "deselect"])
    corners.append(("D1", d.get("D1"), d1_doc, d1_obs))
    d2 = c.d2_resubmit or {}
    d2_obs = None
    if isinstance(d2.get("status"), int):
        d2_obs = not (200 <= d2["status"] < 300)          # True = terminal
    d2_doc = _stance(d.get("D2", ""), ["terminal", "cannot", "final", "immutable", "locked"],
                     ["resubmit", "again", "re-submit", "reopen"])
    corners.append(("D2", d.get("D2"), d2_doc, d2_obs))
    e = _pv(c, "empty")
    d3_obs = None
    if isinstance(e, dict) and not _pe(e):
        pre = e.get("preSyncState") or {}
        if isinstance(pre, dict) and ("emptyWithProgress" in pre or "blocked" in pre):
            d3_obs = not bool(pre.get("blocked")) if "blocked" in pre \
                else bool(pre.get("emptyWithProgress"))
    d3_doc = _stance(d.get("D3", ""), ["empty", "progress", "placeholder", "render"],
                     ["block", "spinner only", "wait", "hold"])
    corners.append(("D3", d.get("D3"), d3_doc, d3_obs))

    scores, parts = [], {}
    for name, text, doc, obs in corners:
        documented = bool(text and len(text) >= 15)
        if not documented:
            s = 0.0
        elif doc is None or obs is None:
            s = 0.5      # documented; consistency not machine-inferable this run
        else:
            s = 1.0 if doc == obs else 0.0
        scores.append(s)
        parts[name] = {"documented": documented, "doc_stance": doc, "observed": obs,
                       "score": s}
    return g(sum(scores) / 3,
             ", ".join(f"{n}={parts[n]['score']}" for n, _t, _d, _o in corners),
             "an undocumented or contradicted corner does not pass (§5.7)", parts=parts)


# ══ J: journeys ══════════════════════════════════════════════════════════════════════════════

@check("j_loads_data", "J")
def _(c: Ctx):
    p = _pv(c, "load")
    if _pe(p):
        return unavail(_pe(p))
    rows = p.get("renderedRowCount") or 0
    claimed = p.get("totalClaimedInDom")
    want = c.expected_total_at_probe if isinstance(c.expected_total_at_probe, int) \
        else c.expected_total_at_load
    reconciles = isinstance(want, int) and claimed == want
    return g((0.5 if rows > 0 else 0) + (0.5 if reconciles else 0),
             f"{rows} rows rendered, DOM claims {claimed} (want {want})", "",
             parts={"rendered_rows": rows, "total_claimed": claimed})


@check("j_console_clean", "J")
def _(c: Ctx):
    errs = []
    for name in ("load", "sync"):
        p = _pv(c, name)
        if isinstance(p, dict) and not _pe(p):
            ce = p.get("consoleErrors") or {}
            errs += ce.get("texts") or [""] * (ce.get("count") or 0)
    n = len(errs)
    return g(1.0 if n == 0 else (0.5 if n == 1 else 0.0),
             f"{n} console error(s) across load+sync" + (f": {errs[0][:100]}" if errs else ""),
             "JS errors in normal use" if n else "")


@check("j_first_use", "J")
def _(c: Ctx):
    p = _pv(c, "load")
    if _pe(p):
        return unavail(_pe(p))
    rows = p.get("renderedRowCount") or 0
    ttfd = _ladder(p.get("timeToFirstDataMs"),
                   [(1.0, 2000), (0.75, 3000), (0.5, 4500), (0.25, 8000)])
    want = c.expected_total_at_probe if isinstance(c.expected_total_at_probe, int) \
        else c.expected_total_at_load
    reconcile = 1.0 if (isinstance(want, int) and p.get("totalClaimedInDom") == want) else 0.0
    ce = (p.get("consoleErrors") or {}).get("count", 0)
    clean = 1.0 if ce == 0 else 0.6      # console noise bounds the leg, never zeroes (sb-6)
    score, parts = compound(
        {"first_data": ttfd, "reconciles": reconcile, "console_clean": clean},
        {"rendered_rows": rows > 0})
    return g(score, f"ttfd={p.get('timeToFirstDataMs')}ms reconcile={reconcile} "
             f"consoleErrs={ce}",
             "first use is one experience — slow, wrong, or noisy each bounds it",
             parts=parts)


@check("j_sync_journey", "J")
def _(c: Ctx):
    p = _pv(c, "sync")
    if _pe(p):
        return unavail(_pe(p))
    sc = p.get("syncCausal") if isinstance(p.get("syncCausal"), dict) else p
    parts = {"button_found": bool(sc.get("found")),
             "in_flight_state": bool(sc.get("everDisabled")
                                     or p.get("disabledDuringSync")),
             "completed": bool(sc.get("completed")),
             "state_stays_correct": bool(sc.get("viewRefreshed")
                                         or sc.get("tableHashChanged"))}
    return g(sum(parts.values()) / 4,
             ", ".join(k for k, v in parts.items() if v) or "sync button never found",
             "the spec's headline interactive flow fails in a real browser"
             if not all(parts.values()) else "", parts=parts)


@check("j_workflow_journey", "J")
def _(c: Ctx):
    """CRITICAL — the journey ladder: F1 driven end-to-end through the UI (§4.5)."""
    p = _pv(c, "flow")
    if _pe(p):
        return unavail(_pe(p))
    if not any(k in p for k in ("roleTokenAccepted", "draftCreated", "approveCausal")):
        if p.get("timedOut"):
            return unavail("flow sections lost to the probe's hard cap")
        return _absent("workflow UI (role token / draft form / approve)")

    def _sec(key: str) -> Dict:
        v = p.get(key)
        return v if isinstance(v, dict) else {}

    steps = [("roleTokenAccepted", bool(_sec("roleTokenAccepted").get("found"))),
             ("draftCreated", bool(_sec("draftCreated").get("draftId"))),
             ("submitCausal", bool(_sec("submitCausal").get("reached"))),
             ("approveCausal", bool(_sec("approveCausal").get("reachedApproved"))),
             ("draftListStates", bool(_sec("draftListStates").get("found"))),
             ("notificationSeen", bool(p.get("notificationSeen"))),
             ("paymentInTable", bool(p.get("paymentInTable")))]
    done = sum(1 for _n, v in steps if v)
    score, parts = compound(
        {"journey": done / len(steps)},
        {"token_accepted": bool(_sec("roleTokenAccepted").get("found")),
         "draft_created": bool(_sec("draftCreated").get("draftId"))})
    return g(score, ", ".join(n for n, v in steps if v) or "no step completed",
             "approval cannot complete through the UI — the workflow is dead",
             parts={**parts, **{n: bool(v) for n, v in steps}})


@check("j_workflow_reject", "J")
def _(c: Ctx):
    p = _pv(c, "flow")
    if _pe(p):
        return unavail(_pe(p))
    rc = p.get("rejectCausal") if isinstance(p.get("rejectCausal"), dict) else {}
    dls = p.get("draftListStates") if isinstance(p.get("draftListStates"), dict) else {}
    listed = any(isinstance(r_, dict) and r_.get("state") == "rejected"
                 for r_ in (dls.get("rows") or []))
    parts = {"reject_completed": bool(rc.get("clicked") and rc.get("reachedRejected")),
             "rejected_state_listed": listed,
             "reject_notification": bool(rc.get("notificationSeen"))}
    return g(sum(parts.values()) / 3,
             ", ".join(k for k, v in parts.items() if v) or "rejection never completed",
             "the checker's reject half of the workflow is dead", parts=parts)


@check("j_notifications_feed", "J")
def _(c: Ctx):
    deg = c.notif_partition or {}
    live = c.notif_heal or {}
    if _pe(deg) and _pe(live):
        return unavail("feed probes unavailable in both partition and heal windows")
    deg_state = _notif_state(deg) if not _pe(deg) else None
    live_state = _notif_state(live) if not _pe(live) else None
    within = c.heal_live_secs is not None and c.heal_live_secs <= 5.0
    parts = {"degraded_during_partition": deg_state == "degraded",
             "live_after_heal": live_state == "live",
             "recovered_within_5s": within,
             "no_reload": bool(live.get("noReload", True))}
    return g(sum(parts.values()) / 4,
             f"partition state={deg_state}, heal state={live_state}, "
             f"live in {c.heal_live_secs}s",
             "the feed must visibly degrade and heal itself without a reload (§4.2)",
             parts=parts)


@check("j_error_state", "J")
def _(c: Ctx):
    p = _pv(c, "error")
    if _pe(p):
        return unavail(_pe(p))
    actionable = p.get("errorStateVisible") and re.search(
        r"try again|retry|check|running|refresh|vendor|offline",
        str(p.get("actionableText") or ""), re.I)
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
    p = _pv(c, "empty")
    if _pe(p):
        return unavail(_pe(p))
    rows = p.get("renderedRowCount") or 0
    if rows > 0:
        return g(0, f"{rows} rows rendered on an EMPTY db", "phantom data on first run")
    pre = p.get("preSyncState") or {}
    if isinstance(pre, dict) and (pre.get("emptyWithProgress") or pre.get("blocked") is False):
        return g(1.0, f"pre-sync state rendered: {pre}", "")
    if p.get("emptyStateVisible"):
        return g(1.0, f"empty text: {str(p.get('emptyStateText'))[:50]}", "")
    if (p.get("bodyTextLength") or 0) > 0:
        return g(0.3, "page not blank but no empty state", "a fresh install reads as broken")
    return g(0.0, "blank page on empty db", "a fresh install shows nothing")


# ══ V: rendered visual truth ═════════════════════════════════════════════════════════════════

@check("v_dates_readable", "V")
def _(c: Ctx):
    p = _pv(c, "load")
    if _pe(p):
        return unavail(_pe(p))
    texts = [t for t in (p.get("dateTexts") or []) if t and t.strip()]
    if not texts:
        return g(0, "no date cells rendered", "the Date column is empty in a real browser")
    iso = [t for t in texts if re.search(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}", t)]
    if iso:
        return g(0, f"raw ISO-8601 shown to users: {iso[0][:60]}",
                 "the spec forbids machine timestamps in the rendered page")
    human = any(re.search(r"[A-Za-z]{3}", t) or re.search(r"\d{1,2}[./]\d{1,2}", t)
                for t in texts)
    return g(1.0 if human else 0.5, f"rendered dates like {texts[0][:50]}",
             "" if human else "formatted but not recognizably locale-readable")


@check("v_money_presentation", "V")
def _(c: Ctx):
    """Presentation half only — the exponent/digits truth is b_money_rendered's (CRITICAL,
    evidence-disjoint per F1)."""
    p = _pv(c, "load")
    if _pe(p):
        return unavail(_pe(p))
    texts = [t for t in (p.get("amountTexts") or []) if isinstance(t, str) and t.strip()]
    if not texts:
        return g(0, "no amount cells rendered", "money is the product")
    tagged = sum(1 for t in texts if _detect_currency(t))
    return g(tagged / len(texts),
             f"{tagged}/{len(texts)} amount cells carry a recognizable currency token",
             "an amount with no currency is ambiguous money")


@check("v_status_badges", "V")
def _(c: Ctx):
    p = _pv(c, "load")
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


@check("v_responsive_375", "V")
def _(c: Ctx):
    p = _pv(c, "load")
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
        return g(0.0, "no h-scroll but zero rendered rows at 375px (vacuous)",
                 "empty pages never scroll")
    if "tapTargetsOk" in vp:
        taps = bool(vp.get("tapTargetsOk"))
        return g(1.0 if taps else 0.6,
                 f"no h-scroll, {rows} rows, tap targets {'ok' if taps else 'small'}", "")
    return g(1.0, f"no h-scroll, {rows} rows (tap targets not measured by this probe)", "")


@check("v_styling", "V")
def _(c: Ctx):
    p = _pv(c, "load")
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
             f"stylesheet={parts['stylesheet']}, {s.get('distinctBackgroundCount')} "
             f"backgrounds, font '{font[:30]}', header={parts['branded_header']}",
             "", parts=parts)


# ══ P: performance (CAL rungs via _kp_ladder; idle flatness is the frozen §3.2 rule) ═════════

def _drag_wall_ms(d: Dict) -> Optional[float]:
    w = d.get("watch") or {}
    c0, c1 = w.get("c0") or {}, w.get("c1") or {}
    if isinstance(c0.get("t"), (int, float)) and isinstance(c1.get("t"), (int, float)):
        return round(c1["t"] - c0["t"], 1)
    return None


def _stream_apply_ms(c: Ctx):
    """(median applyMs over graded batches, refusal-or-None) from the viz stream section."""
    st, ref = _viz_section(c, "stream")
    if ref:
        return None, ref
    st = st or {}
    vals = sorted(x["applyMs"] for x in (st.get("perBatch") or [])
                  if isinstance(x, dict) and isinstance(x.get("applyMs"), (int, float)))
    if not vals:
        return None, None
    return vals[len(vals) // 2], None


@check("p_drag_frames", "P")
def _(c: Ctx):
    d, ref = _viz_section(c, "dragBudget")
    if ref:
        return ref
    d = d or {}
    fr = d.get("deltaFrames")
    if fr is None:
        return g(0, "frames not measurable over the scripted drag",
                 "the 3D view must stay interactive under input")
    rungs = [(s, -b) for s, b in (tuple(r) for r in TH["drag_frames_rungs"])]
    return g(_ladder(-fr, rungs),
             f"{fr} frames over the {DRAG_MOVES}-move budget drag ({_drag_wall_ms(d)}ms)",
             "the 3D view must stay interactive under input at N=12,288",
             parts={"frames": fr, "wall_ms": _drag_wall_ms(d)})


@check("p_idle_flatness", "P")
def _(c: Ctx):
    i, ref = _viz_section(c, "idle")
    if ref:
        return ref
    i = i or {}
    windows = [w for w in (i.get("windows") or []) if isinstance(w, dict)]
    valid = [w for w in windows
             if isinstance(w.get("defaultDraws"), int) and not w.get("batchLanded")]
    if not valid:
        return g(0, "no idle windows sampled", "demand rendering unproven")
    worst = max(w["defaultDraws"] for w in valid)
    return g(1.0 if worst == 0 else 0.0,
             f"{worst} default-FBO draws in the worst 500ms rest window "
             f"({len(valid)} sampled)",
             "continuous-rAF renderers fail the frozen §3.2 demand-render rule by design",
             parts={"max_draws_500ms": worst, "windows": len(valid)})


@check("p_stream_apply", "P")
def _(c: Ctx):
    s, ref = _stream_apply_ms(c)
    if ref:
        return ref
    if s is None:
        return g(0, "no stream batch applied", "the live stream never landed")
    return g(_kp_ladder(s, [tuple(r) for r in TH["stream_apply_ms_rungs"]]),
             f"batch visible in {s} ms", "", parts={"ms": s})


@check("p_under_stream", "P")
def _(c: Ctx):
    m = c.under_stream or {}
    if not m:
        return unavail("under-stream measurement never ran (fire_burst surface missing)")
    overlap = m.get("overlap_frac")
    floor = float(TH["underload_overlap_floor"])
    if overlap is None or overlap < floor:
        return unavail(f"only {overlap if overlap is not None else '?'} of reader requests "
                       f"overlapped the burst (floor {floor}) — load unproven")
    if not m.get("correct"):
        return g(0, f"p95={m.get('p95_ms')}ms but responses wrong/empty under load",
                 "fast wrong answers are not performance")
    return g(_kp_ladder(m.get("p95_ms"), [tuple(r) for r in TH["underload_p95_rungs"]]),
             f"p95={m.get('p95_ms')}ms with readers during the stream burst "
             f"(overlap {overlap:.2f})", "",
             parts={"p95_ms": m.get("p95_ms"), "overlap_frac": overlap, "n": m.get("n")})


@check("p_api_latency", "P")
def _(c: Ctx):
    vals = [v.get("p95_ms") for v in (c.api_lat or {}).values()
            if isinstance(v, dict) and isinstance(v.get("p95_ms"), (int, float))]
    if not vals:
        return unavail("idle latency battery never ran")
    worst = max(vals)
    return g(_kp_ladder(worst, [tuple(r) for r in TH["api_p95_ms_rungs"]]),
             f"worst idle p95 {worst} ms across {sorted(c.api_lat)}", "",
             parts={k: v.get("p95_ms") for k, v in (c.api_lat or {}).items()
                    if isinstance(v, dict)})


@check("p_sync_wall", "P")
def _(c: Ctx):
    v = c.sync1_wall_ms
    if v is None:
        return g(0.0, "sync #1 never completed inside the harness budget",
                 "the full walk is the product's first job")
    return g(_kp_ladder(v, [tuple(r) for r in TH["sync_wall_ms_rungs"]]),
             f"sync #1 wall {v:.0f} ms (self-driven, faults included)", "",
             parts={"wall_ms": v})


# ══ T: the 3D contract — five mechanisms, partial rungs (§3) ═════════════════════════════════

@check("t_context_real", "T")
def _(c: Ctx):
    p = _pv(c, "viz")
    if _pe(p):
        return unavail(_pe(p))
    gl = p.get("gl") or {}
    ctxs = [x for x in (gl.get("contexts") or []) if isinstance(x, dict)]
    cr, ref = _viz_section(c, "contextReal")
    if ref:
        return ref
    cr = cr or {}
    main_ctx = next((x for x in ctxs
                     if not x.get("offscreen") and x.get("canvasId") == "viz3d"), None)
    attrs = cr.get("askedAttrs") or (main_ctx or {}).get("askedAttrs") or {}
    parts = {
        "webgl_on_viz3d": main_ctx is not None
                          or bool(cr.get("canvasFound") and cr.get("contextType")),
        "attrs_pinned": attrs.get("antialias") is False and attrs.get("alpha") is False,
        "backing_matches_rect": bool(cr.get("backingOk")),
        "draw_calls": (gl.get("defDraws") or 0) >= 1,
        "coverage": (cr.get("gridNonBg") or 0) >= 3 and (cr.get("gridDistinct") or 0) >= 2,
    }
    s = (0.25 * parts["webgl_on_viz3d"] + 0.15 * parts["attrs_pinned"]
         + 0.2 * parts["backing_matches_rect"] + 0.15 * parts["draw_calls"]
         + 0.25 * parts["coverage"])
    return g(s, f"ctx={len(ctxs)} type={cr.get('contextType')} "
             f"nonBg={cr.get('gridNonBg')}/{cr.get('gridTotal')} "
             f"distinct={cr.get('gridDistinct')} backingOk={cr.get('backingOk')}",
             "no real 3D surface — the panel is decoration or absent", parts=parts)


@check("t_layout_basis", "T")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    lay, ref = _viz_section(c, "layout")
    if ref:
        return ref
    lay = lay or {}
    got = lay.get("got") or lay
    want = c.pack.layout_basis()
    cells = {"d0": got.get("d0") == want["d0"],
             "D0": got.get("D0") == want["D0"],
             "R0": got.get("R0") == want["R0"]}
    stream_sec, _r2 = _viz_section(c, "stream")
    after = (stream_sec or {}).get("layoutAfter")
    unmoved = None
    if isinstance(after, dict):
        got2 = after.get("got") if isinstance(after.get("got"), dict) else None
        if got2 is not None:
            unmoved = all(got2.get(k) == want[k] for k in ("d0", "D0", "R0"))
    score = sum(cells.values()) / 3
    if unmoved is False:
        score = min(score, 0.25)     # the basis moved on a create — the §3.1 pin broke
    return g(score, f"layout got={ {k: got.get(k) for k in ('d0', 'D0', 'R0')} } "
             f"want={want} unmoved_after_stream={unmoved}",
             "the layout basis is locked at load and never moves for a streamed create",
             parts={**cells, "unmoved_after_stream": unmoved})


@check("t_scene_binding", "T")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "no data to bind", "n/a", parts={"vacuous_root": "sync_completeness"})
    d, ref = _viz_section(c, "digest")
    if ref:
        return ref
    d = d or {}
    got = d.get("got") or {}
    want = c.viz_digest_expected or c.pack.expected_scene_digest()
    keys = ("count", "Sh", "Sh2", "Sx", "Sz", "Sxh", "Szh")
    oks = {}
    for k in keys:
        gv, ev = got.get(k), want.get(k)
        oks[k] = isinstance(gv, (int, float)) and \
            abs(gv - ev) <= max(0.5, 1e-4 * abs(ev))
    frac = sum(oks.values()) / len(keys)
    return g(1.0 if frac == 1.0 else 0.5 * frac,
             f"{sum(oks.values())}/{len(keys)} digest moments within tolerance "
             f"(count got={got.get('count')} want={want.get('count')})",
             "the scene does not encode the data — the §3.1 transform is broken",
             parts={**oks, "got": got, "want": want})


@check("t_height_pixels", "T")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no data to measure", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    h, ref = _viz_section(c, "heightPixels")
    if ref:
        return ref
    h = h or {}
    cases = [x for x in (h.get("cases") or []) if isinstance(x, dict)]
    if not cases:
        return g(0, "no height-pixel cases measured", "the digest has no pixel leg (R9)")
    ok = sum(1 for x in cases if x.get("within3px") and x.get("colorOk"))
    curs = {str(x.get("cur")) for x in cases}
    exotic = ("JPY" in curs) and ("KWD" in curs)
    score = (ok / len(cases)) * (1.0 if exotic else 0.7)
    return g(score, f"{ok}/{len(cases)} instance tops within ±3 px "
             f"(currencies {sorted(curs)})",
             "app-reported digests need pixel truth; JPY/KWD heights expose exponent bugs",
             parts={"ok": ok, "total": len(cases), "jpy_kwd_present": exotic})


@check("t_draw_budget", "T")
def _(c: Ctx):
    d, ref = _viz_section(c, "dragBudget")
    if ref:
        return ref
    d = d or {}
    dd = d.get("deltaDefaultDrawsAtRaf")
    ff = d.get("deltaFrames")
    if dd is None:
        return g(0, "wrapper counted no draws over the budget window",
                 "the ≤8-draws budget is the instancing forcing function")
    moves = d.get("moves", DRAG_MOVES)
    lim_frames = DRAW_BUDGET * max(ff or 0, 1)
    lim_moves = DRAW_BUDGET * (moves + 8)
    vel = d.get("velocityAfter") or {}
    ended_slow = True
    if isinstance(vel.get("vyaw"), (int, float)):
        ended_slow = abs(vel.get("vyaw") or 0) < 2 and abs(vel.get("vpitch") or 0) < 2
    parts = {"within_frames_budget": dd <= lim_frames,
             "within_moves_budget": dd <= lim_moves,
             "ended_slow": ended_slow,
             "drew_at_all": (dd or 0) >= 1}
    both = parts["within_frames_budget"] and parts["within_moves_budget"]
    score = 0.0 if not parts["drew_at_all"] else (1.0 if both else 0.25)
    return g(score, f"ΔD={dd} ΔF={ff} over M={moves} moves "
             f"(limits {lim_frames}/{lim_moves})",
             "over-budget draw calls mean per-instance draws — the technique the budget forbids",
             parts={**parts, "dD": dd, "dF": ff})


@check("t_pick_buffer", "T")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no instances to pick", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    k, ref = _viz_section(c, "picks")
    if ref:
        return ref
    k = k or {}
    cases = [x for x in (k.get("results") or []) if isinstance(x, dict)]
    if not cases:
        return g(0, "no graded pick points", "the pick buffer is the occlusion-truth mechanism")

    def _ok(x):
        return bool(x.get("pickAgrees")) and bool(x.get("pixelAgrees"))
    frac = sum(1 for x in cases if _ok(x)) / len(cases)
    want_kinds = ("occluded-by-lower-n", "occluded-by-higher-n", "partial-occlusion",
                  "background-in-hull")
    kind_ok = {kk: any(_ok(x) for x in cases if x.get("cls") == kk) for kk in want_kinds}
    constructions = sum(kind_ok.values()) / len(want_kinds)
    return g(0.6 * frac + 0.4 * constructions,
             f"{sum(1 for x in cases if _ok(x))}/{len(cases)} decisive picks agree "
             f"three ways; constructions {kind_ok}",
             "the occlusion constructions kill last-drawn-wins in both index orders",
             parts={"frac": round(frac, 3), **kind_ok})


@check("t_pick_real_pass", "T")
def _(c: Ctx):
    pc, ref = _viz_section(c, "pickCounters")
    if ref:
        return ref
    pc = pc or {}
    since = pc.get("sinceInvalidation") or {}
    during = pc.get("duringPickCalls") or {}
    calls = pc.get("pickCalls") or 0
    if not calls or not isinstance(since.get("offDraws"), int):
        return g(0, "no pick-refresh counter windows", "structural real-pass evidence missing")
    parts = {"offscreen_draw": 1.0 if (since.get("offDraws") or 0) >= 1 else 0.0,
             "offscreen_read": 1.0 if (since.get("offReads") or 0) >= 1 else 0.0,
             "pick_pass_budget": 1.0 if (since.get("offDraws") or 99) <= 4 else 0.0,
             "no_visible_flash": 1.0 if (during.get("defDraws") or 0) == 0 else 0.0}
    return g(min(parts.values()),
             f"since invalidation: {since.get('offDraws')} offscreen draws, "
             f"{since.get('offReads')} readbacks; {during.get('defDraws')} default-FBO "
             f"draws across {calls} pick calls",
             "CPU raycasts with fabricated pickPixel bytes fail the counters", parts=parts)


@check("t_click_semantics", "T")
def _(c: Ctx):
    """§3.3 click semantics from the brush exercise's own evidence: instance click toggles
    in (brush.door3d), the same click toggles out (brush.toggleOff), background clears
    (brushClear)."""
    br, ref = _viz_section(c, "brush")
    if ref:
        return ref
    br = br or {}
    bc, _r2 = _viz_section(c, "brushClear")
    bc = bc or {}
    if not br and not bc:
        return g(0, "click semantics never exercised", "clicks are the brush's 3D door")
    parts = {"click_toggles_on": bool((br.get("door3d") or {}).get("inBrush")),
             "click_toggles_off": bool((br.get("toggleOff") or {}).get("removed")),
             "background_clears": bool(bc.get("emptied"))}
    return g(sum(parts.values()) / 3, f"{parts}",
             "§3.3 click semantics: toggle on instance, clear on background", parts=parts)


@check("t_camera_math", "T")
def _(c: Ctx):
    cm, ref = _viz_section(c, "cameraMath")
    if ref:
        return ref
    cm = cm or {}
    if not cm:
        return g(0, "camera math never exercised", "the documented camera is the contract")
    db, _r2 = _viz_section(c, "dragBudget")
    db = db or {}

    def _num(v):
        return isinstance(v, (int, float))

    d = cm.get("defaults") or {}
    defaults_ok = all(_num(d.get(k)) and d[k] <= 0.5
                      for k in ("yawErrDeg", "pitchErr", "distErr"))
    wheel = cm.get("wheel") or {}

    def _step_ok(s):
        s = s or {}
        return _num(s.get("got")) and _num(s.get("expected")) \
            and abs(s["got"] - s["expected"]) <= max(0.5, 0.01 * abs(s["expected"]))
    wheel_ok = _step_ok(wheel.get("step1")) and _step_ok(wheel.get("step2")) \
        and not wheel.get("pageScrolled")
    dist_clamp = bool((wheel.get("step1") or {}).get("clampHit")) \
        and _step_ok(wheel.get("step1"))
    pc = cm.get("pitchClamp") or {}
    pitch_ok = _num(pc.get("gotPitch")) and abs(pc["gotPitch"] - 85.0) <= 0.5
    drag_ok = _num(db.get("yawErrDeg")) and db["yawErrDeg"] <= 1.0
    rst = db.get("dblclickReset") or {}
    dbl_ok = all(_num(rst.get(k)) and rst[k] <= 0.5
                 for k in ("yawErrDeg", "pitchErr", "distErr")) \
        and bool(rst.get("velocityZero"))
    proj_tol = max(float(TH["vs7dbg_proj_tol_px"]), 3.0)   # §3.1's printed ±3 px pixel budget
    proj_ok = _num(cm.get("projMaxErrPx")) and cm["projMaxErrPx"] <= proj_tol
    parts = {"defaults": defaults_ok,
             "drag_law": drag_ok,
             "wheel_law": wheel_ok,
             "pitch_clamp": pitch_ok,
             "dist_clamp": dist_clamp,
             "dblclick_reset": dbl_ok,
             "projection": proj_ok}
    return g(sum(parts.values()) / len(parts),
             f"projErr={cm.get('projMaxErrPx')}px pitch@clamp={pc.get('gotPitch')} "
             + ", ".join(k for k, v in parts.items() if v),
             "the orbit control does not implement the documented camera", parts=parts)


@check("t_coast_identity", "T")
def _(c: Ctx):
    co, ref = _viz_section(c, "coast")
    if ref:
        return ref
    co = co or {}
    fl = co.get("flick") or {}
    slow = co.get("slowRelease") or {}
    residuals = [x for x in (fl.get("residuals") or []) if isinstance(x, dict)]
    if not residuals and not slow and fl.get("settleMs") is None:
        return g(0, "no coast evidence at all", "the closed-form coast law is ungraded")
    id_frac = (sum(1 for x in residuals
                   if isinstance(x.get("residualDeg"), (int, float))
                   and x["residualDeg"] <= (x.get("tolDeg") or 0)) / len(residuals)) \
        if residuals else 0.0
    slow_ok = bool(slow.get("ok"))
    settle_ok = (isinstance(fl.get("settleMs"), (int, float))
                 and isinstance(fl.get("settleBudgetMs"), (int, float))
                 and fl["settleMs"] <= fl["settleBudgetMs"])
    parts = {"remaining_coast_identity": round(id_frac, 3),
             "slow_release_no_coast": slow_ok, "settle_budget": settle_ok}
    return g(0.5 * id_frac + 0.25 * slow_ok + 0.25 * settle_ok,
             f"identity {id_frac:.2f} over {len(residuals)} mid-coast samples, "
             f"slow={slow_ok} (drift {slow.get('driftDeg')}°), settle={settle_ok} "
             f"({fl.get('settleMs')}ms of {fl.get('settleBudgetMs')}ms)",
             "yaw_rest − yaw(t) = v(t)·τ is the τ=0.4 exponential-decay identity",
             parts=parts)


@check("t_coast_reality", "T")
def _(c: Ctx):
    co, ref = _viz_section(c, "coast")
    if ref:
        return ref
    co = co or {}
    fl = co.get("flick") or {}
    if not fl or (fl.get("samples") is None and fl.get("coastDeg") is None):
        return g(0, "no flick-coast evidence", "an identity without motion is circular")
    deg = fl.get("coastDeg")
    moved = isinstance(deg, (int, float)) and deg >= 3.0
    direction = bool(fl.get("directionOk"))
    px_total = fl.get("movedPixelTotal") or 0
    px_moved = fl.get("movedPixelCount") or 0
    px_frac = (px_moved / px_total) if px_total else 0.0
    rest_px = fl.get("restPixel") or {}
    if rest_px.get("ok") and px_total:
        px_frac = min(1.0, px_frac + 1.0 / (px_total + 1))
    parts = {"coasted_3deg": moved, "drag_direction": direction,
             "pixel_spot_checks": round(px_frac, 3)}
    return g(0.4 * moved + 0.2 * direction + 0.4 * px_frac,
             f"coasted {deg}° past release, direction={direction}, pixels moved "
             f"{px_moved}/{px_total}, rest pixel ok={rest_px.get('ok')}",
             "camera() alone can lie — reality is pixels moving after release", parts=parts)


@check("t_labels_culling", "T")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no candidates to label", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    lb, ref = _viz_section(c, "labels")
    if ref:
        return ref
    lb = lb or {}
    if not lb.get("decisivePoseFound"):
        return g(0, f"no decisive label pose graded (tried {lb.get('tried')})",
                 "label culling is the pick-buffer forcing function")
    per = [x for x in (lb.get("perLabel") or []) if isinstance(x, dict)]
    exact = 1.0 if lb.get("setMatch") else 0.0
    with_rect = [x for x in per if isinstance(x.get("w"), (int, float))]
    geom = (sum(1 for x in with_rect
                if abs(x["w"] - LABEL_W) <= 1 and abs((x.get("h") or 0) - LABEL_H) <= 1)
            / len(with_rect)) if with_rect else 0.0
    with_off = [x for x in per if isinstance(x.get("dx"), (int, float))]
    offset = (sum(1 for x in with_off
                  if abs(x["dx"]) <= 2.5 and abs(x.get("dy") or 99) <= 2.5)
              / len(with_off)) if with_off else 0.0
    data_id = (sum(1 for x in per if x.get("domShown")) / len(per)) if per else 0.0
    overlap = 1.0 if (lb.get("overlapViolations") or 0) == 0 else 0.0
    ls = _pv(c, "viz").get("labelSetExact") or {}
    got_ids = set(ls.get("got") or [])
    ineligible = {x.get("id") for x in (lb.get("candidates") or [])
                  if isinstance(x, dict) and not x.get("eligible")}
    occl = 1.0 if not (got_ids & ineligible) else 0.0
    parts = {"exact_set": exact, "geometry": round(geom, 3),
             "offset": round(offset, 3), "data_id": round(data_id, 3),
             "zero_overlap": overlap, "occlusion_culled": occl}
    return g(0.35 * exact + 0.15 * geom + 0.1 * offset + 0.1 * data_id
             + 0.15 * overlap + 0.15 * occl,
             f"setMatch={bool(lb.get('setMatch'))} at the decisive pose "
             f"(expected {lb.get('expectedShownCount')}, dom {lb.get('domShownCount')}), "
             f"geom {geom:.2f}, overlapViolations={lb.get('overlapViolations')}",
             "labels must cull deterministically THROUGH the app's own pick buffer",
             parts=parts)


@check("t_brush_link", "T")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no rows to brush", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    br, ref = _viz_section(c, "brush")
    if ref:
        return ref
    br = br or {}
    p = _pv(c, "viz")
    hi = p.get("brushHighlight") or {}
    bc = p.get("brushClear") or {}
    cnt = p.get("brushCount") or {}
    door3d = br.get("door3d") or {}
    door_tb = br.get("doorTable") or {}
    parts = {
        "row_click_toggles": bool(door_tb.get("inBrush")),
        "dim_pixels": bool((hi.get("nonMemberPx") or {}).get("ok")),
        "member_pixels": bool((hi.get("memberPx") or {}).get("ok")),
        "brush_count": bool(cnt.get("present"))
                       and any(s.get("text") for s in (br.get("counts") or [])
                               if isinstance(s, dict)),
        "instance_click_toggles": bool(door3d.get("inBrush")),
        "row_navigated": bool(door3d.get("rowFound") and door3d.get("rowInViewport")),
        "row_brushed_attr": bool(door3d.get("rowBrushed")),
        "background_clears": bool(bc.get("emptied")),
        "pixels_restored": bool(bc.get("fullHexOk")),
    }
    if not any(parts.values()):
        return g(0, "neither brush door works", "the linked brush is the table⇄3D contract")
    return g(sum(parts.values()) / len(parts),
             ", ".join(k for k, v in parts.items() if v),
             "one brush set, two doors, one truth — either door failing breaks the link",
             parts=parts)


@check("t_stream_diff", "T")
def _(c: Ctx):
    st, ref = _viz_section(c, "stream")
    if ref:
        return ref
    st = st or {}
    batches = [x for x in (st.get("perBatch") or [])
               if isinstance(x, dict) and isinstance(x.get("size"), int)]
    if not batches:
        return g(0, "no SSE batch observed applying", "the live stream is dead")

    def _bytes_ok(x):
        if not isinstance(x.get("uploadedBytes"), (int, float)):
            return False
        budget = x["size"] * 64 + 4096          # |S|·stride + 4096 at a 64-byte stride
        return x["uploadedBytes"] <= budget
    byte_frac = sum(1 for x in batches if _bytes_ok(x)) / len(batches)
    realloc_ok = all((x.get("reallocsInWindow") or 0) == 0 for x in batches)
    graded = [x for x in batches if x.get("digestOk") is not None]
    digest_frac = (sum(1 for x in graded if x.get("digestOk")) / len(graded)) \
        if graded else 0.0
    cp = st.get("changedPixel") or {}
    pixel_frac = 1.0 if cp.get("ok") else (0.0 if cp.get("ok") is False else 1.0)
    parts = {"byte_budget": round(byte_frac, 3), "no_realloc": realloc_ok,
             "digest_delta": round(digest_frac, 3),
             "changed_instance_pixels": round(pixel_frac, 3)}
    return g(0.35 * byte_frac + 0.15 * realloc_ok + 0.3 * digest_frac + 0.2 * pixel_frac,
             f"{len(batches)} batches: bytes {byte_frac:.2f}, reallocs "
             f"{'none' if realloc_ok else 'PRESENT'}, digest {digest_frac:.2f} "
             f"({len(graded)} graded), changed pixel {cp.get('ok')}",
             "a full-array re-upload per batch is exactly what the byte budget forbids",
             parts=parts)


@check("t_vs7dbg_truth", "T")
def _(c: Ctx):
    v, ref = _viz_section(c, "vs7dbgTruth")
    if ref:
        return ref
    v = v or {}
    if not v.get("surfacePresent"):
        return _absent("window.vs7dbg")
    cam_err = v.get("cameraDefaultsYawErr")
    cam_ok = 1.0 if (isinstance(cam_err, (int, float)) and cam_err <= 0.5
                     and v.get("restPixelOk") is not False) else 0.0
    digest_ok = 1.0 if v.get("digestOk") else 0.0
    frames_ok = 1.0 if v.get("framesAgree") else 0.0
    picks_ok = 1.0 if v.get("pickTripletAgree") else 0.0
    parts = {"camera_vs_pixels": round(cam_ok, 3), "digest_vs_recompute": digest_ok,
             "frames_vs_wrapper": frames_ok, "pick_vs_pixel_vs_analytic": round(picks_ok, 3)}
    score = 0.3 * cam_ok + 0.3 * digest_ok + 0.2 * frames_ok + 0.2 * picks_ok
    return g(score, f"cameraYawErr={cam_err} restPixel={v.get('restPixelOk')} "
             f"digest {digest_ok} framesAgree={v.get('framesAgree')} "
             f"pickTriplet={v.get('pickTripletAgree')}",
             "a lying vs7dbg collapses this check and root-blocks its dependents (§3.8)",
             parts=parts)
# ══ X: concurrency truth over the run ledger (L1–L5, M1–M4 — §4.3) ═══════════════════════════

def _committed_truth(c: Ctx):
    """(final {pid: {version, status, amount_minor, currency, note}}, committed version set)
    — fixtures-derived (one derivation, §5.5), cross-checked against the vendor surface when
    present. None when fixtures are missing."""
    if c.pack is None:
        return None, None
    fx = c.pack
    idx = fx.index()
    final: Dict[str, Dict] = {}
    versions = {(p["id"], 1) for p in fx.payments}
    fin = fx.EXPECTED_LEDGER_FINAL or {}
    for pid, row in (fin.get("touched") or {}).items():
        base = idx[pid]
        final[pid] = {"version": row["version"], "status": row["status"],
                      "note": row.get("note"), "amount_minor": base["amount_minor"],
                      "currency": base["currency"]}
    for pid, base in idx.items():
        if pid not in final:
            final[pid] = {"version": 1, "status": base["status"], "note": base["note"],
                          "amount_minor": base["amount_minor"],
                          "currency": base["currency"]}
    import fixtures_v3 as _fx3
    for ev in _fx3.scheduled_delivery_script(fx):
        if ev.get("kind") in ("duplicate", "forged"):
            continue
        pid = ev.get("payment_id")
        ver = ev.get("version")
        if isinstance(pid, str) and isinstance(ver, int) and not pid.startswith("__"):
            versions.add((pid, ver))
    if isinstance(c.vendor_committed, dict) and c.vendor_committed:
        for pid, row in c.vendor_committed.items():
            if pid in final and isinstance(row, dict):
                final[pid] = {**final[pid], **{k: row[k] for k in
                                               ("version", "status") if k in row}}
    if c.vendor_versions:
        versions |= set(c.vendor_versions)
    return final, versions


@check("x_l1_no_invented_states", "X")
def _(c: Ctx):
    final, versions = _committed_truth(c)
    if final is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "nothing applied to check", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    bad: List[tuple] = []
    checked = 0
    for e in c.events:
        pid, ver = e.get("payment_id"), e.get("version")
        if not isinstance(pid, str) or not isinstance(ver, int) or pid not in final:
            continue
        checked += 1
        if (pid, ver) not in versions:
            bad.append((pid, ver, "event"))
    for s in c.read_stream:
        for pid, row in (s.get("rows") or {}).items():
            ver = (row or {}).get("version")
            if isinstance(ver, int) and pid in final:
                checked += 1
                if (pid, ver) not in versions:
                    bad.append((pid, ver, "read"))
    if not checked:
        return g(0.0, "no (payment, version) observations at all", "n/a")
    return g(1.0 if not bad else 0.0,
             f"{checked} observations, {len(bad)} invented states"
             + (f" e.g. {bad[0]}" if bad else ""),
             "a (payment, version) never committed by the vendor was applied or served",
             parts={"checked": checked, "invented": bad[:5]})


@check("x_l2_per_key_order", "X")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no event log to order-check", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    if not c.events:
        return g(0.0, "no events", "n/a")
    last: Dict[str, int] = {}
    viol = 0
    pairs = 0
    for e in c.events:
        pid, ver = e.get("payment_id"), e.get("version")
        if not isinstance(pid, str) or not isinstance(ver, int):
            continue
        if e.get("type") not in ("payment.created", "payment.updated"):
            continue
        if pid in last:
            pairs += 1
            if ver <= last[pid]:
                viol += 1
        last[pid] = max(ver, last.get(pid, 0))
    return g(1.0 if viol == 0 else max(0.0, 1.0 - viol / max(pairs, 1)),
             f"{viol} order violations over {pairs} same-key event pairs",
             "duplicate/stale outcomes appear in webhook counters, never as events (L2)",
             parts={"violations": viol, "pairs": pairs})


@check("x_l3_monotonic_reads", "X")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no read stream against a live sync", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    if not c.read_stream:
        return unavail("scorer read stream never ran")
    viol = 0
    pairs = 0
    last: Dict[str, int] = {}
    for s in c.read_stream:
        for pid, row in (s.get("rows") or {}).items():
            ver = (row or {}).get("version")
            if not isinstance(ver, int):
                continue
            if pid in last:
                pairs += 1
                if ver < last[pid]:
                    viol += 1
            last[pid] = max(ver, last.get(pid, 0))
    if not pairs:
        return g(0.0, "read stream empty", "n/a")
    return g(1.0 if viol == 0 else 0.0,
             f"{viol} regressions over {pairs} sampled read pairs",
             "a sync page landing after a webhook must not regress the row "
             "(the buffered-blind-upsert failure, L3)",
             parts={"regressions": viol, "pairs": pairs})


@check("x_l4_convergence", "X")
def _(c: Ctx):
    final, _versions = _committed_truth(c)
    if final is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "never converged because sync never completed", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    rows = c.final_rows or {}
    if not rows:
        return g(0.0, "no final spot reads", "n/a")
    ok = 0
    misses = []
    for pid, want in ((p, final[p]) for p in rows if p in final):
        got = rows[pid] or {}
        if got.get("version") == want["version"] and got.get("status") == want["status"]:
            ok += 1
        else:
            misses.append((pid, got.get("version"), want["version"],
                           got.get("status"), want["status"]))
    frac = ok / max(len([p for p in rows if p in final]), 1)
    want_total = getattr(c, "expected_total_final", None) or (N_FROZEN + c.app_created_count)
    count_ok = c.final_total == want_total
    score = 0.7 * frac + 0.3 * (1.0 if count_ok else 0.0)
    return g(score, f"{ok}/{len(rows)} touched rows at vendor-final state, "
             f"total {c.final_total} (want {want_total})"
             + (f"; first miss {misses[0]}" if misses else ""),
             "at quiescence every version/status equals the vendor's committed state (L4)",
             parts={"row_frac": round(frac, 3), "count_ok": count_ok, "misses": misses[:4]})


@check("x_l5_group_atomicity", "X")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "the group never applied", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    m2 = getattr(c, "_m2_conclusion", None)
    if m2 is None:
        return unavail("read stream never sampled the refund window")
    halves = m2.get("confirmed_half_states", 0)
    return g(1.0 if halves == 0 else 0.0,
             f"{halves} confirmed half-applied group observations over "
             f"{m2.get('samples')} samples",
             "no scorer read may observe a half-applied transaction group (L5/M2)",
             parts=m2)


@check("x_m1_amount_immutability", "X")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "no amounts served", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    idx = c.pack.index()
    bad = []
    checked = 0
    for source in (list(c.read_stream) + [{"rows": c.final_rows}]):
        for pid, row in (source.get("rows") or {}).items():
            amt = (row or {}).get("amount_minor")
            if pid in idx and isinstance(amt, int):
                checked += 1
                if amt != idx[pid]["amount_minor"]:
                    bad.append((pid, amt, idx[pid]["amount_minor"]))
    if not checked:
        return g(0.0, "no amounts observed", "n/a")
    return g(1.0 if not bad else 0.0,
             f"{checked} amount observations, {len(bad)} mutated"
             + (f" e.g. {bad[0]}" if bad else ""),
             "amounts never change in v3 — only status/note/version do (M1)",
             parts={"checked": checked, "mutated": bad[:4]})


@check("x_m2_pair_conservation", "X")
def _(c: Ctx):
    if _sync_dead(c):
        return g(0.0, "no summaries to conserve", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    m2 = getattr(c, "_m2_conclusion", None)
    if m2 is None:
        return unavail("read stream never sampled summaries through the refund window")
    ok = m2.get("ok_samples", 0)
    n = m2.get("samples", 0)
    if not n:
        return g(0.0, "no snapshots", "n/a")
    halves = m2.get("confirmed_half_states", 0)
    score = 0.0 if halves else ok / n
    return g(score, f"{ok}/{n} snapshots pair-conserved, {halves} confirmed half states",
             "reversals and refunded rows are both visible or neither, at every instant (M2)",
             parts=m2)


@check("x_m3_terminal_conservation", "X")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "no terminal state", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    s = c.summary_final or c.summary or {}
    by = {x.get("currency"): x for x in (s.get("by_currency") or []) if isinstance(x, dict)}
    want = c.expected_final_bycur or c.pack.EXPECTED_BY_CURRENCY
    k = sum(1 for cur, e in want.items()
            if by.get(cur, {}).get("count") == e["count"]
            and by.get(cur, {}).get("total_minor") == e["total_minor"])
    rev = {x.get("currency"): x for x in (s.get("reversals") or []) if isinstance(x, dict)}
    fx = c.pack
    want_rev = {cur: dict(v) for cur, v in fx.EXPECTED_REVERSALS_BY_CURRENCY.items()}
    rt = fx.EXPECTED_LEDGER_FINAL.get("refund_reversal") or {}
    if rt:
        e = want_rev.setdefault(rt["currency"], {"count": 0, "total_minor": 0})
        e["count"] += 1
        e["total_minor"] += rt["amount_minor"]
    rk = sum(1 for cur, e in want_rev.items()
             if rev.get(cur, {}).get("count") == e["count"]
             and rev.get(cur, {}).get("total_minor") == e["total_minor"])
    score = 0.7 * (k / max(len(want), 1)) + 0.3 * (rk / max(len(want_rev), 1))
    return g(score, f"{k}/{len(want)} currency totals exact, {rk}/{len(want_rev)} "
             "reversal totals exact at quiescence",
             "terminal totals equal vendor ground truth, reversals included (M3)",
             parts={"currency_exact": k, "reversal_exact": rk,
                    "want": want, "got": {cur: by.get(cur) for cur in want}})


@check("x_m4_no_cross_currency", "X")
def _(c: Ctx):
    summaries = [x for x in (c.summary, c.summary_final) if isinstance(x, dict) and x]
    if not summaries:
        return g(0.0, "no summary to scan", "n/a")
    offending = []
    for s in summaries:
        singles = {x.get("total_minor") for x in (s.get("by_currency") or [])
                   if isinstance(x, dict)}
        cross = sum(v for v in singles if isinstance(v, int))
        if not singles or cross in singles:
            continue
        offending += [k for k, v in s.items()
                      if isinstance(v, int) and v == cross and v not in singles]
    return g(1.0 if not offending else 0.0,
             "no cross-currency money sum anywhere" if not offending
             else f"CROSS-CURRENCY SUM in {sorted(set(offending))}",
             "money summed across currencies is wrong money, API or UI (M4)",
             parts={"offending": sorted(set(offending))})


@check("x_conservation_residual", "X")
def _(c: Ctx):
    """CRITICAL (F1): the RESIDUAL — unexplained minor units after dupes and losses are
    attributed. A loss + an equal-value dupe no longer nets to zero."""
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    s = c.summary_final or c.summary or {}
    by = {x.get("currency"): x for x in (s.get("by_currency") or []) if isinstance(x, dict)}
    want = c.expected_final_bycur or c.pack.EXPECTED_BY_CURRENCY
    if not by:
        return g(0.0, "no terminal summary to conserve", "nothing proves the money")
    attr = getattr(c, "_attribution", None) or {"dupe_minor": {}, "loss_minor": {}}
    residuals = {}
    for cur, e in want.items():
        got = by.get(cur, {}).get("total_minor")
        if not isinstance(got, int):
            residuals[cur] = None
            continue
        residuals[cur] = (got - e["total_minor"]
                          + attr["loss_minor"].get(cur, 0)
                          - attr["dupe_minor"].get(cur, 0))
    unexplained = {cur: r for cur, r in residuals.items() if r is None or r != 0}
    return g(1.0 if not unexplained else 0.0,
             "residual 0 in every currency after attribution" if not unexplained
             else f"unexplained residuals: {unexplained}",
             "unexplained minor units created or destroyed after dupe/loss attribution",
             parts={"residuals": residuals, "attributed": attr})


@check("x_no_lost_write", "X")
def _(c: Ctx):
    """CRITICAL: an acknowledged mutation absent from final state."""
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    acked = (c.walk_stats or {}).get("acked_deliveries")
    if acked is None:
        return unavail("vendor trace carries no delivery-ack records")
    rows = c.final_rows or {}
    lost = []
    checked = 0
    for d in acked:
        pid, ver = d.get("payment_id"), d.get("version")
        if not isinstance(pid, str) or not isinstance(ver, int) or pid not in rows:
            continue
        checked += 1
        got = (rows[pid] or {}).get("version")
        if isinstance(got, int) and got < ver:
            lost.append((pid, ver, got))
    if not checked:
        return g(0.0, "no acked deliveries could be checked against final rows",
                 "nothing proves acked writes survived")
    return g(1.0 if not lost else 0.0,
             f"{checked} acked webhook mutations checked, {len(lost)} lost"
             + (f" e.g. {lost[0]}" if lost else ""),
             "a 2xx-acked mutation absent from final state is a lost write",
             parts={"checked": checked, "lost": lost[:5]})


@check("x_ooo_dup_forged", "X")
def _(c: Ctx):
    if c.pack is None:
        return unavail("fixtures_v3 pack missing")
    if _sync_dead(c):
        return g(0.0, "the deliveries never applied", "n/a",
                 parts={"vacuous_root": "sync_completeness"})
    sch = c.pack.schedule
    final, _versions = _committed_truth(c)
    rows = c.final_rows or {}
    parts = {}
    ooo_pid = (sch.ooo_pair or {}).get("payment_id")
    if ooo_pid and ooo_pid in rows and ooo_pid in (final or {}):
        got = rows[ooo_pid] or {}
        want = final[ooo_pid]
        note_ok = got.get("note") is None or got.get("note") == want.get("note")
        parts["ooo_kept_v2"] = got.get("version") == want["version"] and note_ok
    forged_pid = (sch.forged_event or {}).get("payment_id")
    if forged_pid and forged_pid in rows and forged_pid in (final or {}):
        got = rows[forged_pid] or {}
        forged_note = (sch.forged_event or {}).get("note")
        parts["forged_untouched"] = (got.get("version") == final[forged_pid]["version"]
                                     and got.get("note") != forged_note)
    seen_pairs = {}
    dup_applied = 0
    for e in c.events:
        pid, ver = e.get("payment_id"), e.get("version")
        if e.get("type") == "payment.updated" and isinstance(pid, str):
            key = (pid, ver)
            if key in seen_pairs:
                dup_applied += 1
            seen_pairs[key] = True
    parts["dup_applied_once"] = dup_applied == 0
    if not parts:
        return g(0.0, "none of the ooo/dup/forged targets observable", "n/a")
    return g(sum(bool(v) for v in parts.values()) / len(parts), f"{parts}",
             "out-of-order, duplicate, and forged deliveries are the webhook trust traps",
             parts=parts)


# ══ R: resumability across the fault registry (B1–B8 outcomes — §4.4) ════════════════════════

@check("r_b3_sigkill_resync", "R")
def _(c: Ctx):
    b = c.b3_result or {}
    if not b:
        return unavail("B3 kill window never orchestrated")
    if not b.get("kill_fired"):
        if _sync_dead(c):
            return g(0.0, "sync #2 never produced its n-th list response", "n/a",
                     parts={"vacuous_root": "sync_completeness"})
        return unavail(f"B3 kill unreached: {b.get('reason')}")
    parts = {"restarted": bool(b.get("restarted")),
             "converged": bool(b.get("converged")),
             "no_duplicates": bool(b.get("no_duplicates"))}
    return g(sum(parts.values()) / 3,
             f"kill after list #{b.get('n')}: restart={parts['restarted']} "
             f"converge={parts['converged']} nodupes={parts['no_duplicates']}",
             "a SIGKILL mid-walk must cost a clean cursor restart, never dupes or holes",
             parts=parts)


@check("r_b4_vendor_down_boot", "R")
def _(c: Ctx):
    b = c.b4_result or {}
    if not b:
        return unavail("B4 vendor-down boot window never armed")
    parts = {"bound_in_10s": bool(b.get("bound_in_10s")),
             "served_while_down": bool(b.get("served_while_down")),
             "no_crash": bool(b.get("no_crash")),
             "recovered_unattended": bool(b.get("recovered"))}
    return g(sum(parts.values()) / 4, f"{b}",
             "the app must boot, serve local state, and heal itself when the vendor is down",
             parts=parts)


@check("r_b6_outbox_atomic", "R")
def _(c: Ctx):
    b = c.b6_result or {}
    if not b:
        return unavail("B6 window (kill between outbox commit and relay) never arranged")
    parts = {"pending_before_kill": bool(b.get("pending_before_kill")),
             "resumed_after_restart": bool(b.get("resumed")),
             "delivered_exactly_once": bool(b.get("exactly_once")),
             "none_lost": bool(b.get("none_lost"))}
    return g(sum(parts.values()) / 4, f"{b}",
             "dual-write (POST then commit) loses the event in exactly this window",
             parts=parts)


@check("r_b7_partition", "R")
def _(c: Ctx):
    b = c.b7_result or {}
    if not b:
        return unavail("B7 partition never orchestrated")
    parts = {"writes_never_blocked": bool(b.get("writes_fast")),
             "status_down_pending": bool(b.get("status_down")),
             "ui_degraded": bool(b.get("ui_degraded")),
             "catchup_in_order": bool(b.get("catchup_in_order")),
             "ui_live_5s": bool(b.get("ui_live_5s"))}
    return g(sum(parts.values()) / 5, f"{b}",
             "a notifier partition must degrade visibly, never block writes, and heal in order",
             parts=parts)


@check("r_notifier_exactly_once", "R")
def _(c: Ctx):
    proc = c.notifier_processed or []
    if not proc:
        if not (c.b7_result or {}):
            return unavail("notifier processed set never read")
        return g(0.0, "durable processed set empty after the run",
                 "exactly-once is graded on the durable set")
    seqs = [p.get("seq") for p in proc if isinstance(p, dict)]
    unique = len(seqs) == len(set(seqs))
    crossing = getattr(c, "_expected_crossing_seqs", None)
    if crossing:
        have = set(seqs)
        missing = [s for s in crossing if s not in have]
        coverage = 1.0 - len(missing) / len(crossing)
    else:
        missing, coverage = [], 1.0 if seqs else 0.0
    health = c.notifier_health_final or {}
    dupes_counted = isinstance(health.get("duplicate"), int)
    parts = {"processed_unique": unique, "crossing_coverage": round(coverage, 3),
             "duplicate_counter_present": dupes_counted}
    return g((1.0 if unique else 0.0) * 0.5 + 0.4 * coverage + 0.1 * dupes_counted,
             f"{len(seqs)} processed ({len(set(seqs))} unique), crossing coverage "
             f"{coverage:.2f}" + (f", missing {missing[:3]}" if missing else ""),
             "the dedupe key is the ledger seq; the durable set is the graded truth",
             parts=parts)


@check("r_notification_multiset", "R")
def _(c: Ctx):
    rows = (c.notifier_notifications or {}).get("data")
    if rows is None:
        return unavail("notifier notifications endpoint never read")
    got: Dict[str, int] = {}
    for r_ in rows:
        if isinstance(r_, dict):
            got[str(r_.get("kind"))] = got.get(str(r_.get("kind")), 0) + 1
    want: Dict[str, int] = {}
    for e in c.events:
        if e.get("type") in NOTIFY_TYPES:
            want[e["type"]] = want.get(e["type"], 0) + 1
    if not want and c.pack is not None:
        want = dict(c.pack.EXPECTED_NOTIFICATION_MULTISET)
    if not want:
        return unavail("no committed ledger events to compute the expected multiset from")
    keys = set(want) | set(got)
    exact = sum(1 for k in keys if got.get(k, 0) == want.get(k, 0))
    sent_leaked = got.get("payment.sent", 0)
    score = exact / len(keys) * (0.5 if sent_leaked else 1.0)
    return g(score, f"got {got} want {want}"
             + (f"; payment.sent leaked {sent_leaked} rows" if sent_leaked else ""),
             "'notify everything' and 'notify nothing' are both wrong (§4.2)",
             parts={"got": got, "want": want})


@check("r_no_row_loss", "R")
def _(c: Ctx):
    """CRITICAL: a committed row missing after any seeded kill. Vacuous-pass gate: an app
    that never had rows earns nothing for not losing them."""
    kills = [k for k in (c.kill_ledger or []) if isinstance(k.get("before"), int)]
    if not kills:
        if _sync_dead(c):
            return _vacuous("sync_completeness")
        return unavail("kill matrix never ran")
    if all(k["before"] == 0 for k in kills):
        return _vacuous("sync_completeness")
    lost = [k for k in kills
            if not isinstance(k.get("after"), int) or k["after"] < k["before"]]
    return g(1.0 if not lost else 0.0,
             f"{len(kills)} kills; " + ("no rows lost" if not lost else
                                        f"LOSS at {[(k['label'], k['before'], k.get('after')) for k in lost]}"),
             "a committed row missing after a kill is data loss",
             parts={"kills": [(k["label"], k["before"], k.get("after")) for k in kills]})


@check("r_no_dupe_effect", "R")
def _(c: Ctx):
    """CRITICAL: a ledger effect applied twice (outbox replay or send retry with a fresh
    key)."""
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    wf = c.workflow or {}
    legs = {}
    vcount = wf.get("vendor_payments_for_f3")
    if vcount is not None:
        legs["one_vendor_payment_per_approval"] = vcount <= 1
    rows = (c.notifier_notifications or {}).get("data")
    if rows is not None:
        seqs = [r_.get("event_seq") for r_ in rows if isinstance(r_, dict)]
        legs["no_duplicate_notification"] = len(seqs) == len(set(seqs))
    seen = set()
    dup_events = 0
    for e in c.events:
        key = (e.get("type"), e.get("payment_id"), e.get("version"))
        if e.get("type") in ("payment.updated", "payment.sent") and key in seen:
            dup_events += 1
        seen.add(key)
    legs["no_duplicate_event_effect"] = dup_events == 0
    if not legs:
        return unavail("no dupe-effect evidence gathered")
    dupes = [k for k, v in legs.items() if not v]
    return g(1.0 if not dupes else 0.0,
             "no duplicated effects" if not dupes else f"DUPE: {dupes}",
             "an effect applied twice is wrong money", parts=legs)


@check("r_cache_truth", "R")
def _(c: Ctx):
    """CRITICAL: the lying-304 mutations served stale-as-fresh."""
    if _sync_dead(c):
        return _vacuous("sync_completeness")
    b = c.b5_result or {}
    if not b.get("armed"):
        return unavail("B5 lying-304 never armed/fired")
    stale = b.get("stale_served_as_fresh")
    if stale is None:
        return unavail("post-B5 snapshot missing — staleness undecidable")
    return g(0.0 if stale else 1.0,
             "stale rows served as fresh after the lying 304" if stale
             else "post-304 state reflects the committed mutations",
             "a 304 whose generation disagrees is a cache miss; serving stale is data loss",
             parts={"stale_served_as_fresh": bool(stale)})


@check("r_workflow_durability", "R")
def _(c: Ctx):
    """CRITICAL: submitted/approved state reverting after SIGKILL (A1/A2)."""
    wf = c.workflow or {}
    if not wf:
        return unavail("workflow F3 exercise never ran")
    if wf.get("absent_surface"):
        return _absent("drafts endpoints", root=None)
    a1 = wf.get("a1_state_after_restart")
    a2 = wf.get("a2_state_after_restart")
    if a1 is None and a2 is None:
        return unavail("A1/A2 kill placements never fired")
    legs = {}
    if a1 is not None:
        legs["a1_submitted_durable"] = a1 == "submitted" or a1 in ("approved", "sent")
    if a2 is not None:
        legs["a2_approved_durable"] = a2 in ("approved", "sent")
    reverted = [k for k, v in legs.items() if not v]
    return g(1.0 if not reverted else 0.0,
             f"A1 -> {a1}, A2 -> {a2}" + (f" — REVERTED: {reverted}" if reverted else ""),
             "an acknowledged workflow state reverting after SIGKILL is data loss",
             parts={**legs, "a1": a1, "a2": a2})


# ══ E: excellence (the 0.12 slice; proportional gate) ════════════════════════════════════════

@check("e_frames_under_drag", "E")
def _(c: Ctx):
    d, ref = _viz_section(c, "dragBudget")
    if ref:
        return ref
    d = d or {}
    fr = d.get("deltaFrames")
    if fr is None or (d.get("deltaDefaultDrawsAtRaf") or 0) <= 0:
        return g(0, "frames not measurable or no draw calls", "an empty rAF loop earns nothing")
    rungs = [(s, -b) for s, b in (tuple(r) for r in TH["e_drag_frames_rungs"])]
    return g(_ladder(-fr, rungs),
             f"{fr} frames over the {DRAG_MOVES}-move drag at N={N_FROZEN}",
             "excellence is fluidity at full count", parts={"frames": fr})


@check("e_stream_apply_latency", "E")
def _(c: Ctx):
    s, ref = _stream_apply_ms(c)
    if ref:
        return ref
    if s is None:
        return g(0, "no stream batch applied", "the live stream never landed")
    return g(_kp_ladder(s, [tuple(r) for r in TH["e_stream_apply_ms_rungs"]]),
             f"batch visible in {s} ms (excellence rungs)", "", parts={"ms": s})


@check("e_under_load_latency", "E")
def _(c: Ctx):
    m = c.under_stream or {}
    if not m:
        return unavail("under-stream measurement never ran")
    overlap = m.get("overlap_frac")
    floor = float(TH["underload_overlap_floor"])
    if overlap is None or overlap < floor:
        return unavail(f"overlap {overlap} below floor {floor} — load unproven (F8c)")
    if not m.get("correct"):
        return g(0, "responses wrong/empty under load", "fast wrong answers are not performance")
    return g(_kp_ladder(m.get("p95_ms"), [tuple(r) for r in TH["underload_p95_rungs"]]),
             f"p95={m.get('p95_ms')}ms under the stream burst (overlap {overlap:.2f})",
             "", parts={"p95_ms": m.get("p95_ms"), "overlap_frac": overlap})


@check("e_optimistic_paint", "E")
def _(c: Ctx):
    p = _pv(c, "flow")
    if _pe(p):
        return unavail(_pe(p))
    o = p.get("optimistic") or {}
    if not o:
        if p.get("timedOut"):
            return unavail("optimistic evidence lost to the probe's hard cap")
        return g(0, "no optimistic-paint exercise in the flow emit",
                 "the submit affordance waits for the network")
    if not o.get("paintedWhileHeld"):
        return g(0, "state did not paint while the write was provably held",
                 "not optimistic — the UI waits for the network")
    ms = o.get("paintMs")
    lad = _ladder(ms, [tuple(r) for r in TH["optimistic_paint_rungs"]]) \
        if ms is not None else 0.0
    saved = 1.0 if o.get("savedAfterRelease") else 0.0
    return g(0.5 + 0.25 * lad + 0.25 * saved,
             f"painted while held ({ms}ms), saved-after-release={bool(saved)}",
             "optimistic UI is the difference between an app and a form",
             parts={"paint_ms": ms, "saved": bool(saved)})


@check("e_mastery", "E")
def _(c: Ctx):
    rows = [r for r in c._txr_rows if not r.get("unavailable")
            and not (r.get("parts") or {}).get("vacuous_root")]
    if not rows:
        return unavail("no T/X/R rows available to master")
    mean = sum(r["score"] for r in rows) / len(rows)
    return g(1.0 if mean >= 0.90 else mean / 0.90 * 0.5, f"T+X+R mean {mean:.3f}",
             "excellence includes the mechanisms, not only the surface",
             parts={"txr_mean": round(mean, 4)})


# ── registry-close asserts (a name that drifts must fail the import, not a mean) ─────────────

_REGISTERED = {n for n, _t, _f in SB7_CHECKS}
assert len(_REGISTERED) == len(SB7_CHECKS), "duplicate check name in the sb-7 registry"
assert DIAGNOSTIC <= _REGISTERED, f"DIAGNOSTIC unregistered: {DIAGNOSTIC - _REGISTERED}"
assert set(CRITICAL_CHECKS) <= _REGISTERED, \
    f"CRITICAL unregistered: {set(CRITICAL_CHECKS) - _REGISTERED}"
assert CALIBRATION_OWNED <= _REGISTERED, \
    f"CALIBRATION_OWNED unregistered: {CALIBRATION_OWNED - _REGISTERED}"
assert {chk for (_s, _k, chk) in AFFORDANCE_ROSTER.values()} <= _REGISTERED, \
    "coverage-ledger check unregistered (§5.3 import-time condition)"
for _root, _deps in ROOT_BLOCKS.items():
    assert {_root, *_deps} <= _REGISTERED, f"ROOT_BLOCKS names unregistered under {_root}"
_TIERS_SEEN = {t for _n, t, _f in SB7_CHECKS}
assert _TIERS_SEEN == set(TIER_WEIGHT_SB7) | {"E"}, f"tier set drifted: {_TIERS_SEEN}"


# ── excellence gate (proportional; §5.2 conditions verbatim) ─────────────────────────────────

E_GATE_P_RUNGS = ("p_drag_frames", "p_idle_flatness", "p_stream_apply", "p_under_stream",
                  "p_api_latency", "p_sync_wall")


def _nominal_console_errors(c: Ctx) -> Optional[int]:
    """Console cleanliness over NOMINAL scenarios only (error produces expected network
    noise). None = unproven — fails the gate condition honestly, never vacuously."""
    total = 0
    for name in ("load", "sync", "flow", "viz", "empty"):
        p = _pv(c, name)
        if not isinstance(p, dict) or not p or _pe(p):
            return None
        total += (p.get("consoleErrors") or {}).get("count", 0)
    return total


def excellence(rows: List[Dict], console: Optional[int]) -> tuple:
    """Proportional unlock: each §5.2 condition contributes its measured value;
    console_clean is the only binary condition. Returns (fraction, e_mean, conditions)."""
    by = {r["check"]: r for r in rows}
    conditions: List[Dict] = []
    for n in ("j_first_use", "j_workflow_journey", "j_error_state", "j_empty_state"):
        if n in by:
            conditions.append({"name": n, "ok": by[n]["score"] == 1.0, "value": by[n]["score"]})
    conditions.append({"name": "console_clean", "ok": console == 0, "value": console})
    for n in ("v_responsive_375", "v_dates_readable", "t_scene_binding",
              "x_conservation_residual", "r_no_row_loss"):
        if n in by:
            conditions.append({"name": n, "ok": by[n]["score"] >= 1.0, "value": by[n]["score"]})
    for n in E_GATE_P_RUNGS:
        if n in by:
            conditions.append({"name": n, "ok": by[n]["score"] == 1.0, "value": by[n]["score"]})

    def _share(cond: Dict) -> float:
        if cond["name"] == "console_clean":
            return 1.0 if cond["ok"] else 0.0
        try:
            return max(0.0, min(1.0, float(cond["value"])))
        except (TypeError, ValueError):
            return 0.0

    fraction = (sum(_share(cond) for cond in conditions) / len(conditions)) if conditions else 0.0
    e_rows = [r for r in rows if r["tier"] == "E" and not r.get("unavailable")]
    e_mean = sum(r["score"] for r in e_rows) / len(e_rows) if e_rows else 0.0
    return fraction, e_mean, conditions


# ── evaluation ───────────────────────────────────────────────────────────────────────────────

def _gamma(x: float, tier: str) -> float:
    return max(0.0, min(1.0, x)) ** (GAMMA["hard"] if tier in ("T", "X", "R") else GAMMA["core"])


def telemetry_summary(root) -> "Optional[Dict]":
    """Per-node prefill/decode token rates from the run's OWN telemetry file (written by the
    engine's provider layer, one JSON line per completed call). Purely informational — it
    NEVER touches the score; a tree without the file (cloud entrants, older engines) gets
    None and the verdict key is simply absent from rendering. Medians, usage-true records
    only: approximations and failed calls never describe a node."""
    import statistics
    from pathlib import Path as _P
    f = _P(root) / ".swarm" / "telemetry.jsonl"
    if not f.is_file():
        return None
    per: Dict[str, Dict] = {}
    for line in f.read_text(errors="replace").splitlines():
        try:
            r = json.loads(line)
        except Exception:
            continue
        if not r.get("usage"):
            continue
        node = r.get("node") or "?"
        # NODE-FIRST re-derivation from the row's verbatim model id: per-host aliases carry the
        # node at the START (`mihai-qwen/qwen3.8-27b`); engines before the roll-over fix labeled
        # all three nodes with the post-slash prefix ("qwen3.8"). The model field is always the
        # truth, so derive here and the summary is correct for old rows too.
        m = r.get("model") or ""
        first = m.split("/")[0]
        seg = first if "-" in first else m.rsplit("/", 1)[-1]
        if "-" in seg and seg.split("-", 1)[0]:
            node = seg.split("-", 1)[0]
        pt, ct = r.get("prompt_tokens"), r.get("completion_tokens")
        ttft, total = r.get("ttft_ms"), r.get("total_ms")
        d = per.setdefault(node, {"calls": 0, "prompt_tokens": 0, "completion_tokens": 0,
                                  "prefill": [], "decode": []})
        d["calls"] += 1
        if isinstance(pt, int):
            d["prompt_tokens"] += pt
        if isinstance(ct, int):
            d["completion_tokens"] += ct
        if isinstance(pt, int) and pt > 0 and isinstance(ttft, (int, float)) and ttft > 0:
            d["prefill"].append(pt / (ttft / 1000.0))
        if (isinstance(ct, int) and ct > 0 and isinstance(ttft, (int, float))
                and isinstance(total, (int, float)) and total > ttft):
            d["decode"].append(ct / ((total - ttft) / 1000.0))
    if not per:
        return None
    out: Dict = {"nodes": {}, "calls": 0, "prompt_tokens": 0, "completion_tokens": 0}
    all_pre: List[float] = []
    all_dec: List[float] = []
    for node, d in sorted(per.items()):
        out["nodes"][node] = {
            "calls": d["calls"],
            "prompt_tokens": d["prompt_tokens"],
            "completion_tokens": d["completion_tokens"],
            "prefill_tok_s": round(statistics.median(d["prefill"]), 1) if d["prefill"] else None,
            "decode_tok_s": round(statistics.median(d["decode"]), 1) if d["decode"] else None,
        }
        out["calls"] += d["calls"]
        out["prompt_tokens"] += d["prompt_tokens"]
        out["completion_tokens"] += d["completion_tokens"]
        all_pre += d["prefill"]
        all_dec += d["decode"]
    out["prefill_tok_s"] = round(statistics.median(all_pre), 1) if all_pre else None
    out["decode_tok_s"] = round(statistics.median(all_dec), 1) if all_dec else None
    return out


def evaluate(c: Ctx) -> Dict:
    rows: List[Dict] = []
    for name, tier, fn in SB7_CHECKS:
        if tier == "E":
            continue
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})
    c._txr_rows = [r for r in rows if r["tier"] in ("T", "X", "R")]
    for name, tier, fn in SB7_CHECKS:
        if tier != "E":
            continue
        try:
            outcome = fn(c)
        except Exception as exc:
            outcome = {"score": 0.0, "detail": f"PROBE ERROR: {exc}", "consequence": "probe bug"}
        rows.append({"check": name, "tier": tier, **outcome})
    result = compose_from_rows(rows, _nominal_console_errors(c), c)
    # Mihai 2026-08-19: the posted benchmark itself must show the run's token rates.
    result["telemetry"] = telemetry_summary(c.root)
    return result


# ── SEVERITY: measured inputs, transforms (F2), multiplier + dedup (F3), F9 ─────────────────

DATA_LOSS_CRITICALS = {"sync_completeness", "r_no_row_loss", "r_cache_truth",
                       "r_workflow_durability"}
CLIFF_CRITICALS = {"x_conservation_residual", "x_no_lost_write", "r_no_dupe_effect"}


def _critical_severity_input(r: Dict) -> float:
    """What the CRITICAL multiplier reads. F2: each critical owns its transform — data-loss
    fractions cliff to <= 0.5 on ANY confirmed silent loss; b_buckets_dst's severity leg is
    the DST-window cells; a cross-currency sum is severity 0. F9: an absent surface is
    severity 0 (an invariant that cannot be evidenced is unproven)."""
    s = max(0.0, min(1.0, r["score"]))
    parts = r.get("parts") or {}
    if parts.get("absent_surface"):
        return 0.0
    if s >= 1.0 - 1e-9:
        return 1.0
    name = r["check"]
    if name in DATA_LOSS_CRITICALS:
        return min(s, 0.5)
    if name in CLIFF_CRITICALS:
        return 0.0
    if name == "b_buckets_dst":
        v = parts.get("dst_window_ok")
        return max(0.0, min(1.0, float(v))) if isinstance(v, (int, float)) else s
    if name == "b_money_rendered":
        if parts.get("cross_currency_sum"):
            return 0.0
        return s
    if name == "server_runs":
        return 0.0 if s < 0.4 else 1.0
    if name == "j_loads_data":
        return 0.0 if s == 0.0 else 1.0
    return s


def compose_from_rows(rows: List[Dict], console: Optional[int],
                      c: Optional[Ctx] = None) -> Dict:
    """The pure composition: rows -> verdict. Split from evaluate so severity_selftest can
    push synthetic single-defect row sets through the REAL scoring math (§5.4).

    F3 semantics: rows carrying parts["vacuous_root"] price ZERO through their tier (their
    0 stays in the mean — the vacuous-pass gate already refused the credit), attribute to
    the named root, and fire NO multiplier of their own; a CRITICAL root that already
    multiplied (severity < 1) likewise suppresses its ROOT_BLOCKS critical dependents'
    multipliers — one defect multiplies once, never ×0.6⁴. Verified by the selftest's
    dead-sync band [0.10, 0.25]. Only unavailable rows are excluded from tier means
    (harness loss is never app evidence)."""
    unavailable = [r["check"] for r in rows if r.get("unavailable")]
    vacuous = {r["check"]: (r.get("parts") or {}).get("vacuous_root")
               for r in rows if (r.get("parts") or {}).get("vacuous_root")}
    tiers: Dict[str, Dict] = {}
    inner = 0.0
    for tier, w in TIER_WEIGHT_SB7.items():
        sub = [r for r in rows if r["tier"] == tier and r["check"] not in DIAGNOSTIC]
        avail = [r for r in sub if not r.get("unavailable")]
        mean = sum(_gamma(r["score"], tier) for r in avail) / len(avail) if avail else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(avail), "weight": w,
                       **({"unavailable": [r["check"] for r in sub if r.get("unavailable")]}
                          if len(avail) != len(sub) else {})}
        inner += mean * w
    gate_fraction, e_mean, gate_conditions = excellence(rows, console)
    score = 0.88 * inner + E_WEIGHT * gate_fraction * e_mean

    crit_floor = float(TH.get("critical_multiplier_floor", 0.6))
    crit_sev: Dict[str, float] = {}
    for r in rows:
        if r["check"] in CRITICAL_CHECKS and not r.get("unavailable"):
            crit_sev[r["check"]] = _critical_severity_input(r)
    suppressed_by: Dict[str, str] = {}
    for name in crit_sev:
        if name in vacuous:
            suppressed_by[name] = f"vacuous:{vacuous[name]}"
            continue
        for root, deps in ROOT_BLOCKS.items():
            if (root != name and name in deps and root in crit_sev
                    and crit_sev[root] < 1.0 - 1e-9):
                suppressed_by[name] = f"root:{root}"
                break
    crit_rows: List[Dict] = []
    crit_mult = 1.0
    for r in rows:
        name = r["check"]
        if name not in crit_sev:
            continue
        sev = crit_sev[name]
        if sev >= 1.0 - 1e-9:
            continue
        if name in suppressed_by:
            crit_rows.append({"check": name, "score": r["score"],
                              "severity_input": round(sev, 4), "factor": 1.0,
                              "suppressed": suppressed_by[name],
                              "why": CRITICAL_CHECKS[name]})
            continue
        factor = crit_floor + (1.0 - crit_floor) * sev
        crit_mult *= factor
        crit_rows.append({"check": name, "score": r["score"],
                          "severity_input": round(sev, 4), "factor": round(factor, 4),
                          "why": CRITICAL_CHECKS[name]})
    pre_severity = score
    score *= crit_mult
    tiers["E"] = {"mean": round(gate_fraction * e_mean, 4),
                  "gate": gate_fraction >= 1.0, "gate_fraction": round(gate_fraction, 4),
                  "weight": E_WEIGHT}
    out = {"score": round(score, 4), "scorer_version": SCORER_VERSION,
           "fixture_seed": getattr(c, "fixture_seed", None) if c is not None else None,
           "inner": round(inner, 4), "excellence_gate": gate_fraction >= 1.0,
           "excellence": {"fraction": round(gate_fraction, 4), "e_mean": round(e_mean, 4),
                          "conditions": gate_conditions},
           "critical": {"floor": crit_floor, "multiplier": round(crit_mult, 4),
                        "pre_severity_score": round(pre_severity, 4), "rows": crit_rows},
           "calibration": ("frozen" if CALIBRATED else
                           "UNCALIBRATED — sb7-thresholds.json defaults; rc-grade only"),
           "gamma": GAMMA, "k_p": K_P,
           "tiers": tiers, "checks": rows,
           "probe_unavailable": unavailable,
           "vacuous": vacuous,
           "harness_missing": list(getattr(c, "harness_missing", []) if c is not None else []),
           "sched_unreached": list(getattr(c, "sched_unreached", []) if c is not None else []),
           "root_causes": attribute_root_causes(rows),
           "excellent": gate_fraction >= 1.0 and score >= 0.88 and not unavailable,
           "solid": score >= 0.55}
    return out


def severity_selftest() -> List[str]:
    """§5.4's skeleton-first monotonicity proof, run by --reference: synthetic row sets
    through the REAL composition. Covers the sb-6 chain verbatim, the single-defect
    consequence classes, F1 (residual attribution), F2 (severity sweeps with cost bands),
    F3 (dead-sync band + no compounding), F9 (unavail exclusion; absent < violated),
    dominance, and gradient sanity. An inversion refuses the freeze."""
    def rows_all_perfect() -> List[Dict]:
        return [{"check": n, "tier": t, "score": 1.0, "detail": "synthetic",
                 "consequence": ""} for n, t, _ in SB7_CHECKS]

    def scenario(overrides: Dict[str, float], parts: Optional[Dict[str, Dict]] = None,
                 unavail_names: tuple = ()) -> List[Dict]:
        rows = rows_all_perfect()
        for r in rows:
            n = r["check"]
            if n in overrides:
                r["score"] = overrides[n]
            if parts and n in parts:
                r["parts"] = parts[n]
            if n in unavail_names:
                r["score"] = 0.0
                r["unavailable"] = True
        return rows

    def score_of(rows: List[Dict], console: int = 0) -> Dict:
        return compose_from_rows(rows, console)

    fails: List[str] = []

    def expect(cond: bool, msg: str):
        if not cond:
            fails.append(msg)

    baseline = score_of(rows_all_perfect())["score"]
    expect(abs(baseline - 1.0) < 1e-6, f"all-perfect must compose to 1.0 (got {baseline})")

    # sb-6 chain verbatim
    console_err = score_of(scenario({"j_console_clean": 0.0, "j_first_use": 0.6}),
                           console=1)["score"]
    wrong_money = score_of(scenario(
        {"b_money_rendered": 0.0},
        parts={"b_money_rendered": {"cross_currency_sum": ["grand_total"]}}))["score"]
    data_loss = score_of(scenario({"r_no_row_loss": 0.0}))["score"]
    dead_flow = score_of(scenario({"j_loads_data": 0.0, "j_first_use": 0.0}))["score"]
    cosmetic = score_of(scenario({"v_styling": 0.5}))["score"]
    minor = score_of(scenario({"d_content_types": 0.5}))["score"]
    expect(wrong_money < console_err,
           f"WRONG MONEY ({wrong_money:.4f}) must cost more than a console error ({console_err:.4f})")
    expect(data_loss < console_err,
           f"DATA LOSS ({data_loss:.4f}) must cost more than a console error ({console_err:.4f})")
    expect(dead_flow < console_err,
           f"DEAD FLOW ({dead_flow:.4f}) must cost more than a console error ({console_err:.4f})")
    expect(console_err < cosmetic,
           f"console error ({console_err:.4f}) must cost more than cosmetic ({cosmetic:.4f})")
    expect(console_err < minor,
           f"console error ({console_err:.4f}) must cost more than minor ({minor:.4f})")

    # single-defect consequence classes, each in its cost band (below console_err)
    singles = {
        "duplicated effect": score_of(scenario({"r_no_dupe_effect": 0.0}))["score"],
        "stale-cache-served": score_of(scenario({"r_cache_truth": 0.0}))["score"],
        "approval revert": score_of(scenario({"r_workflow_durability": 0.0}))["score"],
        "lost ack": score_of(scenario({"x_no_lost_write": 0.0}))["score"],
    }
    for name, v in singles.items():
        expect(v < console_err,
               f"CRITICAL {name} ({v:.4f}) must cost more than a console error ({console_err:.4f})")
        expect(v < cosmetic and v < minor,
               f"CRITICAL {name} ({v:.4f}) must cost more than cosmetic ({cosmetic:.4f}) "
               f"and minor ({minor:.4f})")
        expect(v <= baseline - 0.30,
               f"CRITICAL {name} ({v:.4f}) must sit in its consequence band (<= 0.70)")

    # F1: one dupe fires exactly one critical; loss + equal-value dupe fires two
    one_dupe = score_of(scenario({"r_no_dupe_effect": 0.0}))
    fired = [r for r in one_dupe["critical"]["rows"] if not r.get("suppressed")]
    expect(len(fired) == 1,
           f"F1: one dupe must fire exactly one critical (fired {len(fired)})")
    offset = score_of(scenario({"r_no_dupe_effect": 0.0, "x_no_lost_write": 0.0}))
    fired2 = [r for r in offset["critical"]["rows"] if not r.get("suppressed")]
    expect(len(fired2) == 2,
           f"F1: loss + equal-value dupe must fire two criticals (fired {len(fired2)})")
    expect(offset["score"] < one_dupe["score"] < baseline,
           "F1: the offset pair must cost more than a single dupe")

    # F2: severity-input sweeps with per-class cost bands (not only orderings at zero)
    sweep_points = (0.25, 0.5, 0.75, 0.98)
    for cls, mk in (
        ("sync_completeness", lambda v: scenario({"sync_completeness": v})),
        ("b_buckets_dst", lambda v: scenario({"b_buckets_dst": v},
                                             parts={"b_buckets_dst": {"dst_window_ok": v}})),
        ("b_money_rendered", lambda v: scenario({"b_money_rendered": v})),
        ("j_workflow_journey", lambda v: scenario({"j_workflow_journey": v})),
    ):
        vals = [score_of(mk(v))["score"] for v in sweep_points]
        for a, b in zip(vals, vals[1:]):
            expect(a <= b + 1e-9,
                   f"F2 sweep {cls}: composed must be monotone in severity ({vals})")
        expect(vals[-1] < baseline - 1e-6,
               f"F2 sweep {cls}: a 0.98-complete critical must still cost (got {vals[-1]})")
    near_loss = score_of(scenario({"sync_completeness": 0.98}))["score"]
    expect(near_loss <= 0.85,
           f"F2: a 0.98-complete sync with confirmed silent loss must cost >= 0.15 "
           f"(composed {near_loss:.4f}) — the loss cliff (<=0.5 severity) must bite")

    # F3: dead-sync composite — realistic row footprint; band 0.10–0.25; no compounding
    dead_deps = ROOT_BLOCKS["sync_completeness"]
    overrides = {"sync_completeness": 0.0, "j_first_use": 0.0, "j_sync_journey": 0.0}
    parts_map: Dict[str, Dict] = {}
    for d in dead_deps:
        overrides[d] = 0.0
        parts_map[d] = {"vacuous_root": "sync_completeness"}
    for n, t, _fn in SB7_CHECKS:
        if t in ("T", "X", "P", "V") or (t == "E" and n != "e_optimistic_paint"):
            overrides.setdefault(n, 0.0)
            if n in CRITICAL_CHECKS and n != "sync_completeness":
                parts_map.setdefault(n, {"vacuous_root": "sync_completeness"})
    dead_sync = score_of(scenario(overrides, parts=parts_map))
    ds = dead_sync["score"]
    expect(0.10 <= ds <= 0.25,
           f"F3: the dead-sync composite must land in [0.10, 0.25] (got {ds:.4f})")
    ds_fired = [r for r in dead_sync["critical"]["rows"] if not r.get("suppressed")]
    expect(len(ds_fired) == 1,
           f"F3: dead sync must multiply ONCE, not compound (fired {len(ds_fired)})")
    for name, v in singles.items():
        expect(ds < v, f"F3: dead sync ({ds:.4f}) must score below the working-sync "
               f"single-defect '{name}' ({v:.4f})")

    # F9: unavailable rows are excluded, never zeros; absent < present-with-one-violation
    unav = score_of(scenario({}, unavail_names=("t_labels_culling",)))
    zeroed = score_of(scenario({"t_labels_culling": 0.0}))
    expect(unav["score"] > zeroed["score"],
           "F9: an unavailable row must never price like an app zero")
    expect(unav["probe_unavailable"] == ["t_labels_culling"],
           "F9: the verdict must name its unavailable rows")
    absent = score_of(scenario(
        {"j_workflow_journey": 0.0, "r_workflow_durability": 0.0},
        parts={"j_workflow_journey": {"absent_surface": "workflow UI"},
               "r_workflow_durability": {"absent_surface": "drafts endpoints"}}))["score"]
    one_violation = score_of(scenario({"r_workflow_durability": 0.0}))["score"]
    expect(absent < one_violation,
           f"F9: absent surface ({absent:.4f}) must score strictly below "
           f"implemented-with-one-violation ({one_violation:.4f})")

    # dominance: one violated invariant > ALL cosmetic and minor defects combined
    all_cosmetic = score_of(scenario(
        {n: 0.5 for n, t, _f in SB7_CHECKS if t in ("V", "D")}))["score"]
    one_invariant = score_of(scenario({"x_no_lost_write": 0.0}))["score"]
    expect(one_invariant < all_cosmetic,
           f"dominance: one violated invariant ({one_invariant:.4f}) must cost more than "
           f"every cosmetic+minor defect combined ({all_cosmetic:.4f})")

    # gradient sanity: quality never outbids harm
    kills_half = score_of(scenario(
        {n: 0.5 for n in ("r_b3_sigkill_resync", "r_b4_vendor_down_boot",
                          "r_b6_outbox_atomic", "r_b7_partition")}))["score"]
    kills_perfect_one_invariant = score_of(scenario({"r_no_dupe_effect": 0.0}))["score"]
    expect(kills_half > kills_perfect_one_invariant,
           f"gradient: kill-matrix 0.5 with invariants held ({kills_half:.4f}) must beat "
           f"kill-matrix 1.0 with one invariant violated ({kills_perfect_one_invariant:.4f})")
    return fails


def format_report(result: Dict, title: str = "") -> str:
    t = result["tiers"]
    order = ["A", "B", "C", "D", "J", "V", "P", "T", "X", "R", "E"]
    head = (f"{title}  {100 * result['score']:.1f}%   " +
            " · ".join(f"{k} {100 * t[k]['mean']:.0f}%" for k in order if k in t))
    lines = [head, f"  seed {result.get('fixture_seed')}"]
    if not CALIBRATED:
        lines.append(f"  {UNCALIBRATED_BANNER}")
    if result.get("probe_unavailable"):
        lines.append(f"  ⚠ {len(result['probe_unavailable'])} check(s) PROBE-UNAVAILABLE "
                     f"(excluded from means, NOT app zeros): "
                     f"{', '.join(result['probe_unavailable'][:8])}")
    if result.get("vacuous"):
        lines.append(f"  ⚠ {len(result['vacuous'])} vacuous critical leg(s) attributed to "
                     f"their root: {', '.join(sorted(result['vacuous']))[:120]}")
    if result.get("harness_missing"):
        lines.append(f"  ⚠ vendor-mock surfaces missing: "
                     f"{', '.join(result['harness_missing'][:8])}")
    if result.get("sched_unreached"):
        lines.append(f"  ⚠ {len(result['sched_unreached'])} schedule entr(ies) unreached "
                     f"([A]-class, R1): "
                     f"{', '.join(str(x.get('sched-unreached', x)) for x in result['sched_unreached'][:6])}")
    crit = result.get("critical") or {}
    if crit.get("rows"):
        lines.append(f"  ⚠ CRITICAL multiplier ×{crit.get('multiplier')} "
                     f"(pre-severity {crit.get('pre_severity_score')}): "
                     + "; ".join(f"{r['check']} sev={r['severity_input']}"
                                 + (" [suppressed]" if r.get("suppressed") else "")
                                 for r in crit["rows"][:6]))
    lines.append("")
    for root, blocked in (result.get("root_causes") or {}).items():
        lines.append(f"  ⚠ {len(blocked)} further check(s) are downstream of `{root}` "
                     f"(<1.0) — ONE defect, not {len(blocked) + 1}: {', '.join(blocked)}")
        lines.append("")
    for row in sorted(result["checks"], key=lambda r: (order.index(r["tier"]), r["check"])):
        pct = f"{100 * row['score']:>3.0f}%"
        flag = "◌" if row.get("unavailable") else \
            ("∅" if (row.get("parts") or {}).get("vacuous_root") else " ")
        diag = "·" if row["check"] in DIAGNOSTIC else " "
        lines.append(f" {flag}{diag}{row['tier']:<2} {pct} {row['check']:<28} "
                     f"{str(row.get('detail', ''))[:78]}")
        if row["score"] < 1.0 and row.get("consequence") not in ("n/a", "probe bug", "", None):
            lines.append(f"           └ {row['consequence']}")
    return "\n".join(lines)
# ── gathering ────────────────────────────────────────────────────────────────────────────────

def _kill(proc) -> None:
    if proc and proc.poll() is None:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except Exception:
            proc.kill()
        try:
            proc.wait(timeout=10)
        except Exception:
            pass


def _spawn_ledgerd(c: Ctx, env: Dict, db_dir: Path, port: int, nport: int,
                   vendor_url: str, tokens_file: Path):
    return subprocess.Popen(
        [sys.executable, "-m", "app.ledgerd", "--db-dir", str(db_dir),
         "--port", str(port), "--notifier", f"http://127.0.0.1:{nport}",
         "--vendor", vendor_url, "--tokens-file", str(tokens_file)],
        cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, start_new_session=True)


def _spawn_notifierd(c: Ctx, env: Dict, db_dir: Path, port: int):
    return subprocess.Popen(
        [sys.executable, "-m", "app.notifierd", "--db-dir", str(db_dir),
         "--port", str(port)],
        cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, start_new_session=True)


def _wait_status(url: str, secs: float, proc=None) -> Optional[float]:
    """Seconds until the URL answers ANY HTTP status, or None. A dead process short-circuits."""
    t0 = time.time()
    while time.time() - t0 < secs:
        if proc is not None and proc.poll() is not None:
            return None
        st, _b, _r, _h = _get(url, timeout=2)
        if st is not None:
            return round(time.time() - t0, 1)
        time.sleep(0.3)
    return None


def _total_of(base: str) -> Optional[int]:
    _s, body, _r, _h = _get(f"{base}/api/payments?limit=1", timeout=8)
    t = (body or {}).get("total") if isinstance(body, dict) else None
    return t if isinstance(t, int) else None


def _wait_total(base: str, want: int, secs: float) -> tuple:
    """(reached: bool, wall_ms or None, last_total). Polls until total >= want."""
    t0 = time.time()
    last = None
    while time.time() - t0 < secs:
        t = _total_of(base)
        if t is not None:
            last = t
            if t >= want:
                return True, round((time.time() - t0) * 1000, 1), t
        time.sleep(1.5)
    return False, None, last


def _row_of(base: str, pid: str) -> Optional[Dict]:
    _s, body, _r, _h = _get(f"{base}/api/payments/{pid}", timeout=8)
    return body if isinstance(body, dict) and body.get("id") else None


def _snapshot_rows(base: str, pids: List[str]) -> Dict[str, Dict]:
    out: Dict[str, Dict] = {}
    for pid in pids:
        row = _row_of(base, pid)
        if row is not None:
            out[pid] = row
    return out


def _sse_head(base: str, timeout: float = 5) -> Dict:
    try:
        req = urllib.request.Request(f"{base}/api/stream")
        with urllib.request.urlopen(req, timeout=timeout) as r:
            head = r.read(160)
            return {"status": r.status, "ctype": r.headers.get("Content-Type", ""),
                    "head": head.decode(errors="replace")}
    except Exception as e:
        return {"status": None, "ctype": "", "error": f"{type(e).__name__}"}


class _ReadStream(threading.Thread):
    """The scorer's timestamped read stream (§4.3): /api/summary + the seeded targets every
    250 ms; summary FIRST so a legit atomic flip cannot masquerade as a half state twice."""

    def __init__(self, base: str, pids: List[str]):
        super().__init__(daemon=True)
        self.base, self.pids = base, pids
        self.samples: List[Dict] = []
        self._stop = threading.Event()

    def stop(self):
        self._stop.set()

    def run(self):
        while not self._stop.is_set():
            t = time.time()
            _s, summ, _r, _h = _get(f"{self.base}/api/summary", timeout=4)
            rows = {}
            for pid in self.pids:
                _s2, row, _r2, _h2 = _get(f"{self.base}/api/payments/{pid}", timeout=4)
                if isinstance(row, dict) and row.get("id"):
                    rows[pid] = row
            if isinstance(summ, dict) and summ:
                self.samples.append({"t": t, "summary": summ, "rows": rows})
            self._stop.wait(0.25)


class _TraceTail:
    """Incremental reader over the vendor's JSONL trace — kill triggers tail the trace
    (§4.1 kill-placement honesty), they never guess sub-request instants."""

    def __init__(self, path: Path):
        self.path = Path(path)
        self._buf = ""
        self._pos = 0

    def new_events(self) -> List[Dict]:
        try:
            with self.path.open("r", errors="replace") as f:
                f.seek(self._pos)
                chunk = f.read()
                self._pos = f.tell()
        except FileNotFoundError:
            return []
        self._buf += chunk
        out: List[Dict] = []
        while "\n" in self._buf:
            line, self._buf = self._buf.split("\n", 1)
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except Exception:
                pass
        return out

    def wait_for(self, count_fn, want: int, timeout: float) -> tuple:
        """Accumulates events until count_fn-total >= want. (reached, seen)."""
        t0 = time.time()
        seen = 0
        while time.time() - t0 < timeout:
            for e in self.new_events():
                seen += count_fn(e)
                if seen >= want:
                    return True, seen
            time.sleep(0.15)
        return False, seen


def _measure_under_stream(base: str, V) -> Dict:
    """Readers during a vendor-fired stream burst (F8c lineage: overlap is recorded so an
    unproven load refuses instead of crediting)."""
    if V is None or not hasattr(V, "fire_burst"):
        return {}
    window = {"t0": None, "t1": None}

    def _burst():
        window["t0"] = time.time()
        try:
            ret = V.fire_burst()
            if isinstance(ret, dict):
                window["t0"] = ret.get("t0", window["t0"])
                window["t1"] = ret.get("t1")
        except Exception:
            pass
        window["t1"] = window["t1"] or time.time()

    # Harness fix: a fast app finishes the 24-delivery burst in well under the old
    # 0.2 s reader head start, leaving overlap 0.0 and an unfair refusal — readers
    # start FIRST and keep reading until the burst has completed (bounded).
    burst_done = threading.Event()
    go = threading.Event()
    lock = threading.Lock()
    samples: List[Dict] = []

    def _burst_wrapped():
        go.set()
        try:
            _burst()
        finally:
            burst_done.set()

    def _reader():
        go.wait(timeout=10)
        n = 0
        while n < 40 and (n < 5 or not burst_done.is_set()):
            n += 1
            t0 = time.time()
            status, body, _raw, _h = _get(f"{base}/api/payments?limit=50", timeout=10)
            t1 = time.time()
            with lock:
                samples.append({"t0": t0, "t1": t1, "ms": (t1 - t0) * 1000,
                                "ok": status == 200 and isinstance(body, dict)
                                and isinstance(body.get("total"), int)
                                and bool(body.get("data"))})
    readers = [threading.Thread(target=_reader) for _ in range(6)]
    for r_ in readers:
        r_.start()
    bt = threading.Thread(target=_burst_wrapped)
    bt.start()
    for r_ in readers:
        r_.join(timeout=60)
    bt.join(timeout=60)
    if not samples:
        return {}
    s0, s1 = window["t0"], window["t1"] or time.time()
    overlapped = [x for x in samples if s0 is not None and x["t1"] > s0 and x["t0"] < s1]
    ms = sorted(x["ms"] for x in samples)
    p95 = ms[min(len(ms) - 1, int(0.95 * len(ms)))]
    return {"p95_ms": round(p95, 1), "n": len(samples),
            "overlap_frac": round(len(overlapped) / len(samples), 3),
            "correct": all(x["ok"] for x in samples)}


def _fetch_events(base: str, token: Optional[str] = None) -> List[Dict]:
    """/api/events requires a bearer token (spec §3: any of the three roles) — the admin
    token rides every fetch so a spec-compliant 401 never reads as an empty ledger."""
    out: List[Dict] = []
    after = 0
    for _ in range(80):
        _s, body, _r, _h = _get_auth(f"{base}/api/events?after={after}&limit=1000",
                                     token, timeout=15)
        evs = (body or {}).get("events") if isinstance(body, dict) else None
        if not isinstance(evs, list) or not evs:
            break
        out.extend(e for e in evs if isinstance(e, dict))
        seqs = [e.get("seq") for e in evs if isinstance(e.get("seq"), int)]
        if not seqs:
            break
        after = max(seqs)
        latest = body.get("latest_seq")
        if isinstance(latest, int) and after >= latest:
            break
    return out


def _analyze_trace(c: Ctx) -> None:
    """Trace → walk stats, B1/B2/B5 evidence, resync conditional counts, sched-unreached."""
    segs = _segments(c.trace)
    c.sched_unreached = [e for e in c.trace if e.get("sched-unreached")]
    # Sync #1's walk is DEFINED by the vendor's sync ordinal, not by the wall-clock
    # phase boundary (harness fix: an app whose self-driven retry lands before the
    # driver's post-refusal sleep expires starts its walk inside the "boot" segment,
    # and phase-keyed stats then blamed the app for pages it provably served).
    lp1 = _list_path()
    lists1 = [e for e in c.trace
              if e.get("path") == lp1 and e.get("method") == "GET"
              and e.get("sync") == 1]
    if not lists1:
        lists1 = _list_events(segs.get("sync1", []))
    served1 = [e for e in lists1 if e.get("status") == 200]
    pages1 = [e.get("page") for e in served1 if e.get("page") is not None]
    undoc = sum(1 for e in lists1
                if any(k in (e.get("query") or {})
                       for k in ("limit", "offset", "page", "start", "skip")))
    # Delivery lines are the vendor's own shape: {"webhook_delivery": <event_id>, "kind":
    # apply|stale|duplicate|forged, "delivery_status": <app HTTP status>, "sched": ...}.
    # (payment_id, version) per event_id comes from the fixtures' scheduled script — one
    # derivation (§5.5), never re-parsed from the wire.
    has_deliv = any("webhook_delivery" in e for e in c.trace)
    # F9/DESIGN §4.1: an app that never registered a webhook produced only sched-partial
    # 'no_webhook_registered' lines — that is APP behavior (0 deliveries, registered=false),
    # never a missing vendor surface.
    never_registered = any(e.get("sched-partial") and e.get("reason") == "no_webhook_registered"
                           for e in c.trace)
    deliv = [e for e in c.trace if "webhook_delivery" in e] if has_deliv \
        else ([] if never_registered else None)
    acked = None
    forged_status = None
    nonforged = 0
    nonforged_2xx = 0
    if deliv is not None:
        emap: Dict[str, Dict] = {}
        if c.pack is not None:
            fxm = _fixtures()
            if fxm is not None and hasattr(fxm, "scheduled_delivery_script"):
                try:
                    emap = {ev["event_id"]: ev
                            for ev in fxm.scheduled_delivery_script(c.pack)}
                except Exception:
                    emap = {}
        acked = []
        for e in deliv:
            st_d = e.get("delivery_status")
            ok2 = isinstance(st_d, int) and 200 <= st_d < 300
            if e.get("sched") == "forged_event" or e.get("kind") == "forged":
                forged_status = st_d
                continue
            nonforged += 1
            if ok2:
                nonforged_2xx += 1
                ev = emap.get(e.get("webhook_delivery"))
                if ev and ev.get("kind") == "apply" \
                        and isinstance(ev.get("version"), int):
                    acked.append({"payment_id": ev.get("payment_id"),
                                  "version": ev["version"]})
    c.walk_stats = {
        "sync1_lists": len(lists1),
        "sync1_200": len(served1),
        "dup_served_pages": (len(pages1) - len(set(pages1))) if pages1 else 0,
        "undocumented_params": undoc,
        "webhook_deliveries": nonforged if deliv is not None else None,
        "webhook_registered": not never_registered,
        "webhook_2xx": nonforged_2xx,
        "acked_deliveries": acked,
        "forged_status": forged_status,
    }
    c.sync1_reqs = len(lists1)

    # c_send_idempotency inputs: every POST /v3/payments in the trace (the vendor's own
    # outbound deliveries log method "POST(out)", so exact-match filtering excludes them).
    sends = [e for e in c.trace
             if e.get("method") == "POST" and e.get("path") == "/v3/payments"]
    send_keys = [e.get("idempotency_key") for e in sends if e.get("idempotency_key")]
    rep = (c.workflow or {}).get("idempotency_replays") or {}
    c.workflow["send_requests"] = len(sends)
    c.workflow["send_with_key"] = len(send_keys)
    c.workflow["retry_reused_key"] = (len(send_keys) != len(set(send_keys))
                                      or bool(rep.get("total_replays")))

    armed_drop = any(e.get("sched") == "drop_after_page" for e in c.trace)
    walk_complete = len(served1) >= PAGES
    c.b1_result = {"armed": armed_drop,
                   "resumed": walk_complete,
                   "no_full_restart": (len(set(pages1)) == len(pages1)) if pages1
                   else len(served1) <= PAGES + 2,
                   "walk_completed": walk_complete}

    fives = [i for i, e in enumerate(lists1) if e.get("status") == 500]
    armed_500 = any(e.get("sched") == "http500_page" for e in c.trace) or bool(fives)
    single_retry = len(fives) == 1
    waited = True
    if fives and all("t" in e for e in lists1):
        i = fives[0]
        nxt = next((e for e in lists1[i + 1:] if e.get("status") == 200), None)
        if nxt is not None:
            try:
                waited = (float(nxt["t"]) - float(lists1[i]["t"])) >= \
                    fixtures_v3_retry_after() - 0.3
            except Exception:
                waited = True
    c.b2_result = {"armed": armed_500, "single_retry": single_retry,
                   "waited": waited, "continued": walk_complete}

    cond = n304 = reqs = 0
    uncond_new_offsets: set = set()
    lying_in_segs = False
    for name in ("sync2", "sync3", "sync4"):
        for e in _list_events(segs.get(name, [])):
            reqs += 1
            if e.get("if_none_match") or e.get("conditional"):
                cond += 1
            else:
                off = e.get("offset")
                if isinstance(off, int) and off >= N_FROZEN:
                    uncond_new_offsets.add(off)   # a page born after fixture load —
                    #                               no validator can exist on first visit
            if e.get("status") == 304:
                n304 += 1
                if e.get("lying"):
                    lying_in_segs = True
    c.resync_reqs, c.resync_cond, c.resync_304 = reqs, cond, n304
    c.resync_allowed_uncond = min(len(uncond_new_offsets), 4) + (1 if lying_in_segs else 0)

    # B5 by the vendor's own per-line facts — the lying 304s carry "lying": true and a
    # "sync" ordinal; the armed one additionally carries sched: "stale_304_sync". Phase
    # names never enter it (the armed sync's ordinal is whatever sync presented staleness).
    lp5 = _list_path()
    lies = [e for e in c.trace
            if e.get("path") == lp5 and e.get("status") == 304 and e.get("lying")]
    if lies:
        first = lies[0]
        s_ord, off, t0 = first.get("sync"), first.get("offset"), first.get("t", 0)
        after = [e for e in c.trace
                 if e.get("path") == lp5 and e.get("method") == "GET"
                 and e.get("t", 0) > t0 and e.get("sync") == s_ord]
        uncond = sum(1 for e in after
                     if e.get("status") == 200 and e.get("offset") == off
                     and not e.get("if_none_match"))
        c.b5_result = {**c.b5_result, "armed": True, "sync_ordinal": s_ord,
                       "unconditional_refetches": uncond,
                       "conditional_repeats": len(lies) - 1,
                       "propagated": c.b5_result.get("propagated"),
                       "stale_served_as_fresh": c.b5_result.get("stale_served_as_fresh")}
    else:
        c.b5_result = {**c.b5_result, "armed": c.b5_result.get("armed", False)}


def fixtures_v3_retry_after() -> float:
    fxm = _fixtures()
    return float(getattr(fxm, "RETRY_AFTER_SECS", 2)) if fxm else 2.0


def _conclude_read_stream(c: Ctx) -> None:
    """M2/L5 conclusion over the read stream: pair conservation at every snapshot; a half
    state counts only when CONFIRMED by the next sample too (two sequential GETs can
    straddle one atomic flip — one transient disagreement is read skew, not evidence)."""
    if c.pack is None or not c.read_stream:
        return
    fx = c.pack
    rt = fx.schedule.refund_txn or {}
    pid = rt.get("payment_id")
    if not pid:
        return
    row0 = fx.index().get(pid) or {}
    cur, amt = row0.get("currency"), row0.get("amount_minor", 0)
    boot = fx.EXPECTED_REVERSALS_BY_CURRENCY.get(cur, {}).get("total_minor", 0)
    samples = ok = confirmed = 0
    prev_half = False
    for s in c.read_stream:
        summ = s.get("summary") or {}
        row = (s.get("rows") or {}).get(pid)
        if not summ or not isinstance(row, dict):
            continue
        samples += 1
        refunded = row.get("status") == "refunded"
        rev = {x.get("currency"): x for x in (summ.get("reversals") or [])
               if isinstance(x, dict)}
        got = rev.get(cur, {}).get("total_minor", 0)
        want = boot + (amt if refunded else 0)
        if got == want:
            ok += 1
            prev_half = False
        else:
            if prev_half:
                confirmed += 1
            prev_half = True
    c._m2_conclusion = {"samples": samples, "ok_samples": ok,
                        "confirmed_half_states": confirmed,
                        "refund_pid": pid, "currency": cur}


def _write_expect_pack(c: Ctx, path: Path, created_rows: List[Dict],
                       d1_target: str) -> None:
    """The probe's expectation pack: fixtures_v3's own PROBE_EXPECT (one derivation, §5.5)
    UNIONED with the probe-header fields (columnar records in arrival order incl. the
    vendor-committed creates, stream.mutateIds, approval drafts) — both documented pack
    shapes are served so neither sibling contract drifts."""
    fx = c.pack

    def _row_key(r_: Dict):
        return (_instant(r_.get("_instant") or r_.get("created_at") or "") or 0.0,
                str(r_.get("id")))
    # §3.1: the graded load serves (created_at instant ASC, id ASC) — creates committed
    # before the load INTERLEAVE chronologically, they do not append.
    rows = sorted(fx.payments + created_rows, key=_row_key)
    pack = dict(getattr(fx, "PROBE_EXPECT", None) or {})
    pack.update({
        "seed": c.fixture_seed,
        "count": len(rows),
        "records": {k: [r.get(k if k != "day" else "_day", r.get("day"))
                        for r in rows] for k in
                    ("id", "amount_minor", "currency", "status", "day")},
        "tokens": dict(fx.tokens),
        "approval": {"drafts": [{k: d[k] for k in
                                 ("amount_minor", "currency", "counterparty", "note")}
                                for d in fx.schedule.approval_fixture]},
        "stream": {"mutateIds": [d1_target] if d1_target else []},
    })
    path.write_text(json.dumps(pack))


def gather(root: Path, vendor_port: int, db_dir: Path, trace_path: Path,
           mark_phase=None, seed: Optional[str] = None) -> Ctx:
    root = Path(root).resolve()
    db_dir = Path(db_dir).resolve()
    trace_path = Path(trace_path).resolve()
    c = Ctx(root)
    c.db_dir = db_dir
    c.trace_path = trace_path
    V = _vendor()
    fxm = _fixtures()

    def _need(name: str) -> bool:
        if V is not None and hasattr(V, name):
            return True
        c.harness_missing.append(name)
        return False

    def mark(name: str, reset=None):
        if mark_phase:
            try:
                mark_phase(name, reset) if reset is not None else mark_phase(name)
            except TypeError:
                mark_phase(name)
            except Exception:
                pass

    # ── 0: files, fixtures, tokens ───────────────────────────────────────────────────────────
    if not (root / "app").is_dir():
        nested = [p for p in root.iterdir() if p.is_dir() and (p / "app").is_dir()]
        if nested:
            c.root, c.pkg, c.web = nested[0], nested[0] / "app", nested[0] / "web"
    for w in WEB_FILES:
        f = c.root / w
        c.asset_bytes[w.split("/")[-1]] = f.stat().st_size if f.is_file() else None
    html_path = c.root / "web/index.html"
    if html_path.is_file():
        c.html = html_path.read_text(errors="replace")
    dec_path = c.root / "DECISIONS.md"
    if dec_path.is_file():
        c.decisions_text = dec_path.read_text(errors="replace")
        c.decisions = _parse_decisions(c.decisions_text)

    c.fixture_seed = seed
    fx = None
    sch = None
    if fxm is not None and seed:
        try:
            fx = fxm.build(seed)
            sch = fx.schedule
            c.pack = fx
            c.schedule = sch
        except Exception as e:
            c.harness_missing.append(f"fixtures_v3.build:{type(e).__name__}")
    else:
        c.harness_missing.append("fixtures_v3")

    tokens = dict(fx.tokens) if fx is not None else {}
    tokens_file = root / "sb7-tokens.json"
    tokens_file.write_text(json.dumps(tokens))
    shots = root / "sb7-shots"
    shots.mkdir(exist_ok=True)
    expect_path = root / "sb7-expect.json"
    probe_env = {"BENCH_SB7_TOKENS": json.dumps(tokens), "BENCH_SHOTS_DIR": str(shots),
                 "BENCH_SB7_EXPECT": str(expect_path), "SB7_EXPECT_FILE": str(expect_path)}

    db_dir.mkdir(parents=True, exist_ok=True)
    lport, nport = _free_port(), _free_port()
    base = f"http://127.0.0.1:{lport}"
    nbase = f"http://127.0.0.1:{nport}"
    vendor_url = f"http://127.0.0.1:{vendor_port}"
    env = {**os.environ, "PYTHONPATH": str(c.root), "PYTHONDONTWRITEBYTECODE": "1"}

    ledger_proc = notifier_proc = None
    reader: Optional[_ReadStream] = None
    try:
        # ── 1: B4 — vendor refuses connections for the first w s of boot ──────────────────────
        mark("boot")
        w_secs = sch.vendor_down_boot_secs if sch is not None else 0
        armed_b4 = False
        if w_secs and _need("arm_boot_refusal"):
            try:
                V.arm_boot_refusal(w_secs)
                armed_b4 = True
            except Exception:
                c.harness_missing.append("arm_boot_refusal:failed")
        arm_t0 = time.time()
        boot_t0 = time.time()
        notifier_proc = _spawn_notifierd(c, env, db_dir, nport)
        ledger_proc = _spawn_ledgerd(c, env, db_dir, lport, nport, vendor_url, tokens_file)
        bind_secs = _wait_status(f"{base}/", 25, ledger_proc)
        if bind_secs is None and ledger_proc.poll() is not None:
            c.boot_error = (ledger_proc.stdout.read() or "")[-800:]
        c.port_bound = bind_secs is not None
        c.boot_secs = bind_secs
        if bind_secs is None and ledger_proc.poll() is None:
            c.proc_alive_5s = True
        n_bind = _wait_status(f"{nbase}/health", 12, notifier_proc)
        if n_bind is not None:
            _s, nh, _r, _h = _get(f"{nbase}/health", timeout=3)
            c.notifier_health_boot = nh if isinstance(nh, dict) else {}
        served_down = False
        if c.port_bound and armed_b4 and (time.time() - arm_t0) < w_secs:
            st_p, body_p, _r, _h = _get(f"{base}/api/payments?limit=1", timeout=3)
            served_down = st_p == 200 and isinstance(body_p, dict)
        c.b4_result = {"armed": armed_b4, "refusal_secs": w_secs,
                       "bound_in_10s": bind_secs is not None and bind_secs <= 10,
                       "served_while_down": served_down,
                       "no_crash": ledger_proc.poll() is None
                       and notifier_proc.poll() is None}
        if c.port_bound:
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

        if not c.port_bound:
            return c    # boot ladder scores it; nothing downstream is measurable

        # ── 2/3: sync #1 (self-driven) with the read stream over the racing window ────────────
        if armed_b4:
            remain = w_secs - (time.time() - arm_t0)
            if remain > 0:
                time.sleep(remain + 1.0)
        if fx is not None:
            c.read_targets = list(getattr(fx, "READ_TARGETS", []) or [])
            refund_pid = sch.refund_txn["payment_id"]
            if refund_pid not in c.read_targets:
                c.read_targets.append(refund_pid)   # the M2/L5 pair-conservation witness
            reader = _ReadStream(base, c.read_targets)
            reader.start()
        # The vendor reset happened at mark("boot"), BEFORE B4 was armed and the app booted;
        # marking sync1 with reset here would race an app whose first list request landed in
        # the gap after the refusal window (its walk state would be wiped mid-flight).
        mark("sync1", False)
        reached, wall, last = _wait_total(base, N_FROZEN, 420)
        c.sync1_done = reached
        c.sync1_wall_ms = wall
        c.b4_result["recovered"] = last is not None and last > 0
        mark("postsync1", False)

        # ── 4: API battery ────────────────────────────────────────────────────────────────────
        maker = tokens.get("maker")
        checker = tokens.get("checker")
        admin = tokens.get("admin")
        _s, c.payments, _r, hdrs = _get(f"{base}/api/payments")
        c.api_ctype = (hdrs or {}).get("Content-Type", "")
        _s, c.page_deep, _r, _h = _get(f"{base}/api/payments?limit=5&offset=200")
        _s, c.filtered, _r, _h = _get(f"{base}/api/payments?status=refunded&limit=10")
        _s, c.sorted_desc, _r, _h = _get(f"{base}/api/payments?sort=-created_at&limit=10")
        _s, c.summary, _r, _h = _get(f"{base}/api/summary")
        _s, c.buckets, _r, _h = _get(f"{base}/api/buckets")
        _s, c.viz_records, _r, _h = _get(f"{base}/api/viz/records", timeout=60)
        _s, c.outbox_status, _r, _h = _get(f"{base}/api/outbox/status")
        _s, c.notifications_proxy, _r, _h = _get(f"{base}/api/notifications?limit=10")
        c.stream_head = _sse_head(base)
        c.events = _fetch_events(base, admin)
        est, ebody, _r, _h = _get_auth(f"{base}/api/events?after=0&limit=5", admin)
        c.events_shape = {"status": est,
                          "keys": sorted(ebody) if isinstance(ebody, dict) else []}
        if fx is not None:
            mc_slot = fx.value_slots[sch.midwalk_create["value_slot"]]
            cells = {(x["day"], x["status"]): x["count"]
                     for x in fx.EXPECTED_BUCKETS["cells"]}
            key = (mc_slot["day"], "pending")
            if key in cells:
                cells[key] += 1
            # The racing window's page-triggered status flips (race mutations + the
            # refund) are COMMITTED and webhook-delivered before the battery reads
            # buckets — a correct app's cells reflect them, so the expectation must
            # (harness fix: comparing against untouched fixture cells failed every
            # correct app on ~2 cells per status-kind mutation).
            fxm2 = _fixtures()
            if fxm2 is not None and hasattr(fxm2, "scheduled_delivery_script"):
                idx0 = fx.index()
                flips: Dict[str, tuple] = {}
                for ev in fxm2.scheduled_delivery_script(fx):
                    if ev.get("page") is None or ev.get("kind") in ("duplicate",
                                                                    "forged"):
                        continue
                    if ev.get("type") != "payment.updated":
                        continue
                    pid, ver = ev.get("payment_id"), ev.get("version")
                    if pid in idx0 and isinstance(ver, int) \
                            and ver > flips.get(pid, (0, ""))[0]:
                        flips[pid] = (ver, ev.get("status"))
                for pid, (_v, status) in flips.items():
                    brow = idx0[pid]
                    if status and status != brow["status"]:
                        cells[(brow["_day"], brow["status"])] -= 1
                        cells[(brow["_day"], status)] = \
                            cells.get((brow["_day"], status), 0) + 1
            c.buckets_expected = cells
            c.expected_total_at_load = N_FROZEN + 1

        def _envelope_ok(body) -> bool:
            e = _error_obj(body)
            return bool(e) and isinstance(e.get("code"), str) \
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
            c.val_matrix.append((path, st, st == want_status))
            # A STRING ERROR BODY MUST SCORE, NOT CRASH.
            #
            # The isinstance guard covered `body` and not `body["error"]`, so an app answering
            # {"error": "Not found"} -- an entirely ordinary shape, and exactly what r0 returned from
            # GET / -- reached "Not found".get("field_errors") and killed the whole scoring run with an
            # AttributeError. A tree that cannot be scored is worse than a tree that scores zero: the
            # first tells you nothing and the second is a measurement.
            #
            # This changes no verdict. A string error genuinely carries no field_errors, so fe stays None
            # and paths_ok stays False, which is the failing reading it always should have been.
            fe = _error_obj(body).get("field_errors")
            paths_ok = bool(fe) and all(
                isinstance(x, dict) and isinstance(x.get("path"), str)
                and isinstance(x.get("code"), str) for x in fe)
            c.envelope_cases.append({"path": path, "envelope_ok": _envelope_ok(body),
                                     "expects_field_errors": expects_fe,
                                     "field_paths_ok": paths_ok})
        draft_probe = {"amount_minor": 1200, "currency": "EUR",
                       "counterparty": {"name": "Auth Probe", "country": "DE"},
                       "note": "auth-matrix"}
        # drafts:approve:maker targets a NONEXISTENT id with the wrong role — the spec never
        # pins whether role or existence is checked first, so 403 and 404 are both correct.
        for label, fn, want in [
            ("drafts:create:no-token", lambda: _post_auth(f"{base}/api/drafts", draft_probe, None), 401),
            ("drafts:create:bad-token", lambda: _post_auth(f"{base}/api/drafts", draft_probe, "0" * 32), 401),
            ("drafts:create:admin", lambda: _post_auth(f"{base}/api/drafts", draft_probe, admin), 403),
            ("drafts:approve:maker", lambda: _post_auth(f"{base}/api/drafts/draft_nope/approve", {}, maker), (403, 404)),
            ("drafts:approve:unknown", lambda: _post_auth(f"{base}/api/drafts/draft_nope_2/approve", {}, checker), 404),
            ("drafts:list:admin", lambda: _get_auth(f"{base}/api/drafts", admin), 200),
            ("events:admin", lambda: _get_auth(f"{base}/api/events?after=0&limit=1", admin), 200),
        ]:
            try:
                st = fn()[0]
            except Exception:
                st = None
            ok = st in want if isinstance(want, tuple) else st == want
            c.auth_matrix.append((label, st, ok))
        st_bad, body_bad, _r, _h = _post_auth(f"{base}/api/drafts",
                                              {"amount_minor": "x", "currency": "EUR"},
                                              maker)
        c.val_matrix.append(("/api/drafts[invalid]", st_bad, st_bad == 400))
        c.envelope_cases.append({"path": "/api/drafts[invalid]",
                                 "envelope_ok": _envelope_ok(body_bad),
                                 "expects_field_errors": True,
                                 "field_paths_ok": bool(_error_obj(body_bad).get("field_errors"))})
        for jp in ("/api/payments?limit=1", "/api/summary", "/api/buckets",
                   "/api/outbox/status", "/api/nope", "/api/payments?limit=-1"):
            stj, _bj, rawj, hj = _get(f"{base}{jp}")
            try:
                json.loads(rawj if isinstance(rawj, str) else rawj.decode(errors="replace"))
                parses = True
            except Exception:
                parses = False
            c.json_probe.append((jp, stj, parses, (hj.get("Content-Type", "") if hj else "")))

        # ── 5: B3 — SIGKILL after the n-th list response of sync #2, restart, converge ────────
        before = _total_of(base) or 0
        c.rows_before_kill = before
        mark("sync2")
        tail = _TraceTail(trace_path)
        tail.new_events()                      # consume history; count only sync2 lists
        _kill(ledger_proc)
        st_n_alive, _bn, _rn, _hn = _get(f"{nbase}/health", timeout=3)
        c.notifier_alive_during_kill = st_n_alive == 200
        c.kill_ledger.append({"label": "b3-enter", "before": before, "after": None})
        ledger_proc = _spawn_ledgerd(c, env, db_dir, lport, nport, vendor_url, tokens_file)
        n_kill = sch.sigkill_after_list if sch is not None else 3
        lp = _list_path()
        reached_n, seen = tail.wait_for(
            lambda e: 1 if (e.get("path") == lp and e.get("method") == "GET"
                            and e.get("status") == 200) else 0,
            n_kill, timeout=90)
        b3 = {"kill_fired": reached_n, "n": n_kill, "reason": None}
        if reached_n:
            _kill(ledger_proc)
            c.kill_ledger.append({"label": "b3-midwalk", "before": before, "after": None})
            mark("sync3")
            ledger_proc = _spawn_ledgerd(c, env, db_dir, lport, nport, vendor_url,
                                         tokens_file)
            up = _wait_status(f"{base}/", 20, ledger_proc)
            b3["restarted"] = up is not None
            conv, _wall3, last3 = _wait_total(base, N_FROZEN + 1, 300)
            b3["converged"] = conv
            b3["no_duplicates"] = isinstance(last3, int) and last3 <= N_FROZEN + 4
            for k in c.kill_ledger:
                if k["after"] is None:
                    k["after"] = last3
                    k["restarted"] = b3.get("restarted")
        else:
            b3["reason"] = f"sync2 produced only {seen} list responses (sched-unreached, R1)"
            c.sched_unreached.append({"sched-unreached": "sigkill_after_list",
                                      "reason": b3["reason"]})
            _wait_status(f"{base}/", 20, ledger_proc)
            _wait_total(base, N_FROZEN + 1, 120)
        c.b3_result = b3
        for k in c.kill_ledger:
            if k.get("after") is None:
                k["after"] = _total_of(base)
                k.setdefault("restarted", ledger_proc.poll() is None)
        # post-B3 staleness snapshot (B5 may have armed on ANY stale-presenting sync so far;
        # the later post-sync-now snapshot overwrites with an even-later view)
        if sch is not None:
            targets = [m["payment_id"] for m in sch.race_mutations[sch.race_pages[0]]]
            snap = _snapshot_rows(base, targets)
            final_truth, _v = _committed_truth(c)
            stale = any(isinstance((snap.get(p) or {}).get("version"), int)
                        and final_truth and p in final_truth
                        and snap[p]["version"] < final_truth[p]["version"]
                        for p in targets if p in snap)
            c.b5_result.update({"propagated": not stale, "stale_served_as_fresh": stale})

        # ── 6: B7 partition + B6 + A1/A2 (workflow F3 over the API) ───────────────────────────
        mark("partition", False)
        b7: Dict = {}
        x_thresh = sch.partition_after_event if sch is not None else 1
        t0 = time.time()
        seen_x = False
        while time.time() - t0 < 45:
            _s, pr, _r, _h = _get(f"{nbase}/notify/processed?after=0", timeout=5)
            plist = (pr or {}).get("processed") if isinstance(pr, dict) else None
            if isinstance(plist, list) and any(
                    isinstance(p.get("seq"), int) and p["seq"] >= x_thresh for p in plist):
                c.notifier_processed_prekill = plist
                seen_x = True
                break
            time.sleep(1.0)
        if not seen_x:
            c.sched_unreached.append({"sched-unreached": "partition_after_event",
                                      "reason": "durable processed never reached X "
                                                "([U] — vendor/app produced no crossing "
                                                "deliveries)"})
        _kill(notifier_proc)
        notifier_proc = None
        if _need("fire_partition_commits"):
            try:
                V.fire_partition_commits()
            except Exception:
                c.harness_missing.append("fire_partition_commits:failed")
        f3 = next((d for d in (sch.approval_fixture if sch is not None else [])
                   if d["ref"] == "F3"), None)
        wf: Dict = {}
        if f3 is not None:
            t_create0 = time.time()
            st_c, body_c, _r, _h = _post_auth(
                f"{base}/api/drafts",
                {k: f3[k] for k in ("amount_minor", "currency", "counterparty", "note")},
                maker)
            create_ms = (time.time() - t_create0) * 1000
            did = (body_c or {}).get("id") if isinstance(body_c, dict) else None
            wf["create_status"], wf["create_ms"] = st_c, round(create_ms, 1)
            if st_c == 404:
                wf["absent_surface"] = True
            if did and st_c in (200, 201):
                c.draft_ids["F3"] = did
                t_sub0 = time.time()
                st_s, _bs, _r, _h = _post_auth(f"{base}/api/drafts/{did}/submit", {}, maker)
                wf["submit_status"] = st_s
                wf["submit_ms"] = round((time.time() - t_sub0) * 1000, 1)
                _s, ob, _r, _h = _get(f"{base}/api/outbox/status")
                c.outbox_during_partition = ob if isinstance(ob, dict) else {}
                st_np, body_np, _r, _h = _get(f"{base}/api/notifications?limit=5")
                c.notifications_proxy_down = (st_np, body_np)
                before_a1 = _total_of(base) or 0
                _kill(ledger_proc)                      # A1 (doubles as the B6 kill)
                ledger_proc = _spawn_ledgerd(c, env, db_dir, lport, nport, vendor_url,
                                             tokens_file)
                up = _wait_status(f"{base}/", 20, ledger_proc)
                after_a1 = _total_of(base)
                c.kill_ledger.append({"label": "a1-post-submit", "before": before_a1,
                                      "after": after_a1, "restarted": up is not None})
                st_d, drafts, _r, _h = _get_auth(f"{base}/api/drafts", admin)
                state = None
                for row in ((drafts or {}).get("data") or
                            (drafts if isinstance(drafts, list) else [])):
                    if isinstance(row, dict) and row.get("id") == did:
                        state = row.get("state")
                wf["a1_state_after_restart"] = state
                c.notif_partition = _probe("load", base, env=probe_env, timeout=90)
                if _need("arm_hold_create"):
                    try:
                        V.arm_hold_create()
                    except Exception:
                        c.harness_missing.append("arm_hold_create:failed")
                st_a = None

                def _approve():
                    nonlocal st_a
                    st_a, _b, _r2, _h2 = _post_auth(
                        f"{base}/api/drafts/{did}/approve", {}, checker, timeout=20)
                at = threading.Thread(target=_approve)
                at.start()
                at.join(timeout=2.5)                    # inside the held send window
                before_a2 = _total_of(base) or before_a1
                _kill(ledger_proc)                      # A2
                at.join(timeout=5)
                wf["approve_status"] = st_a
                if hasattr(V or object(), "release_hold_create"):
                    try:
                        V.release_hold_create()
                    except Exception:
                        pass
                ledger_proc = _spawn_ledgerd(c, env, db_dir, lport, nport, vendor_url,
                                             tokens_file)
                up2 = _wait_status(f"{base}/", 20, ledger_proc)
                after_a2 = _total_of(base)
                c.kill_ledger.append({"label": "a2-post-approve", "before": before_a2,
                                      "after": after_a2, "restarted": up2 is not None})
                state2 = None
                deadline = time.time() + 45
                while time.time() < deadline:
                    _s, drafts2, _r, _h = _get_auth(f"{base}/api/drafts", admin)
                    for row in ((drafts2 or {}).get("data") or
                                (drafts2 if isinstance(drafts2, list) else [])):
                        if isinstance(row, dict) and row.get("id") == did:
                            state2 = row.get("state")
                    if state2 in ("approved", "sent"):
                        break
                    time.sleep(2.0)
                wf["a2_state_after_restart"] = state2
                if _need("vendor_payment_count"):
                    try:
                        # vendor_payment_count() is the TOTAL collection size; at this
                        # instant the only legitimate creates are the midwalk (+1) and
                        # F3's one send — anything beyond is a fresh-key dupe.
                        wf["vendor_payments_for_f3"] = max(
                            0, int(V.vendor_payment_count()) - (N_FROZEN + 1))
                    except Exception:
                        pass
                if V is not None and hasattr(V, "idempotency_replays"):
                    try:
                        wf["idempotency_replays"] = V.idempotency_replays()
                    except Exception:
                        pass
                wf["writes_fast"] = create_ms < 2000 and wf.get("submit_ms", 9e9) < 2000
        c.workflow = wf
        ob = c.outbox_during_partition or {}
        b7["ledger_alive"] = ledger_proc is not None and ledger_proc.poll() is None
        b7["writes_fast"] = bool(wf.get("writes_fast"))
        b7["status_down"] = ob.get("notifier") == "down" and (ob.get("pending") or 0) > 0
        b7["ui_degraded"] = _notif_state(c.notif_partition or {}) == "degraded"
        c.b6_result = {"pending_before_kill": (ob.get("pending") or 0) > 0}

        # heal
        mark("heal", False)
        heal_t0 = time.time()
        notifier_proc = _spawn_notifierd(c, env, db_dir, nport)
        _wait_status(f"{nbase}/health", 15, notifier_proc)
        live_at = None
        deadline = time.time() + 30
        while time.time() < deadline:
            st_p, _bp, _r, _h = _get(f"{base}/api/notifications?limit=1", timeout=3)
            _s, ob2, _r, _h = _get(f"{base}/api/outbox/status", timeout=3)
            if st_p == 200 and isinstance(ob2, dict) and ob2.get("notifier") == "up":
                live_at = time.time() - heal_t0
                c.outbox_after_heal = ob2
                break
            time.sleep(0.5)
        c.heal_live_secs = round(live_at, 1) if live_at is not None else None
        time.sleep(3.0)                                # let the relay drain
        c.notif_heal = _probe("load", base, env=probe_env, timeout=90)
        _s, pr2, _r, _h = _get(f"{nbase}/notify/processed?after=0", timeout=8)
        c.notifier_processed = ((pr2 or {}).get("processed")
                                if isinstance(pr2, dict) else []) or []
        _s, nn, _r, _h = _get(f"{nbase}/notify/notifications?limit=200", timeout=8)
        c.notifier_notifications = nn if isinstance(nn, dict) else {}
        _s, nh2, _r, _h = _get(f"{nbase}/health", timeout=5)
        c.notifier_health_final = nh2 if isinstance(nh2, dict) else {}
        seqs_after = [p.get("seq") for p in c.notifier_processed
                      if isinstance(p, dict) and isinstance(p.get("seq"), int)]
        b7["catchup_in_order"] = seqs_after == sorted(seqs_after) and bool(seqs_after)
        b7["ui_live_5s"] = (c.heal_live_secs is not None and c.heal_live_secs <= 5.0
                            and _notif_state(c.notif_heal or {}) in ("live", None))
        c.b7_result = b7
        pre_seqs = {p.get("seq") for p in (c.notifier_processed_prekill or [])
                    if isinstance(p, dict)}
        c.events = _fetch_events(base, admin)          # refresh: workflow events included
        crossing = [e.get("seq") for e in c.events
                    if e.get("type") in OUTBOX_TYPES and isinstance(e.get("seq"), int)]
        c._expected_crossing_seqs = crossing
        have = set(seqs_after)
        c.b6_result.update({
            "resumed": bool(have - pre_seqs),
            "exactly_once": len(seqs_after) == len(set(seqs_after)),
            "none_lost": all(s in have for s in crossing) if crossing else False,
        })

        # ── 7: probes ─────────────────────────────────────────────────────────────────────────
        # The DOM total a correct app claims at probe time is N + every create committed so
        # far (midwalk + F3 have round-tripped by now; F1 lands during the flow probe).
        if V is not None and hasattr(V, "vendor_payment_count"):
            try:
                c.expected_total_at_probe = int(V.vendor_payment_count())
            except Exception:
                pass
        c.probes["load"] = _probe("load", base, env=probe_env)
        c.probes["flow"] = _probe("flow", base, env=probe_env, timeout=180)
        st_f2, drafts_f2, _r, _h = _get_auth(f"{base}/api/drafts?state=rejected", admin)
        f2_id = None
        for row in ((drafts_f2 or {}).get("data") or
                    (drafts_f2 if isinstance(drafts_f2, list) else [])):
            if isinstance(row, dict) and row.get("id") and row.get("id") != c.draft_ids.get("F3"):
                f2_id = row.get("id")
                break
        if f2_id:
            st_rs, _b, _r, _h = _post_auth(f"{base}/api/drafts/{f2_id}/submit", {}, maker)
            c.d2_resubmit = {"draft_id": f2_id, "status": st_rs}
        mark("sync4")
        c.probes["sync"] = _probe("sync", base, env=probe_env)
        if sch is not None:
            targets = [m["payment_id"] for m in sch.race_mutations[sch.race_pages[0]]]
            snap = _snapshot_rows(base, targets)
            final_truth, _v = _committed_truth(c)
            stale = any(isinstance((snap.get(p) or {}).get("version"), int)
                        and final_truth and p in final_truth
                        and snap[p]["version"] < final_truth[p]["version"]
                        for p in targets if p in snap)
            c.b5_result.update({"propagated": not stale, "stale_served_as_fresh": stale})

        # vendor-truth creates -> expectation pack -> the 3D probes
        created_rows: List[Dict] = []
        if _need("commit_ledger"):
            try:
                c.vendor_committed = V.commit_ledger() or {}
            except Exception:
                pass
        if V is not None and hasattr(V, "commit_versions"):
            try:
                c.vendor_versions = set(map(tuple, V.commit_versions() or []))
            except Exception:
                pass
        if fx is not None:
            idx = fx.index()
            for pid, row in sorted((c.vendor_committed or {}).items()):
                if pid in idx or not isinstance(row, dict):
                    continue
                created_rows.append({
                    "id": pid, "amount_minor": row.get("amount_minor"),
                    "currency": row.get("currency"), "status": row.get("status", "pending"),
                    "day": row.get("day"), "_day": row.get("day"),
                    "created_at": row.get("created_at"), "version": row.get("version", 1)})
            if not created_rows:
                mc = sch.midwalk_create
                slot = fx.value_slots[mc["value_slot"]]
                created_rows.append({"id": "__midwalk__", "amount_minor": mc["amount_minor"],
                                     "currency": mc["currency"], "status": "pending",
                                     "day": slot["day"], "_day": slot["day"],
                                     "created_at": slot["created_at"], "version": 1})
            c.app_created_count = len(created_rows)
            c.expected_total_final = N_FROZEN + len(created_rows)
            # §3.1: the graded page load happens AFTER these creates were committed, so the
            # serve order interleaves them chronologically — the vendor's own created_at/day
            # place them; no slot attribution can drift.
            try:
                c.viz_digest_expected = fx.expected_scene_digest_at_load(created_rows)
            except Exception:
                c.viz_digest_expected = fx.expected_scene_digest()
            want_cur = {cur: dict(v) for cur, v in fx.EXPECTED_BY_CURRENCY.items()}
            for r_ in created_rows:
                cur = r_.get("currency")
                if cur in want_cur and isinstance(r_.get("amount_minor"), int):
                    want_cur[cur]["count"] += 1
                    want_cur[cur]["total_minor"] += r_["amount_minor"]
            c.expected_final_bycur = want_cur
            d1 = getattr(fx, "d1_target", None) or {}
            d1_target = d1.get("payment_id") or fx.payments[0]["id"]
            _write_expect_pack(c, expect_path, created_rows, d1_target)
            # ONE viz session carries the coast + stream evidence; the D1 mutation fires
            # ~mid-viz, once the probe has armed the D1 brush and while the page's SSE
            # observer is provably listening (never around a probe that is not running).
            viz_t = _ProbeThread("viz", base, env=probe_env)
            viz_t.start()
            d1_deadline = time.time() + 110
            while time.time() < d1_deadline and viz_t.is_alive():
                time.sleep(1.0)
            if V is not None and hasattr(V, "fire_d1_mutation"):
                try:
                    V.fire_d1_mutation()
                except Exception:
                    c.harness_missing.append("fire_d1_mutation:failed")
            else:
                c.harness_missing.append("fire_d1_mutation")
            viz_t.join(timeout=260)
            c.probes["viz"] = viz_t.result
        else:
            c.probes["viz"] = {"_probe_error": "expectation pack unavailable "
                               "(fixtures missing) — F17 refusal"}

        # error journey: the spec's error state is BACKEND unreachable or erroring — the
        # probe blocks the app's own /api from the page (--block-api); vendor-down alone
        # leaves a spec-correct page serving local data with no error text.
        c.probes["error"] = _probe("error", base, "--block-api", env=probe_env)

        # ── 8: perf ───────────────────────────────────────────────────────────────────────────
        c.api_lat = {
            "payments": perf_probe.measure_endpoint(base, "/api/payments?limit=50"),
            "summary": perf_probe.measure_endpoint(base, "/api/summary"),
        }
        c.under_stream = _measure_under_stream(base, V)

        # ── 9: quiescence + final reads ───────────────────────────────────────────────────────
        mark("final", False)
        if V is not None and hasattr(V, "quiescent"):
            deadline = time.time() + 40
            while time.time() < deadline:
                try:
                    if V.quiescent():
                        c.quiesced = True
                        break
                except Exception:
                    break
                time.sleep(1.0)
        else:
            c.harness_missing.append("quiescent")
            time.sleep(3.0)
        # re-capture the vendor truth AFTER the last vendor-side mutations (the D1 flip and
        # the under-stream burst) so L1/L4 grade against the versions that now exist.
        if V is not None and hasattr(V, "commit_ledger"):
            try:
                c.vendor_committed = V.commit_ledger() or {}
            except Exception:
                pass
        if V is not None and hasattr(V, "commit_versions"):
            try:
                c.vendor_versions = set(map(tuple, V.commit_versions() or []))
            except Exception:
                pass
        _s, c.summary_final, _r, _h = _get(f"{base}/api/summary")
        c.final_total = _total_of(base)
        target_pids: List[str] = []
        if fx is not None:
            target_pids = [m["payment_id"] for k in sch.race_pages
                           for m in sch.race_mutations[k]]
            target_pids += [sch.refund_txn["payment_id"], sch.ooo_pair["payment_id"],
                            sch.forged_event["payment_id"]]
            target_pids += [x["payment_id"] for x in sch.partition_commits
                            if x.get("kind") == "vendor_flip"]
            target_pids += [r_["id"] for r_ in created_rows if r_["id"] != "__midwalk__"]
        c.final_rows = _snapshot_rows(base, target_pids)
        missing_creates: Dict[str, int] = {}
        for r_ in created_rows:
            pid = r_["id"]
            if pid == "__midwalk__" or not isinstance(r_.get("amount_minor"), int):
                continue
            if pid not in c.final_rows:
                cur = r_.get("currency")
                missing_creates[cur] = missing_creates.get(cur, 0) + r_["amount_minor"]
        dupe_minor: Dict[str, int] = {}
        extra_pay = (c.workflow or {}).get("vendor_payments_for_f3")
        if isinstance(extra_pay, int) and extra_pay > 1 and f3 is not None:
            dupe_minor[f3["currency"]] = (extra_pay - 1) * f3["amount_minor"]
        c._attribution = {"dupe_minor": dupe_minor, "loss_minor": missing_creates}
        c.events = _fetch_events(base, admin)
        c._expected_crossing_seqs = [e.get("seq") for e in c.events
                                     if e.get("type") in OUTBOX_TYPES
                                     and isinstance(e.get("seq"), int)]
        _s, pr3, _r, _h = _get(f"{nbase}/notify/processed?after=0", timeout=8)
        if isinstance(pr3, dict) and pr3.get("processed"):
            c.notifier_processed = pr3["processed"]
        _s, nn2, _r, _h = _get(f"{nbase}/notify/notifications?limit=200", timeout=8)
        if isinstance(nn2, dict):
            c.notifier_notifications = nn2

        # empty instance: vendor down => the pre-sync/D3 window is probeable at leisure
        edb = c.root / "sb7-empty-db"
        shutil.rmtree(edb, ignore_errors=True)
        edb.mkdir(exist_ok=True)
        elp, enp = _free_port(), _free_port()
        eledger = enotif = None
        vendor_held = False
        try:
            if V is not None and hasattr(V, "set_down"):
                try:
                    V.set_down(True)
                    vendor_held = True
                except Exception:
                    pass
            enotif = _spawn_notifierd(c, env, edb, enp)
            eledger = _spawn_ledgerd(c, env, edb, elp, enp, vendor_url, tokens_file)
            if _wait_status(f"http://127.0.0.1:{elp}/", 15, eledger) is not None:
                # the probe's boot scenario carries the empty/D3 evidence (preSyncState);
                # gather stores it under the "empty" evidence key
                c.probes["empty"] = _probe("boot", f"http://127.0.0.1:{elp}",
                                           env=probe_env)
            else:
                c.probes["empty"] = {"_probe_error": "empty-instance never served"}
        finally:
            _kill(eledger)
            _kill(enotif)
            if vendor_held:
                try:
                    V.set_down(False)
                except Exception:
                    pass
            shutil.rmtree(edb, ignore_errors=True)

        # ── 10: combined entrypoint smoke (vendor held down — no second full walk) ────────────
        mark("combined", False)
        cdb = c.root / "sb7-combined-db"
        shutil.rmtree(cdb, ignore_errors=True)
        cdb.mkdir(exist_ok=True)
        clp, cnp = _free_port(), _free_port()
        cproc = None
        vendor_held = False
        try:
            if V is not None and hasattr(V, "set_down"):
                try:
                    V.set_down(True)
                    vendor_held = True
                except Exception:
                    pass
            cproc = subprocess.Popen(
                [sys.executable, "-m", "app", "--db-dir", str(cdb),
                 "--ledger-port", str(clp), "--notifier-port", str(cnp),
                 "--vendor", vendor_url, "--tokens-file", str(tokens_file)],
                cwd=c.root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, start_new_session=True)
            booted = _wait_status(f"http://127.0.0.1:{clp}/", 15, cproc) is not None
            st_l, _b, _r, _h = _get(f"http://127.0.0.1:{clp}/api/payments?limit=1",
                                    timeout=4)
            st_n, _b, _r, _h = _get(f"http://127.0.0.1:{cnp}/health", timeout=4)
            c.combined_boot = {"booted": booted, "ledger_ok": st_l == 200,
                               "notifier_ok": st_n == 200,
                               "detail": f"boot={booted} ledger={st_l} notifier={st_n}"}
        except Exception as e:
            c.combined_boot = {"booted": False, "ledger_ok": False, "notifier_ok": False,
                               "detail": f"{type(e).__name__}: {e}"[:120]}
        finally:
            _kill(cproc)
            if vendor_held:
                try:
                    V.set_down(False)
                except Exception:
                    pass
            shutil.rmtree(cdb, ignore_errors=True)
    finally:
        if reader is not None:
            reader.stop()
            reader.join(timeout=5)
            c.read_stream = reader.samples
        _kill(ledger_proc)
        _kill(notifier_proc)

    if trace_path.is_file():
        c.trace = [json.loads(ln) for ln in trace_path.read_text().splitlines()
                   if ln.strip()]
        _analyze_trace(c)
    _conclude_read_stream(c)
    return c


# ── CLI: hermetic scoring; --reference is the freeze gate ────────────────────────────────────

def _reference_gate(result: Dict, ctx: Optional[Ctx] = None) -> List[str]:
    """DESIGN §5.8: golden aces every non-CAL check (>= 0.95 house sweep), every CRITICAL is
    exactly 1.0, no probe-unavailable rows, the E gate opens, and every coverage-ledger row
    produced non-vacuous causal evidence. Returns failures (empty = pass)."""
    fails: List[str] = []
    for r in result["checks"]:
        if r["check"] in CRITICAL_CHECKS and not r.get("unavailable"):
            if r["score"] < 1.0 - 1e-9:
                fails.append(f"{r['check']}: CRITICAL {r['score']:.4f} != 1.0 — criticals "
                             "are facts a correct app achieves, never budgets (§5.4)")
            continue
        if r["check"] in CALIBRATION_OWNED or r["check"] in DIAGNOSTIC:
            continue
        if r.get("unavailable"):
            fails.append(f"{r['check']}: PROBE UNAVAILABLE — harness defect blocks freeze")
        elif r["score"] < 0.95:
            fails.append(f"{r['check']}: {r['score']:.2f} < 0.95 — {str(r.get('detail'))[:80]}")
    if not result.get("excellence_gate"):
        fails.append("E gate FAILED — a gate the reference cannot pass is locked shut for "
                     "harness reasons")
    if ctx is not None:
        for anchor, (scenario, key, chk) in AFFORDANCE_ROSTER.items():
            emit = _pv(ctx, scenario)
            if _pe(emit) or not emit.get(key):
                fails.append(f"coverage ledger: {anchor} -> {scenario}.{key} produced no "
                             f"non-vacuous causal evidence (backs {chk})")
    return fails


def _draw_seed() -> str:
    return os.urandom(8).hex()


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Score a Meridian-v3 payments-console tree "
                                             "under sb-7.")
    ap.add_argument("--tree", type=Path, required=True, help="root of the built tree")
    ap.add_argument("--port", type=int, default=8899, help="vendor mock port (8899+ policy)")
    ap.add_argument("--seed", type=str, default=None,
                    help="16-hex fixture seed (omitted: drawn fresh and stamped into the "
                         "verdict; re-scoring an archive replays its recorded seed)")
    ap.add_argument("--reference", action="store_true",
                    help="freeze gate: refuse freeze-marked output unless the reference "
                         "aces every non-CAL check, every critical is exactly 1.0, the "
                         "severity selftest passes, and the coverage ledger is non-vacuous")
    ap.add_argument("--json-out", type=Path, help="write the verdict JSON here")
    args = ap.parse_args()
    args.tree = args.tree.resolve()

    fails = severity_selftest()
    if fails:
        print("REFUSED: severity_selftest() failed — the scorer's own gradient is "
              "inverted; nothing may be graded until it holds:", file=sys.stderr)
        for f in fails:
            print(f"  ✗ {f}", file=sys.stderr)
        return 4

    V = _vendor()
    if V is None or not hasattr(V, "serve"):
        print("REFUSED: vendor_service_v3.serve is not available — the sb-7 mock is a "
              "sibling deliverable and scoring without it would grade against nothing.",
              file=sys.stderr)
        return 2
    if _fixtures() is None or _schedule_mod() is None:
        print("REFUSED: fixtures_v3/schedule_sb7 are not importable — every expectation "
              "derives from them (§5.5).", file=sys.stderr)
        return 2

    seed = args.seed or _draw_seed()
    seed = seed.lower()
    if not (len(seed) == 16 and all(ch in "0123456789abcdef" for ch in seed)):
        print(f"REFUSED: --seed must be 16 hex chars (got {seed!r})", file=sys.stderr)
        return 2

    trace = args.tree / "vendor-trace-sb7.jsonl"
    # Hermetic wipes (F895-F897 at the CLI): a graded db, tokens file, expectation pack,
    # shots dir or trace left by a PREVIOUS run poisons boot resume, ETag cheapness, and
    # every probe expectation. Every invocation starts as the first one did.
    trace.unlink(missing_ok=True)
    for leftover in ("sb7-tokens.json", "sb7-expect.json"):
        (args.tree / leftover).unlink(missing_ok=True)
    for leftover_dir in ("graded-sb7-db", "sb7-empty-db", "sb7-combined-db", "sb7-shots"):
        shutil.rmtree(args.tree / leftover_dir, ignore_errors=True)

    try:
        server = V.serve(args.port, trace, seed=seed)
    except TypeError:
        server = V.serve(args.port, trace)
    docs_status = None
    if args.reference:
        docs_status = _get(f"http://127.0.0.1:{args.port}"
                           f"{getattr(V, 'DOCS_PATH', '/v3/docs')}", timeout=5)[0]
    try:
        ctx = gather(args.tree, args.port, args.tree / "graded-sb7-db", trace,
                     mark_phase=getattr(V, "mark_phase", None), seed=seed)
    finally:
        try:
            server.shutdown()
        except Exception:
            pass
    result = evaluate(ctx)
    print(format_report(result, f"sb-7 {args.tree.name}"))
    if args.json_out:
        args.json_out.write_text(json.dumps(result, indent=2, default=str))

    if args.reference:
        gate_fails = _reference_gate(result, ctx)
        gate_fails.extend(severity_selftest())
        if docs_status != 200:
            gate_fails.append(
                f"vendor_docs_v3.md missing or not served (GET /v3/docs -> {docs_status}) "
                "— the spec's first instruction dead-ends without the docs artifact")
        if gate_fails:
            print("\nREFERENCE GATE: REFUSED — freeze is blocked. Harness defects, not "
                  "app defects, until proven otherwise:", file=sys.stderr)
            for f in gate_fails:
                print(f"  ✗ {f}", file=sys.stderr)
            return 3
        marker = {"reference_pass": True, "scorer_version": SCORER_VERSION,
                  "score": result["score"], "fixture_seed": seed,
                  "date": time.strftime("%Y-%m-%d"),
                  "thresholds_calibrated": CALIBRATED,
                  "thresholds_sha256": hashlib.sha256(
                      THRESHOLDS_FILE.read_bytes()).hexdigest()
                  if THRESHOLDS_FILE.is_file() else None}
        (args.tree / "sb7-reference-pass.json").write_text(json.dumps(marker, indent=2))
        print("\nREFERENCE GATE: PASS — sb7-reference-pass.json written; CAL rungs may "
              "now be fitted from this tree's worst-of-5 distribution (§5.6).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
