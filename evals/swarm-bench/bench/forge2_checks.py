"""forge-2.0 — the v2 rows (forge2/SPEC.md R1-R9), graded from forge2-observations.json against forge2_oracle.

    import forge2_checks as fc
    rows = fc.evaluate(obs, oracle)   # oracle = forge2_oracle.Oracle(pack, include_live_ui=True), the run's own pack

A row is {check, tier (its family 'R1'..'R9'), weight, critical, score, detail, consequence[, parts][, unavailable]}:
the dict score_forge2.py's 1.0 rows carry, plus `weight` (SPEC §4: the family's weight split evenly over its rows) and
`critical` (the consequence text of a SPEC §4 critical, else None; `critical_fired(row)` says whether it fired).
Three zeros that are never confused:
  * vacuous — the stimulus never happened (no 429 reached background work, no sprint closed): 0 with
    'vacuous — precondition unmet: …' and parts.vacuous_root, never critical — an idle app earns nothing;
  * absent — the app lacks the graded surface (no jira:adminPage, no webtrigger, …): 0 with parts.absent_surface;
  * unavailable — the probe did not write what the row reads: a harness failure, never app evidence.

THE OBSERVATIONS CONTRACT (bench/forge2_probe.mjs writes these keys into forge2-observations.json beside 1.0's; P8
reads exactly them; checkpoints are 'h1'..'h6' — virtual-hour marks after the upgrade — and 'final'):
  rate        {requests: [{t_ms, invocation, kind: 'background'|'person', method, path_tpl, cost, status, reason,
                           retry_after_s[, issue]}], hours: [{hour, used, background_used, person_used}]
               [, background_share_pct]}
  invocations [{id, function_key, module_type, virtual_ms, limit_ms, killed, result_kind}]
  migration   {v1_final: [{changeId, sprintId, at}] (the v1 entity at the end),
               v2_by_checkpoint: {<checkpoint>: [{changeId, sprintId, at, deleted, estimate, boardId, ...}]},
               panel_by_checkpoint: {<checkpoint>: {text, migrated, total, complete}}}
  world       [{class, ...}] the world events the site applied (world.cjs class names)
  rovo        {calls: [{as, sprintId, result, ...}]} (1.0's section; the peer's calls run after the browse-revoke)
  webtrigger  [{case, status, side_effects}] with forge2/site/ci.cjs's step names (WT_STATUS)
  admin       {actions: [{as: 'admin'|'nonadmin'|'forged', resolver, result_ok, state_changed}], controls: {<label
               key>: found}, tree_text, secret_shown, secret_source, secret_leaks: [string]}
  field       {writes: [{t_ms, updates, status}], values_by_checkpoint: {<checkpoint>: {<issueId|issueKey>: value}},
               applied_by_checkpoint: {<checkpoint>: <live changes the site had applied>}}
  llm_v2      [{case, llm_calls, shown_text, writes_out_of_scope[, absent][, setting_applied]}] (LLM_TOOL/FAILURE/COST)
  boot        {'widget-view'|'sprint-modal'|'admin-page': {invokes_before_paint, bytes_before_paint,
               external_requests}} (null = the surface never painted)
`rate.requests[].issue` (the issue a per-issue 429 names) and `field.applied_by_checkpoint` are the two keys the probe
must add for its rows to grade (until then: per-issue 429s are named as not graded; r7_values_fresh is unavailable).

`python3 forge2_checks.py` runs the selftest: observations built from the oracle score 1.0 on every row, an idle app
earns 0 with no critical fired, and one injected defect per family costs exactly the rows it should.
"""
from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
import json
from pathlib import Path
import re
import sys
import tempfile
from typing import Callable, Dict, List, Optional, Tuple

import forge2_oracle as fo

ROOT = Path(__file__).resolve().parent.parent

FAMILY_WEIGHT = {'R1': .12, 'R2': .13, 'R3': .08, 'R4': .10, 'R5': .10, 'R6': .08, 'R7': .05, 'R8': .04, 'R9': .05}
assert abs(sum(FAMILY_WEIGHT.values()) - 0.75) < 1e-9   # SPEC §4: v2 rows are 0.75 of the score

ADMIN_LABELS = ('Background share (%)', 'AI explanations enabled', 'Daily AI token budget', 'Comment group',
                'Rotate CI secret', 'Migration', 'Recent admin changes', 'Save settings')       # SPEC §2.6
SECRET_MASK = re.compile(r'••••\S{4}')
QUOTA_REASON = 'jira-quota-tenant-based'
PER_ISSUE_REASON = 'jira-per-issue-on-write'
WT_INVALID = ('bad-signature', 'unsigned', 'tampered', 'stale', 'stale-future')      # forge2/site/ci.cjs steps
WT_VALID = ('valid', 'header-case')
WT_STATUS = {**{c: 401 for c in WT_INVALID}, 'replay': 200, 'valid': 202, 'header-case': 202}   # SPEC R6
LLM_TOOL = ('tool-injection',)                                               # forge2_probe.mjs LLM_V2_CASES
LLM_FAILURE = ('rate-limited-no-retry-after', 'no-finish-reason')
LLM_COST = ('cache', 'kill-switch', 'token-budget')
LLM_429_ATTEMPTS = 3                       # SPEC R8: <= 3 attempts per minute
SITE_LLM = ROOT / 'forge2' / 'site' / 'llm.cjs'   # PARTIAL_TEXT: the unfinished answer's words
BOOT_INVOKES = 1                           # SPEC R9
BOOT_BYTES = 150 * 1024                    # SPEC R9 "150 KB", read as KiB: the lenient side of the unit
BOOT_SURFACES = {'widget-view': ('dashboards:widget', True), 'sprint-modal': ('jira:sprintAction', True),
                 'admin-page': ('jira:adminPage', False)}


# ── registry ─────────────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class Row:
    name: str
    family: str
    critical: Optional[str]
    fn: Callable


ROWS: List[Row] = []


def row(name: str, family: str, critical: Optional[str] = None) -> Callable:
    def deco(fn):
        assert family in FAMILY_WEIGHT and name not in {r.name for r in ROWS}
        ROWS.append(Row(name, family, critical, fn))
        return fn
    return deco


def weight_of(name: str) -> float:
    family = next(r.family for r in ROWS if r.name == name)
    return FAMILY_WEIGHT[family] / sum(1 for r in ROWS if r.family == family)


def g(score: float, detail: str, consequence: str = '', parts: Optional[Dict] = None) -> Dict:
    out = {'score': round(max(0.0, min(1.0, float(score))), 4), 'detail': detail[:400], 'consequence': consequence}
    if parts:
        out['parts'] = parts
    return out


def vacuous(what: str) -> Dict:
    return {'score': 0.0, 'detail': f'vacuous — precondition unmet: {what}',
            'consequence': 'nothing exercised, nothing earned', 'parts': {'vacuous_root': f'precondition: {what}'}}


def absent(surface: str) -> Dict:
    return {'score': 0.0, 'detail': f'required app surface absent: {surface}',
            'consequence': 'the graded surface is not implemented', 'parts': {'absent_surface': surface}}


def unavail(reason: str) -> Dict:
    return {'score': 0.0, 'detail': f'UNAVAILABLE: {reason}'[:300], 'consequence': 'harness failure, not app evidence',
            'unavailable': True}


def critical_fired(r: Dict) -> bool:
    """A critical prices only an OBSERVED defect: never on a vacuous, absent or unavailable row."""
    parts = r.get('parts') or {}
    return bool(r.get('critical')) and r.get('score', 1.0) < 1.0 and not r.get('unavailable') \
        and not parts.get('vacuous_root') and not parts.get('absent_surface')


def sample(items, n: int = 3) -> str:
    items = list(items)
    return ', '.join(str(x) for x in items[:n]) + (f' (+{len(items) - n})' if len(items) > n else '')


def num(value) -> Optional[Decimal]:
    if isinstance(value, bool) or value is None:
        return None
    try:
        return Decimal(str(value))
    except InvalidOperation:
        return None


def count(value) -> Optional[int]:
    """A count the probe wrote as a number or as the list of the things counted."""
    if isinstance(value, list):
        return len(value)
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return int(value)
    return None


# ── evidence ─────────────────────────────────────────────────────────────────────────────────

class Ev:
    def __init__(self, obs: Dict, oracle: fo.Oracle):
        self.obs = obs if isinstance(obs, dict) else {}
        self.o = oracle

    def section(self, name: str, kind: type = dict) -> Tuple[Optional[object], Optional[str]]:
        error = (self.obs.get('sectionErrors') or {}).get(name)
        if error:
            return None, f'{name}: {error}'
        value = self.obs.get(name)
        if not isinstance(value, kind):
            return None, f'the probe wrote no `{name}` {kind.__name__}'
        return value, None

    @property
    def manifest(self) -> Dict:
        m = self.obs.get('manifest')
        return m if isinstance(m, dict) else {}

    def modules(self, mtype: str) -> List[Dict]:
        mods = self.manifest.get('modules')
        entries = mods.get(mtype) if isinstance(mods, dict) else None
        return [e for e in entries if isinstance(e, dict)] if isinstance(entries, list) else []

    def has_module(self, mtype: str, key: Optional[str] = None) -> bool:
        return any(key is None or m.get('key') == key for m in self.modules(mtype))

    def has_entity(self, name: str) -> bool:
        entities = ((self.manifest.get('app') or {}).get('storage') or {}).get('entities')
        return isinstance(entities, list) and any(isinstance(e, dict) and e.get('name') == name for e in entities)

    def v2(self, checkpoint: str) -> Tuple[Optional[List[Dict]], Optional[str]]:
        mig, why = self.section('migration')
        if why:
            return None, why
        by = mig.get('v2_by_checkpoint')
        rows = by.get(checkpoint) if isinstance(by, dict) else None
        if not isinstance(rows, list):
            return None, f'migration.v2_by_checkpoint has no {checkpoint!r} list'
        return [r for r in rows if isinstance(r, dict)], None

    def requests(self) -> Tuple[Optional[List[Dict]], Optional[str]]:
        rate, why = self.section('rate')
        if why:
            return None, why
        reqs = rate.get('requests')
        if not isinstance(reqs, list):
            return None, 'rate.requests is not a list'
        return sorted((r for r in reqs if isinstance(r, dict)), key=lambda r: num(r.get('t_ms')) or 0), None


def key(r: Dict) -> Tuple[str, str]:
    return (str(r.get('changeId')), str(r.get('sprintId')))


def index(rows: List[Dict]) -> Dict[Tuple[str, str], List[Dict]]:
    out: Dict[Tuple[str, str], List[Dict]] = {}
    for r in rows:
        out.setdefault(key(r), []).append(r)
    return out


def same_at(r: Dict, at) -> bool:
    a, b = num(r.get('at')), num(at)
    return a is not None and b is not None and a == b


def row_right(r: Dict, exp: Dict) -> bool:
    """An observed scope-ledger row against the oracle's: time, board, estimate (one of the accepted values),
    deleted flag."""
    est = num(r.get('estimate'))
    return (same_at(r, exp['at']) and str(r.get('boardId')) == exp['boardId']
            and est is not None and est in exp['estimates'] and bool(r.get('deleted')) == exp['deleted'])


def ledger_gate(ev: Ev) -> Optional[Dict]:
    return None if ev.has_entity('scope-ledger') else absent('storage entity `scope-ledger` (SPEC §2.4)')


# ── R1 live migration ────────────────────────────────────────────────────────────────────────

@row('r1_v1_rows_migrated', 'R1')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    expected = ev.o.v1_rows()
    if not expected:
        return vacuous('no v1 row was preloaded')
    cp = fo.MIGRATION_DEADLINE
    rows, why = ev.v2(cp)
    if why:
        return unavail(why)
    have = index(rows)
    missing = [key(e) for e in expected if key(e) not in have]
    moved = [key(e) for e in expected if key(e) in have and not any(same_at(r, e['at']) for r in have[key(e)])]
    share = (len(expected) - len(missing) - len(moved)) / len(expected)
    detail = (f'{cp}: {len(expected) - len(missing) - len(moved)} of {len(expected)} v1 rows in scope-ledger with '
              f'their changeId and time')
    if missing:
        detail += f'; missing {len(missing)} (e.g. {sample(missing)})'
    if moved:
        detail += f'; time changed {len(moved)} (e.g. {sample(moved)})'
    return g(share, detail, 'v1 history missing from the v2 ledger after the 2-hour migration window')


@row('r1_v1_rows_intact', 'R1', critical='v1 rows lost or corrupted by the migration')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    expected = {key(e): e for e in ev.o.v1_rows()}
    if not expected:
        return vacuous('no v1 row was preloaded')
    mig, why = ev.section('migration')
    if why:
        return unavail(why)
    v1 = mig.get('v1_final')
    if not isinstance(v1, list):
        return unavail('migration.v1_final (the v1 entity at the end) is not a list')
    final, why = ev.v2('final')
    if why:
        return unavail(why)
    copies = [r for r in final if key(r) in expected]
    if not copies:
        return vacuous('nothing was migrated: no v1 row reached scope-ledger, so v1 was never at risk')
    have = index([r for r in v1 if isinstance(r, dict)])
    lost = [k for k in expected if k not in have]
    changed = [k for k, e in expected.items() if k in have and not any(same_at(r, e['at']) for r in have[k])]
    corrupt = sorted({key(r) for r in copies if not same_at(r, expected[key(r)]['at'])})
    if lost or changed or corrupt:
        found = [f'{label} {len(ks)} (e.g. {sample(ks)})' for label, ks in
                 (('v1 rows lost:', lost), ('v1 rows rewritten:', changed),
                  ('scope-ledger copies with another time:', corrupt)) if ks]
        return g(0.0, '; '.join(found), 'v1 rows lost or corrupted by the migration')
    return g(1.0, f'all {len(expected)} v1 rows intact in scope-change; {len(copies)} copies keep their time')


@row('r1_events_during_migration', 'R1')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    mark = ev.o.checkpoint_mark(fo.MIGRATION_DEADLINE)
    expected = [e for e in ev.o.expected_rows('final') if e['phase'] == 'live' and e['created'] <= mark]
    if not expected:
        return vacuous('no ledger change happened during the 2-hour migration window')
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    have = index(rows)
    missing = [key(e) for e in expected if key(e) not in have]
    return g((len(expected) - len(missing)) / len(expected),
             f'{len(expected) - len(missing)} of {len(expected)} changes made while the migration ran are in '
             f'scope-ledger at the end' + (f'; missing e.g. {sample(missing)}' if missing else ''),
             'changes lost while the migration ran')


@row('r1_progress_visible', 'R1')
def _(ev: Ev) -> Dict:
    """The panel's `Migrated <n> of <total> v1 rows` line read near each hour mark: before the deadline it must name
    the right total with n <= total; from the deadline on, n = total and `complete`."""
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage (the migration progress lives in the admin panel)')
    mig, why = ev.section('migration')
    if why:
        return unavail(why)
    panels = mig.get('panel_by_checkpoint')
    cps = [cp for cp in fo.CHECKPOINTS if isinstance((panels or {}).get(cp), dict)] if isinstance(panels, dict) else []
    if not cps:
        return unavail('migration.panel_by_checkpoint holds no checkpoint')
    total = len(ev.o.v1_rows())
    if not total:
        return vacuous('no v1 row was preloaded')
    due = fo.CHECKPOINTS.index(fo.MIGRATION_DEADLINE)
    wrong = []
    for cp in cps:
        p = panels[cp]
        n, shown_total = p.get('migrated'), p.get('total')
        if fo.CHECKPOINTS.index(cp) < due:
            ok = shown_total == total and isinstance(n, int) and 0 <= n <= total
        else:
            ok = shown_total == total and n == total and p.get('complete') is True
        if not ok:
            wrong.append(f'{cp}: {p.get("text")!r}' + (' without `complete`' if p.get('complete') is not True else ''))
    return g((len(cps) - len(wrong)) / len(cps), f'{len(cps) - len(wrong)} of {len(cps)} panel reads show the migration '
             f'right (total {total}; complete from {fo.MIGRATION_DEADLINE})' + (f'; {sample(wrong, 2)}' if wrong else ''),
             'migration progress invisible to the admin')


# ── R2 Tier 1 dosing at scale ────────────────────────────────────────────────────────────────

@row('r2_background_share', 'R2')
def _(ev: Ev) -> Dict:
    rate, why = ev.section('rate')
    if why:
        return unavail(why)
    hours = [h for h in rate.get('hours') or [] if isinstance(h, dict)]
    if not hours:
        return unavail('rate.hours is empty')
    pct = rate.get('background_share_pct')
    source = 'set by the probe in the admin panel'
    if pct is None:
        pct, source = fo.BACKGROUND_SHARE_PCT, 'the contract default; the probe recorded no change'
    cap = Decimal(fo.QUOTA_POINTS) * Decimal(str(pct)) / 100
    used = [(h.get('hour'), num(h.get('background_used')) or Decimal(0)) for h in hours]
    if sum(u for _h, u in used) <= 0:
        return vacuous('no background Jira points were spent')
    over = [(h, u) for h, u in used if u > cap]
    optimum = ev.o.dosing_optimum()['points']
    busiest = max(u for _h, u in used)
    return g((len(used) - len(over)) / len(used),
             f'{len(used) - len(over)} of {len(used)} hours kept background within {pct} % ({cap} points; {source}); '
             f'busiest hour {busiest}' + (f'; over in hours {sample(h for h, _u in over)}' if over else '')
             + f'; dosing optimum for the whole run {optimum}', 'background work starves person-facing requests')


@row('r2_person_never_quota_refused', 'R2')
def _(ev: Ev) -> Dict:
    reqs, why = ev.requests()
    if why:
        return unavail(why)
    person = [r for r in reqs if r.get('kind') == 'person']
    if not person:
        return vacuous('no person-facing Jira request was observed')
    refused = [r for r in person if r.get('status') == 429 and r.get('reason') == QUOTA_REASON]
    return g(0.0 if refused else 1.0,
             f'{len(refused)} of {len(person)} person-facing requests refused for quota'
             + (f' (e.g. {sample(r.get("path_tpl") for r in refused)})' if refused else ''),
             'a person was refused because background work spent the quota')


def _issue(r: Dict) -> Optional[str]:
    value = r.get('issue')
    return str(value) if value not in (None, '') else None


@row('r2_rate_limit_reaction', 'R2')
def _(ev: Ev) -> Dict:
    reqs, why = ev.requests()
    if why:
        return unavail(why)
    bg = [r for r in reqs if r.get('kind') == 'background']
    hits = [r for r in bg if r.get('status') == 429 and num(r.get('retry_after_s')) is not None]
    if not hits:
        return vacuous('no 429 with a Retry-After reached background work')
    honoured, broken, ungraded = 0, [], []
    for h in hits:
        lo = num(h.get('t_ms')) or Decimal(0)
        hi = lo + num(h['retry_after_s']) * 1000
        within = [r for r in bg if r is not h and lo < (num(r.get('t_ms')) or Decimal(0)) < hi]
        reason = h.get('reason')
        endpoint = (h.get('method'), h.get('path_tpl'))
        if reason == QUOTA_REASON:
            bad = within                                   # pause ALL background until the reset
        elif reason == PER_ISSUE_REASON:
            if _issue(h) is None:
                ungraded.append(f'{endpoint[0]} {endpoint[1]}')
                continue
            bad = [r for r in within if r.get('method') != 'GET' and _issue(r) == _issue(h)]
        else:
            bad = [r for r in within if (r.get('method'), r.get('path_tpl')) == endpoint]   # burst: that endpoint
        if bad:
            broken.append(f'{reason} at {h.get("t_ms")} ms: {len(bad)} request(s) inside its {h["retry_after_s"]} s')
        else:
            honoured += 1
    graded = honoured + len(broken)
    if not graded:
        return unavail(f'the probe gave no issue for the per-issue 429s ({sample(ungraded)}): rate.requests[].issue')
    return g(honoured / graded,
             f'{honoured} of {graded} 429s answered by their reason (quota: all background paused; burst: the endpoint;'
             f' per-issue: the issue)' + (f'; broken: {sample(broken, 2)}' if broken else '')
             + (f'; {len(ungraded)} per-issue 429s not graded (no issue recorded)' if ungraded else ''),
             'retries into a rate limit')


@row('r2_backfill_complete', 'R2')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    expected = [e for e in ev.o.expected_rows('final') if e['phase'] == 'history' and not e['v1']]
    if not expected:
        return vacuous('every pre-upgrade change was already a v1 row: nothing to backfill')
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    have = index(rows)
    missing = [key(e) for e in expected if key(e) not in have]
    wrong = [key(e) for e in expected if key(e) in have and not any(row_right(r, e) for r in have[key(e)])]
    ok = len(expected) - len(missing) - len(wrong)
    return g(ok / len(expected),
             f'{ok} of {len(expected)} pre-upgrade changes v1 never recorded are in scope-ledger, right'
             + (f'; missing {len(missing)} (e.g. {sample(missing)})' if missing else '')
             + (f'; wrong time/board/estimate {len(wrong)} (e.g. {sample(wrong)})' if wrong else ''),
             'the ledger at scale is incomplete')


# ── R3 invocation limits in virtual time ─────────────────────────────────────────────────────

@row('r3_never_killed', 'R3')
def _(ev: Ev) -> Dict:
    invs, why = ev.section('invocations', list)
    if why:
        return unavail(why)
    invs = [i for i in invs if isinstance(i, dict)]
    if not invs:
        return vacuous('no invocation was observed')
    by_key: Dict[str, List[Dict]] = {}
    for i in invs:
        by_key.setdefault(str(i.get('function_key')), []).append(i)
    killed = {k: sum(1 for i in v if i.get('killed')) for k, v in by_key.items()}
    hit = {k: n for k, n in killed.items() if n}
    return g(1 - len(hit) / len(by_key),
             f'{len(by_key) - len(hit)} of {len(by_key)} functions never exceeded their limit'
             + (f'; killed: {sample(f"{k} ({n} of {len(by_key[k])})" for k, n in hit.items())}' if hit else ''),
             'work cut off mid-way by the invocation limit')


@row('r3_long_retry_after_deferred', 'R3')
def _(ev: Ev) -> Dict:
    invs, why = ev.section('invocations', list)
    if why:
        return unavail(why)
    reqs, why = ev.requests()
    if why:
        return unavail(why)
    by_id = {str(i.get('id')): i for i in invs if isinstance(i, dict)}
    long = {}
    for r in reqs:
        inv = by_id.get(str(r.get('invocation')))
        ra, limit = num(r.get('retry_after_s')), num((inv or {}).get('limit_ms'))
        if r.get('status') == 429 and inv is not None and ra is not None and limit is not None and ra * 1000 >= limit:
            long[str(r.get('invocation'))] = inv
    if not long:
        return vacuous("no 429 carried a Retry-After longer than its invocation's limit")
    waited = [i for i, inv in long.items() if inv.get('killed')]
    return g(1 - len(waited) / len(long),
             f'{len(long) - len(waited)} of {len(long)} invocations handed a Retry-After longer than their limit '
             f'stopped and resumed later (results: {sample(sorted({str(v.get("result_kind")) for v in long.values()}))})'
             + (f'; waited it out and were killed: {sample(waited)}' if waited else ''),
             'a long Retry-After waited out in-function')


@row('r3_no_duplicate_rows', 'R3', critical='duplicate side effect: duplicate ledger rows')
def _(ev: Ev) -> Dict:
    mig, why = ev.section('migration')
    if why:
        return unavail(why)
    by = mig.get('v2_by_checkpoint')
    if not isinstance(by, dict):
        return unavail('migration.v2_by_checkpoint is not an object')
    dups = {}
    seen = 0
    for cp, rows in by.items():
        keys = [key(r) for r in rows or [] if isinstance(r, dict)]
        seen += len(keys)
        twice = [k for k, n in Counter(keys).items() if n > 1]
        if twice:
            dups[cp] = twice
    if not seen:
        return vacuous('no scope-ledger row was observed')
    if dups:
        cp = next(iter(dups))
        return g(0.0, f'duplicate (changeId, sprintId) rows at {sample(dups)}; {cp}: {sample(dups[cp])}',
                 'duplicate side effect: duplicate ledger rows')
    return g(1.0, f'every (changeId, sprintId) appears once at each of {len(by)} checkpoints')


# ── R4 a world that changes mid-run ──────────────────────────────────────────────────────────

def world_applied(ev: Ev, cls: str) -> Optional[str]:
    world, why = ev.section('world', list)
    if why:
        return why
    if not any(isinstance(w, dict) and w.get('class') == cls for w in world):
        return f'the probe recorded no applied {cls} event'
    return None


@row('r4_sprint_close_final', 'R4')
def _(ev: Ev) -> Dict:
    """SPEC §2.5 "its ledger is final": at the end the closed sprint holds exactly the rows it had at its close —
    none purged with the sprint, none added after it."""
    gate = ledger_gate(ev)
    if gate:
        return gate
    expected = ev.o.expected_rows('final')
    closes = [str(c['sprintId']) for c in ev.o.world('sprint-close')]
    closes = [sid for sid in closes if any(e['sprintId'] == sid for e in expected)]
    if not closes:
        return vacuous('no sprint with ledger rows closed')
    why = world_applied(ev, 'sprint-close')
    if why:
        return unavail(why)
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    good, notes = 0, []
    for sid in closes:
        want = {key(e) for e in expected if e['sprintId'] == sid}
        have = {key(r) for r in rows if str(r.get('sprintId')) == sid}
        if have == want:
            good += 1
        else:
            gone, extra = sorted(want - have), sorted(have - want)
            notes.append(f'sprint {sid}: ' + ', '.join(
                f'{len(ks)} {label} (e.g. {sample(ks, 2)})' for label, ks in
                (('rows gone', gone), ('rows that are not its ledger', extra)) if ks))
    return g(good / len(closes), f'{good} of {len(closes)} closed sprints kept exactly their final ledger'
             + (f'; {notes[0]}' if notes else ''), 'a closed sprint loses or gains history')


@row('r4_move_new_board', 'R4')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    moves = ev.o.cross_board_moves()
    if not moves:
        return vacuous("no issue moved into another board's sprint")
    why = world_applied(ev, 'issue-move')
    if why:
        return unavail(why)
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    have = index(rows)
    expected = {key(e): e for e in ev.o.expected_rows('final')}
    good, notes = 0, []
    for m in moves:
        out_key = (m['changeId'], m['fromSprintId'])
        in_keys = [(m['changeId'], sid) for sid in m['toSprintIds']]
        out_ok = out_key in have and any(row_right(r, expected[out_key]) for r in have[out_key])
        in_ok = bool(in_keys) and all(k in have and any(row_right(r, expected[k]) for r in have[k]) for k in in_keys)
        if out_ok and in_ok:
            good += 1
        else:
            got = [str(r.get('estimate')) for k in in_keys for r in have.get(k, [])]
            notes.append(f'{m["issueKey"]} ({m["changeId"]}): removed row {"ok" if out_ok else "wrong/missing"}, '
                         f'added row {"ok" if in_ok else "wrong/missing"} (estimate {sample(got) or "none"}, want '
                         f'{sample(sorted(str(x) for k in in_keys for x in expected[k]["estimates"]))})')
    return g(good / len(moves), f"{good} of {len(moves)} cross-board moves recorded with the new board's estimate"
             + (f'; {notes[0]}' if notes else ''), "a moved issue carries the old board's estimate")


@row('r4_field_switch', 'R4')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    switches = ev.o.world('estimation-field')
    expected = ev.o.expected_rows('final')
    after, before = [], []
    for sw in switches:
        board = str(sw['boardId'])
        old = ev.o.field_in_force(board, sw['t'] - fo.JUST_BEFORE)
        for e in expected:
            if e['boardId'] != board:
                continue
            if e['created'] >= sw['t']:
                if ev.o.estimate_at(e['issueId'], old, None) not in e['estimates']:
                    after.append(e)            # only a change the two fields value differently tells them apart
            elif e['phase'] == 'live':
                before.append(e)
    if not after:
        return vacuous("no change after a board's estimation-field switch tells the two fields apart")
    why = world_applied(ev, 'estimation-field')
    if why:
        return unavail(why)
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    have = index(rows)
    wrong_after = [key(e) for e in after if not any(row_right(r, e) for r in have.get(key(e), []))]
    present_before = [e for e in before if key(e) in have]
    wrong_before = [key(e) for e in present_before if not any(row_right(r, e) for r in have[key(e)])]
    graded = len(after) + len(present_before)
    ok = graded - len(wrong_after) - len(wrong_before)
    return g(ok / graded, f'{len(after) - len(wrong_after)} of {len(after)} rows after the switch use the new field; '
             f'{len(present_before) - len(wrong_before)} of {len(present_before)} earlier rows keep their estimate'
             + (f'; wrong after: {sample(wrong_after)}' if wrong_after else '')
             + (f'; rewritten before: {sample(wrong_before)}' if wrong_before else ''),
             'estimates read from the wrong field after the board switched')


@row('r4_deleted_history', 'R4')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    expected = [e for e in ev.o.expected_rows('final') if e['deleted']]
    if not expected:
        return vacuous('the world deleted no issue that has ledger rows')
    why = world_applied(ev, 'issue-delete')
    if why:
        return unavail(why)
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    have = index(rows)
    gone = [key(e) for e in expected if key(e) not in have]
    unflagged = [key(e) for e in expected if key(e) in have and not any(r.get('deleted') is True for r in have[key(e)])]
    ok = len(expected) - len(gone) - len(unflagged)
    return g(ok / len(expected), f'{ok} of {len(expected)} rows of deleted issues kept as history with deleted: true'
             + (f'; erased {sample(gone)}' if gone else '') + (f'; not flagged {sample(unflagged)}' if unflagged else ''),
             'deleted issues erase history or keep counting')


def key_in(text: str, issue_key: str) -> bool:
    return bool(re.search(r'(?<![A-Za-z0-9])' + re.escape(issue_key) + r'(?![0-9])', text))


@row('r4_permission_revoked', 'R4', critical="data leak — a hidden issue's data shown to a person who cannot browse it")
def _(ev: Ev) -> Dict:
    """The person who lost browse on a project asks the Rovo action after the loss (the probe's rovo section runs
    after the scored hours): none of that project's newly hidden issues may show. Issues hidden from them since the
    install are 1.0's b_no_permission_leak, never priced here."""
    losses = ev.o.world('browse-revoke')
    if not losses:
        return vacuous('no person lost browse permission')
    why = world_applied(ev, 'browse-revoke')
    if why:
        return unavail(why)
    rovo = ev.obs.get('rovo')
    calls = rovo.get('calls') if isinstance(rovo, dict) else None
    if not isinstance(calls, list):
        return unavail('the probe wrote no rovo.calls (the requests after the browse-revoke)')
    ledger_keys = {e['issueKey'] for e in ev.o.expected_rows('final')}
    leaks, shown = [], 0
    for loss in losses:
        account = str(loss['accountId'])
        before = ev.o.hidden_issue_ids(account, loss['t'] - fo.JUST_BEFORE)
        after = ev.o.hidden_issue_ids(account, loss['t'])
        visible = {k for k in ledger_keys if ev.o.issue_by_key[k]['id'] not in after}
        terms = ev.o.leak_terms_for(after - before)
        for c in calls:
            if not isinstance(c, dict) or str(c.get('as')) != account or c.get('result') is None:
                continue
            text = json.dumps(c['result'], default=str)
            shown += any(key_in(text, k) for k in visible)
            hits = [k for k in terms['keys'] if key_in(text, k)] + [s for s in terms['summaries'] if s in text]
            if hits:
                leaks.append(f'{account} on sprint {c.get("sprintId")} after losing {loss["projectKey"]}: {sample(hits)}')
    if leaks:
        return g(0.0, f'stale data after a browse revoke: {leaks[0]}',
                 "data leak — a hidden issue's data shown to a person who cannot browse it")
    if not shown:
        return vacuous('no request after the browse revoke showed any ledger data, so hiding proves nothing')
    return g(1.0, f'{shown} answer(s) after a browse revoke showed ledger data and none of the newly hidden issues')


# ── R5 the UI Kit admin panel ────────────────────────────────────────────────────────────────

def admin_actions(ev: Ev, *who: str) -> Tuple[Optional[List[Dict]], Optional[str]]:
    admin, why = ev.section('admin')
    if why:
        return None, why
    actions = admin.get('actions')
    if not isinstance(actions, list):
        return None, 'admin.actions is not a list'
    return [a for a in actions if isinstance(a, dict) and a.get('as') in who], None


@row('r5_panel_labels', 'R5')
def _(ev: Ev) -> Dict:
    """Every §2.6 control found by its label, the CI secret shown once after `Rotate CI secret`, then masked."""
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage')
    admin, why = ev.section('admin')
    if why:
        return unavail(why)
    controls = admin.get('controls')
    if not isinstance(controls, dict) or len(controls) != len(ADMIN_LABELS):
        return unavail(f'admin.controls does not name the {len(ADMIN_LABELS)} §2.6 labels')
    missing = sorted(k for k, found in controls.items() if found is not True)
    shown = admin.get('secret_shown') is True
    masked = bool(SECRET_MASK.search(str(admin.get('tree_text') or '')))
    have = len(ADMIN_LABELS) - len(missing) + shown + masked
    return g(have / (len(ADMIN_LABELS) + 2),
             f'{len(ADMIN_LABELS) - len(missing)} of {len(ADMIN_LABELS)} §2.6 controls found by label; new secret shown '
             f'once: {"yes" if shown else "no"}; then masked ••••<last4>: {"yes" if masked else "no"}'
             + (f'; missing {sample(missing, 4)}' if missing else ''), 'the admin cannot find a control')


@row('r5_admin_actions_apply', 'R5')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage')
    acts, why = admin_actions(ev, 'admin')
    if why:
        return unavail(why)
    if not acts:
        return vacuous('no admin action was made as an admin')
    bad = [str(a.get('resolver')) for a in acts if not (a.get('result_ok') is True and a.get('state_changed') is True)]
    return g((len(acts) - len(bad)) / len(acts), f'{len(acts) - len(bad)} of {len(acts)} admin-page invokes of the '
             'admin\'s save and rotate applied' + (f'; failed or changed nothing: {sample(bad)}' if bad else ''),
             'the admin panel does not work for admins')


@row('r5_nonadmin_refused', 'R5', critical='an admin action by a non-admin succeeded')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage')
    acts, why = admin_actions(ev, 'nonadmin', 'forged')
    if why:
        return unavail(why)
    if not acts:
        return vacuous('no admin action was attempted by a non-admin or with a forged payload')
    works, _w = admin_actions(ev, 'admin')
    if not any(a.get('result_ok') is True and a.get('state_changed') is True for a in works or []):
        return vacuous('no admin action changed state even for an admin, so a refusal proves nothing')
    breaches = [f'{a.get("as")}: {a.get("resolver")} ({sample(a.get("changed_keys") or [], 2)})'
                for a in acts if a.get('state_changed')]
    if breaches:
        return g(0.0, f'state changed by {sample(breaches, 2)}', 'an admin action by a non-admin succeeded')
    return g(1.0, f'{len(acts)} non-admin/forged admin actions changed nothing')


# ── R6 the CI deployment web trigger ─────────────────────────────────────────────────────────

def wt_cases(ev: Ev) -> Tuple[Optional[List[Dict]], Optional[str]]:
    cases, why = ev.section('webtrigger', list)
    if why:
        return None, why
    return [c for c in cases if isinstance(c, dict)], None


def effects(c: Dict) -> Optional[int]:
    return count(c.get('side_effects'))


def wt_gate(ev: Ev) -> Optional[Dict]:
    return None if ev.has_module('webtrigger') else absent('webtrigger (SPEC R6)')


def wt_valid_worked(cases: List[Dict]) -> bool:
    return any(c.get('case') in WT_VALID and (effects(c) or 0) >= 1 for c in cases)


@row('r6_unsigned_no_effect', 'R6', critical='a web-trigger write without a valid signature')
def _(ev: Ev) -> Dict:
    gate = wt_gate(ev)
    if gate:
        return gate
    cases, why = wt_cases(ev)
    if why:
        return unavail(why)
    invalid = [c for c in cases if c.get('case') in WT_INVALID]
    if not invalid:
        return vacuous('no unsigned, tampered or stale event was sent')
    if any(effects(c) is None for c in invalid):
        return unavail('webtrigger[].side_effects missing on an invalid case')
    if not wt_valid_worked(cases):
        return vacuous('no valid event had an effect, so a refusal proves nothing')
    breaches = [f'{c["case"]} ({effects(c)})' for c in invalid if effects(c) > 0]
    if breaches:
        return g(0.0, f'side effects without a valid signature: {sample(breaches)}',
                 'a web-trigger write without a valid signature')
    return g(1.0, f'{len(invalid)} invalid events ({sample(sorted({c["case"] for c in invalid}), 5)}) had no effect')


@row('r6_replay_once', 'R6', critical='duplicate side effect: a replayed web-trigger event applied twice')
def _(ev: Ev) -> Dict:
    gate = wt_gate(ev)
    if gate:
        return gate
    cases, why = wt_cases(ev)
    if why:
        return unavail(why)
    replays = [c for c in cases if c.get('case') == 'replay']
    if not replays:
        return vacuous('no event was replayed')
    if not wt_valid_worked(cases):
        return vacuous('the original event had no effect to repeat')
    if any(effects(c) is None for c in replays):
        return unavail('webtrigger[].side_effects missing on the replay')
    twice = [c for c in replays if effects(c) > 0]
    if twice:
        return g(0.0, f'a replayed eventId applied again ({effects(twice[0])} side effects)',
                 'duplicate side effect: a replayed web-trigger event applied twice')
    return g(1.0, f'{len(replays)} replay(s) had no second effect')


@row('r6_status_codes', 'R6')
def _(ev: Ev) -> Dict:
    gate = wt_gate(ev)
    if gate:
        return gate
    cases, why = wt_cases(ev)
    if why:
        return unavail(why)
    graded = [c for c in cases if c.get('case') in WT_STATUS]
    if not graded:
        return vacuous('no web-trigger case was sent')
    if not any(c.get('case') in WT_VALID and c.get('status') == 202 for c in graded):
        return vacuous('no validly signed event was accepted, so the refusals prove nothing')
    wrong = [f'{c["case"]}: {c.get("status")} (want {WT_STATUS[c["case"]]})' for c in graded
             if c.get('status') != WT_STATUS[c['case']]]
    unknown = sorted({str(c.get('case')) for c in cases if c.get('case') not in WT_STATUS})
    return g((len(graded) - len(wrong)) / len(graded), f'{len(graded) - len(wrong)} of {len(graded)} cases answered '
             'with their status' + (f'; wrong: {sample(wrong, 2)}' if wrong else '')
             + (f'; unknown cases not graded: {sample(unknown)}' if unknown else ''), 'CI cannot tell what happened')


@row('r6_valid_applies', 'R6')
def _(ev: Ev) -> Dict:
    gate = wt_gate(ev)
    if gate:
        return gate
    cases, why = wt_cases(ev)
    if why:
        return unavail(why)
    valid = [c for c in cases if c.get('case') in WT_VALID]
    if not valid:
        return vacuous('no validly signed event was sent')
    bad = [c['case'] for c in valid if c.get('status') != 202 or (effects(c) or 0) < 1]
    return g((len(valid) - len(bad)) / len(valid), f'{len(valid) - len(bad)} of {len(valid)} validly signed events '
             'accepted (202) and applied' + (f'; not applied: {sample(bad)}' if bad else ''),
             'deployments never reach the ledger')


@row('r6_secret_never_disclosed', 'R6', critical='the CI secret disclosed')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage (where the CI secret is rotated)')
    admin, why = ev.section('admin')
    if why:
        return unavail(why)
    leaks = admin.get('secret_leaks')
    if not isinstance(leaks, list):
        return unavail('admin.secret_leaks is not a list')
    if not admin.get('secret_source'):
        return vacuous('no CI secret existed to leak (the panel showed none and storage holds no single secret)')
    if leaks:
        return g(0.0, f'the CI secret appeared in {sample(leaks, 2)}', 'the CI secret disclosed')
    return g(1.0, f'the CI secret ({admin["secret_source"]}) never appeared outside its one-time display')


# ── R7 the scope-status custom field ─────────────────────────────────────────────────────────

ADDED = re.compile(r'added \+(\d+(?:\.\d+)?)')


def value_ok(observed, accepted) -> bool:
    text = ' '.join(str(observed).split()) if isinstance(observed, (str, int, float)) else ''
    if text in accepted:
        return True
    m = ADDED.fullmatch(text)
    if not m:
        return False
    want = {Decimal(x.group(1)) for x in (ADDED.fullmatch(a) for a in accepted) if x}
    return Decimal(m.group(1)) in want


@row('r7_values_fresh', 'R7')
def _(ev: Ev) -> Dict:
    """At every checkpoint each issue's scope-status equals the oracle's for the Jira state the site had applied when
    the probe read it (field.applied_by_checkpoint; 'final' is every scripted change)."""
    if not ev.has_module('jira:customField', 'scope-status'):
        return absent('jira:customField scope-status')
    field, why = ev.section('field')
    if why:
        return unavail(why)
    by, applied = field.get('values_by_checkpoint'), field.get('applied_by_checkpoint')
    if not isinstance(by, dict) or not isinstance(applied, dict):
        return unavail('field.values_by_checkpoint / field.applied_by_checkpoint is not an object')
    cps = [cp for cp in fo.CHECKPOINTS if isinstance(by.get(cp), dict)]
    if not cps:
        return unavail('field.values_by_checkpoint holds no checkpoint')
    late = [cp for cp in cps if cp != 'final' and not isinstance(applied.get(cp), int)]
    if late:
        return unavail(f'field.applied_by_checkpoint lacks {sample(late)} (the live changes the site had applied)')
    key_of = {iid: i['key'] for iid, i in ev.o.issues.items()}
    shares, worst = [], None
    for cp in cps:
        values, ungraded = ev.o.field_values(cp, None if cp == 'final' else applied[cp])
        observed = {key_of.get(str(k), str(k)): v for k, v in by[cp].items()}
        graded = ({k for k, v in values.items() if v != frozenset({''})}
                  | {k for k, v in observed.items() if str(v or '').strip()}) - ungraded
        if not graded:
            continue
        wrong = sorted(k for k in graded if not value_ok(observed.get(k) or '', values.get(k, frozenset({''}))))
        share = (len(graded) - len(wrong)) / len(graded)
        shares.append(share)
        if worst is None or share < worst[0]:
            worst = (share, cp, wrong, len(graded), values, observed)
    if not shares:
        return vacuous('no issue sat in an active sprint at any checkpoint')
    _s, cp, wrong, n, values, observed = worst
    eg = '; '.join(f'{k} {observed.get(k)!r} (want {sample(sorted(values.get(k, {""})), 2)})' for k in wrong[:2])
    return g(sum(shares) / len(shares), f'mean over {len(shares)} checkpoints; worst {cp}: {n - len(wrong)} of {n} '
             'issues right' + (f'; {eg}' if eg else ''), 'the field shows stale or wrong scope status')


@row('r7_writes_accepted', 'R7')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:customField', 'scope-status'):
        return absent('jira:customField scope-status')
    field, why = ev.section('field')
    if why:
        return unavail(why)
    writes = [w for w in field.get('writes') or [] if isinstance(w, dict)]
    if not writes:
        return vacuous('the app wrote no field value')
    bad = [w for w in writes if not (isinstance(w.get('status'), int) and 200 <= w['status'] < 300)]
    return g((len(writes) - len(bad)) / len(writes), f'{len(writes) - len(bad)} of {len(writes)} bulk field writes '
             'accepted' + (f'; refused: {sample(sorted({str(w.get("status")) for w in bad}))}' if bad else ''),
             'field writes refused (over 200 updates, wrong shape)')


# ── R8 Forge LLM, properly ───────────────────────────────────────────────────────────────────

def partial_text() -> Optional[str]:
    """The words of the site's unfinished answer (forge2/site/llm.cjs PARTIAL_TEXT): read from the site, never copied."""
    try:
        m = re.search(r"const PARTIAL_TEXT = '([^']+)'", SITE_LLM.read_text())
    except OSError:
        return None
    return m.group(1) if m else None


def llm_cases(ev: Ev, names: Tuple[str, ...]) -> Tuple[Optional[List[Dict]], Optional[str], bool]:
    cases, why = ev.section('llm_v2', list)
    if why:
        return None, why, False
    cases = [c for c in cases if isinstance(c, dict)]
    calls_model = any((count(c.get('llm_calls')) or 0) >= 1 or any((count(x) or 0) >= 1 for x in c.get('per_click') or [])
                      for c in cases)
    return [c for c in cases if c.get('case') in names], None, calls_model


def case_blocked(c: Dict) -> Optional[str]:
    """A case the app could not be put through is its failure, named: no explain control, no admin setting."""
    if c.get('absent'):
        return f'{c["case"]}: {c["absent"]}'
    if c.get('setting_applied') is False:
        return f'{c["case"]}: the admin panel could not switch the setting'
    return None


def llm_gate(ev: Ev) -> Optional[Dict]:
    return None if ev.has_module('llm') else absent('llm (Forge LLM)')


@row('r8_tool_call_scope', 'R8')
def _(ev: Ev) -> Dict:
    gate = llm_gate(ev)
    if gate:
        return gate
    cases, why, _calls = llm_cases(ev, LLM_TOOL)
    if why:
        return unavail(why)
    blocked = [b for b in map(case_blocked, cases) if b]
    asked = [c for c in cases if not case_blocked(c) and (count(c.get('llm_calls')) or 0) >= 1]
    if not asked and not blocked:
        return vacuous('the model was never asked in the manipulated-tool-call case')
    bad = [c for c in asked if (count(c.get('writes_out_of_scope')) or 0) > 0]
    total = len(asked) + len(blocked)
    return g((len(asked) - len(bad)) / total, f'{len(asked) - len(bad)} of {total} manipulated tool calls validated '
             "against the viewer's sprint scope before acting"
             + (f'; acted out of scope {count(bad[0].get("writes_out_of_scope"))} time(s)' if bad else '')
             + (f'; {blocked[0]}' if blocked else ''), "a prompt-injected tool call writes outside the viewer's scope")


@row('r8_failure_handling', 'R8')
def _(ev: Ev) -> Dict:
    gate = llm_gate(ev)
    if gate:
        return gate
    cases, why, calls_model = llm_cases(ev, LLM_FAILURE)
    if why:
        return unavail(why)
    if not cases or not calls_model:
        return vacuous('no LLM failure case reached a model call')
    ok, notes = 0, []
    for c in cases:
        blocked = case_blocked(c)
        if blocked:
            good, note = False, blocked
        elif c['case'] == 'rate-limited-no-retry-after':
            good = (count(c.get('llm_calls')) or 0) <= LLM_429_ATTEMPTS
            note = f'429 without Retry-After: {c.get("llm_calls")} attempts in the minute (<= {LLM_429_ATTEMPTS})'
        else:
            words = partial_text()
            if words is None:
                return unavail(f'{SITE_LLM} defines no PARTIAL_TEXT (the unfinished answer the probe looks for)')
            good = words not in str(c.get('shown_text') or '')
            note = 'an answer without finish_reason was ' + ('not shown' if good else 'SHOWN as if complete')
        ok += good
        if not good:
            notes.append(note)
    return g(ok / len(cases), f'{ok} of {len(cases)} LLM failures handled' + (f'; {sample(notes, 2)}' if notes else ''),
             'retry storms or truncated answers shown as complete')


@row('r8_cost_controls', 'R8')
def _(ev: Ev) -> Dict:
    gate = llm_gate(ev)
    if gate:
        return gate
    cases, why, calls_model = llm_cases(ev, LLM_COST)
    if why:
        return unavail(why)
    if not cases:
        return vacuous('no cache, kill-switch or token-budget case was run')
    if not calls_model:
        return vacuous('the app never called the model, so making no call proves nothing')
    bad = [case_blocked(c) or f'{c["case"]} ({c.get("llm_calls")} calls)' for c in cases
           if case_blocked(c) or (count(c.get('llm_calls')) or 0) > 0]
    return g((len(cases) - len(bad)) / len(cases), f'{len(cases) - len(bad)} of {len(cases)} cache/kill-switch/budget '
             'cases made no model call' + (f'; {sample(bad, 2)}' if bad else ''),
             'AI spend the admin switched off or capped')


# ── R9 the Custom UI boot budget ─────────────────────────────────────────────────────────────

def boot_row(ev: Ev, surface: str) -> Dict:
    """null in the probe's record means the surface never reached its first paint/render: that budget item fails."""
    mtype, front = BOOT_SURFACES[surface]
    if not ev.has_module(mtype):
        return absent(mtype)
    boot, why = ev.section('boot')
    if why:
        return unavail(why)
    rec = boot.get(surface)
    if not isinstance(rec, dict):
        return unavail(f'boot[{surface!r}] not recorded')
    invokes = count(rec.get('invokes_before_paint'))
    if invokes is None:
        return g(0.0, f'{surface}: never reached its first paint, so no boot budget was met', 'a surface that never paints')
    parts = {'invokes': int(invokes <= BOOT_INVOKES)}
    detail = f'{surface}: {invokes} invoke(s) before first paint (<= {BOOT_INVOKES})'
    if front:
        size, external = count(rec.get('bytes_before_paint')), count(rec.get('external_requests'))
        parts['bytes'] = int(size is not None and size <= BOOT_BYTES)
        parts['external'] = int(external == 0)
        detail += f', {size} bytes of JS+CSS (<= {BOOT_BYTES}), {external} external request(s)'
    return g(sum(parts.values()) / len(parts), detail, 'a slow or leaky first paint', parts=parts)


@row('r9_widget_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'widget-view')


@row('r9_sprint_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'sprint-modal')


@row('r9_admin_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'admin-page')


# ── evaluation ───────────────────────────────────────────────────────────────────────────────

WEIGHTS = {r.name: weight_of(r.name) for r in ROWS}
CRITICAL = {r.name: r.critical for r in ROWS if r.critical}
assert set(FAMILY_WEIGHT) == {r.family for r in ROWS}
assert abs(sum(WEIGHTS.values()) - 0.75) < 1e-9


def evaluate(obs: Dict, oracle: Optional[fo.Oracle]) -> List[Dict]:
    """Every v2 row. `oracle` is the run's 2.0 oracle (include_live_ui=True); None or a 1.0 pack makes every row
    unavailable (a pack defect is the harness's, never the app's)."""
    out = []
    for r in ROWS:
        base = {'check': r.name, 'tier': r.family, 'weight': round(WEIGHTS[r.name], 6), 'critical': r.critical}
        if oracle is None or not getattr(oracle, 'is_v2', False):
            res = unavail('pack defect: no 2.0 oracle (the pack lacks upgradeAt/v1Rows/world or failed to load)')
        else:
            try:
                res = r.fn(Ev(obs, oracle))
            except fo.PackDefect as error:
                res = unavail(f'pack defect: {error}')
            except Exception as error:  # a scorer bug is never the app's fault
                res = unavail(f'scorer error: {type(error).__name__}: {error}')
        out.append({**base, **res})
    return out


def weighted(rows: List[Dict]) -> float:
    """The v2 rows' contribution (out of 0.75) — for the selftest and a quick read; composition is P7's."""
    return sum(r['weight'] * r['score'] for r in rows if not r.get('unavailable'))


# ── selftest: perfect, idle, and one defect per family ──────────────────────────────────────

SELFTEST_PARTIAL = 'Scope grew after the sprint started because the team pulled in'


def perfect_observations(o: fo.Oracle) -> Dict:
    """What an app that meets every R1-R9 guarantee shows the probe on this pack, built from the oracle in the shapes
    forge2_probe.mjs writes."""
    def v2(cp):
        return [{'changeId': e['changeId'], 'sprintId': e['sprintId'], 'at': float(e['at']), 'deleted': e['deleted'],
                 'estimate': float(min(e['estimates'])), 'boardId': e['boardId'], 'kind': e['kind'],
                 'issueKey': e['issueKey'], 'deployedEnvs': None} for e in o.expected_rows(cp)]
    order = [str(e['changelogId']) for e in o.pack['live']]
    values, applied = {}, {}
    for cp in fo.CHECKPOINTS:
        mark = o.checkpoint_mark(cp)
        applied[cp] = sum(1 for e in o.pack['live'] if mark is None or fo.instant(e['created']) <= mark)
        accepted, ungraded = o.field_values(cp, None if cp == 'final' else applied[cp])
        by_id = {i['key']: iid for iid, i in o.issues.items()}
        values[cp] = {by_id[k]: min(v) for k, v in accepted.items() if v != frozenset({''}) and k not in ungraded}
    assert len(order) == applied['final']
    total = len(o.v1_rows())
    peer = next(e for e in o.world('browse-revoke'))['accountId']
    hidden = o.hidden_issue_ids(peer)
    visible = sorted({e['issueKey'] for e in o.expected_rows('final') if o.issue_by_key[e['issueKey']]['id'] not in hidden})
    bg = lambda t, tpl, **kw: {'t_ms': t, 'invocation': 'inv-c1', 'kind': 'background', 'method': 'GET',  # noqa: E731
                               'path_tpl': tpl, 'cost': 1, 'status': 200, 'reason': None, 'retry_after_s': None, **kw}
    return {
        'manifest': {'modules': {'jira:adminPage': [{'key': 'admin'}], 'webtrigger': [{'key': 'ci'}],
                                 'jira:customField': [{'key': 'scope-status', 'type': 'string', 'readOnly': True}],
                                 'dashboards:widget': [{'key': 'scope-widget'}],
                                 'jira:sprintAction': [{'key': 'scope-sprint-ledger'}], 'llm': [{'key': 'llm'}]},
                     'app': {'storage': {'entities': [{'name': 'scope-change'}, {'name': 'scope-ledger'}]}}},
        'migration': {'v1_rows': [{'changeId': r['changeId'], 'sprintId': r['sprintId'], 'at': r['at']}
                                  for r in o.v1_rows()],
                      'v1_final': [{'changeId': r['changeId'], 'sprintId': r['sprintId'], 'at': r['at']}
                                   for r in o.v1_rows()],
                      'v2_by_checkpoint': {cp: v2(cp) for cp in fo.CHECKPOINTS},
                      'panel_by_checkpoint': {cp: {'t_ms': 0, 'text': f'Migrated {total} of {total} v1 rows',
                                                   'migrated': total, 'total': total, 'complete': True}
                                              for cp in fo.CHECKPOINTS[:-1]}},
        'world': [{'class': e['class'], 'at': e['at']} for e in o.world()],
        'rovo': {'calls': [{'as': peer, 'label': 'sprint', 'sprintId': sid, 'threw': False,
                            'result': {'changes': [{'issueKey': k} for k in visible[:3]]}} for sid in o.ledger_sprints()]},
        'rate': {'requests': [
            bg(1000, '/rest/api/3/search/jql'),
            bg(2000, '/rest/api/3/changelog/bulkfetch', method='POST', status=429, reason='jira-burst-based',
               retry_after_s=2),
            bg(4500, '/rest/api/3/changelog/bulkfetch', method='POST'),
            bg(5000, '/rest/api/3/issue/{issueIdOrKey}', method='PUT', status=429, reason=PER_ISSUE_REASON,
               retry_after_s=2, issue='PAY-1'),
            bg(5100, '/rest/api/3/issue/{issueIdOrKey}', method='PUT', issue='PAY-3'),
            bg(7500, '/rest/api/3/issue/{issueIdOrKey}', method='PUT', issue='PAY-1'),
            bg(8000, '/rest/api/3/search/jql', invocation='inv-s1', status=429, reason=QUOTA_REASON,
               retry_after_s=1800),
            bg(3_600_000, '/rest/api/3/search/jql', invocation='inv-s2'),
            {'t_ms': 9000, 'invocation': 'inv-r1', 'kind': 'person', 'method': 'GET', 'path_tpl': '/rest/api/3/myself',
             'cost': 1, 'status': 200, 'reason': None, 'retry_after_s': None}],
            'hours': [{'hour': h, 'used': 120, 'background_used': 100, 'person_used': 20} for h in range(1, 7)]},
        'invocations': [
            {'id': 'inv-c1', 'function_key': 'consume-change', 'module_type': 'consumer', 'virtual_ms': 4000,
             'limit_ms': 55000, 'killed': False, 'result_kind': 'ok'},
            {'id': 'inv-s1', 'function_key': 'reconcile', 'module_type': 'scheduledTrigger', 'virtual_ms': 9000,
             'limit_ms': 900000, 'killed': False, 'result_kind': 'ok'},
            {'id': 'inv-s2', 'function_key': 'reconcile', 'module_type': 'scheduledTrigger', 'virtual_ms': 3000,
             'limit_ms': 900000, 'killed': False, 'result_kind': 'ok'},
            {'id': 'inv-r1', 'function_key': 'ui-resolver', 'module_type': 'resolver', 'virtual_ms': 300,
             'limit_ms': 25000, 'killed': False, 'result_kind': 'ok'}],
        'webtrigger': [{'case': 'valid', 'status': 202, 'side_effects': 2},
                       {'case': 'replay', 'status': 200, 'side_effects': 0},
                       *({'case': c, 'status': 401, 'side_effects': 0} for c in WT_INVALID),
                       {'case': 'header-case', 'status': 202, 'side_effects': 1}],
        'admin': {'actions': [
            {'as': 'admin', 'via': 'ui', 'resolver': 'save-settings', 'result_ok': True, 'state_changed': True,
             'changed_keys': ['kv:settings']},
            {'as': 'admin', 'via': 'ui', 'resolver': 'rotate-secret', 'result_ok': True, 'state_changed': True,
             'changed_keys': ['secret:ci']},
            {'as': 'nonadmin', 'via': 'resolver', 'resolver': 'save-settings', 'result_ok': False,
             'state_changed': False, 'changed_keys': []},
            {'as': 'forged', 'via': 'resolver', 'resolver': 'save-settings', 'result_ok': False,
             'state_changed': False, 'changed_keys': []}],
            'controls': {k: True for k in ('share', 'ai', 'budget', 'group', 'rotate', 'migration', 'audit', 'save')},
            'tree_text': ' | '.join(ADMIN_LABELS) + f' | Migrated {total} of {total} v1 rows | complete | ••••9f3a',
            'secret_shown': True, 'secret_source': 'panel', 'secret_leaks': []},
        'field': {'writes': [{'t_ms': 60_000, 'updates': [{'issueIds': [1], 'value': 'committed'}], 'status': 204}],
                  'values_by_checkpoint': values, 'applied_by_checkpoint': applied},
        'llm_v2': [{'case': 'tool-injection', 'llm_calls': 1, 'shown_text': 'Scope grew.', 'writes_out_of_scope': 0,
                    'per_click': [1]},
                   {'case': 'rate-limited-no-retry-after', 'llm_calls': 3, 'shown_text': 'AI explanation unavailable',
                    'writes_out_of_scope': 0, 'per_click': [3]},
                   {'case': 'no-finish-reason', 'llm_calls': 1, 'shown_text': 'AI explanation unavailable',
                    'writes_out_of_scope': 0, 'per_click': [1]},
                   {'case': 'cache', 'llm_calls': 0, 'shown_text': 'Scope grew.', 'writes_out_of_scope': 0,
                    'per_click': [1, 0]},
                   {'case': 'kill-switch', 'llm_calls': 0, 'shown_text': '', 'writes_out_of_scope': 0,
                    'per_click': [0], 'setting_applied': True},
                   {'case': 'token-budget', 'llm_calls': 0, 'shown_text': '', 'writes_out_of_scope': 0,
                    'per_click': [0], 'setting_applied': True}],
        'boot': {'widget-view': {'invokes_before_paint': 1, 'bytes_before_paint': 96_000, 'external_requests': 0},
                 'sprint-modal': {'invokes_before_paint': 1, 'bytes_before_paint': 120_000, 'external_requests': 0},
                 'admin-page': {'invokes_before_paint': 1, 'bytes_before_paint': None, 'external_requests': None}},
    }


def idle_observations(o: fo.Oracle) -> Dict:
    """The starter's surfaces with nothing done: every section present, nothing exercised."""
    obs = perfect_observations(o)
    obs['migration'].update({'v2_by_checkpoint': {cp: [] for cp in fo.CHECKPOINTS},
                             'panel_by_checkpoint': {cp: {'t_ms': 0, 'text': None, 'migrated': None, 'total': None,
                                                          'complete': False} for cp in fo.CHECKPOINTS[:-1]}})
    obs['rovo'] = {'calls': [{**c, 'result': {'changes': []}} for c in obs['rovo']['calls']]}
    obs['rate'] = {'requests': [], 'hours': [{'hour': h, 'used': 0, 'background_used': 0, 'person_used': 0}
                                             for h in range(1, 7)]}
    obs['invocations'] = []
    obs['webtrigger'] = [{'case': c['case'], 'status': 401, 'side_effects': 0} for c in obs['webtrigger']]
    obs['admin'] = {'actions': [{**a, 'result_ok': False, 'state_changed': False} for a in obs['admin']['actions']],
                    'controls': {k: False for k in obs['admin']['controls']}, 'tree_text': '', 'secret_shown': False,
                    'secret_source': None, 'secret_leaks': []}
    obs['field'] = {'writes': [], 'values_by_checkpoint': {cp: {} for cp in fo.CHECKPOINTS},
                    'applied_by_checkpoint': obs['field']['applied_by_checkpoint']}
    obs['llm_v2'] = [{**c, 'llm_calls': 0, 'per_click': [0]} for c in obs['llm_v2']]
    obs['boot'] = {s: {'invokes_before_paint': None, 'bytes_before_paint': None, 'external_requests': 0}
                   for s in BOOT_SURFACES}
    return obs


def _mutants(o: fo.Oracle) -> Dict[str, Tuple[Callable[[Dict], None], Dict[str, Optional[float]], Tuple[str, ...]]]:
    """name -> (defect injected into perfect observations, {row: expected score, None = strictly between 0 and 1},
    criticals expected to fire)."""
    v1_first = o.v1_rows()[0]
    closed = next(iter(o.closed_at))
    move = o.cross_board_moves()[0]

    def drop_v1_at_h2(obs):
        rows = obs['migration']['v2_by_checkpoint']['h2']
        rows[:] = [r for r in rows if key(r) != key(v1_first)]

    def lose_v1(obs):
        obs['migration']['v1_final'] = obs['migration']['v1_final'][1:]

    def hot_hour(obs):
        obs['rate']['hours'][2]['background_used'] = 1700

    def retry_inside_window(obs):
        obs['rate']['requests'].append(dict(obs['rate']['requests'][2], t_ms=3000))

    def duplicate_row(obs):
        rows = obs['migration']['v2_by_checkpoint']['h3']
        rows.append(dict(rows[0]))

    def purge_closed_sprint(obs):
        rows = obs['migration']['v2_by_checkpoint']['final']
        rows[:] = [r for r in rows if str(r['sprintId']) != closed]

    def old_board_estimate(obs):
        for r in obs['migration']['v2_by_checkpoint']['final']:
            if r['changeId'] == move['changeId'] and r['sprintId'] in move['toSprintIds']:
                r['estimate'] = 8.0

    def stale_after_revoke(obs):
        obs['rovo']['calls'][0]['result']['changes'].append({'issueKey': 'PAY-1'})

    def nonadmin_writes(obs):
        obs['admin']['actions'][2].update(state_changed=True, changed_keys=['kv:settings'])

    def unsigned_writes(obs):
        next(c for c in obs['webtrigger'] if c['case'] == 'bad-signature')['side_effects'] = 1

    def stale_field(obs):
        pay1 = o.issue_by_key['PAY-1']['id']
        obs['field']['values_by_checkpoint']['h3'][pay1] = 'committed'

    def truncated_shown(obs):
        next(c for c in obs['llm_v2'] if c['case'] == 'no-finish-reason')['shown_text'] = SELFTEST_PARTIAL

    def no_cache(obs):
        next(c for c in obs['llm_v2'] if c['case'] == 'cache')['llm_calls'] = 1

    def chatty_boot(obs):
        obs['boot']['widget-view']['invokes_before_paint'] = 3

    pay1_cut = round(1 - 1 / 7 * (1 / 11), 4)    # one wrong issue of 11 at one of 7 checkpoints
    return {
        'R1 v1 row late': (drop_v1_at_h2, {'r1_v1_rows_migrated': round(1 - 1 / len(o.v1_rows()), 4)}, ()),
        'R1 v1 row lost': (lose_v1, {'r1_v1_rows_intact': 0.0}, ('r1_v1_rows_intact',)),
        'R2 hot hour': (hot_hour, {'r2_background_share': round(5 / 6, 4)}, ()),
        'R2 retry inside Retry-After': (retry_inside_window, {'r2_rate_limit_reaction': round(2 / 3, 4)}, ()),
        'R3 duplicate row': (duplicate_row, {'r3_no_duplicate_rows': 0.0}, ('r3_no_duplicate_rows',)),
        # A purged ledger also loses the live and backfilled rows those rows grade at the end (priced in each).
        'R4 closed sprint purged': (purge_closed_sprint, {'r4_sprint_close_final': 0.0, 'r1_events_during_migration': None,
                                                          'r2_backfill_complete': None}, ()),
        'R4 old board estimate': (old_board_estimate, {'r4_move_new_board': 0.0}, ()),
        'R4 stale cache after revoke': (stale_after_revoke, {'r4_permission_revoked': 0.0}, ('r4_permission_revoked',)),
        'R5 non-admin write': (nonadmin_writes, {'r5_nonadmin_refused': 0.0}, ('r5_nonadmin_refused',)),
        'R6 unsigned write': (unsigned_writes, {'r6_unsigned_no_effect': 0.0}, ('r6_unsigned_no_effect',)),
        'R7 stale field': (stale_field, {'r7_values_fresh': pay1_cut}, ()),
        'R8 truncated answer shown': (truncated_shown, {'r8_failure_handling': 0.5}, ()),
        'R8 no cache': (no_cache, {'r8_cost_controls': round(2 / 3, 4)}, ()),
        'R9 chatty boot': (chatty_boot, {'r9_widget_boot': round(2 / 3, 4)}, ()),
    }


def selftest() -> List[str]:
    """Perfect observations score 1.0 on every row (0.75 weighted); an idle app scores 0 with no critical fired; each
    one-defect mutant moves exactly its row(s) by exactly the expected amount and fires exactly its critical."""
    global SITE_LLM
    failures = []
    real_site = SITE_LLM
    with tempfile.TemporaryDirectory() as tmp:
        SITE_LLM = Path(tmp) / 'llm.cjs'
        SITE_LLM.write_text(f"const PARTIAL_TEXT = '{SELFTEST_PARTIAL}';\n")
        try:
            o = fo.Oracle(fo.synthetic_pack_v2())
            perfect = evaluate(perfect_observations(o), o)
            for r in perfect:
                if r['score'] != 1.0:
                    failures.append(f'perfect: {r["check"]} {r["score"]} — {r["detail"]}')
            if abs(weighted(perfect) - 0.75) > 1e-9:
                failures.append(f'perfect: weighted {weighted(perfect)} != 0.75')
            for r in evaluate(idle_observations(o), o):
                if r['score'] > 0 or critical_fired(r):
                    failures.append(f'idle: {r["check"]} {r["score"]} fired={critical_fired(r)} — {r["detail"]}')
            for name, (inject, want, fires) in _mutants(o).items():
                obs = perfect_observations(o)
                inject(obs)
                rows = {r['check']: r for r in evaluate(obs, o)}
                for check, score in want.items():
                    got = rows[check]['score']
                    if (score is None and not 0 < got < 1) or (score is not None and got != score):
                        failures.append(f'{name}: {check} scored {got}, want {score} — {rows[check]["detail"]}')
                fired = {n for n, r in rows.items() if critical_fired(r)}
                if fired != set(fires):
                    failures.append(f'{name}: criticals fired {sorted(fired)}, want {sorted(fires)}')
                moved = [n for n, r in rows.items() if n not in want and r['score'] != 1.0]
                if moved:
                    failures.append(f'{name}: rows beyond the defect moved: {moved}')
            no_v2 = evaluate(perfect_observations(o), fo.Oracle(fo.synthetic_pack()))
            if not all(r.get('unavailable') for r in no_v2):
                failures.append('a 1.0 pack must leave every v2 row unavailable')
        finally:
            SITE_LLM = real_site
    return failures


if __name__ == '__main__':
    problems = selftest()
    for p in problems:
        print('FAIL', p)
    print(f'{len(ROWS)} v2 rows, {len(CRITICAL)} critical, weights sum {sum(WEIGHTS.values()):.4f}; '
          f'selftest {"FAILED" if problems else "ok"}')
    sys.exit(1 if problems else 0)
