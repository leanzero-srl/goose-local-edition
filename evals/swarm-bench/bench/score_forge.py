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
CALIB_SHA256 = 'TBD-AT-FREEZE'  # baked at freeze; a calibrated file must hash to this


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
    for name in ('critical_multiplier_floor', 'economy_rungs', 'ui_round_trip_rungs', 'idempotent_rerun_rungs',
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
CALIBRATION_OWNED = {'e_reconcile_economy', 'e_event_economy', 'e_ui_round_trips', 'e_idempotent_rerun'}

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
    (0.899, 'production robustness', ('t_no_double_count', 't_out_of_order', 't_retry_after_honoured',
                                      'r_heal_dropped', 'r_rate_limit', 'r_pagination', 'b_no_permission_leak',
                                      'b_comment_exactly_once', 'v_csp_clean', 'v_console_clean')),
)

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
        except fo.PackDefect as error:
            self.oracle, self.oracle_error = None, str(error)
        self.manifest = self.obs.get('manifest') if isinstance(self.obs.get('manifest'), dict) else None
        self.harness_missing = list(self.obs.get('harnessMissing') or [])
        self._row_cache: Dict = {}

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


def _tokens(item: Dict) -> set:
    leaves = {t for t in (_leaf_text(v) for v in fo.iter_leaves(item.get('value'))) if t is not None}
    leaves |= {t for t in re.split(r'[^A-Za-z0-9]+', str(item.get('key') or '')) if t}
    return leaves


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
    return [(tok, item) for tok, item in rows if change.change_id in tok and change.sprint_id in tok]


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
    return (any(_page_style(call) for call in c.all_calls('jira')), '>= 1 paginated read')


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
    listed = any(r.get('rows') for r in c.sprint_renders()) or any(
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
    staged = run.get('stageReached') is not None and run.get('stageReached') == run.get('stagesTotal')
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
    declared = set(((c.manifest or {}).get('permissions') or {}).get('scopes') or [])
    used, missing = set(), set()
    for call in c.all_calls('jira'):
        alt = call.get('scopes')
        if not isinstance(alt, dict):
            return None, f"jira call {call.get('method')} {call.get('path')} carries no OAuth2 scope alternatives"
        classic, granular = set(alt.get('classic') or []), set(alt.get('granular') or [])
        if classic and classic <= declared:
            used |= classic
        elif granular and granular <= declared:
            used |= granular
        else:
            missing |= (classic or granular) - declared
            used |= (classic or granular) & declared
    if c.all_calls('kvs'):
        if 'storage:app' in declared:
            used.add('storage:app')
        else:
            missing.add('storage:app')
    extra = declared - used
    return {'declared': sorted(declared), 'used': sorted(used), 'missing': sorted(missing), 'extra': sorted(extra)}, None


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
    used_api = any('getWidgetEditApi' in _bridge_ops(s) for s in edits)
    picks = (c.ui().get('edit') or {}).get('picks') or []
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


def _person_numbers(c: Ctx, sid: str) -> List[Dict]:
    """Every person-facing reading of a sprint's four numbers (as text) across the surfaces."""
    out = []
    for r in c.sprint_renders():
        if str(r.get('sprintId')) == sid and r.get('metrics'):
            out.append(r['metrics'])
    for v in (c.ui().get('widget') or {}).get('views') or []:
        for s in v.get('sprints') or []:
            if str(s.get('id')) == sid and s.get('metrics'):
                out.append(s['metrics'])
    for call in c.rovo_calls('sprint'):
        res = call.get('result')
        if str(call.get('sprintId')) == sid and isinstance(res, dict):
            out.append({'committed': _json_text(res.get('committed')), 'added': _json_text(res.get('added')),
                        'removed': _json_text(res.get('removed')),
                        'creep': '—' if res.get('creepPercent') is None else
                        (f"{Decimal(str(res['creepPercent'])):.1f}%" if isinstance(res.get('creepPercent'), (int, float))
                         else str(res.get('creepPercent')))})
    return out


def _json_text(v):
    return None if isinstance(v, bool) or not isinstance(v, (int, float)) else format(Decimal(str(v)).normalize(), 'f')


def _sprint_numbers_right(c: Ctx, sid: str) -> bool:
    n = c.oracle.numbers(sid)
    readings = _person_numbers(c, sid)
    return bool(readings) and any(metrics_ok(m, n) == 4 for m in readings)


@check('t_out_of_order', 'T', needs=('live', 'ui', 'rovo'))
def _(c):
    live = sorted(c.oracle.live, key=lambda e: (e.get('delivery') or {}).get('slot', 0))
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


def _retry_verdict(c: Ctx, scope: str, phases: Tuple[str, ...]) -> Tuple[float, str]:
    fault = _fault(c, scope)
    calls = sorted(c.calls(phases, 'jira'), key=lambda x: x.get('t') or 0)
    hit = next((x for x in calls if x.get('fault') == fault['id']), None)
    ra = float(fault.get('retryAfter') or 0)
    early = [x for x in calls if x.get('earlyRetry') == fault['id']]
    if early:
        return 0.0, f"retried {len(early)}x inside the {ra:g}s Retry-After window"
    inv_id = hit.get('inv')
    inv = next((i for i in c.invocations(phases) if i.get('inv') == inv_id), {})
    deliveries = [d for ph in phases for d in c.phase(ph).get('deliveries') or [] if d.get('inv') == inv_id]
    later_same = [x for x in calls if x.get('inv') == inv_id and (x.get('t') or 0) > (hit.get('t') or 0)]
    if inv.get('threw') or inv.get('timedOut') or any(d.get('result') in ('throw', 'timeout') for d in deliveries):
        return 0.0, f'the invocation threw/timed out on the 429 (platform backoff, not Retry-After): {inv.get("error")}'
    retry_after = inv.get('retryAfter')
    if retry_after is None:
        retry_after = next((d.get('retryAfter') for d in deliveries if d.get('result') == 'retry'), None)
    if retry_after is not None:
        return (1.0, f'InvocationError retryAfter {retry_after}s >= {ra:g}s') if float(retry_after) >= ra else \
            (0.0, f'InvocationError retryAfter {retry_after}s < {ra:g}s')
    if later_same:
        gap = (later_same[0].get('t') or 0) - (hit.get('t') or 0)
        return (1.0, f'waited {gap:.1f} virtual s in-invocation') if gap >= ra else (0.0, f'retried after {gap:.1f}s < {ra:g}s')
    return 0.0, 'the 429 was never retried'


@check('t_retry_after_honoured', 'T', pre=pre_consumer_fault, needs=('live', 'heal'))
def _(c):
    score, why = _retry_verdict(c, 'consumer-of-change', ('live', 'heal'))
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


def _walk_complete(style: str, pages: List[Dict]) -> Tuple[bool, str]:
    marks = []
    for p in pages:
        q, body = _query(p), p.get('body') if isinstance(p.get('body'), dict) else {}
        marks.append(q.get('nextPageToken') or body.get('nextPageToken') or q.get('startAt') or body.get('startAt') or '')
    if len(set(marks)) != len(marks):
        return False, 'a page was read twice'
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


@check('r_pagination', 'R', pre=pre_paginated, needs=('backfill', 'live', 'heal', 'rerun', 'ui', 'rovo'))
def _(c):
    calls = c.all_calls('jira')
    legacy = [x for x in calls if _page_style(x) == 'legacy']
    walks = _walks([x for x in calls if _page_style(x) != 'legacy'])
    if any(not isinstance(x.get('response'), dict) for w in walks.values() for x in w[-1:]) and walks and \
            all(not isinstance(w[-1].get('response'), dict) for w in walks.values()):
        return unavail('no paginated response bodies were recorded')
    done, bad = 0, []
    for key, pages in walks.items():
        complete, why = _walk_complete(_page_style(pages[0]), pages)
        done += complete
        if not complete:
            bad.append(f'{key[1]}: {why}')
    total = len(walks) + len(legacy)
    return g(done / total if total else 0, f'{done}/{total} paginated reads walked to their end'
             + (f'; {len(legacy)} call(s) to the removed /rest/api/3/search' if legacy else '')
             + (f'; {bad[:3]}' if bad else ''), 'pages silently skipped')


@check('r_rate_limit', 'R', pre=pre_scheduled_fault, needs=('backfill',))
def _(c):
    score, why = _retry_verdict(c, 'scheduled-run', ('backfill',))
    sched = [i for i in c.invocations(('backfill',)) if i.get('kind') == 'scheduled']
    if score and not any(i.get('ok') for i in sched):
        score, why = 0.0, why + '; the scheduled run did not complete'
    return g(score, why, 'the reconcile ignores Retry-After')


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
        if op in ('entity/query', 'entity/get') and body.get('entityName') in ledger:
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
        expected = [ch.change_id for ch in c.oracle.visible_changes(str(r.get('sprintId')), c.oracle.viewer)]
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
    return g(1 - len(bad) / len(inv), f'{len(inv) - len(bad)}/{len(inv)} invokes returned a defined, structured result'
             + (f"; bad: {[(r.get('functionKey'), r.get('error')) for r in bad[:3]]}" if bad else ''),
             'resolver contract broken')


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
        if sid in c.oracle.all_numbers():
            checks += 1
            ok += _num(r.get('hiddenCount')) == c.oracle.hidden_count(sid, c.oracle.viewer)
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


@check('b_comment_adf_as_user', 'B', pre=pre_comment_post, needs=('ui',))
def _(c):
    attempts = [x for x in c.obs.get('comments') or [] if x.get('status') not in (429, 403)]
    if not attempts:
        return g(0, 'every comment POST was rate-limited or forbidden; none landed', 'no comment')
    ok, notes = 0, []
    for x in attempts:
        issue = c.oracle.issue_by_key.get(x.get('issueKey'))
        sprint_ids = [sid for sid in c.oracle.all_numbers()
                      if any(ch.issue_key == x.get('issueKey') for ch in c.oracle.numbers(sid).changes)]
        text = adf_text(x.get('body')) if x.get('status') in (200, 201) else None
        good = text is not None and x.get('provider') == 'user' and x.get('accountId') == c.oracle.viewer \
            and issue is not None and issue['key'] in text and any(
                c.oracle.numbers(sid).name in text and fo.format_creep(c.oracle.numbers(sid).creep).rstrip('%') in text
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
        expected = c.oracle.sprints_of_board(str(v.get('board')))
        shown = v.get('sprints') or []
        right = sum(1 for i, sid in enumerate(expected) if i < len(shown) and str(shown[i].get('id')) == sid
                    and metrics_ok(shown[i].get('metrics'), c.oracle.numbers(sid)) == 4)
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
        expected = c.oracle.sprints_of_board(str(v.get('board')))
        good, why = chart_ok(v.get('chart') or {}, expected, c.oracle.all_numbers(), tol)
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
    renders = [r for r in c.sprint_renders() if str(r.get('sprintId')) in c.oracle.all_numbers()]
    if not renders:
        return absent('sprint action table')
    total = 0.0
    for r in renders:
        expected = c.oracle.visible_changes(str(r['sprintId']), c.oracle.viewer)
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
    total, ok = 0, 0
    for r in c.sprint_renders():
        if not r.get('rows'):
            continue
        expected = c.oracle.visible_changes(str(r.get('sprintId')), c.oracle.viewer)
        sort_at = r.get('sortAt') or []
        first = sort_at[0] if len(sort_at) > 0 else {}
        second = sort_at[1] if len(sort_at) > 1 else {}
        pts = r.get('sortPoints') or {}
        subs = [
            _sorted_ok(first.get('rows') or [], expected, lambda ch: -ch.at.timestamp())
            and (first.get('ariaSort') or {}).get('at') == 'descending',
            [x for x in second.get('rows') or []] == [ch.change_id for ch in expected]
            and (second.get('ariaSort') or {}).get('at') == 'ascending',
            _sorted_ok(pts.get('rows') or [], expected,
                       lambda ch: (-ch.points, ch.at, fo.changelog_order(ch.change_id)))
            and (pts.get('ariaSort') or {}).get('points') in ('descending', 'ascending'),
        ]
        total += len(subs)
        ok += sum(subs)
    return g(ok / total if total else 0, f'{ok}/{total} sort toggles ordered with aria-sort', 'sorting broken')


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
        styles = s.get('textStyles') or []
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
        if dom is None and s.get('shot') and c.root:
            measured = png_dominant(Path(s['shot']))
            dom = list(measured[0]) if measured else None
            s['blank'] = bool(measured and measured[1] >= float(TH.get('blank_share', 0.995)))
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
    ok = sum(1 for v in views if (v.get('overflow') or {}).get('scrollWidth', 1 << 30) <= (v.get('overflow') or {}).get('clientWidth', 0)
             and v.get('sprintsVisible'))
    return g(ok / len(views), f'{ok}/{len(views)} widget sizes without horizontal overflow, every sprint visible',
             'clips at some width')


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


@check('a_skill_instructions', 'A', pre=pre_skill_md)
def _(c):
    _fm, body = parse_frontmatter(_skill_text(c) or '')
    conds = {'body_le_500_lines': len(body.splitlines()) <= 500, 'names_action': ACTION_KEY in body,
             'names_sprintId': 'sprintId' in body}
    met = sum(conds.values())
    return g(met / 3, f'SKILL.md body {met}/3', 'the skill does not tell the agent how to call the action', parts=conds)


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
    used = len([x for x in c.calls(('backfill',), 'jira')])
    ratio = used / optimum
    return g(ladder(ratio, TH['economy_rungs']), f'backfill {used} Jira calls / optimum {optimum} = {ratio:.2f}x',
             parts={'calls': used, 'optimum': optimum, 'ratio': round(ratio, 3)})


@check('e_event_economy', 'E', needs=('live',))
def _(c):
    if _row(c, 't_event_rows').get('score', 0) < 1.0:
        return g(0, 'event rows inexact — economy is not credited on unfinished work')
    relevant = [e for e in c.oracle.relevant_live() if not (e.get('delivery') or {}).get('dropped')]
    used = len(c.calls(('live',), 'jira', ('trigger', 'consumer')))
    ratio = used / max(1, len(relevant))
    return g(ladder(ratio, TH['economy_rungs']), f'{used} Jira calls for {len(relevant)} relevant changes = '
             f'{ratio:.2f} per change (optimum 1)', parts={'calls': used, 'changes': len(relevant)})


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


@check('e_idempotent_rerun', 'E', needs=('rerun',))
def _(c):
    rows = len([1 for tok, _it in c.kvs_rows('heal') if any(ch.change_id in tok for ch in c.oracle.changes('final'))])
    if not rows:
        return g(0, 'no ledger rows — nothing to keep idempotent')
    writes = sum(_entity_writes(x) for x in c.calls(('rerun',), 'kvs'))
    return g(ladder(writes / rows, TH['idempotent_rerun_rungs']), f'{writes} entity write(s) on a no-change run over {rows} rows')


# ── registry-close asserts ───────────────────────────────────────────────────────────────────

REGISTERED = {n for n, *_ in CHECKS}
assert len(REGISTERED) == len(CHECKS), 'duplicate check name in the forge registry'
TIER_OF = {n: t for n, t, *_ in CHECKS}
assert set(TIER_OF.values()) == set(TIER_WEIGHT) | {'E'}
assert DIAGNOSTIC <= REGISTERED and set(CRITICAL_CHECKS) <= REGISTERED and CALIBRATION_OWNED <= REGISTERED
assert {n for _c, _l, names in ADMISSION_BANDS for n in names} <= REGISTERED
assert set(EXCELLENCE_VALUED) | set(EXCELLENCE_BINARY) <= REGISTERED
assert sum(1 for t in TIER_OF.values() if t != 'E') == 55 and sum(1 for t in TIER_OF.values() if t == 'E') == 4

ROOT_BLOCKS = {
    'l_bundles_load': tuple(n for n, t in TIER_OF.items() if t in ('T', 'R', 'S', 'B', 'U', 'A')),
    'r_backfill_complete': ('u_widget_numbers', 'a_action_result', 'u_ledger_table', 'r_removals_found'),
    'u_widget_loads': ('u_widget_numbers', 'u_widget_chart', 'u_widget_edit_config', 'v_widget_sizes'),
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


def evaluate_rows(c: Ctx) -> List[Dict]:
    rows = []
    for name, tier, fn, pre, needs in CHECKS:
        if tier == 'E':
            continue
        row = _run_check(c, name, tier, fn, pre, needs)
        c._row_cache[name] = row
        rows.append(row)
    for name, tier, fn, pre, needs in CHECKS:
        if tier == 'E':
            rows.append(_run_check(c, name, tier, fn, pre, needs))
    return rows


def evaluate(c: Ctx) -> Dict:
    result = compose_from_rows(evaluate_rows(c), c)
    result['recall_trace'] = recall_trace(c.root)
    return result


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


def admit(rows: List[Dict]) -> Dict:
    by = {r['check']: r for r in rows}
    ceiling, reasons, failed_by_band = 1.0, [], []
    for limit, label, names in ADMISSION_BANDS:
        failed = [n for n in names if not passed(by.get(n))]
        if failed:
            ceiling = min(ceiling, limit)
            failed_by_band.append({'ceiling': limit, 'band': label, 'checks': failed})
            reasons.append(f'{label}: {", ".join(failed)} (maximum {limit:.3f})')
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
    final = min(earned, admission['ceiling'])
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
        'status': status, 'score': round(final, 4), 'rawScore': round(earned, 4), 'inner': round(inner, 4),
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


def _port_holder(_port: int) -> Optional[str]:
    """Forge binds ephemeral ports chosen at scoring time; there is no advertised port to hold (DESIGN §9)."""
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
    return (info if isinstance(info, dict) else {'dir': str(info)}), None


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
    proc = subprocess.run([node, str(fixtures), '--seed', seed, '--out', str(out)], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f'fixtures.cjs failed: {proc.stderr[-400:]}')
    return json.loads(out.read_text())


def clone_tree(src: Path, dest: Path) -> None:
    shutil.copytree(src, dest, symlinks=False, ignore=shutil.ignore_patterns(
        'node_modules', '.forge-dev', 'forge-shots', '.swarm', '__pycache__', 'trace.jsonl', 'verdict.json',
        'engine-console.log', 'forge-observations.json'))


def gather(root: Path, _port, _db_dir, trace_path: Optional[Path] = None, mark_phase=None, seed: Optional[str] = None,
           runtime: str = 'wrapper') -> Ctx:
    """run_build's interface: clone the tree, start the scoring site with `seed`, run the probe, load I5."""
    if not seed:
        raise RuntimeError('REFUSED: score_forge.gather needs the fixture seed')
    kit, why = _kit()
    if kit is None:
        raise RuntimeError('REFUSED: ' + why)
    header = _trace_header(Path(root))
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='forge-score-') as tmp:
        tmp = Path(tmp)
        app = tmp / 'app'
        clone_tree(Path(root), app)
        pack = build_pack(seed, tmp / 'pack.json')
        obs_path = tmp / 'forge-observations.json'
        shots = Path(root) / 'forge-shots'
        shutil.rmtree(shots, ignore_errors=True)
        cmd = [_render_node(), str(PROBE_SCRIPT), '--app', str(app), '--kit', str(kit.get('dir')), '--seed', seed,
               '--out', str(obs_path), '--shots', str(shots), '--runtime', runtime, '--repo', str(ROOT)]
        proc = subprocess.Popen(cmd, cwd=tmp)
        _CHILDREN.append(proc)
        code = proc.wait()
        if not obs_path.is_file():
            raise RuntimeError(f'REFUSED: forge_probe.mjs exited {code} without observations')
        obs = json.loads(obs_path.read_text())
        shutil.copy2(obs_path, Path(root) / 'forge-observations.json')
    ctx = Ctx(Path(root), obs, pack, fixture_seed=seed, dev_seed=header.get('dev_seed'), runtime=runtime)
    ctx.scorer_seconds = round(time.monotonic() - started, 3)
    ctx.kit = kit
    return ctx


def evaluate_with_identity(ctx: Ctx) -> Dict:
    result = evaluate(ctx)
    kit = getattr(ctx, 'kit', {}) or {}
    result.update({'kit_lock_sha256': kit.get('lock_sha256') or (ctx.obs.get('kit') or {}).get('lockSha256'),
                   'wrapper_sha256': kit.get('wrapper_sha256') or (ctx.obs.get('kit') or {}).get('wrapperSha256'),
                   'scorer_seconds': getattr(ctx, 'scorer_seconds', None),
                   'shots': list(ctx.obs.get('shots') or []),
                   'scorer_files_sha256': {n: hashlib.sha256((HERE / n).read_bytes()).hexdigest()
                                           for n in ('score_forge.py', 'forge_oracle.py', 'forge_probe.mjs',
                                                     THRESHOLDS_FILE.name) if (HERE / n).is_file()},
                   'spec_sha256': hashlib.sha256((ROOT / SPEC).read_bytes()).hexdigest()})
    return result


LOCK_FILE = Path(tempfile.gettempdir()) / 'goose-forge-score.lock'


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--tree', type=Path, required=True)
    ap.add_argument('--seed')
    ap.add_argument('--json-out', type=Path, required=True)
    ap.add_argument('--reference', action='store_true')
    ap.add_argument('--runtime', choices=('wrapper', 'shim'), default='wrapper')
    ap.add_argument('--port', type=int, help='accepted for the rescorer; forge binds ephemeral ports')
    a = ap.parse_args(argv)
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
        ctx = gather(tree, None, None, seed=seed, runtime=a.runtime)
        result = evaluate_with_identity(ctx)
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
