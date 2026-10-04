"""forge-1.0 — grade a built Scope Ledger Forge app (forge/DESIGN.md §8–§9) by RUNNING it.

    score_forge.py --tree <run tree> --seed <fixture_seed> --json-out v.json [--reference] [--runtime shim]

Mirrors the SB scorer machinery (score_sb7.py) with forge's own registry, nothing imported from it:
`@check(name, tier, pre=…)` registry with PRECONDITIONS (G5: a row whose surface was never exercised scores 0
as `vacuous_root`, priced 0, no multiplier of its own — an idle app cannot collect "nothing went wrong"
credit), `compose_from_rows` (`earned = (0.88·inner + 0.12·gate·e_mean) × critical multiplier`, floor 0.6),
the 7-member CRITICAL registry with per-critical severity transforms, ROOT_BLOCKS attribution + multiplier
dedup, the four ADMISSION bands (`final = min(earned, ceilings)`; passing awards nothing), the proportional
EXCELLENCE gate, `severity_selftest()` (wired into every run and into `--reference`), the `--reference`
freeze gate, and the calibration pin (`CALIB_SHA256` over forge-thresholds.json).

Evidence is interface I5: `forge_probe.mjs` drives WP1's emulator (I2) through §8.7's scoring sequence and
writes `forge-observations.json`; this module grades it against forge_oracle.py on the run's own pack (I1).
`evaluate(ctx)` is PURE over (observations, pack): the unit tests and the severity selftest push synthetic
observations through the real checks.

I5 — forge-observations.json, the fields this scorer reads (the probe normalises WP1's logs into it):
  manifest (parsed manifest.yml | null), manifestError, kit {pins {pkg: version}, lockSha256, wrapperSha256},
  lint {runs: [{counts {errors, warnings}, problems [{sev, message, file, line, linter}], stageReached,
        stagesTotal, crashed}]}  (two runs; must be identical),
  build {functions: [{key, handler, bundled, loaded, exported, error, forgePackages {pkg: version},
        outsideKit [paths]}]},
  phases {backfill|live|heal|rerun: {calls [call], invocations [inv], deliveries [delivery], kvsAfter
        {entities {name: [{key, value}]}, keys [{key, value}]}, events [{changelogId, slot, duplicate,
        triggerInvocations [inv ids]}] (live only)}},
    call = {t (virtual s), inv, kind (trigger|consumer|scheduled|resolver|action|ui), moduleKey, provider
            (app|user|none), service (jira|kvs|queue|egress), method, path, body, status, response,
            scopes {classic [..], granular [..]} (jira: the endpoint's OAuth2 alternatives), fault (id of the
            scripted fault that answered), earlyRetry (fault id when retried inside its window), limitError}
    inv  = {inv, kind, moduleKey, functionKey, t0, t1, ok, threw, error, errorName, timedOut, retryAfter}
    delivery = {eventId, inv, attempt, result (ok|retry|throw|timeout), retryAfter, t}
  rovo {calls: [{as, label (sprint|unknown|missing), sprintId, threw, result, error}]},
  comments [{t, issueKey, provider, accountId, status, body, fault}]   (every comment POST the site saw),
  ui {surfaces [surface], widget {noConfig, views, secondInstance}, edit {options, picks, reopen},
      sprintAction [render], notStarted, invokeResponses [{surface, functionKey, response, threw, undefined}]},
  harnessMissing [str], sectionErrors {section: reason}, shots [paths].
"""
from __future__ import annotations

import argparse
import copy
from decimal import Decimal, InvalidOperation
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
from typing import Callable, Dict, Iterable, List, Optional, Tuple
import zlib

import forge_oracle as fo

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
PROBE_SCRIPT = HERE / 'forge_probe.mjs'
SPEC = 'forge/public/spec-build-forge.md'
CONTRACT = 'forge/public/FORGE-CONTRACT.md'

# ── calibration-owned thresholds ─────────────────────────────────────────────────────────────

THRESHOLDS_FILE = HERE / 'forge-thresholds.json'
CALIB_SHA256 = 'c4a6cbb8b2e569fddc69f5d8d2c129d064f3aed9170e9b4fbc55a12ad1dbf625'  # refrozen 2026-10-04: golden x5 on the stringency scoring site (forge-thresholds.json calibration.seeds)


def _load_thresholds() -> Dict:
    raw = THRESHOLDS_FILE.read_bytes()
    try:
        data = json.loads(raw)
    except ValueError:
        raise SystemExit(f'forge-thresholds.json is unparsable — refusing to score: {THRESHOLDS_FILE}')
    if data.get('calibrated'):
        digest = hashlib.sha256(raw).hexdigest()
        if CALIB_SHA256 == 'TBD-AT-FREEZE' or digest != CALIB_SHA256:
            raise SystemExit('forge-thresholds.json claims calibrated=true but its sha256 does not match the pin '
                             f'in score_forge.py ({digest[:16]}… vs {CALIB_SHA256[:16]}…) — refusing to score.')
    for name in ('critical_multiplier_floor', 'economy_rungs', 'event_economy_top', 'ui_round_trip_rungs',
                 'idempotent_rerun_rungs',
                 'contrast_min', 'chart_tolerance_px', 'color_tolerance'):
        if name not in data:
            raise SystemExit(f'forge-thresholds.json lacks {name!r} — refusing to score on a silent default')
    return data


TH = _load_thresholds()
CALIBRATED = bool(TH.get('calibrated'))
VERSION = 'forge-1.0' if CALIBRATED else 'forge-1.0-rc'
UNCALIBRATED_BANNER = ('FORGE-1.0 UNCALIBRATED — forge-thresholds.json carries rc defaults (no CALIB pin); '
                       'scores are rc-grade (forge-1.0-rc), never board-grade.')

# ── weights (DESIGN §8.1, asserted) ──────────────────────────────────────────────────────────

TIER_WEIGHT = {'L': .08, 'K': .10, 'T': .16, 'R': .14, 'S': .08, 'B': .12, 'U': .16, 'V': .08, 'A': .08}
E_WEIGHT = 0.12
INNER_WEIGHT = 1.0 - E_WEIGHT
assert abs(sum(TIER_WEIGHT.values()) - 1.0) < 1e-9
TIER_ORDER = ('L', 'K', 'T', 'R', 'S', 'B', 'U', 'V', 'A', 'E')

DIAGNOSTIC = {'u_widget_loads'}
CALIBRATION_OWNED = {'e_reconcile_economy', 'e_event_economy', 'e_ui_round_trips'}

CRITICAL_CHECKS = {
    'l_deployable': 'deploy blocked — lint errors',
    'l_bundles_load': 'crash — the app does not run',
    't_no_double_count': 'wrong numbers — a change counted twice',
    'r_backfill_complete': 'data loss — changes silently missing',
    'b_no_permission_leak': 'data leak — hidden issues shown to a person',
    'b_comment_exactly_once': "duplicate side effect on a customer's Jira",
    'u_widget_loads': 'dead primary flow — no data on the dashboard',
}
assert not set(CRITICAL_CHECKS) & CALIBRATION_OWNED

# ADMISSION (DESIGN §8.5): the four maxima the prompt's "Score bands" states in words (parity-tested).
ADMISSION_BANDS = (
    (0.499, 'deployable', ('l_deployable', 'l_bundles_load')),
    (0.699, 'working ledger', ('u_widget_loads', 't_event_rows', 'r_backfill_complete', 's_storage_scope',
                               's_entity_index_used')),
    (0.799, 'current platform, complete surfaces', ('k_dashboard_widget', 'k_widget_edit_bridge', 'k_rovo_skill',
                                                    'u_widget_edit_config', 'u_ledger_table', 'a_action_result',
                                                    'v_theme_tokens', 'v_dark_mode')),
    # The prompt's list, word by word: "Any duplicate, ordering, rate-limit, pagination, permission, policy, console,
    # LLM or realtime defect". Ordering is the table's sort and the index's change-time order as well as delivery
    # order; permission is every per-person outcome (the hidden count, the Rovo action's per-person list) and the
    # scopes the app asks for; LLM is the model the app names as well as the explanation (2026-10-03 stringency).
    (0.899, 'production robustness', ('t_no_double_count', 't_out_of_order', 't_retry_after_honoured',
                                      'r_heal_dropped', 'r_rate_limit', 'r_pagination', 'b_no_permission_leak',
                                      'b_comment_exactly_once', 'b_realtime_payload_clean', 'u_llm_explain',
                                      'u_widget_live', 'v_csp_clean', 'v_console_clean',
                                      'u_ledger_sort', 's_index_order', 'b_hidden_count', 'a_action_permissions',
                                      'l_scopes', 'k_llm_model_current')),
)
# The graded band (DESIGN §8.5, 2006de559): with n of its rows failing the ceiling is max(floor, top - step·(n-1)).
# Single source: admit() reads it, and leanzero.net's sync (scripts/sync-forge-public.py) re-derives the caps from it.
GRADED_BAND = ('production robustness', 0.899, 0.03, 0.799)
assert GRADED_BAND[0] == ADMISSION_BANDS[-1][1] and GRADED_BAND[1] == ADMISSION_BANDS[-1][0]
assert GRADED_BAND[3] == ADMISSION_BANDS[-2][0]   # never undercuts the band above it


# Owner rule 2026-10-03 22:4x: a final never sits exactly on a band cap ("two models at exactly 0.899 is a big red
# flag"). A capped run keeps its own earned gradient below the cap: final = min(earned, cap - BAND_PULL·(1 - earned)).
# Continuous and monotone in earned, equal to the cap only at earned = 1, and a no-op when nothing caps (cap 1.0).
# The largest pull below a cap, BAND_PULL·(1 - cap) at earned = cap (0.00505 at 0.899), stays under the graded
# band's 0.03 step, so one more band defect always costs more than any earned difference.
BAND_PULL = 0.05   # policy ratio: the share of the earned shortfall a capped final keeps (owner rule above)
assert BAND_PULL * (1 - ADMISSION_BANDS[0][0]) < GRADED_BAND[2]


def capped_final(earned: float, ceiling: float) -> float:
    return min(earned, ceiling - BAND_PULL * (1.0 - earned))


def published_score(final: float, ceiling: float) -> float:
    """Four decimals. A final strictly below its cap is never printed AS the cap: a near-perfect capped run (earned
    0.9999, pull 0.000005) would round back onto 0.899 — the tie the owner rule forbids — so it is truncated instead."""
    shown = round(final, 4)
    if ceiling < 1.0 and final < ceiling - 1e-12 and shown >= ceiling:
        shown = math.floor(final * 1e4) / 1e4
    return shown


def band_ceiling(limit: float, label: str, failed: int) -> float:
    if label == GRADED_BAND[0]:
        _l, top, step, floor = GRADED_BAND
        return round(max(floor, top - step * (failed - 1)), 4)
    return limit

EXCELLENCE_VALUED = ('t_event_rows', 'r_heal_dropped', 'u_widget_numbers', 'u_ledger_table', 'b_comment_adf_as_user')
EXCELLENCE_BINARY = ('v_console_clean', 'v_csp_clean', 'l_lint_warnings')

# Platform facts the K rows grade (DESIGN §8.2 k_current_apis; RESEARCH §2 runtime list, fetched 2026-10-02).
CURRENT_RUNTIMES = {'nodejs22.x', 'nodejs24.x'}
BACKGROUND_KINDS = ('trigger', 'consumer', 'scheduled')
LEDGER_SOURCES = ('event', 'reconcile')

# ── registry ─────────────────────────────────────────────────────────────────────────────────

CHECKS: List[tuple] = []


def check(name: str, tier: str, pre: Optional[Callable] = None, needs: Tuple[str, ...] = ()) -> Callable:
    """`pre(c) -> (ok, what)` is the row's precondition (DESIGN §8.2 column); `needs` names the I5 sections
    whose harness failure makes the row unavailable (never an app zero)."""
    def deco(fn):
        CHECKS.append((name, tier, fn, pre, needs))
        return fn
    return deco


def g(score: float, detail: str, consequence: str = '', parts: Optional[Dict] = None) -> Dict:
    row = {'score': round(max(0.0, min(1.0, float(score))), 4), 'detail': detail[:400], 'consequence': consequence}
    if parts:
        row['parts'] = parts
    return row


def unavail(reason: str) -> Dict:
    """F9: harness-attributable absence only. Excluded from the tier mean; the verdict becomes unpublishable."""
    return {'score': 0.0, 'detail': f'UNAVAILABLE: {reason}'[:300], 'consequence': 'harness failure, not app evidence',
            'unavailable': True}


def vacuous(what: str) -> Dict:
    """G5: the precondition was not met — the surface was never exercised, so a no-defect row cannot pay."""
    return {'score': 0.0, 'detail': f'vacuous — precondition unmet: {what}',
            'consequence': 'nothing exercised, nothing earned', 'parts': {'vacuous_root': f'precondition: {what}'}}


def absent(surface: str) -> Dict:
    """F9: a required app surface is missing — 0, severity 0 for a critical, never a refusal."""
    return {'score': 0.0, 'detail': f'required app surface absent: {surface}',
            'consequence': 'the graded surface is not implemented', 'parts': {'absent_surface': surface}}


# ── context ──────────────────────────────────────────────────────────────────────────────────

class Ctx:
    def __init__(self, root: Optional[Path], obs: Dict, pack: Dict, fixture_seed: Optional[str] = None,
                 dev_seed: Optional[str] = None, runtime: str = 'wrapper'):
        self.root = Path(root) if root else None
        self.obs = obs or {}
        self.pack = pack
        self.fixture_seed = fixture_seed
        self.dev_seed = dev_seed
        self.runtime = runtime
        self.oracle_error = None
        try:
            self.oracle = fo.Oracle(pack)
            # The site after the live-UI slot (DESIGN §8.7 step 8): what every surface opened after the live step shows.
            self.oracle_ui = fo.Oracle(pack, include_live_ui=True)
        except fo.PackDefect as error:
            self.oracle, self.oracle_ui, self.oracle_error = None, None, str(error)
        self.manifest = self.obs.get('manifest') if isinstance(self.obs.get('manifest'), dict) else None
        self.harness_missing = list(self.obs.get('harnessMissing') or [])
        self._row_cache: Dict = {}

    def oracle_for(self, after_live: bool):
        return self.oracle_ui if after_live else self.oracle

    # manifest
    def modules(self, mtype: str) -> List[Dict]:
        mods = (self.manifest or {}).get('modules') or {}
        entries = mods.get(mtype) if isinstance(mods, dict) else None
        return [e for e in entries if isinstance(e, dict)] if isinstance(entries, list) else []

    def module_types(self) -> List[str]:
        mods = (self.manifest or {}).get('modules') or {}
        return [t for t, v in mods.items() if t != 'function' and isinstance(v, list) and v] if isinstance(mods, dict) else []

    def functions(self) -> List[Dict]:
        return self.modules('function')

    def resources(self) -> List[Dict]:
        res = (self.manifest or {}).get('resources')
        return [r for r in res if isinstance(r, dict)] if isinstance(res, list) else []

    # evidence
    def section_error(self, *names: str) -> Optional[str]:
        errors = self.obs.get('sectionErrors') or {}
        for name in names:
            if errors.get(name):
                return f'{name}: {errors[name]}'
        return None

    def phase(self, name: str) -> Dict:
        return ((self.obs.get('phases') or {}).get(name)) or {}

    def calls(self, phases: Iterable[str] = ('backfill', 'live', 'heal', 'rerun'), service: Optional[str] = None,
              kinds: Optional[Iterable[str]] = None) -> List[Dict]:
        out = []
        kinds = set(kinds) if kinds else None
        for ph in phases:
            for call in self.phase(ph).get('calls') or []:
                if service and call.get('service') != service:
                    continue
                if kinds and call.get('kind') not in kinds:
                    continue
                out.append({**call, '_phase': ph})
        return out

    def all_calls(self, service: Optional[str] = None) -> List[Dict]:
        out = self.calls(service=service)
        for extra in ((self.obs.get('rovo') or {}).get('calls') or []):
            for call in extra.get('calls') or []:
                if not service or call.get('service') == service:
                    out.append({**call, '_phase': 'rovo'})
        for call in ((self.obs.get('ui') or {}).get('calls') or []):
            if not service or call.get('service') == service:
                out.append({**call, '_phase': 'ui'})
        return out

    def invocations(self, phases=('backfill', 'live', 'heal', 'rerun')) -> List[Dict]:
        return [{**inv, '_phase': ph} for ph in phases for inv in self.phase(ph).get('invocations') or []]

    def kvs_rows(self, phase: str) -> List[Tuple[set, Dict]]:
        snap = self.phase(phase).get('kvsAfter') or {}
        out = []
        for _entity, items in (snap.get('entities') or {}).items():
            for item in items or []:
                out.append((_tokens(item), item))
        for item in snap.get('keys') or []:
            out.append((_tokens(item), item))
        return out

    def ui(self) -> Dict:
        return self.obs.get('ui') or {}

    def surfaces(self) -> List[Dict]:
        return [s for s in self.ui().get('surfaces') or [] if isinstance(s, dict)]

    def rendered_surfaces(self) -> List[Dict]:
        return [s for s in self.surfaces() if s.get('rendered')]

    def sprint_renders(self) -> List[Dict]:
        return [r for r in self.ui().get('sprintAction') or [] if isinstance(r, dict)]

    def rovo_calls(self, label: Optional[str] = None, who: Optional[str] = None) -> List[Dict]:
        calls = (self.obs.get('rovo') or {}).get('calls') or []
        return [r for r in calls if (label is None or r.get('label') == label) and (who is None or r.get('as') == who)]

    def read_tree(self, rel: str) -> Optional[str]:
        if not self.root:
            return None
        path = self.root / rel
        try:
            return path.read_text(errors='replace') if path.is_file() else None
        except OSError:
            return None


# ── evidence helpers ─────────────────────────────────────────────────────────────────────────

def _leaf_text(value) -> Optional[str]:
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, float) and value.is_integer():
        return str(int(value))
    return str(value)


class Tokens(set):
    """A ledger row's scalar leaves (and its key's alphanumeric runs) for exact-leaf matching. `sprint` holds
    the leaves stored under a sprint-named field: when a row has one, the sprint is matched THERE, so a stored
    estimate of 8 never reads as sprint 8."""
    sprint: set


def _walk(value, under_sprint: bool, out: Tokens, depth: int = 0) -> None:
    if depth > 31:
        return
    if isinstance(value, dict):
        for k, v in value.items():
            _walk(v, under_sprint or 'sprint' in str(k).lower(), out, depth + 1)
    elif isinstance(value, (list, tuple)):
        for v in value:
            _walk(v, under_sprint, out, depth + 1)
    else:
        t = _leaf_text(value)
        if t is not None:
            out.add(t)
            if under_sprint:
                out.sprint.add(t)


def _tokens(item: Dict) -> Tokens:
    out = Tokens()
    out.sprint = set()
    _walk(item.get('value'), False, out)
    out |= {t for t in re.split(r'[^A-Za-z0-9]+', str(item.get('key') or '')) if t}
    return out


def _instants(tokens: set) -> set:
    out = set()
    for t in tokens:
        if re.fullmatch(r'\d{12,14}', t):
            out.add(fo.instant(int(t)))
        else:
            parsed = fo.instant(t)
            if parsed:
                out.add(parsed)
    return out


def rows_for(rows: List[Tuple[set, Dict]], change: fo.Change) -> List[Tuple[set, Dict]]:
    return [(tok, item) for tok, item in rows if change.change_id in tok
            and change.sprint_id in (getattr(tok, 'sprint', None) or tok)]


def row_correct(tokens: set, change: fo.Change, sources: Iterable[str]) -> bool:
    return (change.kind in tokens and (change.issue_id in tokens or change.issue_key in tokens)
            and (change.by in tokens or (change.by_name and change.by_name in tokens))
            and change.at in _instants(tokens) and any(s in tokens for s in sources))


def _num(text) -> Optional[Decimal]:
    if isinstance(text, bool) or text is None:
        return None
    if isinstance(text, (int, float)):
        return Decimal(str(text)) if math.isfinite(text) else None
    s = str(text).strip()
    if not re.fullmatch(r'-?\d+(\.\d+)?', s):
        return None
    try:
        return Decimal(s)
    except InvalidOperation:
        return None


def points_text_ok(text, expected: Decimal) -> bool:
    value = _num(text)
    return value is not None and value == expected


def creep_text_ok(text, expected: Optional[Decimal]) -> bool:
    s = str(text or '').strip()
    if expected is None:
        return s == '—'
    m = re.fullmatch(r'(\d+\.\d)%', s)
    return bool(m) and Decimal(m.group(1)) == expected


def metrics_ok(metrics: Dict, n: fo.SprintNumbers) -> int:
    metrics = metrics or {}
    return (points_text_ok(metrics.get('committed'), n.committed) + points_text_ok(metrics.get('added'), n.added)
            + points_text_ok(metrics.get('removed'), n.removed) + creep_text_ok(metrics.get('creep'), n.creep))


def json_number_ok(value, expected: Decimal) -> bool:
    return (isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)
            and Decimal(str(value)) == expected)


def creep_json_ok(value, expected: Optional[Decimal]) -> bool:
    if expected is None:
        return value is None
    return json_number_ok(value, expected)


def ledger_present(c: Ctx) -> bool:
    if not c.oracle:
        return False
    for ph in ('backfill', 'live', 'heal', 'rerun'):
        rows = c.kvs_rows(ph)
        if any(rows_for(rows, ch) for ch in c.oracle.changes('final')):
            return True
    return False


def relative_luminance(rgb) -> float:
    def channel(v):
        v = v / 255.0
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    r, gg, b = (channel(float(x)) for x in rgb[:3])
    return 0.2126 * r + 0.7152 * gg + 0.0722 * b


def contrast(a, b) -> float:
    la, lb = relative_luminance(a), relative_luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def rgb_close(a, b, tol: int) -> bool:
    return (isinstance(a, (list, tuple)) and isinstance(b, (list, tuple)) and len(a) >= 3 and len(b) >= 3
            and all(abs(int(x) - int(y)) <= tol for x, y in zip(a[:3], b[:3])))


def png_dominant(path: Path) -> Optional[Tuple[Tuple[int, int, int], float]]:
    """Dominant colour of an 8-bit non-interlaced RGB/RGBA PNG (Chromium screenshots) and its pixel share."""
    try:
        data = path.read_bytes()
    except OSError:
        return None
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        return None
    pos, idat, width, height, ctype = 8, b'', 0, 0, None
    while pos < len(data):
        length, kind = struct.unpack('>I4s', data[pos:pos + 8])
        chunk = data[pos + 8:pos + 8 + length]
        if kind == b'IHDR':
            width, height, depth, ctype, _c, _f, interlace = struct.unpack('>IIBBBBB', chunk)
            if depth != 8 or interlace or ctype not in (2, 6):
                return None
        elif kind == b'IDAT':
            idat += chunk
        elif kind == b'IEND':
            break
        pos += 12 + length
    bpp = 4 if ctype == 6 else 3
    raw = zlib.decompress(idat)
    stride = width * bpp
    prev = bytearray(stride)
    counts: Dict[Tuple[int, int, int], int] = {}
    offset = 0
    for _y in range(height):
        ftype = raw[offset]
        line = bytearray(raw[offset + 1:offset + 1 + stride])
        offset += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            cc = prev[i - bpp] if i >= bpp else 0
            if ftype == 1:
                line[i] = (line[i] + a) & 0xFF
            elif ftype == 2:
                line[i] = (line[i] + b) & 0xFF
            elif ftype == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 0xFF
            elif ftype == 4:
                p = a + b - cc
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - cc)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else cc)) & 0xFF
        for x in range(0, stride, bpp):
            key = (line[x], line[x + 1], line[x + 2])
            counts[key] = counts.get(key, 0) + 1
        prev = line
    if not counts:
        return None
    color, n = max(counts.items(), key=lambda kv: kv[1])
    return color, n / float(width * height)


def ladder(value: float, rungs) -> float:
    for score, bound in rungs:
        if value <= bound:
            return float(score)
    return 0.0


# ── preconditions ────────────────────────────────────────────────────────────────────────────

def pre_modules(c):
    return (bool(c.module_types()) or c.obs.get('manifestError') is not None, '>= 1 module declared')


def pre_functions(c):
    return (bool(c.functions()), '>= 1 function declared')


def pre_function_loads(c):
    fns = (c.obs.get('build') or {}).get('functions') or []
    return (any(f.get('loaded') and f.get('exported') for f in fns), '>= 1 function loads')


def pre_forge_import(c):
    fns = (c.obs.get('build') or {}).get('functions') or []
    return (any(f.get('forgePackages') for f in fns), '>= 1 bundle imports @forge/*')


def pre_resource_and_function(c):
    return (bool(c.resources()) and bool(c.functions()), '>= 1 resource and >= 1 function')


def pre_product_or_kvs(c):
    return (bool(c.all_calls('jira') or c.all_calls('kvs')), '>= 1 product or KVS call observed')


def pre_product_call(c):
    return (bool(c.all_calls('jira')), '>= 1 product call observed')


def pre_edit_rendered(c):
    return (any(s.get('kind') == 'widget-edit' and s.get('rendered') for s in c.surfaces()), 'edit surface rendered')


def pre_consumer(c):
    return (bool(c.modules('consumer')), 'a consumer exists')


def pre_trigger_push(c):
    return (bool(c.modules('trigger')) and bool(c.calls(service='queue')), 'a trigger exists and >= 1 push observed')


def pre_ledger_row(c):
    return (ledger_present(c), '>= 1 ledger row')


def _fault_fired(c, scope: str) -> bool:
    ids = {f['id'] for f in (c.pack.get('faults') or []) if (f.get('match') or {}).get('scope') == scope}
    return any(call.get('fault') in ids for call in c.all_calls('jira'))


def pre_consumer_fault(c):
    return (_fault_fired(c, 'consumer-of-change'), 'the consumer-path fault fired')


def pre_scheduled_fault(c):
    return (_fault_fired(c, 'scheduled-run'), 'the reconcile fault fired')


def pre_background_jira(c):
    return (bool(c.calls(service='jira', kinds=BACKGROUND_KINDS)), '>= 1 background Jira call')


def pre_paginated(c):
    legacy, _walks_all, paged = _paged_walks(c)
    return (bool(legacy or paged), '>= 1 read the site served in two or more pages')


def pre_scheduled_read(c):
    return (bool(c.calls(service='jira', kinds=('scheduled',))), '>= 1 scheduled-run Jira read')


def pre_kvs_call(c):
    return (bool(c.all_calls('kvs')), '>= 1 KVS call')


def pre_ledger_read(c):
    return (bool(_ledger_reads(c)), '>= 1 ledger read')


def pre_table_rows(c):
    return (any(r.get('rows') for r in c.sprint_renders()), 'table rows rendered')


def pre_kvs_write(c):
    return (any(_is_kvs_write(call) for call in c.all_calls('kvs')), '>= 1 KVS write')


def pre_invoke(c):
    return (bool(_invokes(c)), '>= 1 invoke observed')


def pre_person_list(c):
    listed = any(r.get('rows') for r in c.sprint_renders()) or bool(_llm_calls(c)) or any(
        isinstance(r.get('result'), dict) and r['result'].get('changes') for r in c.rovo_calls())
    return (listed, '>= 1 person-facing change list returned or rendered')


def pre_comment_post(c):
    return (bool(c.obs.get('comments')), '>= 1 comment POST')


def pre_modal_rendered(c):
    return (any(s.get('kind') == 'sprint-action' and s.get('rendered') for s in c.surfaces()), 'modal rendered')


def pre_app_content(c):
    return (bool(c.rendered_surfaces()), 'a surface rendered app content')


def pre_widget_sprints(c):
    return (any(v.get('sprints') for v in (c.ui().get('widget') or {}).get('views') or []), 'widget rendered sprints')


def pre_action_exists(c):
    return (bool(_action_module(c)), 'the action exists')


def pre_skill_md(c):
    return (_skill_text(c) is not None, 'SKILL.md exists')


# ══ L: lint and bundles ══════════════════════════════════════════════════════════════════════

def _lint(c: Ctx) -> Tuple[Optional[Dict], Optional[str]]:
    runs = (c.obs.get('lint') or {}).get('runs') or []
    if len(runs) < 2:
        return None, 'lint did not run twice'
    if any(r.get('crashed') for r in runs):
        return None, 'the linter crashed (harness defect)'
    canon = [json.dumps({k: r.get(k) for k in ('counts', 'problems', 'stageReached', 'stagesTotal')}, sort_keys=True)
             for r in runs]
    if canon[0] != canon[1]:
        return None, 'the two lint runs differ — not a fixed point'
    return runs[0], None


@check('l_deployable', 'L', pre=pre_modules, needs=('lint',))
def _(c):
    if c.obs.get('manifestError'):
        return g(0, f"manifest.yml does not parse: {c.obs['manifestError']}", 'deploy blocked')
    run, why = _lint(c)
    if run is None:
        return unavail(why)
    errors = (run.get('counts') or {}).get('errors', 0)
    # WP1's lint.cjs reports stageReached 'complete' once every stage ran (or the stage number it stopped at).
    staged = run.get('stageReached') == 'complete' or (
        run.get('stageReached') is not None and run.get('stageReached') == run.get('stagesTotal'))
    msgs = [p.get('message') for p in run.get('problems') or [] if p.get('sev') == 'error'][:3]
    if errors == 0 and staged:
        return g(1, 'lint: 0 errors, every stage reached')
    return g(0, f'lint: {errors} error(s), stage {run.get("stageReached")}/{run.get("stagesTotal")}: {msgs}',
             'deploy blocked')


@check('l_bundles_load', 'L', pre=pre_functions, needs=('build',))
def _(c):
    declared = [f.get('key') for f in c.functions()]
    built = {f.get('key'): f for f in (c.obs.get('build') or {}).get('functions') or []}
    good = [k for k in declared if (built.get(k) or {}).get('bundled') and built[k].get('loaded') and built[k].get('exported')]
    bad = {k: (built.get(k) or {}).get('error') or 'not built' for k in declared if k not in good}
    return g(len(good) / len(declared), f'{len(good)}/{len(declared)} functions bundle, load and export their handler'
             + (f'; failing: {bad}' if bad else ''), 'the app does not run', parts={'failing': bad})


@check('l_lint_warnings', 'L', pre=pre_function_loads, needs=('lint',))
def _(c):
    run, why = _lint(c)
    if run is None:
        return unavail(why)
    msgs = sorted({p.get('message') for p in run.get('problems') or [] if p.get('sev') == 'warning'})
    return g(max(0.0, 1 - 0.2 * len(msgs)), f'{len(msgs)} distinct lint warning(s): {msgs[:4]}',
             'warnings cost points, never cap', parts={'warnings': msgs})


@check('l_real_packages', 'L', pre=pre_forge_import, needs=('build',))
def _(c):
    pins = (c.obs.get('kit') or {}).get('pins') or {}
    if not pins:
        return unavail('the kit pins were not recorded')
    fns = [f for f in (c.obs.get('build') or {}).get('functions') or [] if f.get('forgePackages')]
    bad = []
    for f in fns:
        wrong = {p: v for p, v in (f.get('forgePackages') or {}).items() if pins.get(p) != v}
        if wrong or f.get('outsideKit'):
            bad.append({'key': f.get('key'), 'wrongVersions': wrong, 'outsideKit': (f.get('outsideKit') or [])[:3]})
    return g(1 - len(bad) / len(fns), f'{len(fns) - len(bad)}/{len(fns)} bundles resolve @forge/* at the kit pins only',
             'packages outside the kit', parts={'bad': bad})


@check('l_manifest_rules', 'L', pre=pre_resource_and_function, needs=('build',))
def _(c):
    rules: Dict[str, bool] = {}
    paths = [str(r.get('path') or '') for r in c.resources()]
    rules['resource_has_index_html'] = all(c.read_tree(os.path.join(p, 'index.html')) is not None for p in paths)
    rules['resource_not_under_src'] = all(not p.strip('./').startswith('src/') and p.strip('./') != 'src' for p in paths)
    every = [e for t in c.module_types() for e in c.modules(t)]
    rules['no_layout_basic'] = not any(e.get('layout') == 'basic' for e in every)
    keys = [e.get('key') for e in every] + [f.get('key') for f in c.functions()] + [r.get('key') for r in c.resources()]
    rules['keys_unique'] = len(keys) == len(set(keys))
    fn_keys = {f.get('key') for f in c.functions()}
    referenced = set()
    for e in every:
        for ref in (e.get('function'), (e.get('resolver') or {}).get('function') if isinstance(e.get('resolver'), dict) else None):
            if isinstance(ref, str):
                referenced.add(ref)
    built = {f.get('key'): f for f in (c.obs.get('build') or {}).get('functions') or []}
    rules['referenced_functions_exist_and_export'] = all(
        ref in fn_keys and (built.get(ref) or {}).get('exported') for ref in referenced)
    scopes = set(((c.manifest or {}).get('permissions') or {}).get('scopes') or [])
    rules['storage_app_with_kvs'] = (not c.all_calls('kvs')) or 'storage:app' in scopes
    met = sum(rules.values())
    return g(met / len(rules), f'{met}/{len(rules)} manifest rules met: '
             + ', '.join(k for k, v in rules.items() if not v), 'a rule lint misses', parts=rules)


def _scope_evidence(c: Ctx) -> Tuple[Optional[Dict], Optional[str]]:
    """Per call, the OAuth2 alternative it is satisfied by. WP1's site reports the CHOSEN alternative (classic where
    one exists, else the full granular set); a call no declared alternative satisfies is a 401 scope mismatch and
    counts one missing scope per operation."""
    declared = set(((c.manifest or {}).get('permissions') or {}).get('scopes') or [])
    used, missing, tolerated = set(), set(), set()
    for call in c.all_calls('jira'):
        if isinstance(call.get('scopes'), dict):
            # An operation satisfied by an EMPTY alternative (POST /permissions/check: classic [] while its
            # description names read:jira-work) charges neither reading: the scopes it names are tolerated.
            tolerated |= set(call['scopes'].get('tolerated') or [])
        if call.get('scopeMismatch'):
            missing.add(f"(a scope for {call['scopeMismatch']})")
            continue
        if call.get('needsAuthentication') or call.get('status') in (404,) and not call.get('scopes'):
            continue
        alt = call.get('scopes')
        if not isinstance(alt, dict):
            return None, f"jira call {call.get('method')} {call.get('path')} carries no OAuth2 scope alternative"
        if 'chosen' in alt:
            used |= set(alt.get('chosen') or [])
            continue
        classic, granular = set(alt.get('classic') or []), set(alt.get('granular') or [])
        if classic and classic <= declared:
            used |= classic
        elif granular and granular <= declared:
            used |= granular
        else:
            missing |= (classic or granular) - declared
            used |= (classic or granular) & declared
    if any(x.get('missingScope') == 'storage:app' for x in c.all_calls('kvs')):
        missing.add('storage:app')
    # Least privilege is judged on the CODE, not on what this run happened to exercise: a scope some
    # alternative of a route the sources build names is not extra.
    for _rel, path in static_routes(_src_texts(c)):
        for op in ops_for(path):
            for alt in op['alts']:
                tolerated |= alt
            tolerated |= op['named']
    if c.all_calls('kvs'):
        if 'storage:app' in declared:
            used.add('storage:app')
        else:
            missing.add('storage:app')
    extra = declared - used - tolerated
    return {'declared': sorted(declared), 'used': sorted(used), 'missing': sorted(missing), 'extra': sorted(extra),
            'tolerated': sorted(tolerated & declared)}, None


@check('l_scopes', 'L', pre=pre_product_or_kvs, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    ev, why = _scope_evidence(c)
    if ev is None:
        return unavail(why)
    costs = sum(2 if (s.startswith('manage:') or '.admin' in s or 'admin' in s.split(':')[0]) else 1
                for s in ev['missing'] + ev['extra'])
    scripts = ((((c.manifest or {}).get('permissions') or {}).get('content') or {}).get('scripts'))
    if scripts:
        costs += 1
    return g(max(0.0, 1 - 0.25 * costs), f"scopes: missing {ev['missing']}, extra {ev['extra']}"
             + ('; content.scripts relaxation' if scripts else ''), 'scopes beyond the calls', parts=ev)


# ══ K: platform currency ═════════════════════════════════════════════════════════════════════

@check('k_dashboard_widget', 'K')
def _(c):
    if c.modules('jira:dashboardGadget'):
        return g(0, 'the legacy jira:dashboardGadget is declared — it earns nothing', 'stale platform knowledge')
    widgets = c.modules('dashboards:widget')
    if not widgets:
        return absent('dashboards:widget module')
    res = {r.get('key') for r in c.resources()}
    w = widgets[0]
    conds = {'custom_ui_resource': w.get('resource') in res and w.get('render') in (None, 'default'),
             'edit_resource': isinstance(w.get('edit'), dict) and w['edit'].get('resource') in res
             and w['edit'].get('render') in (None, 'default')}
    met = sum(conds.values())
    return g(met / len(conds), f'dashboards:widget {met}/{len(conds)}: {conds}', 'widget not wired', parts=conds)


def _bridge_ops(surface: Dict) -> List[str]:
    return [str(op.get('op') if isinstance(op, dict) else op) for op in surface.get('bridgeOps') or []]


@check('k_widget_edit_bridge', 'K', pre=pre_edit_rendered, needs=('ui',))
def _(c):
    edits = [s for s in c.surfaces() if s.get('kind') == 'widget-edit' and s.get('rendered')]
    picks = (c.ui().get('edit') or {}).get('picks') or []
    # updateConfig/onProductSave exist only on the object getWidgetEditApi returns; an app may ask for it lazily on
    # the first pick, after the surface's first-paint snapshot (measured: golden-forge-alt).
    used_api = any('getWidgetEditApi' in _bridge_ops(s) for s in edits) or any(
        p.get('updateConfigCalls') or p.get('onProductSave') for p in picks)
    carried = [p for p in picks if (p.get('updateConfigCalls') or p.get('onProductSave'))
               and p.get('board') is not None and str(p.get('board')) in
               {t for t in (_leaf_text(v) for v in fo.iter_leaves(p.get('savedConfig'))) if t}]
    if not used_api or not picks:
        return g(0, 'the edit surface never reached getWidgetEditApi', 'config never reaches the dashboard')
    return g(len(carried) / len(picks), f'{len(carried)}/{len(picks)} board picks reached the dashboard through the '
             'widget edit API (updateConfig/onProductSave)', 'config never reaches the dashboard')


def _skill_dir(c: Ctx) -> Optional[str]:
    skills = c.modules('rovo:skill')
    src = (skills[0].get('source') or {}) if skills else {}
    return str(src.get('dir')) if isinstance(src, dict) and src.get('dir') else None


def _skill_text(c: Ctx) -> Optional[str]:
    d = _skill_dir(c)
    return c.read_tree(os.path.join(d, 'SKILL.md')) if d else None


def parse_frontmatter(text: str) -> Tuple[Optional[Dict[str, str]], str]:
    m = re.match(r'\A---\s*\n(.*?)\n---\s*(?:\n|\Z)(.*)\Z', text or '', re.S)
    if not m:
        return None, text or ''
    fm: Dict[str, str] = {}
    current = None
    for line in m.group(1).splitlines():
        km = re.match(r'^([A-Za-z0-9_-]+):\s*(.*)$', line)
        if km:
            current = km.group(1)
            fm[current] = km.group(2).strip()
        elif current and line.startswith((' ', '\t')):
            fm[current] = (fm[current] + ' ' + line.strip()).strip()
    for k, v in list(fm.items()):
        if len(v) >= 2 and v[0] == v[-1] and v[0] in '"\'':
            fm[k] = v[1:-1]
        elif v in ('>', '|', '>-', '|-'):
            fm[k] = ''
    return fm, m.group(2)


SKILL_NAME = re.compile(r'^[a-z0-9]+(?:-[a-z0-9]+)*$')
ACTION_KEY = 'get-sprint-scope'


@check('k_rovo_skill', 'K')
def _(c):
    skills = c.modules('rovo:skill')
    if not skills:
        return absent('rovo:skill module')
    d = _skill_dir(c)
    text = _skill_text(c)
    fm, _body = parse_frontmatter(text or '')
    fm = fm or {}
    dirname = os.path.basename(os.path.normpath(d or ''))
    tools = ((skills[0].get('dependencies') or {}).get('tools')) or []
    allowed = (fm.get('allowed-tools') or '').split()
    agents = c.modules('rovo:agent')
    conds = {
        'source_dir_has_skill_md': text is not None,
        'name_equals_dir': fm.get('name') == dirname and bool(SKILL_NAME.match(fm.get('name') or ''))
        and 1 <= len(fm.get('name') or '') <= 64,
        'description_50_1024': 50 <= len(fm.get('description') or '') <= 1024,
        'allowed_tools_has_action': ACTION_KEY in allowed,
        'dependencies_tools_is_action': list(tools) == [ACTION_KEY],
        'allowed_tools_cover_dependencies': set(tools) <= set(allowed),
        'agent_lists_skill': any(skills[0].get('key') in (a.get('skills') or []) for a in agents),
    }
    met = sum(conds.values())
    return g(met / len(conds), f'rovo:skill {met}/{len(conds)}: ' + ', '.join(k for k, v in conds.items() if not v),
             'the skill is not valid or not wired', parts=conds)


def _src_texts(c: Ctx) -> Dict[str, str]:
    out = {}
    if not c.root or not (c.root / 'src').is_dir():
        return out
    for path in sorted((c.root / 'src').rglob('*')):
        if path.is_file() and path.suffix in ('.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'):
            out[str(path.relative_to(c.root))] = path.read_text(errors='replace')
    return out


STORAGE_IMPORT = re.compile(r"""import\s*\{[^}]*\bstorage\b[^}]*\}\s*from\s*['"]@forge/api['"]"""
                            r"""|require\(\s*['"]@forge/api['"]\s*\)\s*\.\s*storage\b"""
                            r"""|\{[^}]*\bstorage\b[^}]*\}\s*=\s*require\(\s*['"]@forge/api['"]\s*\)""")
FORGE_UI_IMPORT = re.compile(r"""['"]@forge/ui['"]""")
OLD_SEARCH = re.compile(r"""/rest/api/(?:2|3|latest)/search(?![/A-Za-z])""")


@check('k_current_apis', 'K', pre=pre_product_call, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    texts = _src_texts(c)
    errors = ' '.join(str(i.get('error') or '') for i in c.invocations())
    runtime = (((c.manifest or {}).get('app') or {}).get('runtime') or {}).get('name')
    conds = {
        'no_api_storage': not any(STORAGE_IMPORT.search(t) for t in texts.values()),
        'no_forge_ui': not any(FORGE_UI_IMPORT.search(t) for t in texts.values()),
        'no_old_search': not any(OLD_SEARCH.search(t) for t in texts.values())
        and not any(OLD_SEARCH.match(str(call.get('path') or '').split('?')[0] + '#')
                    for call in c.all_calls('jira')),
        'route_everywhere': 'You must create your route' not in errors,
        'current_runtime': runtime in CURRENT_RUNTIMES,
    }
    met = sum(conds.values())
    return g(met / len(conds), f'current APIs {met}/{len(conds)}: ' + ', '.join(k for k, v in conds.items() if not v),
             'stale platform knowledge', parts=conds)


@check('k_consumer_shape', 'K', pre=pre_consumer)
def _(c):
    consumers = c.modules('consumer')
    fn_keys = {cm.get('function') for cm in consumers if isinstance(cm.get('function'), str)}
    resolver_fns = {(e.get('resolver') or {}).get('function') for t in c.module_types() for e in c.modules(t)
                    if isinstance(e.get('resolver'), dict)}
    timed_ok_fns = fn_keys | {e.get('function') for e in c.modules('scheduledTrigger')}
    conds = {
        'function_shape': all(isinstance(cm.get('function'), str) and not cm.get('resolver') for cm in consumers),
        'serves_no_resolver': not (fn_keys & resolver_fns),
        'timeout_only_on_async': all(f.get('key') in timed_ok_fns for f in c.functions() if f.get('timeoutSeconds')),
    }
    met = sum(conds.values())
    return g(met / len(conds), f'consumer shape {met}/{len(conds)}', 'consumer wired wrongly', parts=conds)


def _entities(c: Ctx) -> List[Dict]:
    ents = (((c.manifest or {}).get('app') or {}).get('storage') or {}).get('entities')
    return [e for e in ents if isinstance(e, dict)] if isinstance(ents, list) else []


def _sprint_indexes(c: Ctx) -> Dict[str, set]:
    """entity name -> names of its indexes partitioned by a sprint attribute and ranged by one attribute."""
    out: Dict[str, set] = {}
    for e in _entities(c):
        for idx in e.get('indexes') or []:
            if not isinstance(idx, dict):
                continue
            part, rng = idx.get('partition') or [], idx.get('range') or []
            if len(part) == 1 and re.search(r'sprint', str(part[0]), re.I) and len(rng) == 1:
                out.setdefault(str(e.get('name')), set()).add(str(idx.get('name')))
    return out


@check('k_entity_declared', 'K')
def _(c):
    if not _entities(c):
        return absent('app.storage.entities')
    found = _sprint_indexes(c)
    return g(1 if found else 0, f'entities with a sprint-partitioned, ranged index: {sorted(found)}',
             'the ledger has no sprint index')


# ══ Deploy readiness: the manifest, the module wiring, the scopes and the predicted runtime failures ════════
# Owner, 2026-10-03: "make sure the scorer is extra judicious and has the capability to judge the manifest, judge
# the module usage, understand where it would fail or not". Deterministic, from the manifest, the app's sources, the
# built resources, the shipped OpenAPI and the run's own call log. Every rule cites the platform page it encodes.
# Each finding is labelled `would fail` (on real Forge: deploy refused, a call refused, a surface blocked, data
# missed) or `poor practice` (works, but against the documented guidance), and is PRICED ONCE: a finding another
# row already grades names that row in `graded_by` and costs nothing here (one defect, not N).

DOCS = {
    'trigger': 'https://developer.atlassian.com/platform/forge/manifest-reference/modules/trigger/',
    'consumer': 'https://developer.atlassian.com/platform/forge/runtime-reference/async-events-api/',
    'scheduled': 'https://developer.atlassian.com/platform/forge/limits-scheduled-trigger/',
    'rovo': 'https://developer.atlassian.com/platform/forge/manifest-reference/modules/rovo-agent/',
    'resources': 'https://developer.atlassian.com/platform/forge/manifest-reference/resources/',
    'entities': 'https://developer.atlassian.com/platform/forge/limits-kvs-ce/',
    'uikit': 'https://developer.atlassian.com/platform/forge/ui-kit/upgrade-to-ui-kit-latest/',
    'route': 'https://developer.atlassian.com/platform/forge/apis-reference/fetch-api-product.requestjira/',
    'asuser': 'https://developer.atlassian.com/platform/forge/runtime-reference/forge-resolver/',
    'search': 'https://developer.atlassian.com/changelog/#CHANGE-2046',
    'ratelimit': 'https://developer.atlassian.com/cloud/jira/platform/rate-limiting/',
    'permissions': 'https://developer.atlassian.com/platform/forge/manifest-reference/permissions/',
    'csp': 'https://developer.atlassian.com/platform/forge/extend-ui-with-custom-options/',
    'invocation': 'https://developer.atlassian.com/platform/forge/limits-invocation/',
    'kvs': 'https://developer.atlassian.com/platform/forge/limits-kvs-ce/',
    'storage': 'https://developer.atlassian.com/platform/forge/storage-reference/kvs-migration-from-legacy/',
    'fields': 'https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/',
    'llm': 'https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-reference/',
    'realtime': 'https://developer.atlassian.com/platform/forge/runtime-reference/realtime-events-api/',
}
SCHEDULE_INTERVALS = ('fiveMinute', 'hour', 'day', 'week')          # limits-scheduled-trigger / manifest schema
ENTITY_ATTRIBUTE_TYPES = ('string', 'integer', 'float', 'boolean', 'any')
ENTITY_LIMITS = {'entities': 20, 'indexes': 7, 'attributes': 50}   # limits-kvs-ce (RESEARCH §2, quoted)
TIMEOUT_MAX_S = 900                                                  # limits-invocation (RESEARCH §2)
ROUTE_TEMPLATE = re.compile(r'route\s*`((?:[^`\\]|\\.)*)`')
PLAIN_PRODUCT_CALL = re.compile(r"""\.request(?:Jira|Confluence)\(\s*['"]""")
QUEUE_NEW = re.compile(r"""new\s+Queue\(\s*\{\s*key\s*:\s*(['"]([^'"]+)['"]|([A-Za-z_$][\w$]*))""")
CONST_STRING = r"""(?:const|let|var)\s+{name}\s*=\s*['"]([^'"]+)['"]"""
EXTERNAL_FETCH = re.compile(r"""(?<![\w.])fetch\(\s*['"`](https?://[^/'"`$]+)""")
CUSTOMFIELD_LITERAL = re.compile(r"""['"`]customfield_\d+['"`]""")
RETRY_AFTER_HANDLING = re.compile(r'retry-after|retryAfter|Retry-After', re.I)
CSP_INLINE_HANDLER = re.compile(r'<[^>]+\son[a-z]+\s*=', re.I)
CSP_STYLE_MARKUP = re.compile(r'<style[\s>]|<[^>]+\sstyle\s*=', re.I)
CSP_EXTERNAL = re.compile(r"""(?:src|href)\s*=\s*['"](?:https?:)?//""", re.I)
CSP_ABSOLUTE_ASSET = re.compile(r"""(?:src|href)\s*=\s*['"]/(?!/)""", re.I)
BACKGROUND_TYPES = ('trigger', 'consumer', 'scheduledTrigger')

_OPENAPI_OPS: Optional[List[Dict]] = None


def openapi_ops() -> List[Dict]:
    """The shipped Jira OpenAPI operations (forge/kit/openapi, the same blobs the site checks scopes against):
    method, path regex, OAuth2 alternatives (x-atlassian-oauth2-scopes, else the OAuth2 security entry) and the
    scopes the description names (POST /permissions/check lists Classic read:jira-work under an empty alternative)."""
    global _OPENAPI_OPS
    if _OPENAPI_OPS is None:
        ops = []
        for name in ('jira.json', 'jsw.json'):
            spec = json.loads((ROOT / 'forge' / 'kit' / 'openapi' / name).read_text())
            for template, item in spec['paths'].items():
                regex = re.compile('^' + '/'.join('[^/]+' if re.fullmatch(r'\{.+\}', seg) else re.escape(seg)
                                                  for seg in template.split('/')) + '/?$')
                for method, op in item.items():
                    if method not in ('get', 'put', 'post', 'delete', 'patch'):
                        continue
                    alts = [set(a.get('scopes') or []) for a in op.get('x-atlassian-oauth2-scopes') or []
                            if a.get('scheme') == 'OAuth2']
                    if not alts:
                        alts = [set(s['OAuth2']) for s in op.get('security') or [] if isinstance(s, dict) and 'OAuth2' in s]
                    named = set(re.findall(r'`([a-z]+:[a-z0-9:.\-]+)`', ' '.join(
                        re.findall(r'\*\*(?:Classic|Granular)\*\*:\s*([^\n]+)', op.get('description') or ''))))
                    ops.append({'method': method.upper(), 'template': template, 'regex': regex, 'alts': alts,
                                'named': named, 'literal': sum(1 for s in template.split('/') if s and '{' not in s)})
        ops.sort(key=lambda o: -o['literal'])
        _OPENAPI_OPS = ops
    return _OPENAPI_OPS


def static_routes(texts: Dict[str, str]) -> List[Tuple[str, str]]:
    """(file, path) for every route`…` template in backend or UI sources; ${…} becomes one path segment."""
    out = []
    for rel, text in texts.items():
        for m in ROUTE_TEMPLATE.finditer(text):
            path = re.sub(r'\$\{[^}]*\}', 'X', m.group(1)).split('?')[0]
            if path.startswith('/'):
                out.append((rel, path))
    return out


def ops_for(path: str) -> List[Dict]:
    return [op for op in openapi_ops() if op['regex'].match(path)]


def _function_body(text: str, name: str) -> Optional[str]:
    """The source of an exported handler (`export [async] function name` or `export const name = …`), by brace
    matching that skips strings, template literals and comments."""
    m = re.search(r'export\s+(?:async\s+)?function\s+' + re.escape(name) + r'\b|export\s+const\s+' + re.escape(name) + r'\s*=', text)
    if not m:
        return None
    i = text.find('{', m.end())
    if i < 0:
        return None
    depth, j, quote = 0, i, None
    while j < len(text):
        ch = text[j]
        if quote:
            if ch == '\\':
                j += 2
                continue
            if ch == quote:
                quote = None
        elif ch in '\'"`':
            quote = ch
        elif text.startswith('//', j):
            j = text.find('\n', j)
            if j < 0:
                break
        elif text.startswith('/*', j):
            j = text.find('*/', j) + 1
            if j <= 0:
                break
        elif ch == '{':
            depth += 1
        elif ch == '}':
            depth -= 1
            if depth == 0:
                return text[i:j + 1]
        j += 1
    return None


def _ui_html(c: Ctx) -> Dict[str, str]:
    out = {}
    for r in c.resources():
        rel = str(r.get('path') or '')
        if not rel or rel.startswith('/') or '..' in Path(rel).parts:
            continue
        html = c.read_tree(os.path.join(rel, 'index.html'))
        if html is not None:
            out[rel] = html
    return out


def _row_score(c: Ctx, name: str) -> Optional[float]:
    r = c._row_cache.get(name)
    return None if r is None or r.get('unavailable') or (r.get('parts') or {}).get('vacuous_root') else r['score']


def _charged(c: Ctx, *names: str) -> Optional[str]:
    """The first of `names` whose row already charged a shortfall (it then prices the finding), else None."""
    for name in names:
        score = _row_score(c, name)
        if score is not None and score < 1.0 - 1e-9:
            return name
    return None


def _lint_mentions(c: Ctx, *words: str) -> bool:
    runs = (c.obs.get('lint') or {}).get('runs') or []
    text = ' '.join(str(p.get('message') or '') for p in (runs[0].get('problems') if runs else []) or [])
    return any(w.lower() in text.lower() for w in words)


def deploy_findings(c: Ctx) -> List[Dict]:
    """Every rule's outcome: {rule, area, status pass|fail|n/a, kind would_fail|poor_practice, where, why, doc,
    graded_by}. Pure over the manifest, the tree and the observations."""
    out: List[Dict] = []

    def rule(rid, area, applicable, ok, kind, why, doc, where='', graded_by=None):
        out.append({'rule': rid, 'area': area, 'status': 'n/a' if not applicable else ('pass' if ok else 'fail'),
                    'kind': kind, 'why': why, 'doc': DOCS[doc], 'where': where,
                    **({'graded_by': graded_by} if graded_by and applicable and not ok else {})})

    texts = _src_texts(c)
    backend = '\n'.join(texts.values())
    mods = {t: c.modules(t) for t in c.module_types()}
    fn_keys = {f.get('key') for f in c.functions()}
    handler_of = {f.get('key'): str(f.get('handler') or '') for f in c.functions()}

    # ── manifest ────────────────────────────────────────────────────────────────────────────
    triggers = c.modules('trigger')
    events = [e if isinstance(e, str) else (e or {}).get('eventType') for t in triggers for e in (t.get('events') or [])]
    rule('M1 trigger subscribes avi:jira:updated:issue', 'manifest', bool(triggers), 'avi:jira:updated:issue' in events,
         'would_fail', 'the ledger never hears about issue updates (contract §2)', 'trigger', ', '.join(map(str, events)))
    consumer_queues = {str(cm.get('queue')) for cm in c.modules('consumer') if cm.get('queue')}
    pushed = set()
    for text in texts.values():
        for m in QUEUE_NEW.finditer(text):
            if m.group(2):
                pushed.add(m.group(2))
            else:
                const = re.search(CONST_STRING.format(name=re.escape(m.group(3))), backend)
                if const:
                    pushed.add(const.group(1))
    pushed |= {str((x.get('body') or {}).get('queueName')) for x in c.calls(service='queue')
               if isinstance(x.get('body'), dict) and x['body'].get('queueName')}
    rule('M2 every pushed queue has a consumer', 'manifest', bool(pushed), pushed <= consumer_queues, 'would_fail',
         'events pushed to a queue no consumer listens on are never delivered', 'consumer',
         f'pushed {sorted(pushed)}, consumers {sorted(consumer_queues)}')
    rule('M3 every consumer queue is pushed to', 'manifest', bool(consumer_queues) and bool(pushed),
         consumer_queues <= pushed, 'poor_practice', 'a consumer whose queue nothing pushes to never runs', 'consumer',
         f'consumers {sorted(consumer_queues - pushed)}')
    intervals = [str(s.get('interval')) for s in c.modules('scheduledTrigger')]
    bad_interval = [i for i in intervals if i not in SCHEDULE_INTERVALS]
    rule('M4 scheduledTrigger interval is fiveMinute|hour|day|week', 'manifest', bool(intervals), not bad_interval,
         'would_fail', 'the manifest schema refuses any other interval (deploy blocked)', 'scheduled', ', '.join(bad_interval),
         graded_by='l_deployable' if _lint_mentions(c, 'interval') else None)
    action_keys = {a.get('key') for a in c.modules('action')}
    skill_keys = {s.get('key') for s in c.modules('rovo:skill')}
    dangling = [f"agent {a.get('key')} action {x}" for a in c.modules('rovo:agent') for x in a.get('actions') or [] if x not in action_keys]
    dangling += [f"agent {a.get('key')} skill {x}" for a in c.modules('rovo:agent') for x in a.get('skills') or [] if x not in skill_keys]
    dangling += [f"skill {s.get('key')} tool {x}" for s in c.modules('rovo:skill')
                 for x in ((s.get('dependencies') or {}).get('tools') or []) if x not in action_keys]
    dangling += [f"mcp {m.get('key')} tool {x}" for m in c.modules('rovo:mcp')
                 for x in (m.get('tools') or []) if isinstance(x, str) and x not in action_keys]
    rule('M5 Rovo agent, skill, MCP and action references resolve', 'manifest',
         bool(c.modules('rovo:agent') or c.modules('rovo:skill') or c.modules('rovo:mcp')), not dangling, 'would_fail',
         'a reference to an undeclared module key fails validation', 'rovo', '; '.join(dangling),
         graded_by='l_deployable' if dangling and _lint_mentions(c, *dangling) else None)
    bad_paths = [str(r.get('path')) for r in c.resources()
                 if not r.get('path') or str(r['path']).startswith('/') or '..' in Path(str(r['path'])).parts]
    rule('M6 resource paths are relative and inside the app', 'manifest', bool(c.resources()), not bad_paths, 'would_fail',
         'resources are packaged from the app directory; an absolute or escaping path is not deployable', 'resources',
         ', '.join(bad_paths))
    ent_problems = []
    entities = _entities(c)
    if len(entities) > ENTITY_LIMITS['entities']:
        ent_problems.append(f'{len(entities)} entities > {ENTITY_LIMITS["entities"]}')
    for e in entities:
        attrs = e.get('attributes') if isinstance(e.get('attributes'), dict) else {}
        if len(attrs) > ENTITY_LIMITS['attributes']:
            ent_problems.append(f"{e.get('name')}: {len(attrs)} attributes")
        for an, spec in attrs.items():
            if (spec or {}).get('type') not in ENTITY_ATTRIBUTE_TYPES:
                ent_problems.append(f"{e.get('name')}.{an}: type {(spec or {}).get('type')!r}")
        idx = [i for i in e.get('indexes') or [] if isinstance(i, dict)]
        if len(idx) > ENTITY_LIMITS['indexes']:
            ent_problems.append(f"{e.get('name')}: {len(idx)} indexes")
        for i in idx:
            for an in (i.get('partition') or []) + (i.get('range') or []):
                if an not in attrs:
                    ent_problems.append(f"{e.get('name')}.{i.get('name')}: {an} is not a declared attribute")
    rule('M7 KVS entity indexes name declared attributes within the documented limits', 'manifest', bool(entities),
         not ent_problems, 'would_fail', 'an index over an undeclared attribute or past the limits is refused', 'entities',
         '; '.join(ent_problems[:4]))
    native = [f'{t}:{m.get("key")}' for t, ms in mods.items() for m in ms
              if m.get('render') == 'native' or (isinstance(m.get('edit'), dict) and m['edit'].get('render') == 'native')]
    rule('M8 Custom UI only (no render: native)', 'manifest', bool(mods), not native, 'would_fail',
         'UI Kit surfaces earn nothing in this task (contract §2) and need @forge/react, which is not installed',
         'uikit', ', '.join(native))
    gadget = bool(c.modules('jira:dashboardGadget'))
    rule('M9 no legacy jira:dashboardGadget', 'manifest', bool(mods), not gadget, 'poor_practice',
         'dashboards:widget replaces it ("will be deprecated by 17 May 2027", CDAC 102826)', 'uikit',
         graded_by='k_dashboard_widget')
    runtime = (((c.manifest or {}).get('app') or {}).get('runtime') or {}).get('name')
    rule('M10 runtime is a current Node.js runtime', 'manifest', c.manifest is not None, runtime in CURRENT_RUNTIMES,
         'would_fail', f'runtime {runtime!r} is outside the manifest runtime enum', 'invocation', str(runtime),
         graded_by='k_current_apis')
    unresolved = [f'{t}:{m.get("key")} -> {ref}' for t, ms in mods.items() for m in ms
                  for ref in (m.get('function'), (m.get('resolver') or {}).get('function') if isinstance(m.get('resolver'), dict) else None)
                  if isinstance(ref, str) and ref not in fn_keys]
    built = {f.get('key'): f for f in (c.obs.get('build') or {}).get('functions') or []}
    unexported = [k for k in fn_keys if k in built and not (built[k].get('loaded') and built[k].get('exported'))]
    rule('M11 every function reference resolves to an exported handler in the built bundle', 'manifest',
         bool(fn_keys), not unresolved and not unexported, 'would_fail',
         'a module bound to a missing function or handler fails at deploy or on its first invocation', 'invocation',
         '; '.join(unresolved + unexported), graded_by='l_bundles_load' if unexported else 'l_manifest_rules')
    long_t = [f.get('key') for f in c.functions() if isinstance(f.get('timeoutSeconds'), (int, float)) and f['timeoutSeconds'] > TIMEOUT_MAX_S]
    rule('M12 timeoutSeconds within the documented 900 s', 'manifest', bool(fn_keys), not long_t, 'would_fail',
         'async functions run at most 900 s', 'invocation', ', '.join(map(str, long_t)),
         graded_by='l_deployable' if _lint_mentions(c, 'timeoutSeconds') else None)

    uses_llm = any(re.search(r"""['"]@forge/llm['"]""", t) for t in texts.values())
    rule('M13 @forge/llm is used only with an llm module', 'manifest', uses_llm, bool(c.modules('llm')), 'would_fail',
         "the docs: lint 'will fail with an error like: Error: LLM package is used but 'llm' module is not defined' "
         '(lint 6.3.0 only warns); the emulator refuses the call', 'llm', '', graded_by=_charged(c, 'k_llm_model_current'))

    # ── permissions ─────────────────────────────────────────────────────────────────────────
    declared = set(((c.manifest or {}).get('permissions') or {}).get('scopes') or [])
    routes = static_routes(texts)
    unknown, needs_missing = [], []
    for rel, path in routes:
        ops = ops_for(path)
        if not ops:
            unknown.append(f'{rel}: {path}')
            continue
        if not any(not alt or alt <= declared for op in ops for alt in op['alts']):
            needs_missing.append(f"{rel}: {path} needs one of {[sorted(a) for a in ops[0]['alts']][:2]}")
    rule('P1 every route the code builds is a documented Jira operation', 'permissions', bool(routes), not unknown,
         'would_fail', 'a path outside the shipped OpenAPI answers 404 on Jira Cloud', 'fields', '; '.join(unknown[:3]))
    rule('P2 declared scopes cover every route in the code (OAuth2 rule)', 'permissions', bool(routes),
         not needs_missing, 'would_fail', 'a call whose scopes are not declared answers 401 at runtime', 'permissions',
         '; '.join(needs_missing[:3]), graded_by=_charged(c, 'l_scopes'))
    ev, _why = _scope_evidence(c)
    extra = sorted(set((ev or {}).get('extra') or []))
    rule('P3 no scope beyond what the calls need (least privilege)', 'permissions', bool(declared) and ev is not None,
         not extra, 'poor_practice', 'every declared scope is shown to the admin who installs the app', 'permissions',
         ', '.join(extra), graded_by='l_scopes')
    hosts = sorted({m.group(1) for t in texts.values() for m in EXTERNAL_FETCH.finditer(t)})
    allowed = ' '.join(str(x if isinstance(x, str) else (x or {}).get('address'))
                       for x in ((((c.manifest or {}).get('permissions') or {}).get('external') or {}).get('fetch') or {}).get('backend') or [])
    missing_egress = [h for h in hosts if h.split('://', 1)[-1] not in allowed]
    rule('P4 every external fetch host is declared under permissions.external.fetch', 'permissions', bool(hosts),
         not missing_egress, 'would_fail', 'Forge refuses egress to an undeclared host', 'permissions', ', '.join(missing_egress),
         graded_by='l_deployable' if _lint_mentions(c, 'egress') else None)
    html = _ui_html(c)
    content = (((c.manifest or {}).get('permissions') or {}).get('content') or {})
    styles_ok = 'unsafe-inline' in (content.get('styles') or [])
    csp = []
    for rel, page in html.items():
        if CSP_INLINE_HANDLER.search(page):
            csp.append(f'{rel}: inline event handler')
        if CSP_STYLE_MARKUP.search(page) and not styles_ok:
            csp.append(f'{rel}: <style>/style= without content.styles unsafe-inline')
        if CSP_EXTERNAL.search(page):
            csp.append(f'{rel}: external script/style/font')
        if CSP_ABSOLUTE_ASSET.search(page):
            csp.append(f'{rel}: absolute asset path (404 under the resource prefix)')
    rule('P5 built Custom UI fits the default Forge CSP and resource prefix', 'permissions', bool(html), not csp, 'would_fail',
         'blocked inline handlers/styles, external assets and absolute paths leave a broken or blank surface', 'csp',
         '; '.join(csp[:4]), graded_by=_charged(c, 'v_csp_clean'))

    # ── predicted runtime failures ──────────────────────────────────────────────────────────
    plain = [rel for rel, t in texts.items() if PLAIN_PRODUCT_CALL.search(t)]
    rule('R1 product calls are built with the route tag', 'runtime', bool(texts), not plain, 'would_fail',
         'requestJira with a plain string throws "You must create your route using the \'route\' export"', 'route',
         ', '.join(plain), graded_by=_charged(c, 'k_current_apis'))
    bg_user = []
    for t in BACKGROUND_TYPES:
        for m in c.modules(t):
            handler = handler_of.get(m.get('function'), '')
            if '.' not in handler:
                continue
            file, fn = handler.rsplit('.', 1)
            src = next((txt for rel, txt in texts.items() if Path(rel).with_suffix('').as_posix() == f'src/{file}'), '')
            body = _function_body(src, fn) or ''
            if re.search(r'\basUser\s*\(', body):
                bg_user.append(f'{t}:{m.get("key")} ({handler})')
    runtime_user = _charged(c, 't_no_user_in_async') is not None
    rule('R2 no asUser() in trigger, consumer or scheduled work', 'runtime', bool(texts) and any(c.modules(t) for t in BACKGROUND_TYPES),
         not bg_user and not runtime_user, 'would_fail',
         'asUser needs a user in the invocation; product and async events have none (NeedsAuthenticationError)', 'asuser',
         ', '.join(bg_user), graded_by='t_no_user_in_async' if runtime_user else None)
    rule('R3 no storage from @forge/api (removed in 8.x)', 'runtime', bool(texts),
         not any(STORAGE_IMPORT.search(t) for t in texts.values()), 'would_fail',
         '@forge/api 8.x has no storage export: the first storage call throws', 'storage', graded_by='k_current_apis')
    old = [rel for rel, t in texts.items() if OLD_SEARCH.search(t)]
    rule('R4 no /rest/api/3/search (removed, 410)', 'runtime', bool(texts), not old, 'would_fail',
         'the old search endpoint answers 410 Gone; /search/jql replaces it', 'search', ', '.join(old), graded_by='k_current_apis')
    jql = [rel for rel, path in routes if path.rstrip('/').endswith('/search/jql')]
    rule('R5 /search/jql is walked by nextPageToken', 'runtime', bool(jql), 'nextPageToken' in backend, 'would_fail',
         'a search that stops at the first page misses every issue past it', 'search', ', '.join(jql),
         graded_by=_charged(c, 'r_pagination'))
    # Static: the code reads Retry-After at all. Runtime: no retry landed inside a 429's window (the site answers an
    # early retry with another 429, as Jira does). Either failing is the finding.
    early = _refused_in_window(c.all_calls('jira'))[0]
    rule('R6 Jira calls honour Retry-After', 'runtime', bool(routes),
         bool(RETRY_AFTER_HANDLING.search(backend)) and not early, 'would_fail',
         'Jira answers 429 with Retry-After; a retry inside the window is refused again', 'ratelimit',
         f'{len(early)} early retr(ies) observed' if early else '',
         graded_by=_charged(c, 't_retry_after_honoured', 'r_rate_limit', 'u_comment_flow'))
    cf = [rel for rel, t in texts.items() if CUSTOMFIELD_LITERAL.search(t)]
    rule('R7 no hard-coded custom field id', 'runtime', bool(texts), not cf, 'would_fail',
         'custom field ids differ per site; discover them (contract §8: the scoring site has other ids)', 'fields', ', '.join(cf))
    unbounded = [x for x in c.all_calls('jira') if str(x.get('path', '')).split('?')[0].endswith('/search/jql') and x.get('status') == 400]
    rule('R8 JQL is bounded (no 400 from /search/jql)', 'runtime', any(str(x.get('path', '')).split('?')[0].endswith('/search/jql')
                                                                       for x in c.all_calls('jira')),
         not unbounded, 'would_fail', 'Jira refuses unbounded JQL with 400', 'search', f'{len(unbounded)} refused search(es)')
    rejected = [e for e in _realtime_events(c) if e.get('rejected')]
    bg_publish = []
    for t in BACKGROUND_TYPES:
        for m in c.modules(t):
            handler = handler_of.get(m.get('function'), '')
            if '.' in handler:
                file, fn = handler.rsplit('.', 1)
                src = next((txt for rel, txt in texts.items() if Path(rel).with_suffix('').as_posix() == f'src/{file}'), '')
                if re.search(r'(?<![\w.])(?:realtime\.)?publish\s*\(', _function_body(src, fn) or ''):
                    bg_publish.append(f'{t}:{m.get("key")}')
    rule('R10 Realtime publish() only from frontend-invoked functions', 'runtime',
         bool(_realtime_events(c)) or any('@forge/realtime' in t for t in texts.values()), not rejected and not bg_publish,
         'would_fail', "the docs: 'The publish API is only supported for functions invoked from the app frontend. "
         "This is not currently available for async events' (publishGlobal instead)", 'realtime',
         '; '.join([f"{len(rejected)} publish(es) rejected: {rejected[0].get('rejected')}"] if rejected else []) + '; '.join(bg_publish),
         graded_by=_charged(c, 'u_widget_live'))
    stale = _unlisted_models(c)
    rule('R11 LLM calls name a model id list() returns', 'runtime', bool(_llm_calls(c)), not stale, 'would_fail',
         'a model id the platform does not offer is refused (400)', 'llm', ', '.join(stale),
         graded_by=_charged(c, 'k_llm_model_current'))
    sampling = [e for e in _llm_calls(c) if isinstance(e.get('request'), dict)
                and ({'temperature', 'top_p'} & set(e['request'])) and e.get('status') == 400]
    rule('R12 LLM requests follow the documented sampling rules', 'runtime', bool(_llm_calls(c)), not sampling, 'would_fail',
         "LLM API reference 'Validation rules': temperature and top_p never together; neither on claude-opus-4-7/-4-8/-5 "
         'and claude-sonnet-5', 'llm', f'{len(sampling)} request(s) refused', graded_by=_charged(c, 'u_llm_explain'))
    kvs_err = [x for x in c.all_calls('kvs') if x.get('limitError') or x.get('status') in (400, 413)]
    rule('R9 KVS writes fit the documented types and limits', 'runtime', bool(c.all_calls('kvs')), not kvs_err, 'would_fail',
         'a value, key or integer past the KVS limits is refused', 'kvs',
         ', '.join(str(x.get('limitError') or x.get('status')) for x in kvs_err[:3]),
         graded_by=_charged(c, 's_limits'))
    return out


def deploy_readiness(c: Ctx) -> Dict:
    """The human-readable judgment the run card and the published result show (report-only; the two rows price it)."""
    findings = deploy_findings(c) if c.oracle is not None or c.manifest is not None else []
    fails = [f for f in findings if f['status'] == 'fail']
    lint_run, _ = _lint(c)
    lint_errors = (lint_run or {}).get('counts', {}).get('errors') if lint_run else None
    deploy_blocked = bool(lint_errors) or bool(c.obs.get('manifestError')) or any(
        f['rule'].startswith(('M4', 'M6', 'M7', 'M12')) for f in fails)
    would_fail = [f for f in fails if f['kind'] == 'would_fail']
    if deploy_blocked:
        verdict = 'would not deploy'
    elif would_fail:
        verdict = f'would deploy; {len(would_fail)} runtime failure(s) predicted'
    else:
        verdict = 'would deploy and run'
    return {'verdict': verdict, 'lint_errors': lint_errors, 'checked': len([f for f in findings if f['status'] != 'n/a']),
            'would_fail': would_fail, 'poor_practice': [f for f in fails if f['kind'] == 'poor_practice'],
            'rules': findings}


def _priced(c: Ctx, area_prefixes: Tuple[str, ...]) -> Tuple[float, List[Dict], int]:
    rows = [f for f in deploy_findings(c) if f['rule'].startswith(area_prefixes) and f['status'] != 'n/a']
    priced = [f for f in rows if not f.get('graded_by')]
    failed = [f for f in priced if f['status'] == 'fail']
    return (1 - len(failed) / len(priced) if priced else 1.0), failed, len(rows)


def pre_judgeable(c):
    return (bool(c.functions()) and bool(c.all_calls('jira')), '>= 1 function and >= 1 product call observed')


@check('k_manifest_semantics', 'K', pre=pre_judgeable, needs=('build',))
def _(c):
    score, failed, n = _priced(c, ('M', 'P'))
    return g(score, f'deploy readiness, manifest + permissions: {n} rules checked; failing (priced here): '
             + '; '.join(f"{f['rule']} [{f['where']}]" for f in failed) if failed else
             f'deploy readiness, manifest + permissions: {n} rules checked, none failing here',
             'the manifest or its permissions would fail on real Forge', parts={'failed': [f['rule'] for f in failed]})


@check('k_runtime_risks', 'K', pre=pre_judgeable, needs=('build',))
def _(c):
    score, failed, n = _priced(c, ('R',))
    return g(score, f'deploy readiness, predicted runtime failures: {n} rules checked; failing (priced here): '
             + '; '.join(f"{f['rule']} [{f['where']}]" for f in failed) if failed else
             f'deploy readiness, predicted runtime failures: {n} rules checked, none failing here',
             'code that would fail on real Forge', parts={'failed': [f['rule'] for f in failed]})


# ══ T: event pipeline ════════════════════════════════════════════════════════════════════════

def _live_events(c: Ctx) -> List[Dict]:
    return list(c.phase('live').get('events') or [])


@check('t_trigger_handoff', 'T', pre=pre_trigger_push, needs=('live',))
def _(c):
    relevant = {str(e['changelogId']) for e in c.oracle.relevant_live()}
    calls = c.phase('live').get('calls') or []
    ok, total, notes = 0, 0, []
    for ev in _live_events(c):
        invs = set(ev.get('triggerInvocations') or [])
        if not invs:
            continue
        total += 1
        mine = [x for x in calls if x.get('inv') in invs]
        jira = sum(1 for x in mine if x.get('service') == 'jira')
        pushes = sum(1 for x in mine if x.get('service') == 'queue')
        if str(ev.get('changelogId')) in relevant:
            good = pushes >= 1
        else:
            good = jira == 0 and pushes == 0
        ok += good
        if not good:
            notes.append(f"{ev.get('changelogId')}: jira {jira}, pushes {pushes}")
    writes_in_trigger = [x for x in calls if x.get('kind') == 'trigger' and _is_kvs_write(x)]
    if total == 0:
        return g(0, 'no trigger invocation was recorded for any delivered update', 'the trigger never ran')
    score = ok / total if not writes_in_trigger else min(ok / total, 0.5)
    return g(score, f'{ok}/{total} deliveries handed off correctly'
             + (f'; {len(writes_in_trigger)} ledger write(s) inside the trigger' if writes_in_trigger else '')
             + (f'; {notes[:3]}' if notes else ''), 'trigger does product or queue work it should not')


def _irrelevant_row_ids(c: Ctx) -> set:
    return {str(e['changelogId']) for e in c.oracle.live if not c.oracle._sprint_items(e)}


@check('t_event_rows', 'T', needs=('live',))
def _(c):
    if not c.modules('consumer') and not c.modules('trigger'):
        return absent('trigger + consumer')
    expected = [ch for ch in c.oracle.changes('live') if ch.phase == 'live']
    if not expected:
        return unavail('the pack delivers no live sprint change')
    rows = c.kvs_rows('live')
    tp, fp = 0, 0
    for ch in expected:
        found = rows_for(rows, ch)
        if len(found) >= 1 and row_correct(found[0][0], ch, ('event',)) and len(found) == 1:
            tp += 1
        fp += max(0, len(found) - 1)
    irrelevant = _irrelevant_row_ids(c)
    fp += sum(1 for tok, _item in rows if tok & irrelevant)
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / len(expected)
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return g(f1, f'event rows: {tp}/{len(expected)} exact (precision {precision:.2f}, recall {recall:.2f})',
             'the event path loses or invents ledger rows', parts={'tp': tp, 'fp': fp, 'expected': len(expected)})


@check('t_no_double_count', 'T', pre=pre_ledger_row, needs=('live', 'heal', 'rerun'))
def _(c):
    doubled = []
    for ph in ('live', 'heal', 'rerun'):
        rows = c.kvs_rows(ph)
        for ch in c.oracle.changes('final'):
            n = len(rows_for(rows, ch))
            if n > 1:
                doubled.append(f'{ph}:{ch.change_id}/{ch.sprint_id}x{n}')
    if doubled:
        return g(0, f'changes counted more than once: {sorted(set(doubled))[:6]}', CRITICAL_CHECKS['t_no_double_count'])
    return g(1, 'every change holds at most one ledger row in every phase (duplicated deliveries included)')


def _person_numbers(c: Ctx, sid: str) -> List[Tuple[Dict, object]]:
    """Every person-facing reading of a sprint's four numbers (as text) across the surfaces, each with the oracle of
    the moment it was read (surfaces after the live-UI slot see its changes; the Rovo action runs before it)."""
    out = []
    for r in c.sprint_renders():
        if str(r.get('sprintId')) == sid and r.get('metrics'):
            out.append((r['metrics'], c.oracle_for(r.get('afterLive', True))))
    for v in (c.ui().get('widget') or {}).get('views') or []:
        for s in v.get('sprints') or []:
            if str(s.get('id')) == sid and s.get('metrics'):
                out.append((s['metrics'], c.oracle_for(v.get('afterLive', False))))
    for call in c.rovo_calls('sprint'):
        res = call.get('result')
        if str(call.get('sprintId')) == sid and isinstance(res, dict):
            out.append(({'committed': _json_text(res.get('committed')), 'added': _json_text(res.get('added')),
                        'removed': _json_text(res.get('removed')),
                        'creep': '—' if res.get('creepPercent') is None else
                        (f"{Decimal(str(res['creepPercent'])):.1f}%" if isinstance(res.get('creepPercent'), (int, float))
                         else str(res.get('creepPercent')))}, c.oracle))
    return out


def _json_text(v):
    return None if isinstance(v, bool) or not isinstance(v, (int, float)) else format(Decimal(str(v)).normalize(), 'f')


def _sprint_numbers_right(c: Ctx, sid: str) -> bool:
    readings = _person_numbers(c, sid)
    return bool(readings) and any(metrics_ok(m, o.numbers(sid)) == 4 for m, o in readings)


@check('t_out_of_order', 'T', needs=('live', 'ui', 'rovo'))
def _(c):
    live = sorted((e for e in c.oracle.live if isinstance((e.get('delivery') or {}).get('slot'), int)),
                  key=lambda e: e['delivery']['slot'])
    permuted = set()
    for a, b in zip(live, live[1:]):
        if fo.instant(a['created']) > fo.instant(b['created']):
            permuted |= {str(a['changelogId']), str(b['changelogId'])}
    involved = [ch for ch in c.oracle.changes('live') if ch.change_id in permuted]
    if not involved:
        return unavail('the pack has no permuted pair touching an active sprint')
    rows = c.kvs_rows('live')
    ok_rows = sum(1 for ch in involved if len(rows_for(rows, ch)) == 1 and row_correct(rows_for(rows, ch)[0][0], ch, ch.sources))
    sprints = sorted({ch.sprint_id for ch in involved})
    ok_nums = sum(1 for sid in sprints if _sprint_numbers_right(c, sid))
    score = (ok_rows + ok_nums) / (len(involved) + len(sprints))
    return g(score, f'permuted deliveries: {ok_rows}/{len(involved)} rows exact, {ok_nums}/{len(sprints)} sprints '
             'with oracle numbers', 'membership follows delivery order instead of change time')


@check('t_multi_sprint_parse', 'T', needs=('rerun',))
def _(c):
    multi = set()
    for e in c.oracle.history + c.oracle.live:
        for item in c.oracle._sprint_items(e):
            if len(fo.sprint_ids(item.get('from'))) > 1 or len(fo.sprint_ids(item.get('to'))) > 1:
                multi.add(str(e['changelogId']))
    involved = [ch for ch in c.oracle.changes('final') if ch.change_id in multi]
    if not involved:
        return unavail('the pack has no multi-id Sprint change in an active sprint')
    rows = c.kvs_rows('rerun')
    ok = sum(1 for ch in involved if len(rows_for(rows, ch)) == 1 and row_correct(rows_for(rows, ch)[0][0], ch, ch.sources))
    return g(ok / len(involved), f'{ok}/{len(involved)} multi-id Sprint changes recorded exactly',
             'the Sprint `to` list parsed as one id')


@check('t_reestimate_followed', 'T', needs=('ui', 'rovo'))
def _(c):
    moved = c.oracle.reestimated_sprints()
    if not moved:
        return unavail('the pack has no live estimate change in an active sprint')
    ok = [sid for sid in moved if _sprint_numbers_right(c, sid)]
    return g(len(ok) / len(moved), f'{len(ok)}/{len(moved)} re-estimated sprints show the oracle numbers',
             'estimate changes do not move the numbers')


def _fault(c: Ctx, scope: str) -> Optional[Dict]:
    for f in c.pack.get('faults') or []:
        if (f.get('match') or {}).get('scope') == scope:
            return f
    return None


def _request_key(call: Dict) -> tuple:
    return (call.get('inv'), call.get('method'), call.get('path'), json.dumps(call.get('body'), sort_keys=True, default=str))


def _refused_in_window(calls: List[Dict], fault_id: Optional[str] = None) -> Tuple[List[Dict], List[Dict]]:
    """(early retries, concurrent refusals) among the calls the site refused inside an open fault window. Jira's
    Retry-After "Indicates how many seconds to wait before retrying" (rate-limiting page, RESEARCH §2): an early RETRY
    repeats a request the site had already refused with that fault (same invocation, method, path and body). A
    different request issued beside it — a concurrent branch that never saw the 429 (Luna 2026-10-03: the board
    list's second page 1 ms after the field list's 429, inside Promise.all) — is refused too, as Jira refuses it, but
    retried nothing."""
    early, concurrent, refused = [], [], set()
    for x in sorted(calls, key=lambda x: x.get('t') or 0):
        fid = x.get('earlyRetry') or x.get('fault')
        if not fid or (fault_id is not None and fid != fault_id):
            continue
        if x.get('earlyRetry'):
            (early if (fid, _request_key(x)) in refused else concurrent).append(x)
        if x.get('status') == 429:
            refused.add((fid, _request_key(x)))
    return early, concurrent


def _retry_verdict(c: Ctx, fault: Dict, phases: Tuple[str, ...]) -> Tuple[float, str]:
    calls = sorted(c.calls(phases, 'jira'), key=lambda x: x.get('t') or 0)
    hit = next((x for x in calls if x.get('fault') == fault['id']), None)
    ra = float(fault.get('retryAfter') or 0)
    early, concurrent = _refused_in_window(calls, fault['id'])
    if early:
        return 0.0, f"retried {len(early)}x inside the {ra:g}s Retry-After window"
    inv_id = hit.get('inv')
    inv = next((i for i in c.invocations(phases) if i.get('inv') == inv_id), {})
    deliveries = [d for ph in phases for d in c.phase(ph).get('deliveries') or [] if d.get('inv') == inv_id]
    later_same = [x for x in calls if x.get('inv') == inv_id and (x.get('t') or 0) > (hit.get('t') or 0)
                  and not any(x is y for y in concurrent)]
    if inv.get('threw') or inv.get('timedOut') or any(d.get('result') in ('throw', 'timeout') for d in deliveries):
        return 0.0, f'the invocation threw/timed out on the 429 (platform backoff, not Retry-After): {inv.get("error")}'
    retry_after = inv.get('retryAfter')
    if retry_after is None:
        retry_after = next((d.get('retryAfter') for d in deliveries if d.get('result') == 'retry'), None)
    if retry_after is not None:
        return (1.0, f'InvocationError retryAfter {retry_after}s >= {ra:g}s') if float(retry_after) >= ra else \
            (0.0, f'InvocationError retryAfter {retry_after}s < {ra:g}s')
    side = f'; {len(concurrent)} concurrent request(s) refused in the window, none a retry' if concurrent else ''
    if later_same:
        gap = (later_same[0].get('t') or 0) - (hit.get('t') or 0)
        return (1.0, f'waited {gap:.1f} virtual s in-invocation{side}') if gap >= ra else (0.0, f'retried after {gap:.1f}s < {ra:g}s')
    return 0.0, 'the 429 was never retried'


@check('t_retry_after_honoured', 'T', pre=pre_consumer_fault, needs=('live', 'heal'))
def _(c):
    score, why = _retry_verdict(c, _fault(c, 'consumer-of-change'), ('live', 'heal'))
    fault = _fault(c, 'consumer-of-change')
    target = str((fault.get('match') or {}).get('changelogId') or '')
    landed = [ch for ch in c.oracle.changes('final') if ch.change_id == target]
    once = all(len(rows_for(c.kvs_rows('heal'), ch)) == 1 for ch in landed) if landed else True
    if not once:
        score = 0.0
        why += '; the faulted change did not land exactly once'
    return g(score, why, 'the consumer ignores Retry-After')


@check('t_no_user_in_async', 'T', pre=pre_background_jira, needs=('backfill', 'live', 'heal', 'rerun'))
def _(c):
    user = [x for x in c.calls(service='jira', kinds=BACKGROUND_KINDS) if x.get('provider') == 'user']
    auth = [i for i in c.invocations() if i.get('kind') in BACKGROUND_KINDS
            and 'NeedsAuthentication' in str(i.get('errorName') or i.get('error') or '')]
    if user or auth:
        return g(0, f'{len(user)} asUser call(s), {len(auth)} NeedsAuthenticationError in background work',
                 'background work as a user')
    return g(1, 'background work is asApp only')


# ══ R: reconcile ═════════════════════════════════════════════════════════════════════════════

@check('r_backfill_complete', 'R', needs=('backfill',))
def _(c):
    if not c.modules('scheduledTrigger'):
        return absent('scheduledTrigger module')
    expected = c.oracle.changes('backfill')
    rows = c.kvs_rows('backfill')
    ok = [ch for ch in expected if rows_for(rows, ch) and row_correct(rows_for(rows, ch)[0][0], ch, ('reconcile',))]
    return g(len(ok) / len(expected) if expected else 0, f'{len(ok)}/{len(expected)} historical changes recorded '
             'exactly after the first scheduled run', CRITICAL_CHECKS['r_backfill_complete'],
             parts={'missing': [ch.key for ch in expected if ch not in ok][:12]})


@check('r_removals_found', 'R', needs=('backfill',))
def _(c):
    gone = set()
    for e in c.oracle.history:
        for item in c.oracle._sprint_items(e):
            if fo.sprint_ids(item.get('from')) and not fo.sprint_ids(item.get('to')):
                gone.add(str(e['changelogId']))
    expected = [ch for ch in c.oracle.changes('backfill') if ch.change_id in gone and ch.kind == 'removed']
    if not expected:
        return unavail('the pack has no removed-to-backlog change after an active start')
    rows = c.kvs_rows('backfill')
    ok = sum(1 for ch in expected if rows_for(rows, ch) and row_correct(rows_for(rows, ch)[0][0], ch, ('reconcile',)))
    return g(ok / len(expected), f'{ok}/{len(expected)} removed-to-backlog changes found',
             'issues that left every sprint are missed')


@check('r_heal_dropped', 'R', needs=('live', 'heal'))
def _(c):
    dropped = [ch for ch in c.oracle.changes('final') if ch.dropped]
    if not dropped:
        return unavail('the pack drops no live sprint change')
    before, after = c.kvs_rows('live'), c.kvs_rows('heal')
    ok = sum(1 for ch in dropped if len(rows_for(after, ch)) == 1 and row_correct(rows_for(after, ch)[0][0], ch, ch.sources))
    dropped_keys = {ch.key for ch in dropped}
    extra = 0
    for ch in c.oracle.changes('final'):
        if ch.key in dropped_keys:
            continue
        extra += max(0, len(rows_for(after, ch)) - max(1, len(rows_for(before, ch))))
    return g(ok / (len(dropped) + extra), f'{ok}/{len(dropped)} dropped changes healed exactly once; {extra} other row(s) added',
             'the stream is trusted; what it missed stays missing')


PAGED = (
    (re.compile(r'^/rest/api/3/search/jql$'), 'token'),
    (re.compile(r'^/rest/api/3/changelog/bulkfetch$'), 'token'),
    (re.compile(r'^/rest/api/3/issue/[^/]+/changelog$'), 'offset'),
    (re.compile(r'^/rest/agile/1\.0/board$'), 'offset'),
    (re.compile(r'^/rest/agile/1\.0/board/[^/]+/(?:sprint|issue|backlog)$'), 'offset'),
    (re.compile(r'^/rest/agile/1\.0/sprint/[^/]+/issue$'), 'offset'),
    # jsw.json: the software 1.0 issue lists page by nextPageToken / isLast
    (re.compile(r'^/rest/software/1\.0/(?:sprint|board)/[^/]+/(?:issue|backlog)$'), 'token'),
    (re.compile(r'^/rest/api/(?:2|3|latest)/search$'), 'legacy'),
)


def _page_style(call: Dict) -> Optional[str]:
    path = str(call.get('path') or '').split('?')[0]
    for rx, style in PAGED:
        if rx.match(path):
            return style
    return None


def _query(call: Dict) -> Dict[str, str]:
    from urllib.parse import parse_qsl
    q = str(call.get('path') or '').partition('?')[2]
    return dict(parse_qsl(q, keep_blank_values=True))


def _walks(calls: List[Dict]) -> Dict[tuple, List[Dict]]:
    walks: Dict[tuple, List[Dict]] = {}
    for call in calls:
        style = _page_style(call)
        if not style or call.get('status') != 200:
            continue
        q = {k: v for k, v in _query(call).items() if k not in ('nextPageToken', 'startAt')}
        body = call.get('body') if isinstance(call.get('body'), dict) else {}
        b = {k: v for k, v in body.items() if k not in ('nextPageToken', 'startAt')}
        key = (call.get('inv'), str(call.get('path')).split('?')[0], json.dumps(q, sort_keys=True),
               json.dumps(b, sort_keys=True, default=str))
        walks.setdefault(key, []).append(call)
    return walks


def _page_items(response: Dict) -> int:
    if isinstance(response.get('issueChangeLogs'), list):
        return sum(len(log.get('changeHistories') or []) for log in response['issueChangeLogs'] if isinstance(log, dict))
    return len(response.get('values') or response.get('issues') or response.get('histories') or [])


def _page_start(page: Dict) -> Optional[int]:
    response = page.get('response') if isinstance(page.get('response'), dict) else {}
    body = page.get('body') if isinstance(page.get('body'), dict) else {}
    for value in (response.get('startAt'), _query(page).get('startAt'), body.get('startAt'), 0):
        try:
            return int(value)
        except (TypeError, ValueError):
            continue
    return None


def _offered_more(style: str, page: Dict) -> bool:
    """The site's own answer on this page names a page after it (a nextPageToken, isLast false, or a total
    past the items served so far)."""
    response = page.get('response')
    if not isinstance(response, dict):
        return False
    if style == 'token':
        return bool(response.get('nextPageToken'))
    if 'isLast' in response:
        return response.get('isLast') is False
    total = response.get('total')
    return isinstance(total, int) and (_page_start(page) or 0) + _page_items(response) < total


def _walk_complete(style: str, pages: List[Dict]) -> Tuple[bool, str]:
    marks = []
    for p in pages:
        q, body = _query(p), p.get('body') if isinstance(p.get('body'), dict) else {}
        marks.append(q.get('nextPageToken') or body.get('nextPageToken') or q.get('startAt') or body.get('startAt') or '')
    if len(set(marks)) != len(marks):
        return False, 'a page was read twice'
    # Each page must continue where the site's previous answer left off: the token it handed out, or the offset
    # right after the items it served. Advancing by the REQUESTED maxResults skips items whenever the site serves
    # fewer, which Jira may ("API may return fewer items per page").
    for before, page in zip(pages, pages[1:]):
        prev = before.get('response') if isinstance(before.get('response'), dict) else None
        if prev is None:
            continue
        if style == 'token':
            body = page.get('body') if isinstance(page.get('body'), dict) else {}
            sent = _query(page).get('nextPageToken') or body.get('nextPageToken')
            if prev.get('nextPageToken') and sent != prev.get('nextPageToken'):
                return False, 'a page did not continue from the nextPageToken the site handed out'
        else:
            want = (_page_start(before) or 0) + _page_items(prev)
            if _page_start(page) != want:
                return False, f'a page started at {_page_start(page)}, the previous page ended at {want} (items skipped)'
    last = pages[-1].get('response')
    if not isinstance(last, dict):
        return False, 'no response body was recorded for the last page'
    if style == 'token':
        return (not last.get('nextPageToken')), 'stopped with a nextPageToken outstanding'
    if last.get('isLast') is True:
        return True, ''
    if last.get('isLast') is False:
        return False, 'stopped on isLast=false'
    values = last.get('values') or last.get('issues') or last.get('histories') or []
    total, start = last.get('total'), last.get('startAt') or 0
    if isinstance(total, int):
        return (start + len(values) >= total), f'stopped at {start + len(values)}/{total}'
    return False, 'the page carries neither isLast nor total'


def _endpoint(call: Dict) -> str:
    path = str(call.get('path') or '').split('?')[0]
    return re.sub(r'(?<!/api)/(?:\d+|[A-Z][A-Z0-9_]*-\d+)(?=/|$)', '/{id}', path)   # /rest/api/3 keeps its version


def _paged_walks(c: Ctx) -> Tuple[List[Dict], Dict[tuple, List[Dict]], Dict[tuple, List[Dict]]]:
    """(legacy calls, every walk, the walks the site served in two or more pages). A walk whose first page the site
    answered as the last one never ran the app's paging code: it is evidence of nothing about pagination."""
    calls = c.all_calls('jira')
    legacy = [x for x in calls if _page_style(x) == 'legacy']
    walks = _walks([x for x in calls if _page_style(x) != 'legacy'])
    paged = {k: p for k, p in walks.items() if len(p) > 1 or _offered_more(_page_style(p[0]), p[0])}
    return legacy, walks, paged


@check('r_pagination', 'R', pre=pre_paginated, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    legacy, walks, paged = _paged_walks(c)
    if any(not isinstance(x.get('response'), dict) for w in walks.values() for x in w[-1:]) and walks and \
            all(not isinstance(w[-1].get('response'), dict) for w in walks.values()):
        return unavail('no paginated response bodies were recorded')
    # The scoring site serves every list of >= 2 items in >= 2 pages (pack.paging, forge/site/limits.cjs): a
    # one-page answer holding two or more items means the site did not page it, a harness gap, never the app's.
    if (c.pack.get('paging') or {}).get('rule') == 'half':
        unpaged = sorted({_endpoint(p[0]) for k, p in walks.items() if k not in paged
                          and isinstance(p[0].get('response'), dict) and _page_items(p[0]['response']) >= 2})
        if unpaged:
            return unavail(f'the scoring site served a list of >= 2 items in one page on {unpaged}')
    done, bad = 0, []
    types: Dict[str, List[int]] = {}
    for key, pages in paged.items():
        complete, why = _walk_complete(_page_style(pages[0]), pages)
        done += complete
        tally = types.setdefault(_endpoint(pages[0]), [0, 0])
        tally[0] += complete
        tally[1] += 1
        if not complete:
            bad.append(f'{key[1]}: {why}')
    total = len(paged) + len(legacy)
    single = len(walks) - len(paged)
    return g(done / total if total else 0, f'{done}/{total} reads the site served in two or more pages walked to their '
             'end, each page once, continuing where the site left off ('
             + ', '.join(f'{ep} {ok}/{n}' for ep, (ok, n) in sorted(types.items())) + ')'
             + (f'; {single} one-page read(s), nothing to walk, not graded' if single else '')
             + (f'; {len(legacy)} call(s) to the removed /rest/api/3/search' if legacy else '')
             + (f'; {bad[:3]}' if bad else ''), 'pages silently skipped',
             parts={'paged_walks': len(paged), 'one_page_walks': single, 'legacy_calls': len(legacy),
                    'by_endpoint': {ep: {'complete': ok, 'walks': n} for ep, (ok, n) in sorted(types.items())}})


# The scheduled run each scripted fault targets (pack faults' match.run: 1 = the backfill, 2 = the heal).
SCHEDULED_RUN_PHASE = {1: 'backfill', 2: 'heal'}


@check('r_rate_limit', 'R', pre=pre_scheduled_fault, needs=('backfill', 'heal'))
def _(c):
    # Every scheduled-run 429 that fired: the dev site's (the backfill's 2nd request) and the scoring site's (the
    # backfill's first continuation page, the heal's 2nd request — 2026-10-03 stringency, F7), each retried after its
    # Retry-After with its run completing.
    fired = {call.get('fault') for call in c.all_calls('jira')}
    verdicts = []
    for fault in c.pack.get('faults') or []:
        match = fault.get('match') or {}
        phase = SCHEDULED_RUN_PHASE.get(match.get('run', 1))
        if match.get('scope') != 'scheduled-run' or fault['id'] not in fired or phase is None:
            continue
        score, why = _retry_verdict(c, fault, (phase,))
        sched = [i for i in c.invocations((phase,)) if i.get('kind') == 'scheduled']
        if score and not any(i.get('ok') for i in sched):
            score, why = 0.0, why + f'; the {phase} run did not complete'
        where = 'continuation page' if match.get('continuation') else f"request {match.get('nth')}"
        verdicts.append((score, f'{phase} {where}: {why}'))
    return g(sum(v for v, _w in verdicts) / len(verdicts), '; '.join(w for _v, w in verdicts),
             'the reconcile ignores Retry-After', parts={'faults': len(verdicts)})


@check('r_as_app', 'R', pre=pre_scheduled_read, needs=('backfill', 'heal', 'rerun'))
def _(c):
    calls = c.calls(service='jira', kinds=('scheduled',))
    app = sum(1 for x in calls if x.get('provider') == 'app')
    return g(app / len(calls), f'{app}/{len(calls)} scheduled-run Jira calls asApp', 'reconcile as a user')


@check('r_completes_in_timeout', 'R', needs=('backfill', 'heal', 'rerun'),
       pre=lambda c: (any(x.get('kind') == 'scheduled' for x in c.calls(service='jira')),
                      'a scheduled trigger made >= 1 Jira call'))
def _(c):
    sched = [i for i in c.invocations() if i.get('kind') == 'scheduled']
    ok = sum(1 for i in sched if not i.get('timedOut'))
    return g(ok / len(sched) if sched else 0, f'{ok}/{len(sched)} scheduled invocations inside the module timeout',
             'the scheduled job is killed by its timeout')


# Contract §3 states it as behaviour, not as economy: "a run with nothing new writes nothing" (promoted from the E slice
# 2026-10-03: a stated requirement earns weight, and a rerun that rewrites the ledger is a defect, not a missed bonus).
@check('r_idempotent_rerun', 'R', pre=pre_ledger_row, needs=('heal', 'rerun'))
def _(c):
    rows = len([1 for tok, _it in c.kvs_rows('heal') if any(ch.change_id in tok for ch in c.oracle.changes('final'))])
    if not rows:
        return g(0, 'no ledger rows after the heal — nothing to keep idempotent', 'the rerun is not judged')
    writes = sum(_entity_writes(x) for x in c.calls(('rerun',), 'kvs'))
    return g(ladder(writes / rows, TH['idempotent_rerun_rungs']), f'{writes} entity write(s) on a no-change run over {rows} rows',
             'a scheduled run with nothing new rewrites the ledger', parts={'writes': writes, 'rows': rows})


# ══ S: storage ═══════════════════════════════════════════════════════════════════════════════

def _kvs_op(call: Dict) -> str:
    return str(call.get('path') or '').split('?')[0].replace('/api/v1/', '')


def _is_kvs_write(call: Dict) -> bool:
    return call.get('service') == 'kvs' and _kvs_op(call) in (
        'set', 'entity/set', 'batch/set', 'delete', 'entity/delete', 'batch/delete', 'transaction')


def _entity_writes(call: Dict) -> int:
    op, body = _kvs_op(call), call.get('body') if isinstance(call.get('body'), dict) else {}
    if op in ('entity/set', 'entity/delete'):
        return 1
    if op in ('batch/set', 'batch/delete'):
        items = body.get('items') if isinstance(body.get('items'), list) else body if isinstance(body, list) else []
        return sum(1 for it in items if isinstance(it, dict) and it.get('entityName'))
    if op == 'transaction':
        n = 0
        for key in ('set', 'delete'):
            n += sum(1 for it in body.get(key) or [] if isinstance(it, dict) and it.get('entityName'))
        return n
    return 0


def _ledger_entities(c: Ctx) -> set:
    names = set()
    snap = c.phase('rerun').get('kvsAfter') or c.phase('heal').get('kvsAfter') or {}
    for name, items in (snap.get('entities') or {}).items():
        if c.oracle and any(rows_for([(_tokens(it), it)], ch) for it in items or [] for ch in c.oracle.changes('final')):
            names.add(name)
    return names


def _ledger_reads(c: Ctx) -> List[Dict]:
    ledger = _ledger_entities(c)
    out = []
    for call in c.all_calls('kvs'):
        op = _kvs_op(call)
        body = call.get('body') if isinstance(call.get('body'), dict) else {}
        # A list read of the ledger; a get by key (an idempotency check) is not a scan and not graded here.
        if op == 'entity/query' and body.get('entityName') in ledger:
            out.append(call)
    return out


@check('s_storage_scope', 'S', pre=pre_kvs_call, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    denied = [x for x in c.all_calls('kvs') if x.get('status') == 403]
    if denied:
        return g(0, f'{len(denied)} KVS call(s) refused with 403 (storage:app missing)', 'storage unavailable')
    return g(1, 'KVS used, no 403 from the KVS proxy')


@check('s_entity_index_used', 'S', pre=pre_ledger_read, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    reads = _ledger_reads(c)
    indexes = _sprint_indexes(c)
    sprints = set(c.oracle.sprints) if c.oracle else set()
    ok = 0
    for call in reads:
        body = call['body']
        part = body.get('partition') or []
        if (_kvs_op(call) == 'entity/query' and body.get('indexName') in indexes.get(body.get('entityName'), set())
                and len(part) == 1 and _leaf_text(part[0]) in sprints):
            ok += 1
    return g(ok / len(reads), f'{ok}/{len(reads)} ledger reads are entity queries on the sprint index',
             'the ledger is scanned instead of read by its index')


@check('s_index_order', 'S', pre=pre_table_rows, needs=('ui',))
def _(c):
    renders = [r for r in c.sprint_renders() if r.get('rows')]
    ok = 0
    for r in renders:
        o = c.oracle_for(r.get('afterLive', True))
        expected = [ch.change_id for ch in o.visible_changes(str(r.get('sprintId')), o.viewer)]
        shown = [row.get('changeId') for row in r['rows'] if row.get('changeId') in expected]
        ok += shown == [cid for cid in expected if cid in shown]
    return g(ok / len(renders), f'{ok}/{len(renders)} tables list changes in change-time order',
             'index order lost')


@check('s_limits', 'S', pre=pre_kvs_write, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    errs = [x for x in c.all_calls('kvs') if x.get('limitError') or x.get('status') == 413]
    if errs:
        return g(0, f'{len(errs)} KVS limit error(s): {[x.get("limitError") or x.get("status") for x in errs[:4]]}',
                 'storage writes rejected')
    return g(1, 'no KVS limit errors')


# ══ B: resolvers and person-facing data ═════════════════════════════════════════════════════

def _invokes(c: Ctx) -> List[Dict]:
    return [r for r in c.ui().get('invokeResponses') or [] if isinstance(r, dict)]


@check('b_invoke_contract', 'B', pre=pre_invoke, needs=('ui',))
def _(c):
    inv = _invokes(c)
    bad = [r for r in inv if r.get('threw') or r.get('undefined') or r.get('unknownKey')]
    share = 1 - len(bad) / len(inv)
    detail = f'{len(inv) - len(bad)}/{len(inv)} invokes returned a defined, structured result' + (
        f"; bad: {[(r.get('functionKey'), r.get('error')) for r in bad[:3]]}" if bad else '')
    # Contract §2: "a failure (a Jira error, …) returns a value describing it … and the surface shows it". The scoring
    # site's resolver-read 500 (DESIGN §5.2): judged only when it reached a resolver's Jira read (an app that read no
    # Jira from that surface met no failure) and only on observations that carry it (older ones stay silent).
    rf = c.ui().get('resolverFault')
    if isinstance(rf, dict) and rf.get('fired'):
        hit = rf.get('invoke') or {}
        handled = {'returned_a_value': bool(rf.get('invoke')) and not hit.get('threw'),
                   'surface_shows_it': not rf.get('blank') and not rf.get('pageErrors')}
        ok = all(handled.values())
        return g((share + (1.0 if ok else 0.0)) / 2, detail + f"; a Jira 500 on {hit.get('functionKey') or 'a resolver'}'s "
                 f"read: {'handled' if ok else ', '.join(k for k, v in handled.items() if not v) + ' FAILED'}"
                 + ('' if ok else f" ({hit.get('error') or rf.get('pageErrors') or 'blank surface'})"),
                 'resolver contract broken', parts={'invoke_share': round(share, 4), 'jira_error': handled})
    return g(share, detail, 'resolver contract broken')


def _leak_hits(text: str, terms: Dict[str, List[str]]) -> List[str]:
    hits = [k for k in terms['keys'] if re.search(r'(?<![A-Za-z0-9])' + re.escape(k) + r'(?![0-9])', text)]
    hits += [s for s in terms['summaries'] if s and s in text]
    return hits


@check('b_no_permission_leak', 'B', pre=pre_person_list, needs=('ui', 'rovo'))
def _(c):
    found = []
    viewer_terms = c.oracle.leak_terms(c.oracle.viewer)
    for s in c.surfaces():
        found += [f"{s.get('id')}: {h}" for h in _leak_hits('\n'.join(map(str, s.get('texts') or [])), viewer_terms)]
        found += [f"{s.get('id')}: data-change-id {cid}" for cid in s.get('changeIdAttrs') or []
                  if str(cid) in viewer_terms['changeIds']]
    for r in _invokes(c):
        dump = json.dumps(r.get('response'), default=str)
        found += [f"invoke {r.get('functionKey')}: {h}" for h in _leak_hits(dump, viewer_terms)]
    # LLM prompts sent from a person-facing invocation hold only what that person may browse (DESIGN 2006de559:
    # b_llm_prompt_no_leak folded in).
    for e in _llm_calls(c):
        if e.get('moduleType') in BACKGROUND_TYPES:
            continue
        person = str(e.get('asUser') or c.oracle.viewer)
        prompt = ' '.join(str(m.get('content') if isinstance(m.get('content'), str) else json.dumps(m.get('content')))
                          for m in ((e.get('request') or {}).get('messages') or []) if isinstance(m, dict))
        terms = c.oracle.leak_terms(person)
        found += [f"LLM prompt ({e.get('moduleKey')}): {h}" for h in _leak_hits(prompt, terms)]
        found += [f'LLM prompt: change {x}' for x in terms['changeIds'] if re.search(r'(?<!\d)' + x + r'(?!\d)', prompt)]
    for call in c.rovo_calls():
        terms = c.oracle.leak_terms(str(call.get('as')))
        res = call.get('result')
        found += [f"rovo as {call.get('as')}: {h}" for h in _leak_hits(json.dumps(res, default=str), terms)]
        if isinstance(res, dict):
            found += [f"rovo changeId {ch.get('changeId')}" for ch in res.get('changes') or []
                      if isinstance(ch, dict) and str(ch.get('changeId')) in terms['changeIds']]
    if found:
        return g(0, f'hidden issue data shown to a person: {found[:6]}', CRITICAL_CHECKS['b_no_permission_leak'])
    return g(1, 'no hidden key, summary or change id reached a person')


@check('b_hidden_count', 'B', needs=('ui', 'rovo'))
def _(c):
    checks, ok = 0, 0
    for r in c.sprint_renders():
        sid = str(r.get('sprintId'))
        o = c.oracle_for(r.get('afterLive', True))
        if sid in o.all_numbers():
            checks += 1
            ok += _num(r.get('hiddenCount')) == o.hidden_count(sid, o.viewer)
    for call in c.rovo_calls('sprint'):
        sid = str(call.get('sprintId'))
        if sid in c.oracle.all_numbers():
            checks += 1
            res = call.get('result') if isinstance(call.get('result'), dict) else {}
            ok += res.get('hiddenChanges') == c.oracle.hidden_count(sid, str(call.get('as')))
    if not checks:
        return absent('hidden-count / hiddenChanges')
    return g(ok / checks, f'{ok}/{checks} hidden counts equal the oracle', 'hidden counts wrong')


ADF_NODE_TYPES = {'doc', 'paragraph', 'text', 'heading', 'bulletList', 'orderedList', 'listItem', 'hardBreak',
                  'rule', 'blockquote', 'codeBlock', 'panel', 'table', 'tableRow', 'tableHeader', 'tableCell',
                  'mention', 'emoji', 'inlineCard', 'blockCard', 'status', 'date', 'expand', 'mediaSingle',
                  'media', 'mediaGroup', 'nestedExpand', 'taskList', 'taskItem', 'decisionList', 'decisionItem',
                  'placeholder'}


def adf_text(node, depth: int = 0) -> Optional[str]:
    """Structural ADF check (doc, version 1, every node typed from the ADF node set, text leaves carry
    text) returning the document's text, or None when it is not an ADF document. The site already
    rejects non-ADF bodies with Jira's 400; this reads what was stored."""
    if depth == 0:
        if not isinstance(node, dict) or node.get('type') != 'doc' or node.get('version') != 1 \
                or not isinstance(node.get('content'), list):
            return None
    if not isinstance(node, dict) or node.get('type') not in ADF_NODE_TYPES or depth > 31:
        return None
    if node['type'] == 'text':
        return node.get('text') if isinstance(node.get('text'), str) and node['text'] else None
    parts = []
    for child in node.get('content') or []:
        t = adf_text(child, depth + 1)
        if t is None:
            return None
        parts.append(t)
    if node['type'] in ('mention', 'emoji', 'inlineCard', 'status', 'date'):
        attrs = node.get('attrs') or {}
        parts.append(str(attrs.get('text') or attrs.get('url') or ''))
    return ' '.join(parts)


# Jira Cloud answers a comment POST without ADD_COMMENTS with 400 {"errorMessages":["<display name>, you do not have the
# permission to comment on this issue."],"errors":{}} — not 403 (measured on wolfaenpak 2026-10-03, WP1 d21b58a53).
COMMENT_FORBIDDEN_MESSAGE = re.compile(r'you do not have the permission to comment on this issue', re.I)


def comment_refused(x: Dict) -> bool:
    """A comment POST the platform refused (rate limit or no permission) — an attempt that did not land."""
    if x.get('status') in (429, 403):
        return True
    return x.get('status') == 400 and any(COMMENT_FORBIDDEN_MESSAGE.search(str(m)) for m in x.get('errorMessages') or [])


@check('b_comment_adf_as_user', 'B', pre=pre_comment_post, needs=('ui',))
def _(c):
    attempts = [x for x in c.obs.get('comments') or [] if not comment_refused(x)]
    if not attempts:
        return g(0, 'every comment POST was rate-limited or forbidden; none landed', 'no comment')
    ok, notes = 0, []
    for x in attempts:
        # The comment path names the issue by key or by id (both are Jira's issueIdOrKey).
        o = c.oracle_ui   # comments are posted from the sprint action, after the live-UI slot
        issue = o.issue_by_key.get(x.get('issueKey')) or o.issues.get(str(x.get('issueKey')))
        key = issue['key'] if issue else None
        sprint_ids = [sid for sid in o.all_numbers()
                      if any(ch.issue_key == key for ch in o.numbers(sid).changes)]
        text = adf_text(x.get('body')) if x.get('status') in (200, 201) else None
        good = text is not None and x.get('provider') == 'user' and x.get('accountId') == o.viewer \
            and issue is not None and issue['key'] in text and any(
                o.numbers(sid).name in text and fo.format_creep(o.numbers(sid).creep).rstrip('%') in text
                for sid in sprint_ids)
        ok += good
        if not good:
            notes.append(f"{x.get('issueKey')} status {x.get('status')} via {x.get('provider')}")
    return g(ok / len(attempts), f'{ok}/{len(attempts)} comments are ADF as the viewer naming key, sprint and creep'
             + (f'; {notes[:3]}' if notes else ''), 'comment body or author wrong')


def _post_scenarios(c: Ctx) -> List[Dict]:
    out = []
    for r in c.sprint_renders():
        for name in ('post', 'doubleClick'):
            if isinstance(r.get(name), dict):
                out.append({**r[name], '_name': name, '_sprint': r.get('sprintId')})
    return out


@check('b_comment_exactly_once', 'B', pre=pre_comment_post, needs=('ui',))
def _(c):
    scen = _post_scenarios(c)
    if not scen:
        return g(0, 'no post scenario completed', CRITICAL_CHECKS['b_comment_exactly_once'])
    bad = [f"{s['_name']}@{s['_sprint']}: {s.get('commentsAdded')} comment(s), {s.get('successFlags')} success flag(s)"
           for s in scen if s.get('commentsAdded') != 1 or s.get('successFlags') != 1]
    if bad:
        return g(0, f'not exactly once: {bad[:4]}', CRITICAL_CHECKS['b_comment_exactly_once'])
    return g(1, f'{len(scen)} click/double-click posts each ended with exactly one comment and one success flag '
             '(the 429-once fault included)')


# ══ U: UI function ═══════════════════════════════════════════════════════════════════════════

def _widget(c: Ctx) -> Dict:
    return c.ui().get('widget') or {}


@check('u_widget_loads', 'U', needs=('ui',))
def _(c):
    if not c.modules('dashboards:widget'):
        return absent('dashboards:widget view')
    views = _widget(c).get('views') or []
    live = [v for v in views if any(all(str((s.get('metrics') or {}).get(m) or '').strip()
                                        for m in ('committed', 'added', 'removed', 'creep')) for s in v.get('sprints') or [])]
    return g(1 if live else 0, f'{len(live)}/{len(views)} configured widget views render a sprint with its numbers',
             CRITICAL_CHECKS['u_widget_loads'])


@check('u_widget_numbers', 'U', needs=('ui',))
def _(c):
    views = _widget(c).get('views') or []
    if not views:
        return absent('configured widget view')
    total = 0.0
    for v in views:
        o = c.oracle_for(v.get('afterLive', False))
        expected = o.sprints_of_board(str(v.get('board')))
        shown = v.get('sprints') or []
        right = sum(1 for i, sid in enumerate(expected) if i < len(shown) and str(shown[i].get('id')) == sid
                    and metrics_ok(shown[i].get('metrics'), o.numbers(sid)) == 4)
        total += right / max(len(expected), len(shown)) if (expected or shown) else 1.0
    return g(total / len(views), f'widget numbers: {total:.2f}/{len(views)} views exact (sprints by startDate, '
             'four §1 numbers each)', 'wrong numbers on the dashboard')


def chart_ok(chart: Dict, expected: List[str], numbers: Dict[str, fo.SprintNumbers], tol: float) -> Tuple[bool, str]:
    if not chart or not chart.get('present') or chart.get('count', 1) != 1:
        return False, 'no single svg[data-testid=chart]'
    rects = {(str(r.get('sprintId')), r.get('series')): r for r in chart.get('rects') or []}
    want = [(sid, s) for sid in expected for s in ('committed', 'added', 'removed')]
    if set(rects) != set(want) or len(chart.get('rects') or []) != len(want):
        return False, f'{len(chart.get("rects") or [])} rects for {len(want)} sprint x series'
    pairs = [(float(getattr(numbers[sid], s)), float(rects[(sid, s)].get('height') or 0)) for sid, s in want]
    den = sum(v * v for v, _h in pairs)
    if den == 0:
        return all(h <= tol for _v, h in pairs), 'all values zero'
    k = sum(v * h for v, h in pairs) / den
    worst = max(abs(h - k * v) for v, h in pairs)
    return worst <= tol, f'worst bar off its linear scale by {worst:.2f}px'


@check('u_widget_chart', 'U', needs=('ui',))
def _(c):
    views = _widget(c).get('views') or []
    if not views:
        return absent('configured widget view')
    tol = float(TH['chart_tolerance_px'])
    ok, notes = 0, []
    for v in views:
        o = c.oracle_for(v.get('afterLive', False))
        expected = o.sprints_of_board(str(v.get('board')))
        good, why = chart_ok(v.get('chart') or {}, expected, o.all_numbers(), tol)
        ok += good
        if not good:
            notes.append(why)
    return g(ok / len(views), f'{ok}/{len(views)} charts on one linear scale within {tol:g}px'
             + (f'; {notes[:2]}' if notes else ''), 'the chart misstates the numbers')


@check('u_widget_edit_config', 'U', needs=('ui',))
def _(c):
    if not c.modules('dashboards:widget'):
        return absent('dashboards:widget')
    w, e = _widget(c), c.ui().get('edit') or {}
    conds = {'no_config_needs_config_only': bool((w.get('noConfig') or {}).get('onlyNeedsConfig'))}
    for i, p in enumerate(e.get('picks') or []):
        want = c.oracle.sprints_of_board(str(p.get('board')))
        conds[f'pick{i}_view_shows_board'] = [str(x) for x in p.get('viewSprints') or []] == want and bool(want)
        conds[f'pick{i}_reopen_pressed'] = [str(x) for x in p.get('reopenPressed') or []] == [str(p.get('board'))]
    second = w.get('secondInstance') or {}
    want2 = c.oracle.sprints_of_board(str(second.get('configBoard')))
    conds['second_instance_own_board'] = bool(want2) and [str(x) for x in second.get('sprints') or []] == want2
    if len(conds) < 4:
        conds['picks_recorded'] = False
    met = sum(conds.values())
    return g(met / len(conds), f'edit/config {met}/{len(conds)}: ' + ', '.join(k for k, v in conds.items() if not v),
             'the board choice does not reach the view', parts=conds)


def cells_ok(row: Dict, ch: fo.Change) -> bool:
    cells = row.get('cells') or {}
    return (str(cells.get('issue') or '').strip() == ch.issue_key and points_text_ok(cells.get('points'), ch.points)
            and str(cells.get('kind') or '').strip() == ch.kind and str(cells.get('by') or '').strip() == ch.by_name
            and fo.instant(cells.get('at')) == ch.at and str(cells.get('source') or '').strip() in ch.sources)


TABLE_COLS = ('issue', 'points', 'kind', 'by', 'at', 'source')


@check('u_ledger_table', 'U', needs=('ui',))
def _(c):
    if not c.modules('jira:sprintAction'):
        return absent('jira:sprintAction')
    renders = [r for r in c.sprint_renders() if str(r.get('sprintId')) in c.oracle_ui.all_numbers()]
    if not renders:
        return absent('sprint action table')
    total = 0.0
    for r in renders:
        o = c.oracle_for(r.get('afterLive', True))
        expected = o.visible_changes(str(r['sprintId']), o.viewer)
        rows = r.get('rows') or []
        right = sum(1 for i, ch in enumerate(expected) if i < len(rows) and rows[i].get('changeId') == ch.change_id
                    and cells_ok(rows[i], ch))
        headers_ok = set(TABLE_COLS) <= set(r.get('headers') or [])
        denom = max(len(expected), len(rows))
        total += (right / denom if denom else 1.0) * (1 if headers_ok else 0.5)
    return g(total / len(renders), f'ledger tables: {total:.2f}/{len(renders)} exact (rows, cells, default order)',
             'the sprint ledger is wrong')


def _sorted_ok(ids: List[str], expected: List[fo.Change], key) -> bool:
    by_id = {ch.change_id: ch for ch in expected}
    if sorted(ids) != sorted(by_id):
        return False
    return [key(by_id[i]) for i in ids] == sorted(key(ch) for ch in expected)


@check('u_ledger_sort', 'U', pre=pre_table_rows, needs=('ui',))
def _(c):
    total, ok, after_other = 0, 0, 0
    for r in c.sprint_renders():
        if not r.get('rows'):
            continue
        o = c.oracle_for(r.get('afterLive', True))
        expected = o.visible_changes(str(r.get('sprintId')), o.viewer)
        sort_at = r.get('sortAt') or []
        first = sort_at[0] if len(sort_at) > 0 else {}
        second = sort_at[1] if len(sort_at) > 1 else {}
        # Contract §5: the `at` toggle flips between the default order and its EXACT reverse (the points sort was
        # removed from the contract in 2006de559).
        subs = [
            list(first.get('rows') or []) == [ch.change_id for ch in reversed(expected)]
            and (first.get('ariaSort') or {}).get('at') == 'descending',
            [x for x in second.get('rows') or []] == [ch.change_id for ch in expected]
            and (second.get('ariaSort') or {}).get('at') == 'ascending',
        ]
        # "… starting with ascending when another sort was active": held only where another header's click made ITS
        # sort the active one (the probe's sortAfterOther; an app whose other headers do not sort is not held to it).
        other = r.get('sortAfterOther') or {}
        if other.get('col'):
            subs.append(list(other.get('rows') or []) == [ch.change_id for ch in expected]
                        and (other.get('ariaSort') or {}).get('at') == 'ascending')
            after_other += 1
        total += len(subs)
        ok += sum(subs)
    return g(ok / total if total else 0, f'{ok}/{total} at-toggle states ordered with aria-sort'
             + (f' ({after_other} after another sort was active)' if after_other else ''), 'sorting broken')


@check('u_issue_router', 'U', pre=pre_table_rows, needs=('ui',))
def _(c):
    clicks = [x for r in c.sprint_renders() for x in r.get('router') or []]
    if not clicks:
        return g(0, 'no issue-key click was recorded', 'no router link')
    ok = sum(1 for x in clicks if not x.get('popup') and not x.get('topNavigation') and any(
        (op.get('op') in ('open', 'navigate')) and str(op.get('url') or op.get('target') or '').rstrip('/')
        .endswith(f"/browse/{x.get('issueKey')}") for op in x.get('ops') or []))
    return g(ok / len(clicks), f'{ok}/{len(clicks)} issue keys open /browse/<KEY> through the Forge router',
             'links escape the router')


@check('u_comment_flow', 'U', needs=('ui',))
def _(c):
    renders = [r for r in c.sprint_renders() if isinstance(r.get('post'), dict)]
    if not renders:
        return absent('post-summary flow')
    total, ok = 0, 0
    for r in renders:
        post, forb = r.get('post') or {}, r.get('forbidden') or {}
        subs = [bool((r.get('select') or {}).get('ariaSelected')), post.get('successFlags') == 1]
        if forb:
            subs += [(forb.get('errorFlags') or 0) >= 1 and not forb.get('successFlags'), bool(forb.get('sortWorksAfter'))]
        total += len(subs)
        ok += sum(subs)
    return g(ok / total, f'{ok}/{total} comment-flow steps right (select, success flag, forbidden error flag, modal works)',
             'comment flow broken')


@check('u_modal_close', 'U', pre=pre_modal_rendered, needs=('ui',))
def _(c):
    renders = [r for r in c.sprint_renders() if isinstance(r.get('close'), dict)]
    ok = sum(1 for r in renders if r['close'].get('closeCalled'))
    return g(ok / len(renders) if renders else 0, f'{ok}/{len(renders)} close clicks reached bridge close',
             'the modal cannot be closed')


@check('u_not_started', 'U', needs=('ui',))
def _(c):
    if not c.modules('jira:sprintAction'):
        return absent('jira:sprintAction')
    ns = c.ui().get('notStarted') or {}
    return g(1 if ns.get('onlyNotStarted') else 0, 'future sprint shows not-started only' if ns.get('onlyNotStarted')
             else f'future sprint context: {ns.get("detail") or "not-started not shown alone"}', 'not-started state wrong')


# ══ Forge LLM, Realtime and rovo:mcp (DESIGN 2006de559 / 90ca7eb72) ══════════════════════════════════

REPORT_TOOL = 'report_scope'
# site/llm.cjs SCRIPT: the probe restarts the site's script before the explain clicks, so click i meets answer i.
EXPLAIN_SCRIPT = ('clean', 'digits', 'refusal', 'malformed', 'error')


def _llm_entries(c: Ctx, op: Optional[str] = None) -> List[Dict]:
    return [e for e in (c.obs.get('llm') or {}).get('entries') or [] if op is None or e.get('op') == op]


def _llm_calls(c: Ctx) -> List[Dict]:
    return [e for e in _llm_entries(c) if e.get('op') in ('chat', 'stream')]


def _listed_models(c: Ctx) -> Optional[set]:
    """The model ids the site's list() returns (DESIGN §17.4: the public models page, all active), or None when the
    probe did not record them (then the log entry's own modelStatus decides)."""
    models = (c.obs.get('llm') or {}).get('models')
    if not models:
        return None
    return {str(m.get('model') if isinstance(m, dict) else m) for m in models}


def _unlisted_models(c: Ctx) -> List[str]:
    listed = _listed_models(c)
    if listed is None:
        return sorted({str(e.get('model')) for e in _llm_calls(c) if e.get('modelStatus') != 'active'})
    return sorted({str(e.get('model')) for e in _llm_calls(c) if str(e.get('model')) not in listed})


def _realtime_events(c: Ctx) -> List[Dict]:
    return [e for e in (c.obs.get('realtime') or {}).get('events') or [] if isinstance(e, dict)]


@check('k_rovo_mcp', 'K')
def _(c):
    mcps = c.modules('rovo:mcp')
    if not mcps:
        return absent('rovo:mcp module')
    m = mcps[0]
    tools = [t if isinstance(t, str) else (t or {}).get('action') for t in m.get('tools') or []]
    # docs /manifest-reference/modules/rovo-mcp/: "An app can have at most one rovo:mcp module"; schema 13.6.0:
    # name 1-30 characters, tools are action-key strings.
    conds = {'exactly_one': len(mcps) == 1, 'name_1_30': 1 <= len(str(m.get('name') or '')) <= 30,
             'exposes_action': ACTION_KEY in tools, 'tools_are_action_keys': all(isinstance(t, str) for t in m.get('tools') or [])}
    met = sum(conds.values())
    return g(met / len(conds), f'rovo:mcp {met}/{len(conds)}: ' + ', '.join(k for k, v in conds.items() if not v),
             'the MCP server is not valid or does not expose the action', parts=conds)


@check('k_llm_model_current', 'K', pre=lambda c: (bool(_llm_calls(c)), '>= 1 LLM call'), needs=('ui',))
def _(c):
    llm = c.modules('llm')
    claude = any('claude' in [str(x) for x in (m.get('model') or [])] for m in llm)
    calls = _llm_calls(c)
    stale = _unlisted_models(c)
    conds = {'llm_module_with_claude': claude, 'every_call_names_a_listed_model': not stale}
    met = sum(conds.values())
    return g(met / 2, f'{len(calls)} LLM call(s); model ids list() does not return: {stale}; llm module with claude: {claude}',
             'a hard-coded model id the platform does not offer (DESIGN §17.4)', parts=conds)


def _payload_tokens(payload) -> Tuple[List[str], List[str]]:
    """(numeric tokens, string tokens) of a realtime payload: parsed as JSON when it is JSON, else the raw string."""
    value = payload
    if isinstance(payload, str):
        try:
            value = json.loads(payload)
        except ValueError:
            value = payload
    nums, strs = [], []
    for leaf in fo.iter_leaves(value) if isinstance(value, (dict, list)) else [value]:
        if isinstance(leaf, (int, float)) and not isinstance(leaf, bool):
            nums.append(_leaf_text(leaf))
        elif isinstance(leaf, str):
            (nums if re.fullmatch(r'\d+(\.\d+)?', leaf) else strs).append(leaf)
    return nums, strs


@check('b_realtime_payload_clean', 'B', pre=lambda c: (bool(_realtime_events(c)), '>= 1 realtime publish'), needs=('ui',))
def _(c):
    o = c.oracle_ui
    sprints = set(o.sprints)
    keys = [i['key'] for i in o.issues.values()]
    summaries = [i.get('summary') for i in o.issues.values() if i.get('summary')]
    accounts = set(o.users)
    change_ids = {str(e['changelogId']) for e in o.history + o.live}
    bad = []
    for e in _realtime_events(c):
        nums, strs = _payload_tokens(e.get('payload'))
        text = ' '.join(strs)
        why = [f'number {n}' for n in nums if n not in sprints]
        why += [k for k in keys if re.search(r'(?<![A-Za-z0-9])' + re.escape(k) + r'(?![0-9])', text)]
        why += ['a summary' for x in summaries if x in text][:1]
        why += [f'account {a}' for a in accounts if a in text]
        why += [f'change {x}' for x in change_ids if re.search(r'(?<!\d)' + x + r'(?!\d)', text)]
        if why:
            bad.append(f"{e.get('channel')}: {why[:3]}")
    n = len(_realtime_events(c))
    return g(1 - len(bad) / n, f'{n - len(bad)}/{n} realtime payloads hold sprint ids only' + (f'; {bad[:3]}' if bad else ''),
             'issue data broadcast on a channel that does not enforce permissions')


@check('u_widget_live', 'U', pre=pre_widget_sprints, needs=('ui',))
def _(c):
    live = c.ui().get('live') or {}
    if live.get('absent'):
        return unavail(live['absent'])
    if not live:
        return g(0, 'the live widget step did not run', 'the widget never updates live')
    want = c.oracle_ui.sprints_of_board(str(live.get('board')))
    after = {str(x.get('id')): x.get('metrics') for x in live.get('sprintsAfter') or []}
    right = [sid for sid in want if metrics_ok(after.get(sid) or {}, c.oracle_ui.numbers(sid)) == 4]
    moved = [sid for sid in want if c.oracle.numbers(sid).metrics_text() != c.oracle_ui.numbers(sid).metrics_text()]
    conds = {'subscribed': bool(live.get('subscribed')), 'no_polling_while_idle': live.get('idleInvokes') == 0,
             'no_reload': not live.get('reloaded'), 'shows_new_numbers': bool(moved) and set(moved) <= set(right)
             and len(right) == len(want)}
    met = sum(conds.values())
    return g(met / len(conds), f"live widget {met}/{len(conds)}: " + ', '.join(k for k, v in conds.items() if not v)
             + f"; {live.get('idleInvokes')} idle invoke(s), sprints right after the live changes {len(right)}/{len(want)}",
             'the dashboard does not update live', parts=conds)


ISSUE_KEY = re.compile(r'(?<![A-Za-z0-9])[A-Z][A-Z0-9]+-\d+(?![0-9])')


def explanation_numbers(own_text: str, sprint_name: str) -> List[Decimal]:
    """The numbers of the explanation's own sentence: issue keys and the sprint's name are names, not numbers."""
    text = ISSUE_KEY.sub(' ', own_text.replace(sprint_name, ' ') if sprint_name else own_text)
    return [Decimal(x) for x in re.findall(r'\d+(?:\.\d+)?', text)]


def explanation_ledger_numbers(o, sid: str) -> set:
    """What "the ledger's numbers" can be for the viewer: the four §1 numbers, the hidden count, the counts of the
    visible changes (all, added, removed) and each visible change's points."""
    n = o.numbers(sid)
    visible = o.visible_changes(sid, o.viewer)
    out = {n.committed, n.added, n.removed, Decimal(o.hidden_count(sid, o.viewer)), Decimal(len(visible)),
           Decimal(sum(1 for ch in visible if ch.kind == 'added')), Decimal(sum(1 for ch in visible if ch.kind == 'removed'))}
    out |= {ch.points for ch in visible}
    if n.creep is not None:
        out.add(n.creep)
    return out


def _explain_request_ok(entry: Dict) -> bool:
    req = entry.get('request') or {}
    tools = [t.get('function') or {} for t in req.get('tools') or [] if isinstance(t, dict)]
    tool = next((t for t in tools if t.get('name') == REPORT_TOOL), None)
    props = ((tool or {}).get('parameters') or {}).get('properties') or {}
    shape = (props.get('summary') or {}).get('type') == 'string' and (props.get('changeIds') or {}).get('type') == 'array' \
        and ((props.get('changeIds') or {}).get('items') or {}).get('type') == 'string'
    choice = req.get('tool_choice')
    forced = choice == 'required' or (isinstance(choice, dict) and (choice.get('function') or {}).get('name') == REPORT_TOOL)
    return bool(tool) and shape and forced


def _model_args(entry: Dict) -> Dict:
    msg = (((entry.get('response') or {}).get('choices') or [{}])[0] or {}).get('message') or {}
    calls = msg.get('tool_calls') or []
    args = ((calls[0] or {}).get('function') or {}).get('arguments') if calls else None
    return args if isinstance(args, dict) else {}


@check('u_llm_explain', 'U', pre=lambda c: (bool((c.ui().get('explain') or {}).get('steps')), 'explain control rendered'),
       needs=('ui',))
def _(c):
    ex = c.ui()['explain']
    sid = str(ex.get('sprintId'))
    o = c.oracle_ui
    n = o.numbers(sid) if sid in o.all_numbers() else None
    visible = {ch.change_id for ch in o.visible_changes(sid, o.viewer)} if n else set()
    ledger_digits = set(re.findall(r'\d+(?:\.\d+)?', ' '.join([*n.metrics_text().values(), n.name,
                                                                  str(o.hidden_count(sid, o.viewer))]))) if n else set()
    subs: Dict[str, bool] = {}
    before_click: set = set()
    for i, st in enumerate(ex['steps']):
        # Graded by POSITION: an LLM call the platform refused before the script answered (an unknown model id, a
        # sampling rule) leaves the click without its scripted answer — the clean answer never reached the page.
        kind = EXPLAIN_SCRIPT[i] if i < len(EXPLAIN_SCRIPT) else 'clean'
        entry = st.get('llm') or {}
        label = f'{i}:{kind}'
        shown_ids = {str(x) for x in st.get('idsShown') or []}
        # DESIGN §17.4 gap 29: after a failed attempt, clearing OR keeping the previous explanation both pass, so a
        # failure step is charged only for ids THIS click put on screen, never for the ones still showing from before.
        new_ids, before_click = shown_ids - before_click, shown_ids
        if st.get('llmCalls', 0) != 1 or entry.get('step') != kind:
            subs[f'{label}:one_llm_call_answered_{kind}'] = False
            continue
        subs[f'{label}:report_scope_forced'] = _explain_request_ok(entry)
        text = str(st.get('explanation') or '')
        if kind in ('clean', 'digits'):
            args = _model_args(entry)
            cited = {str(x) for x in args.get('changeIds') or []}
            summary = str(args.get('summary') or '')
            subs[f'{label}:ids_are_cited_visible_ones'] = shown_ids == (cited & visible)
            if kind == 'clean':
                subs[f'{label}:summary_shown'] = bool(summary) and summary in text
            else:
                model_digits = set(re.findall(r'\d+(?:\.\d+)?', summary)) - ledger_digits
                own = set(re.findall(r'\d+(?:\.\d+)?', text))
                subs[f'{label}:no_model_digits'] = summary not in text and not (own & model_digits)
            # Contract §5: the summary shows "only if it holds no digits (otherwise your own sentence with the
            # ledger's numbers)". Judged on the explanation's own words (the probe's explanationOwn: the box without
            # its [data-change-id] elements; observations that predate it stay silent): every number on screen is a
            # ledger number, and the digits answer's replacement sentence carries at least one of the four.
            if n is not None and isinstance(st.get('explanationOwn'), str):
                found = explanation_numbers(st['explanationOwn'], n.name)
                allowed = explanation_ledger_numbers(o, sid)
                subs[f'{label}:every_number_is_a_ledger_number'] = all(x in allowed for x in found)
                if kind == 'digits':
                    four = {n.committed, n.added, n.removed} | ({n.creep} if n.creep is not None else set())
                    subs[f'{label}:own_sentence_has_a_ledger_number'] = any(x in four for x in found)
            subs[f'{label}:no_error_flag'] = not st.get('errorFlags')
        else:
            # The refusal path (DESIGN 103/803): the error flag must come from a resolver that ANSWERED the bad model
            # turn, not from the bridge rejecting an invoke that threw — the page looks the same either way. Judged
            # only when the probe linked invokes to their invocation (observations that predate it stay silent).
            inv = next((r for r in _invokes(c) if r.get('invocationId') and r.get('invocationId') == entry.get('invocationId')), None)
            if inv is not None:
                subs[f'{label}:resolver_answered_{kind}'] = not inv.get('threw')
            subs[f'{label}:error_flag'] = (st.get('errorFlags') or 0) >= 1 and not new_ids
            subs[f'{label}:modal_still_sorts'] = bool(st.get('sortWorksAfter'))
    met = sum(subs.values())
    return g(met / len(subs) if subs else 0, f"explain {met}/{len(subs)} over {len(ex['steps'])} scripted answers: "
             + ', '.join(k for k, v in subs.items() if not v), 'the explanation trusts or mishandles the model', parts=subs)


# ══ V: visual ════════════════════════════════════════════════════════════════════════════════

@check('v_theme_tokens', 'V', pre=pre_app_content, needs=('ui',))
def _(c):
    surfaces = c.rendered_surfaces()
    tol, floor = int(TH['color_tolerance']), float(TH['contrast_min'])
    ok, notes = 0, []
    for s in surfaces:
        tokens = s.get('tokens') or {}
        text_vals = [v for k, v in tokens.items() if k.startswith('--ds-text')]
        link_vals = [v for k, v in tokens.items() if k.startswith('--ds-link')]
        # Contract §7: disabled controls are exempt from the colour and contrast rule.
        styles = [st for st in s.get('textStyles') or [] if st.get('role') != 'disabled']
        good = bool(s.get('enableTheming')) and bool(styles) and bool(text_vals)
        for st in styles:
            pool = text_vals + (link_vals if st.get('role') == 'link' else [])
            if not any(rgb_close(st.get('color'), v, tol) for v in pool):
                good = False
            if not st.get('background') or contrast(st['color'], st['background']) < floor:
                good = False
        ok += good
        if not good:
            notes.append(s.get('id'))
    return g(ok / len(surfaces), f'{ok}/{len(surfaces)} surfaces themed with --ds-text* at >= {floor:g}:1'
             + (f'; failing {notes[:4]}' if notes else ''), 'unreadable in one mode')


@check('v_dark_mode', 'V', pre=pre_app_content, needs=('ui',))
def _(c):
    surfaces = c.rendered_surfaces()
    tol = int(TH['color_tolerance'])
    ok, notes = 0, []
    for s in surfaces:
        fam = [v for k, v in (s.get('tokens') or {}).items() if k.startswith(('--ds-surface', '--ds-elevation-surface'))]
        dom = s.get('dominant')
        good = dom is not None and not s.get('blank') and any(rgb_close(dom, v, tol) for v in fam)
        ok += good
        if not good:
            notes.append(f"{s.get('id')}: dominant {dom}")
    return g(ok / len(surfaces), f'{ok}/{len(surfaces)} surfaces paint a --ds-surface* background in their mode'
             + (f'; {notes[:3]}' if notes else ''), 'dark mode broken')


@check('v_csp_clean', 'V', pre=pre_app_content, needs=('ui',))
def _(c):
    v = sum(len(s.get('cspViolations') or []) for s in c.surfaces())
    f = sum(len(s.get('failedRequests') or []) for s in c.surfaces())
    return g(1 if v == f == 0 else 0, f'{v} CSP violation(s), {f} failed asset request(s)', 'blocked by the Forge CSP')


@check('v_console_clean', 'V', pre=pre_app_content, needs=('ui',))
def _(c):
    errs = [e for s in c.surfaces() if s.get('nominal', True) for e in (s.get('consoleErrors') or []) + (s.get('pageErrors') or [])]
    return g(0 if errs else 1, f'{len(errs)} console/page error(s) on nominal scenarios: {errs[:3]}', 'console errors')


@check('v_widget_sizes', 'V', pre=pre_widget_sprints, needs=('ui',))
def _(c):
    views = [v for v in (c.ui().get('widget') or {}).get('views') or [] if v.get('sprints')]
    # Contract §4: "no number is clipped or truncated" — the probe's metricsClipped (a [data-metric] whose laid-out text
    # leaves the viewport or a clipping ancestor); observations that predate it are judged on overflow alone.
    clipped = sorted({m for v in views for m in v.get('metricsClipped') or []})
    ok = sum(1 for v in views if (v.get('overflow') or {}).get('scrollWidth', 1 << 30) <= (v.get('overflow') or {}).get('clientWidth', 0)
             and v.get('sprintsVisible') and not v.get('metricsClipped'))
    return g(ok / len(views), f'{ok}/{len(views)} widget sizes without horizontal overflow, every sprint visible, no number '
             'clipped' + (f'; clipped: {clipped[:6]}' if clipped else ''), 'clips at some width')


# ══ A: Rovo ══════════════════════════════════════════════════════════════════════════════════

def _action_module(c: Ctx) -> Optional[Dict]:
    return next((a for a in c.modules('action') if a.get('key') == ACTION_KEY), None)


def action_ok(res, want: Dict) -> bool:
    if not isinstance(res, dict):
        return False
    if str(res.get('sprintId')) != want['sprintId'] or res.get('sprintName') != want['sprintName']:
        return False
    if not all(json_number_ok(res.get(k), want[k]) for k in ('committed', 'added', 'removed')):
        return False
    if not creep_json_ok(res.get('creepPercent'), want['creepPercent']) or res.get('hiddenChanges') != want['hiddenChanges']:
        return False
    got = res.get('changes')
    if not isinstance(got, list) or len(got) != len(want['changes']):
        return False
    for a, b in zip(got, want['changes']):
        if not isinstance(a, dict) or str(a.get('changeId')) != b['changeId'] or a.get('issueKey') != b['issueKey'] \
                or a.get('kind') != b['kind'] or not json_number_ok(a.get('points'), b['points']) \
                or fo.instant(a.get('at')) != b['at'] or a.get('by') != b['by']:
            return False
    return True


@check('a_action_result', 'A', needs=('rovo',))
def _(c):
    if not _action_module(c):
        return absent(f'action {ACTION_KEY}')
    sprints = c.oracle.active_sprints()
    calls = {str(r.get('sprintId')): r for r in c.rovo_calls('sprint', c.oracle.viewer)}
    ok = [sid for sid in sprints if sid in calls and not calls[sid].get('threw')
          and action_ok(calls[sid].get('result'), c.oracle.action_result(sid, c.oracle.viewer))]
    return g(len(ok) / len(sprints), f'{len(ok)}/{len(sprints)} active sprints answered exactly',
             'the Rovo action answers wrongly')


@check('a_action_errors', 'A', pre=pre_action_exists, needs=('rovo',))
def _(c):
    calls = c.rovo_calls('unknown') + c.rovo_calls('missing')
    ok = sum(1 for r in calls if not r.get('threw') and isinstance(r.get('result'), dict)
             and isinstance(r['result'].get('error'), str) and r['result']['error'].strip())
    return g(ok / 2, f'{ok}/2 bad sprintId inputs return {{error}} without throwing', 'the action throws')


@check('a_action_permissions', 'A', needs=('rovo',))
def _(c):
    if not _action_module(c):
        return absent(f'action {ACTION_KEY}')
    calls = [r for r in c.rovo_calls('sprint') if str(r.get('sprintId')) in c.oracle.all_numbers()]
    people = sorted({str(r.get('as')) for r in calls})
    if len(people) < 2:
        return unavail('the probe did not invoke the action as two people')
    ok = 0
    for r in calls:
        want = c.oracle.action_result(str(r['sprintId']), str(r['as']))
        res = r.get('result') if isinstance(r.get('result'), dict) else {}
        ids = [str(x.get('changeId')) for x in res.get('changes') or [] if isinstance(x, dict)]
        ok += res.get('hiddenChanges') == want['hiddenChanges'] and ids == [x['changeId'] for x in want['changes']]
    return g(ok / len(calls), f'{ok}/{len(calls)} per-person answers list exactly what that person may browse',
             'per-person visibility wrong')


# The action's answer as contract §6 prints it: what "how to read the result" has to cover (sprintId is the input).
RESULT_KEYS = ('sprintName', 'committed', 'added', 'removed', 'creepPercent', 'hiddenChanges', 'changes')


@check('a_skill_instructions', 'A', pre=pre_skill_md)
def _(c):
    _fm, body = parse_frontmatter(_skill_text(c) or '')
    named = [k for k in RESULT_KEYS if re.search(r'(?<![A-Za-z])' + k + r'(?![A-Za-z])', body)]
    # Contract §6: the body tells the agent "when and how to call get-sprint-scope, what sprintId is, how to read the
    # result, and what to do with an error". Reading the result = naming at least half of its fields; the error
    # clause = the `error` answer named at all.
    conds = {'body_le_500_lines': len(body.splitlines()) <= 500, 'names_action': ACTION_KEY in body,
             'names_sprintId': 'sprintId' in body, 'reads_the_result': 2 * len(named) >= len(RESULT_KEYS),
             'says_what_to_do_with_an_error': bool(re.search(r'\berrors?\b', body, re.I))}
    met = sum(conds.values())
    return g(met / len(conds), f'SKILL.md body {met}/{len(conds)}: result fields named {named}'
             + ('; missing: ' + ', '.join(k for k, v in conds.items() if not v) if met < len(conds) else ''),
             'the skill does not tell the agent how to call the action and read its answer', parts=conds)


# ══ E: excellence rows (rungs are ratios of the oracle optimum; calibration-owned) ═══════════

def _row(c: Ctx, name: str) -> Dict:
    return c._row_cache.get(name) or {}


@check('e_reconcile_economy', 'E', needs=('backfill',))
def _(c):
    if _row(c, 'r_backfill_complete').get('score', 0) < 1.0:
        return g(0, 'backfill incomplete — economy is not credited on unfinished work')
    optimum, missing = c.oracle.reconcile_optimum()
    if optimum is None:
        return unavail(f'pack.limits lacks {missing}')
    # Each scripted 429 the backfill met forces one repeat of the refused read (the scoring site scripts two, F7).
    scheduled = {f['id'] for f in c.pack.get('faults') or [] if (f.get('match') or {}).get('scope') == 'scheduled-run'}
    repeats = sum(1 for x in c.calls(('backfill',), 'jira') if x.get('fault') in scheduled and not x.get('earlyRetry'))
    optimum += repeats
    used = len([x for x in c.calls(('backfill',), 'jira')])
    ratio = used / optimum
    return g(ladder(ratio, TH['economy_rungs']), f'backfill {used} Jira calls / optimum {optimum} (incl. {repeats} forced 429 '
             f'repeat(s)) = {ratio:.2f}x', parts={'calls': used, 'optimum': optimum, 'ratio': round(ratio, 3)})


@check('e_event_economy', 'E', needs=('live',))
def _(c):
    if _row(c, 't_event_rows').get('score', 0) < 1.0:
        return g(0, 'event rows inexact — economy is not credited on unfinished work')
    # DESIGN §8.4: the oracle optimum is ONE read per delivered relevant change (sprint and field metadata cached in
    # KVS; a duplicate delivery needs none), plus the one repeat each consumer-path 429 forces. Graded continuously at
    # that optimum (2026-10-03 stringency, F1): score = min(1, top / (used / optimum)), `top` the golden's worst ratio
    # over the calibration seeds (calibration-owned), so the golden defines 1.0 and every extra request costs.
    relevant = c.oracle.event_optimum()
    faults = sum(1 for x in c.calls(('live',), 'jira') if x.get('fault') and not x.get('earlyRetry')
                 and x.get('fault') in {f['id'] for f in c.pack.get('faults') or []
                                        if (f.get('match') or {}).get('scope') == 'consumer-of-change'})
    optimum = max(1, relevant + faults)
    used = len(c.calls(('live',), 'jira', ('trigger', 'consumer')))
    ratio = used / optimum
    top = float(TH['event_economy_top'])
    return g(min(1.0, top / ratio) if ratio else 1.0, f'{used} Jira calls / optimum {optimum} ({relevant} delivered relevant '
             f'change(s) x 1 read + {faults} forced 429 repeat) = {ratio:.2f}x; 1.0 at <= {top:g}x',
             parts={'calls': used, 'optimum': optimum, 'changes': relevant, 'ratio': round(ratio, 3)})


@check('e_ui_round_trips', 'E', needs=('ui',))
def _(c):
    firsts: Dict[str, int] = {}
    for s in c.rendered_surfaces():
        n = s.get('invokesBeforePaint')
        if isinstance(n, int) and s.get('kind') not in firsts:
            firsts[s['kind']] = n
    if not firsts:
        return g(0, 'no surface rendered app content')
    scores = {k: ladder(max(1, v), TH['ui_round_trip_rungs']) for k, v in firsts.items()}
    return g(sum(scores.values()) / len(scores), f'round trips before first paint: {firsts}', parts=scores)


# ── calibration (DESIGN §13.4 item 4: thresholds frozen with golden ×5 receipts, sha pinned) ──────────────

CALIBRATION_SEEDS = 5   # DESIGN §13.4 item 4
# The two economy ratios are FITTED to the golden's worst-of-5 ratio (rounded up to 0.01, never below the oracle
# optimum 1.0), so the golden defines 1.0 on every calibration seed: the backfill's top rung (every lower rung keeps
# its rc multiple of the top), and the event path's continuous top (score = min(1, top / ratio), 2026-10-03 F1). The
# round-trip rungs sit at their floor already (1 invoke): they are VERIFIED (the golden reaches the top rung on every
# seed), never fitted — a golden that misses them refuses.
FITTED_RUNGS = {'economy_rungs': 'e_reconcile_economy'}
FITTED_TOPS = {'event_economy_top': 'e_event_economy'}
VERIFIED_TOP = ('e_ui_round_trips',)


def _exact_ratio(parts: Dict) -> float:
    # parts['ratio'] is rounded for display; a top rung fitted to a rounded-DOWN ratio would grade the golden below 1.0
    den = parts.get('optimum') or parts.get('changes')
    return parts['calls'] / den if isinstance(parts.get('calls'), int) and den else parts['ratio']


def calibrate(verdicts: List[Dict], rc: Dict) -> Dict:
    """The calibrated thresholds from >= 5 reference-passing golden verdicts on distinct seeds; refuses otherwise."""
    fails = []
    seeds = [v.get('fixture_seed') for v in verdicts]
    if len(set(seeds)) < CALIBRATION_SEEDS or None in seeds:
        fails.append(f'{CALIBRATION_SEEDS} distinct fixture seeds needed, got {seeds}')
    shas = {json.dumps(v.get('scorer_files_sha256'), sort_keys=True) for v in verdicts}
    if len(shas) != 1:
        fails.append('the golden verdicts were scored by different scorer files')
    if any(v.get('scorerVersion') != 'forge-1.0-rc' for v in verdicts):
        fails.append('calibration fits rc-grade verdicts only (thresholds already calibrated?)')
    for v in verdicts:
        fails += [f"{v.get('fixture_seed')}: {f}" for f in reference_failures(v)]
        rows = {r['check']: r for r in v['checks']}
        fails += [f"{v.get('fixture_seed')}: {n} {rows[n]['score']} — the golden misses the floor rung"
                  for n in VERIFIED_TOP if rows[n]['score'] < 1.0]
        fails += [f"{v.get('fixture_seed')}: {n} carries no measured ratio" for n in [*FITTED_RUNGS.values(), *FITTED_TOPS.values()]
                  if not isinstance((rows[n].get('parts') or {}).get('ratio'), (int, float))]
    if fails:
        raise ValueError('calibration refused:\n  ' + '\n  '.join(fails))
    out = json.loads(json.dumps(rc))
    out['calibrated'] = True
    out['note'] = ('forge-1.0 calibrated thresholds: the economy rungs are fitted to the golden worst-of-5 '
                   '(DESIGN §13.4 item 4); everything else keeps its rc receipt. Pinned as CALIB_SHA256 in score_forge.py.')
    for key, row in FITTED_TOPS.items():
        per_seed = {v['fixture_seed']: _exact_ratio(next(r for r in v['checks'] if r['check'] == row)['parts']) for v in verdicts}
        worst = max(per_seed.values())
        out[key] = max(1.0, math.ceil(worst * 100 - 1e-9) / 100)
        shown = {seed: round(r, 4) for seed, r in per_seed.items()}
        out['receipts'][key] = (f'golden worst-of-{len(per_seed)} {row} ratio {round(worst, 4)} (per seed {shown}) -> '
                                f'top {out[key]}; score = min(1, top / ratio), continuous (no rungs)')
    for key, row in FITTED_RUNGS.items():
        per_seed = {v['fixture_seed']: _exact_ratio(next(r for r in v['checks'] if r['check'] == row)['parts']) for v in verdicts}
        worst = max(per_seed.values())
        top = max(1.0, math.ceil(worst * 100 - 1e-9) / 100)
        rc_top = rc[key][0][1]
        out[key] = [[score, round(cut * top / rc_top, 2)] for score, cut in rc[key]]
        shown = {seed: round(r, 4) for seed, r in per_seed.items()}
        out['receipts'][key] = (f'golden worst-of-{len(per_seed)} {row} ratio {round(worst, 4)} (per seed {shown}) -> top rung '
                                f'{top}; lower rungs keep the rc multiples of the top ({rc[key]})')
    out['calibration'] = {'seeds': sorted(seeds), 'scorer_files_sha256': verdicts[0].get('scorer_files_sha256'),
                          'kit_lock_sha256': verdicts[0].get('kit_lock_sha256'), 'wrapper_sha256': verdicts[0].get('wrapper_sha256'),
                          'verified_top': {n: 'golden 1.0 on every seed' for n in VERIFIED_TOP}}
    return out


# ── registry-close asserts ───────────────────────────────────────────────────────────────────

REGISTERED = {n for n, *_ in CHECKS}
assert len(REGISTERED) == len(CHECKS), 'duplicate check name in the forge registry'
TIER_OF = {n: t for n, t, *_ in CHECKS}
assert set(TIER_OF.values()) == set(TIER_WEIGHT) | {'E'}
assert DIAGNOSTIC <= REGISTERED and set(CRITICAL_CHECKS) <= REGISTERED and CALIBRATION_OWNED <= REGISTERED
assert {n for _c, _l, names in ADMISSION_BANDS for n in names} <= REGISTERED
assert set(EXCELLENCE_VALUED) | set(EXCELLENCE_BINARY) <= REGISTERED
assert sum(1 for t in TIER_OF.values() if t != 'E') == 63 and sum(1 for t in TIER_OF.values() if t == 'E') == 3

# Rows that compare a person's change LIST with the oracle: one wrong row in the ledger or one change shown that
# should be hidden makes each of them wrong, and that is one defect, not N (DESIGN §13.4 item 5, gap #23).
CHANGE_LIST_ROWS = ('u_ledger_table', 'u_ledger_sort', 's_index_order', 'u_comment_flow', 'u_issue_router',
                    'a_action_result', 'a_action_permissions', 'b_hidden_count')
# Rows that compare the four §1 numbers (or a value printed from them, like the comment's creep and the points
# cell) with the oracle: wrong numbers are one defect wherever they show.
NUMBER_ROWS = ('u_widget_chart', 'a_action_result', 'u_ledger_table', 'u_ledger_sort', 'b_comment_adf_as_user',
               't_out_of_order', 't_reestimate_followed', 'u_widget_live')
ROOT_BLOCKS = {
    'l_bundles_load': tuple(n for n, t in TIER_OF.items() if t in ('T', 'R', 'S', 'B', 'U', 'A')),
    # measured on m_old_search / m_open_sprints_only / m_storage_api: a backfill that never lands leaves every
    # ledger-derived row wrong, the same scheduled run's heal and rate-limit rows unfinished, and the scopes
    # its calls would have used unexercised (l_scopes reads observed calls).
    'r_backfill_complete': ('u_widget_numbers', 'r_removals_found', *CHANGE_LIST_ROWS, *NUMBER_ROWS, 'r_heal_dropped',
                            'r_rate_limit', 't_multi_sprint_parse', 'l_scopes'),
    # measured on m_ids_only / m_one_estimate_field: wrong numbers show in the chart, the action, the table's
    # points cell and sort, and the comment's creep.
    'u_widget_numbers': NUMBER_ROWS,
    'u_widget_loads': ('u_widget_numbers', 'u_widget_chart', 'u_widget_edit_config', 'v_widget_sizes', 'u_widget_live'),
    # measured on m_dedupe_event_id (seed 0123456789abcdef): one duplicated row -> a duplicate table row, the
    # sort and index-order rows, one comment-flow step, the action's change list and the event-row exactness.
    't_no_double_count': (*CHANGE_LIST_ROWS, 't_event_rows', 't_out_of_order', 't_multi_sprint_parse', 'r_heal_dropped',
                          'u_widget_numbers', 'u_widget_chart', 't_reestimate_followed'),
    # measured on m_asapp_ui: hidden changes listed -> the table, its sort and the action's list are wrong too.
    'b_no_permission_leak': CHANGE_LIST_ROWS,
    # One defect, two band-4 rows (band_defects prices it once): a model id list() does not return fails every explain
    # call (m_llm_unknown_model); a ledger read out of change-time order is a wrong default order and a wrong toggle;
    # a wrong hidden count is wrong in the action's per-person answer too.
    'k_llm_model_current': ('u_llm_explain',),
    's_index_order': ('u_ledger_sort',),
    'b_hidden_count': ('a_action_permissions',),
}
for _root, _deps in ROOT_BLOCKS.items():
    assert {_root, *_deps} <= REGISTERED


def attribute_root_causes(rows: List[Dict]) -> Dict[str, List[str]]:
    by = {r['check']: r for r in rows}
    out = {}
    for root, deps in ROOT_BLOCKS.items():
        r = by.get(root)
        if r is None or r['score'] >= 1.0:
            continue
        blocked = [d for d in deps if d in by and by[d]['score'] < 1.0]
        if blocked:
            out[root] = blocked
    return out


# ── evaluation ───────────────────────────────────────────────────────────────────────────────

def _run_check(c: Ctx, name: str, tier: str, fn, pre, needs) -> Dict:
    if c.oracle is None:
        return {'check': name, 'tier': tier, **unavail(f'pack defect: {c.oracle_error}')}
    why = c.section_error(*needs)
    if why:
        return {'check': name, 'tier': tier, **unavail(why)}
    if pre is not None:
        try:
            ok, what = pre(c)
        except Exception as error:  # a precondition bug is the scorer's, never the app's
            return {'check': name, 'tier': tier, **unavail(f'precondition error: {type(error).__name__}: {error}')}
        if not ok:
            return {'check': name, 'tier': tier, **vacuous(what)}
    try:
        outcome = fn(c)
    except Exception as error:
        outcome = unavail(f'scorer error: {type(error).__name__}: {error}')
    return {'check': name, 'tier': tier, **outcome}


# The deploy-readiness rows price only what no other row already graded, so they run after every other row.
DEFERRED = ('k_manifest_semantics', 'k_runtime_risks')


def evaluate_rows(c: Ctx) -> List[Dict]:
    rows = []
    order = [x for x in CHECKS if x[0] not in DEFERRED] + [x for x in CHECKS if x[0] in DEFERRED]
    for name, tier, fn, pre, needs in order:
        if tier == 'E':
            continue
        row = _run_check(c, name, tier, fn, pre, needs)
        c._row_cache[name] = row
        rows.append(row)
    for name, tier, fn, pre, needs in CHECKS:
        if tier == 'E':
            rows.append(_run_check(c, name, tier, fn, pre, needs))
    return rows


# Contract §8: "The scoring site uses a different seed than the dev site". A build is graded on THREE scoring sites
# (2026-10-03 stringency, F3): the run's own archived fixture_seed plus two derived from it (never the dev seed), and
# each row keeps its WORST seed — a correct app is correct on every site, one that happens to fit one pack is not — while
# the excellence rows (economy ratios, round trips) keep their MEAN, as measurements rather than verdicts.
SCORING_SEEDS = 3


def scoring_seeds(seed: str, dev_seed: Optional[str], n: int = SCORING_SEEDS) -> List[str]:
    out, k = [seed.lower()], 0
    while len(out) < n:
        k += 1
        cand = hashlib.sha256(f'forge-scoring-site:{seed.lower()}:{k}'.encode()).hexdigest()[:16]
        if cand != str(dev_seed or '').lower() and cand not in out:
            out.append(cand)
    return out


def merge_seed_rows(row_sets: List[List[Dict]], seeds: List[str]) -> List[Dict]:
    by_seed = [{r['check']: r for r in rows} for rows in row_sets]
    merged = []
    for row in row_sets[0]:
        name = row['check']
        rs = [b[name] for b in by_seed if name in b]
        scores = {seed: b[name]['score'] for seed, b in zip(seeds, by_seed) if name in b}
        gone = [(seed, b[name]) for seed, b in zip(seeds, by_seed) if name in b and b[name].get('unavailable')]
        if gone:
            seed, r = gone[0]
            out = {**r, 'detail': f'seed {seed}: {r["detail"]}'[:400]}
        elif row['tier'] == 'E':
            out = {**row, 'score': round(sum(r['score'] for r in rs) / len(rs), 4),
                   'detail': f'mean over {len(rs)} scoring sites: {row["detail"]}'[:400]}
        else:
            worst_seed, worst = min(zip(seeds, rs), key=lambda sr: sr[1]['score'])
            out = {**worst, 'detail': (f'worst of {len(rs)} scoring sites (seed {worst_seed}): ' if len(set(scores.values())) > 1
                                       else '') + str(worst.get('detail'))}
            out['detail'] = out['detail'][:400]
        out['seeds'] = scores
        merged.append(out)
    return merged


def evaluate(c: Ctx) -> Dict:
    extra = list(getattr(c, 'seed_ctxs', None) or [])
    rows = evaluate_rows(c)
    seed_scores = None
    if extra:
        sets = [rows] + [evaluate_rows(x) for x in extra]
        seeds = [c.fixture_seed] + [x.fixture_seed for x in extra]
        seed_scores = {s: compose_from_rows(r, x)['score'] for s, r, x in zip(seeds, sets, [c] + extra)}
        for x in extra:
            c.harness_missing += [h for h in x.harness_missing if h not in c.harness_missing]
        rows = merge_seed_rows(sets, seeds)
    result = compose_from_rows(rows, c)
    if seed_scores is not None:
        result['fixture_seeds'] = list(seed_scores)
        result['seed_scores'] = seed_scores
    result['recall_trace'] = recall_trace(c.root)
    result['media'] = media_block(c)
    result['deploy_readiness'] = deploy_readiness(c)
    kit = getattr(c, 'kit', None) or {}
    result.update({'kit_lock_sha256': kit.get('lock_sha256') or (c.obs.get('kit') or {}).get('lockSha256'),
                   'wrapper_sha256': kit.get('wrapper_sha256') or (c.obs.get('kit') or {}).get('wrapperSha256'),
                   'scorer_seconds': getattr(c, 'scorer_seconds', None), 'shots': list(c.obs.get('shots') or []),
                   'scorer_files_sha256': {n: hashlib.sha256((HERE / n).read_bytes()).hexdigest()
                                           for n in ('score_forge.py', 'forge_oracle.py', 'forge_probe.mjs',
                                                     THRESHOLDS_FILE.name)},
                   'spec_sha256': hashlib.sha256((ROOT / SPEC).read_bytes()).hexdigest()})
    return result


def media_block(c: Ctx) -> Dict:
    """The graded browser recording (bench-media/media-manifest.json, SB7.x's shape the desktop publishes through
    /api/benchmark-media). Report-only: a run with no recording scores the same and says why (a named absence)."""
    media = c.obs.get('media')
    if not isinstance(media, dict):
        why = c.section_error('ui') or ('no Custom UI surface was opened' if not c.surfaces() else
                                        'the probe recorded no graded browser session')
        return {'videos': [], 'absent': f'graded browser recording absent: {why}'}
    media = {**media, 'scorerVersion': VERSION}
    if c.root and media.get('manifest'):
        path = c.root / media['manifest']
        if path.is_file():
            path.write_text(json.dumps({k: v for k, v in media.items() if k != 'manifest'}, indent=2) + '\n')
    return media


def recall_trace(root) -> Dict:
    """§8.8 report-only: needs the isolated session database, which run_build does not hand the scorer yet."""
    return {'status': 'unavailable', 'weight': 0,
            'reason': 'the isolated goose session database is not captured beside the tree (DESIGN §8.8)'}


# ── severity: transforms, multiplier + dedup, excellence, admission ──────────────────────────

def passed(row: Optional[Dict]) -> bool:
    return bool(row and isinstance(row.get('score'), (int, float)) and abs(row['score'] - 1.0) < 1e-12
                and not row.get('unavailable') and not (row.get('parts') or {}).get('vacuous_root'))


def critical_severity_input(r: Dict) -> float:
    s = max(0.0, min(1.0, r['score']))
    if (r.get('parts') or {}).get('absent_surface'):
        return 0.0
    if s >= 1.0 - 1e-9:
        return 1.0
    name = r['check']
    if name == 'l_deployable':
        return 0.0
    if name == 'l_bundles_load':
        return 0.0 if s < 0.4 else 1.0
    if name == 'r_backfill_complete':
        return min(s, 0.5)
    if name == 'u_widget_loads':
        return 0.0 if s == 0.0 else 1.0
    return 0.0   # t_no_double_count, b_no_permission_leak, b_comment_exactly_once: cliffs


def excellence(rows: List[Dict]) -> Tuple[float, float, List[Dict]]:
    by = {r['check']: r for r in rows}
    conds = []
    for n in EXCELLENCE_VALUED + tuple(sorted(n for n, t in TIER_OF.items() if t == 'K')):
        r = by.get(n) or {}
        v = 0.0 if (r.get('parts') or {}).get('vacuous_root') or r.get('unavailable') else float(r.get('score') or 0)
        conds.append({'name': n, 'ok': passed(r), 'share': max(0.0, min(1.0, v))})
    for n in EXCELLENCE_BINARY:
        conds.append({'name': n, 'ok': passed(by.get(n)), 'share': 1.0 if passed(by.get(n)) else 0.0})
    fraction = sum(x['share'] for x in conds) / len(conds)
    e_rows = [r for r in rows if r['tier'] == 'E' and not r.get('unavailable')]
    e_mean = sum(r['score'] for r in e_rows) / len(e_rows) if e_rows else 0.0
    return fraction, e_mean, conds


def band_defects(label: str, failed: List[str]) -> List[str]:
    """The graded band counts DEFECTS ("minus 0.03 for each further such defect"), not rows: a failed row that
    ROOT_BLOCKS attributes to another failed row of the same band is that row's shadow (asApp in the UI leaks AND
    miscounts the hidden changes AND lists them in the action — one defect), priced once. Only a root inside the
    band absorbs a row, so a defect is never dropped from the band by a root the band does not charge."""
    if label != GRADED_BAND[0]:
        return failed
    defects = [n for n in failed if not any(n in ROOT_BLOCKS.get(root, ()) for root in failed if root != n)]
    return defects or failed[:1]


def admit(rows: List[Dict]) -> Dict:
    by = {r['check']: r for r in rows}
    ceiling, reasons, failed_by_band = 1.0, [], []
    for limit, label, names in ADMISSION_BANDS:
        failed = [n for n in names if not passed(by.get(n))]
        if failed:
            defects = band_defects(label, failed)
            cap = band_ceiling(limit, label, len(defects))
            ceiling = min(ceiling, cap)
            entry = {'ceiling': cap, 'band': label, 'checks': failed}
            shadows = [n for n in failed if n not in defects]
            if shadows:
                entry['priced_once'] = shadows
            failed_by_band.append(entry)
            reasons.append(f'{label}: {", ".join(failed)} (maximum {cap:.3f}'
                           + (f'; {", ".join(shadows)} priced with their root' if shadows else '') + ')')
    return {'ceiling': ceiling, 'reasons': reasons, 'failedChecksByBand': failed_by_band}


def compose_from_rows(rows: List[Dict], c: Optional[Ctx] = None) -> Dict:
    unavailable = [r['check'] for r in rows if r.get('unavailable')]
    vacuous_rows = {r['check']: (r.get('parts') or {}).get('vacuous_root') for r in rows
                    if (r.get('parts') or {}).get('vacuous_root')}
    tiers: Dict[str, Dict] = {}
    inner = 0.0
    for tier, w in TIER_WEIGHT.items():
        sub = [r for r in rows if r['tier'] == tier and r['check'] not in DIAGNOSTIC]
        avail = [r for r in sub if not r.get('unavailable')]
        mean = sum(max(0.0, min(1.0, r['score'])) for r in avail) / len(avail) if avail else 0.0
        tiers[tier] = {'mean': round(mean, 4), 'checks': len(avail), 'weight': w}
        if len(avail) != len(sub):
            tiers[tier]['unavailable'] = [r['check'] for r in sub if r.get('unavailable')]
        inner += mean * w
    fraction, e_mean, conds = excellence(rows)
    pre_severity = INNER_WEIGHT * inner + E_WEIGHT * fraction * e_mean

    floor = float(TH['critical_multiplier_floor'])
    sev = {r['check']: critical_severity_input(r) for r in rows
           if r['check'] in CRITICAL_CHECKS and not r.get('unavailable')}
    suppressed: Dict[str, str] = {}
    for name in sev:
        if name in vacuous_rows:
            suppressed[name] = f'vacuous:{vacuous_rows[name]}'
            continue
        for root, deps in ROOT_BLOCKS.items():
            if (root != name and name in deps and root in sev and sev[root] < 1.0 - 1e-9
                    and root not in vacuous_rows):
                suppressed[name] = f'root:{root}'
                break
    mult, crit_rows = 1.0, []
    for r in rows:
        name = r['check']
        if name not in sev or sev[name] >= 1.0 - 1e-9:
            continue
        entry = {'check': name, 'score': r['score'], 'severity_input': round(sev[name], 4), 'why': CRITICAL_CHECKS[name]}
        if name in suppressed:
            crit_rows.append({**entry, 'factor': 1.0, 'suppressed': suppressed[name]})
            continue
        factor = floor + (1.0 - floor) * sev[name]
        mult *= factor
        crit_rows.append({**entry, 'factor': round(factor, 4)})
    earned = pre_severity * mult
    admission = admit(rows)
    admission['final_rule'] = f'final = min(earned, ceiling - {BAND_PULL:g} * (1 - earned))'
    final = capped_final(earned, admission['ceiling'])
    tiers['E'] = {'mean': round(fraction * e_mean, 4), 'gate': fraction >= 1.0, 'gate_fraction': round(fraction, 4),
                  'weight': E_WEIGHT}
    harness_missing = list(c.harness_missing) if c is not None else []
    runtime = c.runtime if c is not None else 'wrapper'
    unpublishable = []
    if unavailable:
        unpublishable.append(f'{len(unavailable)} unavailable row(s)')
    if runtime != 'wrapper':
        unpublishable.append(f'runtime {runtime} (the in-repo shim, not the pinned Forge wrapper)')
    status = 'held' if harness_missing else 'scored'
    return {
        'status': status, 'score': published_score(final, admission['ceiling']), 'rawScore': round(earned, 4),
        'inner': round(inner, 4),
        'scorerVersion': VERSION, 'scorer_version': VERSION, 'family': 'forge',
        'fixture_seed': c.fixture_seed if c is not None else None, 'dev_seed': c.dev_seed if c is not None else None,
        'critical': {'floor': floor, 'multiplier': round(mult, 4), 'pre_severity_score': round(pre_severity, 4),
                     'rows': crit_rows,
                     'unsuppressed': [x['check'] for x in crit_rows if not x.get('suppressed')]},
        'admission': admission,
        'excellence': {'fraction': round(fraction, 4), 'e_mean': round(e_mean, 4), 'gate': fraction >= 1.0,
                       'conditions': conds},
        'excellence_gate': fraction >= 1.0,
        'tiers': tiers, 'checks': rows, 'probe_unavailable': unavailable, 'vacuous': vacuous_rows,
        'harness_missing': harness_missing,
        'hold': ({'reason': 'the emulator met calls it does not model (gate 1: never zeroed, never voided)',
                  'harness_missing': harness_missing,
                  'rescore': 'model the gap, then bench_rescore.py replays this tree with its own fixture_seed'}
                 if harness_missing else None),
        'publishable': not unpublishable and status == 'scored',
        'unpublishable_reasons': unpublishable,
        'runtime': runtime,
        'calibration': 'frozen' if CALIBRATED else 'UNCALIBRATED — rc defaults',
        'root_causes': attribute_root_causes(rows),
    }


# ── severity selftest (DESIGN §8.6) ──────────────────────────────────────────────────────────

def _perfect_rows() -> List[Dict]:
    return [{'check': n, 'tier': t, 'score': 1.0, 'detail': 'synthetic'} for n, t, *_ in CHECKS]


def _scenario(overrides: Dict[str, float], parts: Optional[Dict[str, Dict]] = None) -> List[Dict]:
    rows = _perfect_rows()
    for r in rows:
        if r['check'] in overrides:
            r['score'] = overrides[r['check']]
        if parts and r['check'] in parts:
            r['parts'] = parts[r['check']]
    return rows


def selftest_empty_observations(pack: Dict, one_function: bool = False) -> Dict:
    """The observations an empty starter (manifest with no modules) or a one-function app (a trigger that
    does nothing) produces: lint clean, nothing else exercised. Run through the REAL checks by the selftest."""
    if one_function:
        manifest = {'app': {'id': 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000',
                            'runtime': {'name': 'nodejs22.x'}},
                    'modules': {'trigger': [{'key': 'noop', 'function': 'noop', 'events': ['avi:jira:updated:issue']}],
                                'function': [{'key': 'noop', 'handler': 'index.run'}]}}
        build = {'functions': [{'key': 'noop', 'handler': 'index.run', 'bundled': True, 'loaded': True,
                                'exported': True, 'forgePackages': {}, 'outsideKit': []}]}
        live_events = [{'changelogId': e['changelogId'], 'slot': (e.get('delivery') or {}).get('slot'),
                        'duplicate': False, 'triggerInvocations': [f"inv-{e['changelogId']}"]} for e in pack['live']]
        invs = [{'inv': f"inv-{e['changelogId']}", 'kind': 'trigger', 'moduleKey': 'noop', 'functionKey': 'noop',
                 'ok': True} for e in pack['live']]
    else:
        manifest = {'app': {'id': 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000',
                            'runtime': {'name': 'nodejs22.x'}}, 'modules': {}}
        build, live_events, invs = {'functions': []}, [], []
    lint_run = {'counts': {'errors': 0, 'warnings': 0}, 'problems': [], 'stageReached': 3, 'stagesTotal': 3}
    empty_phase = {'calls': [], 'invocations': [], 'deliveries': [], 'kvsAfter': {'entities': {}, 'keys': []}}
    return {'manifest': manifest, 'kit': {'pins': {'@forge/api': '8.2.0'}},
            'lint': {'runs': [lint_run, copy.deepcopy(lint_run)]}, 'build': build,
            'phases': {'backfill': dict(empty_phase), 'live': {**empty_phase, 'events': live_events, 'invocations': invs},
                       'heal': dict(empty_phase), 'rerun': dict(empty_phase)},
            'rovo': {'calls': []}, 'comments': [], 'ui': {'surfaces': [], 'sprintAction': []},
            'harnessMissing': [], 'sectionErrors': {}}


def single_defect_costs() -> Dict[str, float]:
    """§8.6 (10): each weighted or critical row alone at 0 (a measured zero, not vacuous) → final score."""
    return {n: compose_from_rows(_scenario({n: 0.0}))['score'] for n, t, *_ in CHECKS}


def severity_selftest() -> List[str]:
    fails: List[str] = []

    def expect(cond, msg):
        if not cond:
            fails.append(msg)

    def score(rows):
        return compose_from_rows(rows)

    base = score(_perfect_rows())
    expect(abs(base['score'] - 1.0) < 1e-9, f"all-perfect must compose to 1.0 (got {base['score']})")
    # (1) every weighted row earns
    for n, t, *_ in CHECKS:
        if n in DIAGNOSTIC:
            continue
        expect(score(_scenario({n: 0.0}))['rawScore'] < 1.0, f'(1) row {n} earns no weight')
    # (2) one lint warning costs points and never caps
    warn = score(_scenario({'l_lint_warnings': 0.8}))
    expect(warn['score'] < 1.0 and warn['admission']['ceiling'] == 1.0, '(2) a lint warning must cost and never cap')
    # (3) a permission leak scores below a missing chart
    leak = score(_scenario({'b_no_permission_leak': 0.0}))['score']
    chart = score(_scenario({'u_widget_chart': 0.0}))['score']
    expect(leak < chart, f'(3) leak ({leak}) must score below a missing chart ({chart})')
    # (4) a duplicate comment scores below a missing comment
    dup = score(_scenario({'b_comment_exactly_once': 0.0}))['score']
    vac = {'vacuous_root': 'precondition: >= 1 comment POST'}
    missing = score(_scenario({'b_comment_exactly_once': 0.0, 'b_comment_adf_as_user': 0.0, 'u_comment_flow': 0.0},
                              parts={'b_comment_exactly_once': vac, 'b_comment_adf_as_user': vac}))['score']
    expect(dup < missing, f'(4) duplicate comment ({dup}) must score below a missing comment ({missing})')
    # (5) gadget instead of widget is held at 0.799
    gadget = score(_scenario({'k_dashboard_widget': 0.0}))
    expect(gadget['score'] <= 0.799, f"(5) a gadget app must be held at 0.799 (got {gadget['score']})")
    # (6) empty starter and one-function app: scored through the REAL checks, final <= 0.05
    pack = fo.synthetic_pack()
    for label, one in (('empty starter', False), ('one-function app', True)):
        ctx = Ctx(None, selftest_empty_observations(pack, one), pack, fixture_seed=pack['seed'])
        v = compose_from_rows(evaluate_rows(ctx), ctx)
        expect(v['status'] == 'scored' and v['score'] <= 0.05, f"(6) {label} must be scored at <= 0.05 (got {v['score']})")
        expect(not v['probe_unavailable'], f"(6) {label}: rows unavailable {v['probe_unavailable']}")
    # (7) a dead bundle multiplies once and lands <= 0.30
    deps = ROOT_BLOCKS['l_bundles_load']
    over = {n: 0.0 for n in deps}
    over.update({'l_bundles_load': 0.0, 'l_lint_warnings': 0.0, 'l_scopes': 0.0, 'k_widget_edit_bridge': 0.0,
                 'k_current_apis': 0.0})
    parts = {n: {'vacuous_root': 'l_bundles_load'} for n in deps if n in CRITICAL_CHECKS}
    dead = score(_scenario(over, parts))
    fired = [r for r in dead['critical']['rows'] if not r.get('suppressed')]
    expect(len(fired) == 1, f'(7) a dead bundle must multiply once (fired {len(fired)})')
    expect(dead['score'] <= 0.30, f"(7) a dead bundle must land <= 0.30 (got {dead['score']})")
    # (8) a 0.9 backfill multiplies by exactly 0.8
    b9 = score(_scenario({'r_backfill_complete': 0.9}))
    expect(abs(b9['critical']['multiplier'] - 0.8) < 1e-9, f"(8) 0.9 backfill must multiply by 0.8 ({b9['critical']['multiplier']})")
    # (9) dominance: every band failure lands at or below its ceiling
    for limit, label, names in ADMISSION_BANDS:
        for n in names:
            v = score(_scenario({n: 0.0}))
            expect(v['score'] <= limit + 1e-9, f'(9) {n} failing must hold {label} at {limit} (got {v["score"]})')
    # (11) band 4's graded ceiling at n = 1, 2, 3, 4, 5 and every failed row (DESIGN §8.5)
    # Rows no other band row is the root of, so k failed rows are k defects (band_defects prices a shadow once).
    band4 = [n for n in ADMISSION_BANDS[-1][2] if not any(n in ROOT_BLOCKS.get(r, ()) for r in ADMISSION_BANDS[-1][2])]
    for k, want in ((1, 0.899), (2, 0.869), (3, 0.839), (4, 0.809), (5, 0.799), (len(band4), 0.799)):
        got = admit(_scenario({n: 0.0 for n in band4[:k]}))['ceiling']
        expect(abs(got - want) < 1e-9, f'(11) band 4 with {k} failed row(s) must cap at {want} (got {got})')
    # severity ordering: every critical costs more than any single non-critical defect
    costs = single_defect_costs()
    worst_plain = min(v for n, v in costs.items() if n not in CRITICAL_CHECKS and n not in DIAGNOSTIC
                      and n not in {x for _l, _b, names in ADMISSION_BANDS for x in names})
    for n in CRITICAL_CHECKS:
        expect(costs[n] < worst_plain, f'a critical ({n} {costs[n]}) must cost more than any unbanded defect ({worst_plain})')
    return fails


# ── reference gate (DESIGN §13.4 items 1–4) ──────────────────────────────────────────────────

def reference_failures(result: Dict) -> List[str]:
    fails = []
    for r in result['checks']:
        n = r['check']
        if r.get('unavailable'):
            fails.append(f'{n}: UNAVAILABLE — a harness defect blocks the freeze')
            continue
        if (r.get('parts') or {}).get('vacuous_root'):
            fails.append(f'{n}: vacuous on the golden — its contract hook produced no evidence')
            continue
        if n in CALIBRATION_OWNED or n in DIAGNOSTIC and n not in CRITICAL_CHECKS:
            continue
        if n in CRITICAL_CHECKS and r['score'] < 1.0 - 1e-9:
            fails.append(f"{n}: CRITICAL {r['score']} != 1.0 — criticals are facts the golden achieves")
        elif r['score'] < 1.0 - 1e-9:
            fails.append(f"{n}: {r['score']} < 1.0 — {str(r.get('detail'))[:100]}")
    if result.get('harness_missing'):
        fails.append(f"harness_missing: {result['harness_missing'][:6]}")
    if not result.get('excellence_gate'):
        fails.append('E gate shut on the golden')
    if result.get('runtime') != 'wrapper':
        fails.append('the reference must run on the pinned Forge wrapper, not the shim')
    fails += severity_selftest()
    return fails


# ── report ───────────────────────────────────────────────────────────────────────────────────

def format_report(result: Dict, title: str = '') -> str:
    t, crit = result['tiers'], result['critical']
    lines = [f"{title} {result['scorerVersion']}: {result['score']:.4f} [{result['status']}]"
             f"{'' if result['publishable'] else ' UNPUBLISHABLE: ' + '; '.join(result['unpublishable_reasons'])}",
             f"Earned {result['rawScore']:.4f} = (0.88 x inner {result['inner']:.4f} + 0.12 x excellence "
             f"{t['E']['mean']:.4f}) x critical multiplier {crit['multiplier']}; admission ceiling "
             f"{result['admission']['ceiling']:.3f}",
             'Unsuppressed criticals: ' + (', '.join(crit['unsuppressed']) or 'none'),
             *result['admission']['reasons'],
             '  '.join(f"{k} {100 * t[k]['mean']:.0f}%" for k in TIER_ORDER if k in t),
             f"seed {result.get('fixture_seed')} (dev {result.get('dev_seed')}), runtime {result.get('runtime')}"]
    if not CALIBRATED:
        lines.append(UNCALIBRATED_BANNER)
    if result.get('harness_missing'):
        lines.append(f"HELD for rescore — unmodelled: {result['harness_missing'][:6]}")
    dr = result.get('deploy_readiness') or {}
    if dr:
        lines.append(f"Deploy readiness: {dr['verdict']} ({dr['checked']} rules checked)")
        for f in dr.get('would_fail', []) + dr.get('poor_practice', []):
            label = 'would fail on real Forge' if f['kind'] == 'would_fail' else 'works but poor practice'
            lines.append(f"  - {label}: {f['rule']}: {f['why']}" + (f" [{f['where']}]" if f['where'] else '')
                         + (f" (graded by {f['graded_by']})" if f.get('graded_by') else '') + f" — {f['doc']}")
    for root, blocked in (result.get('root_causes') or {}).items():
        lines.append(f'{len(blocked)} check(s) downstream of `{root}`: {", ".join(blocked)}')
    for row in sorted(result['checks'], key=lambda r: (TIER_ORDER.index(r['tier']), r['check'])):
        flag = '◌' if row.get('unavailable') else '∅' if (row.get('parts') or {}).get('vacuous_root') else ' '
        lines.append(f" {flag}{row['tier']} {100 * row['score']:>3.0f}% {row['check']:<26} {str(row.get('detail', ''))[:90]}")
    return '\n'.join(lines)


# ── gathering (I2 through forge_probe.mjs) and the hermetic CLI ──────────────────────────────

_CHILDREN: List[subprocess.Popen] = []


def _reap_children(*_a) -> None:
    for proc in _CHILDREN:
        if proc.poll() is None:
            try:
                os.kill(proc.pid, signal.SIGKILL)   # gate 4: the pid, never its group
            except ProcessLookupError:
                pass


def _draw_seed() -> str:
    return os.urandom(8).hex()


def _port_holder(port: int) -> Optional[str]:
    """run_build's dev site binds the run's port; the scoring site binds an ephemeral one (DESIGN §9)."""
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            sock.bind(('127.0.0.1', port))
        except OSError as error:
            return f'127.0.0.1:{port} is held ({error.strerror})'
    return None


def _render_node() -> Optional[str]:
    return os.environ.get('GOOSE_SWARM_RENDER_NODE') or shutil.which('node')


def _probe_preflight() -> Optional[str]:
    node = _render_node()
    if not node:
        return 'no node runtime (set GOOSE_SWARM_RENDER_NODE)'
    try:
        out = subprocess.run([node, str(PROBE_SCRIPT), '--preflight'], capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as error:
        return f'{node} {PROBE_SCRIPT.name} --preflight failed: {error}'
    if out.returncode != 0:
        return (out.stdout + out.stderr).strip()[-600:] or f'preflight exited {out.returncode}'
    return None


def _kit() -> Tuple[Optional[Dict], Optional[str]]:
    try:
        import forge_kit  # WP1 (DESIGN §10)
    except ImportError:
        return None, 'bench/forge_kit.py is not present (WP1 deliverable) — no pinned kit to score against'
    try:
        info = forge_kit.ensure()
    except Exception as error:
        return None, f'forge_kit.ensure() refused: {error}'
    # forge_kit.ensure() -> {kit_dir, modules_dir, kit_lock_sha256, kit_code_sha256, wrapper_sha256}
    return {**info, 'dir': info['kit_dir'], 'lock_sha256': info['kit_lock_sha256'],
            'wrapper_sha256': info.get('wrapper_sha256'),
            'app_modules': str(Path(info['modules_dir']) / 'app-modules' / 'node_modules')}, None


def _trace_header(tree: Path) -> Dict:
    trace = tree / 'trace.jsonl'
    if not trace.is_file():
        return {}
    try:
        return json.loads(trace.read_text().splitlines()[0])
    except (ValueError, IndexError):
        return {}


def build_pack(seed: str, out: Path) -> Dict:
    fixtures = ROOT / 'forge' / 'site' / 'fixtures.cjs'
    if not fixtures.is_file():
        raise RuntimeError('forge/site/fixtures.cjs is not present (WP1, interface I1)')
    node = _render_node()
    proc = subprocess.run([node, str(fixtures), '--seed', seed, '--out', str(out), '--scoring'], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f'fixtures.cjs failed: {proc.stderr[-400:]}')
    return json.loads(out.read_text())


def clone_tree(src: Path, dest: Path) -> None:
    shutil.copytree(src, dest, symlinks=False, ignore=shutil.ignore_patterns(
        'node_modules', '.forge-dev', 'forge-shots', 'bench-media', '.swarm', '__pycache__', 'trace.jsonl', 'verdict.json',
        'engine-console.log', 'forge-observations.json'))


def _probe_one(root: Path, seed: str, kit: Dict, runtime: str, shots: Path, media_dir: Optional[Path],
               obs_copy: Path) -> Tuple[Dict, Dict]:
    """One scoring site: a fresh clone of the tree, the site seeded with `seed`, the probe; returns (observations, pack)."""
    with tempfile.TemporaryDirectory(prefix='forge-score-') as tmp:
        tmp = Path(tmp)
        app = tmp / 'app'
        clone_tree(Path(root), app)
        pack = build_pack(seed, tmp / 'pack.json')
        obs_path = tmp / 'forge-observations.json'
        shots.mkdir(parents=True, exist_ok=True)
        cmd = [_render_node(), str(PROBE_SCRIPT), '--app', str(app), '--kit', str(kit.get('dir')), '--seed', seed,
               '--out', str(obs_path), '--shots', str(shots), '--runtime', runtime, '--repo', str(ROOT)]
        if media_dir is not None:
            cmd += ['--media', str(media_dir)]
        proc = subprocess.Popen(cmd, cwd=tmp)
        _CHILDREN.append(proc)
        code = proc.wait()
        if not obs_path.is_file():
            raise RuntimeError(f'REFUSED: forge_probe.mjs exited {code} without observations (seed {seed})')
        obs = json.loads(obs_path.read_text())
        # Pixels are read once, here, into the observations, so evaluate() stays pure over (tree, observations,
        # pack) and a kept observation file rescored later grades v_dark_mode the same way.
        for surface in (obs.get('ui') or {}).get('surfaces') or []:
            shot = surface.get('shot')
            measured = png_dominant(Path(shot)) if shot and Path(shot).is_file() else None
            if measured:
                surface['dominant'] = list(measured[0])
                surface['dominantShare'] = round(measured[1], 4)
                surface['blank'] = measured[1] >= float(TH['blank_share'])
        obs_path.write_text(json.dumps(obs))
        obs_copy.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(obs_path, obs_copy)
    return obs, pack


def gather(root: Path, _port, _db_dir, trace_path: Optional[Path] = None, mark_phase=None, seed: Optional[str] = None,
           runtime: str = 'wrapper', single_seed: bool = False) -> Ctx:
    """run_build's interface: clone the tree, start the scoring site with `seed`, run the probe, load I5 — on the
    run's own seed and, unless `single_seed` (the controls and the calibration, which name their seeds), on the two
    derived scoring sites (SCORING_SEEDS). The run's own seed keeps the published shots, recording and
    forge-observations.json; the derived sites' evidence lands under forge-shots/seeds/<seed>/."""
    if not seed:
        raise RuntimeError('REFUSED: score_forge.gather needs the fixture seed')
    kit, why = _kit()
    if kit is None:
        raise RuntimeError('REFUSED: ' + why)
    header = _trace_header(Path(root))
    started = time.monotonic()
    seeds = [seed.lower()] if single_seed else scoring_seeds(seed, header.get('dev_seed'))
    shots = Path(root) / 'forge-shots'
    shutil.rmtree(shots, ignore_errors=True)
    media_dir = Path(root).resolve() / 'bench-media'
    shutil.rmtree(media_dir, ignore_errors=True)
    ctxs = []
    for i, s in enumerate(seeds):
        where = shots if i == 0 else shots / 'seeds' / s
        obs, pack = _probe_one(root, s, kit, runtime, where, media_dir if i == 0 else None,
                               Path(root) / 'forge-observations.json' if i == 0 else where / 'forge-observations.json')
        ctxs.append(Ctx(Path(root), obs, pack, fixture_seed=s, dev_seed=header.get('dev_seed'), runtime=runtime))
    ctx = ctxs[0]
    ctx.seed_ctxs = ctxs[1:]
    ctx.scorer_seconds = round(time.monotonic() - started, 3)
    ctx.kit = kit
    return ctx


LOCK_FILE = Path(tempfile.gettempdir()) / 'goose-forge-score.lock'


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--tree', type=Path)
    ap.add_argument('--seed')
    ap.add_argument('--json-out', type=Path)
    ap.add_argument('--calibrate', type=Path, nargs='+', metavar='GOLDEN_VERDICT',
                    help='fit forge-thresholds.json from >= 5 reference-passing golden verdicts; prints the sha to pin')
    ap.add_argument('--reference', action='store_true')
    ap.add_argument('--runtime', choices=('wrapper', 'shim'), default='wrapper')
    ap.add_argument('--port', type=int, help='accepted for the rescorer; forge binds ephemeral ports')
    ap.add_argument('--single-seed', action='store_true',
                    help='score on --seed alone (the controls and the calibration name their seeds); the default grades '
                         f'{SCORING_SEEDS} scoring sites (contract §8), the worst per correctness row')
    a = ap.parse_args(argv)
    if a.calibrate:
        try:
            fitted = calibrate([json.loads(p.read_text()) for p in a.calibrate], TH)
        except ValueError as e:
            print(f'REFUSED: {e}', file=sys.stderr)
            return 3
        text = json.dumps(fitted, indent=2, ensure_ascii=False)
        for key in [k for k in fitted if k.endswith('_rungs')]:   # one rung list per line, as the rc file
            text = re.sub(r'("%s": )\[[\s\S]*?\n  \]' % re.escape(key), lambda m, k=key: m.group(1) + json.dumps(fitted[k]), text)
        raw = (text + '\n').encode()
        THRESHOLDS_FILE.write_bytes(raw)
        print(f"wrote {THRESHOLDS_FILE.name}; pin CALIB_SHA256 = '{hashlib.sha256(raw).hexdigest()}' in score_forge.py")
        return 0
    if not a.tree or not a.json_out:
        ap.error('--tree and --json-out are required to score')
    fails = severity_selftest()
    if fails:
        print('REFUSED: severity_selftest() failed — the gradient is inverted:\n  ' + '\n  '.join(fails), file=sys.stderr)
        return 4
    if not a.seed:
        print('REFUSED: no --seed. A tree is scored against the fixture_seed its run was fed (trace.jsonl header).',
              file=sys.stderr)
        return 2
    if len(a.seed) != 16 or any(ch not in '0123456789abcdefABCDEF' for ch in a.seed):
        print(f'REFUSED: --seed must be 16 hex characters (got {a.seed!r})', file=sys.stderr)
        return 2
    seed = a.seed.lower()
    tree = a.tree.resolve()
    dev = _trace_header(tree).get('dev_seed')
    if dev and str(dev).lower() == seed:
        print('REFUSED: --seed equals the tree\'s dev_seed — the scoring site must differ from the site the entrant '
              'developed against (DESIGN §5.1)', file=sys.stderr)
        return 2
    why = _probe_preflight()
    if why is None:
        _kit_info, why = _kit()
    if why:
        print(f'REFUSED: preflight failed — {why}', file=sys.stderr)
        return 3
    import atexit
    atexit.register(_reap_children)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_x: (_reap_children(), sys.exit(130)))
    LOCK_FILE.parent.mkdir(parents=True, exist_ok=True)
    with LOCK_FILE.open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)   # serial: one forge scoring per host
        ctx = gather(tree, None, None, seed=seed, runtime=a.runtime, single_seed=a.single_seed)
        result = evaluate(ctx)
    a.json_out.write_text(json.dumps(result, indent=2, default=str))
    print(format_report(result, tree.name))
    if a.reference:
        failures = reference_failures(result)
        if failures:
            print('REFERENCE GATE: REFUSED\n  ' + '\n  '.join(failures), file=sys.stderr)
            return 3
        marker = {'reference_pass': True, 'scorer_version': VERSION, 'score': result['score'], 'fixture_seed': seed,
                  'date': time.strftime('%Y-%m-%d'), 'thresholds_calibrated': CALIBRATED,
                  'thresholds_sha256': hashlib.sha256(THRESHOLDS_FILE.read_bytes()).hexdigest()}
        (tree / 'forge-reference-pass.json').write_text(json.dumps(marker, indent=2))
        print('REFERENCE GATE: PASS')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
