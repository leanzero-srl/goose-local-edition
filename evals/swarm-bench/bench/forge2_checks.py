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

THE OBSERVATIONS CONTRACT (P9 writes these keys into forge2-observations.json beside 1.0's; P8 reads exactly them;
checkpoints are 'h1'..'h6' — virtual-hour marks after the upgrade — and 'final'):
  rate        {requests: [{t_ms, invocation, kind: 'background'|'person', method, path_tpl, cost, status, reason,
                           retry_after_s}], hours: [{hour, used, background_used, person_used}]}
  invocations [{id, function_key, module_type, virtual_ms, limit_ms, killed, result_kind}]
  migration   {v1_rows: [{changeId, sprintId, at}],
               v2_by_checkpoint: {<checkpoint>: [{changeId, sprintId, at, deleted, estimate, boardId}]}}
  world       [{t_ms, class, detail}]
  webtrigger  [{case, status, side_effects}]
  admin       {actions: [{as: 'admin'|'nonadmin'|'forged', resolver, payload, result_ok, state_changed}],
               tree_text: string, secret_leaks: [string]}
  field       {writes: [{t_ms, updates, status}], values_by_checkpoint: {<checkpoint>: {<issueKey>: string}}}
  llm_v2      [{case, llm_calls, shown_text, writes_out_of_scope}]
  boot        {<surface>: {invokes_before_paint, bytes_before_paint, external_requests}}
and the values inside them this module relies on (named in OPTIONAL_KEYS and CASES below): `rate.requests[].issue`
for a per-issue 429, `rate.background_share_pct` when the probe changed the admin share, `world[].detail` a dict
(permission_loss: {accountId, projectKey, next_response}), the webtrigger and llm_v2 `case` names, llm_v2
`model_text` on the no_finish_reason case, `admin.actions[].expect_change` false for a read, boot surfaces
'widget' / 'sprint' / 'admin'.

`python3 forge2_checks.py` runs the selftest: observations built from the oracle score 1.0 on every row, an idle app
earns 0 with no critical fired, and one injected defect per family costs exactly the rows it should.
"""
from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
import json
import re
import sys
from typing import Callable, Dict, List, Optional, Tuple

import forge2_oracle as fo

FAMILY_WEIGHT = {'R1': .12, 'R2': .13, 'R3': .08, 'R4': .10, 'R5': .10, 'R6': .08, 'R7': .05, 'R8': .04, 'R9': .05}
assert abs(sum(FAMILY_WEIGHT.values()) - 0.75) < 1e-9   # SPEC §4: v2 rows are 0.75 of the score

ADMIN_LABELS = ('Background share (%)', 'AI explanations enabled', 'Daily AI token budget', 'Comment group',
                'Rotate CI secret', 'Migration', 'Recent admin changes', 'Save settings')       # SPEC §2.6
SECRET_MASK = re.compile(r'••••\S{4}')
MIGRATED = re.compile(r'Migrated\s+([\d,]+)\s+of\s+([\d,]+)\s+v1 rows', re.I)
QUOTA_REASON = 'jira-quota-tenant-based'
PER_ISSUE_REASON = 'jira-per-issue-on-write'
WT_INVALID = ('missing_signature', 'bad_signature', 'tampered_body', 'stale_timestamp')
WT_VALID = ('valid', 'header_case')
WT_STATUS = {**{c: 401 for c in WT_INVALID}, 'replay': 200, 'valid': 202, 'header_case': 202}   # SPEC R6
LLM_TOOL = ('tool_out_of_scope',)
LLM_FAILURE = ('429_no_retry_after', 'no_finish_reason')
LLM_COST = ('cache_repeat', 'kill_switch', 'token_budget')
LLM_429_ATTEMPTS = 3                       # SPEC R8: <= 3 attempts per minute
BOOT_INVOKES = 1                           # SPEC R9
BOOT_BYTES = 150 * 1024                    # SPEC R9 "150 KB", read as KiB: the lenient side of the unit
BOOT_SURFACES = {'widget': ('dashboards:widget', True), 'sprint': ('jira:sprintAction', True),
                 'admin': ('jira:adminPage', False)}
CASES = {'webtrigger': tuple(WT_STATUS), 'llm_v2': LLM_TOOL + LLM_FAILURE + LLM_COST, 'boot': tuple(BOOT_SURFACES)}
OPTIONAL_KEYS = ('rate.requests[].issue', 'rate.background_share_pct', 'world[].detail.next_response',
                 'llm_v2[].model_text', 'admin.actions[].expect_change')


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
    worst = None
    for cp in (fo.MIGRATION_DEADLINE, 'final'):
        rows, why = ev.v2(cp)
        if why:
            return unavail(why)
        have = index(rows)
        missing = [key(e) for e in expected if key(e) not in have]
        moved = [key(e) for e in expected if key(e) in have and not any(same_at(r, e['at']) for r in have[key(e)])]
        share = (len(expected) - len(missing) - len(moved)) / len(expected)
        if worst is None or share < worst[0]:
            worst = (share, cp, missing, moved)
    share, cp, missing, moved = worst
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
    v1 = mig.get('v1_rows')
    if not isinstance(v1, list):
        return unavail('migration.v1_rows is not a list')
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
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage (the migration progress lives in the admin panel)')
    admin, why = ev.section('admin')
    if why:
        return unavail(why)
    text = admin.get('tree_text')
    if not isinstance(text, str):
        return unavail('admin.tree_text is not a string')
    total = len(ev.o.v1_rows())
    if not total:
        return vacuous('no v1 row was preloaded')
    m = MIGRATED.search(text)
    shown = (int(m.group(1).replace(',', '')), int(m.group(2).replace(',', ''))) if m else None
    parts = {'label': 'Migration' in text, 'total': bool(shown) and shown[1] == total,
             'complete': bool(shown) and shown[0] == total and bool(re.search(r'\bcomplete\b', text, re.I))}
    detail = f'admin panel shows {m.group(0)!r}' if m else f'no "Migrated <n> of {total} v1 rows" text in the admin panel'
    if not parts['complete']:
        detail += f'; `complete` with {total} of {total} not shown'
    return g(sum(parts.values()) / len(parts), detail, 'migration progress invisible to the admin',
             parts={k: int(v) for k, v in parts.items()})


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
    gate = ledger_gate(ev)
    if gate:
        return gate
    closes = [c for c in ev.o.world('sprint_close') if ev.o.changes_after_close(c['sprintId'])]
    if not closes:
        return vacuous('no sprint closed with a change touching it afterwards')
    why = world_applied(ev, 'sprint_close')
    if why:
        return unavail(why)
    rows, why = ev.v2('final')
    if why:
        return unavail(why)
    good, notes = 0, []
    for c in closes:
        sid = str(c['sprintId'])
        after = set(ev.o.changes_after_close(sid))
        mine = [r for r in rows if str(r.get('sprintId')) == sid]
        late = sorted({str(r.get('changeId')) for r in mine if str(r.get('changeId')) in after})
        if not mine:
            notes.append(f'sprint {sid}: no ledger rows at all')
        elif late:
            notes.append(f'sprint {sid}: rows recorded after its close ({sample(late)})')
        else:
            good += 1
    return g(good / len(closes), f'{good} of {len(closes)} closed sprints kept a final ledger'
             + (f'; {"; ".join(notes[:2])}' if notes else ''), 'a closed sprint keeps changing')


@row('r4_move_new_board', 'R4')
def _(ev: Ev) -> Dict:
    gate = ledger_gate(ev)
    if gate:
        return gate
    moves = ev.o.cross_board_moves()
    if not moves:
        return vacuous("no issue moved into another board's sprint")
    why = world_applied(ev, 'issue_move')
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
    switches = ev.o.world('estimation_field_change')
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
    why = world_applied(ev, 'estimation_field_change')
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
    why = world_applied(ev, 'issue_delete')
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


@row('r4_permission_revoked', 'R4', critical="data leak — a hidden issue's data shown to a person who cannot browse it")
def _(ev: Ev) -> Dict:
    losses = ev.o.world('permission_loss')
    if not losses:
        return vacuous('no person lost browse permission')
    world, why = ev.section('world', list)
    if why:
        return unavail(why)
    records = [w for w in world if isinstance(w, dict) and w.get('class') == 'permission_loss']
    leaks, graded, idle = [], 0, 0
    for loss in losses:
        account, project = str(loss['accountId']), loss['projectKey']
        rec = next((w for w in records if isinstance(w.get('detail'), dict)
                    and str(w['detail'].get('accountId')) == account and w['detail'].get('projectKey') == project), None)
        if rec is None or rec['detail'].get('next_response') is None:
            return unavail(f'the probe recorded no request by {account} after losing {project} '
                           '(world[].detail.next_response)')
        text = rec['detail']['next_response']
        text = text if isinstance(text, str) else json.dumps(text, default=str)
        before = ev.o.hidden_issue_ids(account, loss['t'] - fo.JUST_BEFORE)
        after = ev.o.hidden_issue_ids(account, loss['t'])
        newly = after - before
        visible = {ev.o.issues[i]['key'] for i in ev.o.issues if i not in after}
        if not any(re.search(r'(?<![A-Za-z0-9])' + re.escape(k) + r'(?![0-9])', text) for k in visible):
            idle += 1
            continue
        graded += 1
        terms = ev.o.leak_terms_for(newly)
        hits = [k for k in terms['keys'] if re.search(r'(?<![A-Za-z0-9])' + re.escape(k) + r'(?![0-9])', text)]
        hits += [s for s in terms['summaries'] if s in text]
        if hits:
            leaks.append(f'{account} after losing {project}: {sample(hits)}')
    if not graded:
        return vacuous(f'the request after the permission loss showed no ledger data at all ({idle} request(s))')
    if leaks:
        return g(0.0, f'stale data after a permission loss: {leaks[0]}',
                 "data leak — a hidden issue's data shown to a person who cannot browse it")
    return g(1.0, f'{graded} request(s) after a permission loss showed none of the newly hidden issues')


# ── R5 the UI Kit admin panel ────────────────────────────────────────────────────────────────

def admin_actions(ev: Ev, *who: str) -> Tuple[Optional[List[Dict]], Optional[str]]:
    admin, why = ev.section('admin')
    if why:
        return None, why
    actions = admin.get('actions')
    if not isinstance(actions, list):
        return None, 'admin.actions is not a list'
    return [a for a in actions if isinstance(a, dict) and a.get('as') in who], None


def applied(a: Dict) -> bool:
    return a.get('result_ok') is True and (a.get('state_changed') is True or a.get('expect_change') is False)


@row('r5_panel_labels', 'R5')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage')
    admin, why = ev.section('admin')
    if why:
        return unavail(why)
    text = admin.get('tree_text')
    if not isinstance(text, str):
        return unavail('admin.tree_text is not a string')
    missing = [label for label in ADMIN_LABELS if label not in text]
    masked = bool(SECRET_MASK.search(text))
    return g((len(ADMIN_LABELS) - len(missing) + masked) / (len(ADMIN_LABELS) + 1),
             f'{len(ADMIN_LABELS) - len(missing)} of {len(ADMIN_LABELS)} §2.6 labels; secret masked as ••••<last4>: '
             f'{"yes" if masked else "no"}' + (f'; missing {sample(missing, 4)}' if missing else ''),
             'the admin cannot find a control')


@row('r5_admin_actions_apply', 'R5')
def _(ev: Ev) -> Dict:
    if not ev.has_module('jira:adminPage'):
        return absent('jira:adminPage')
    acts, why = admin_actions(ev, 'admin')
    if why:
        return unavail(why)
    if not acts:
        return vacuous('no admin action was made as an admin')
    bad = [str(a.get('resolver')) for a in acts if not applied(a)]
    return g((len(acts) - len(bad)) / len(acts), f'{len(acts) - len(bad)} of {len(acts)} admin actions by an admin '
             'applied' + (f'; failed: {sample(bad)}' if bad else ''), 'the admin panel does not work for admins')


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
    if not any(applied(a) and a.get('state_changed') is True for a in works or []):
        return vacuous('no admin action changed state even for an admin, so a refusal proves nothing')
    breaches = [f'{a.get("as")}: {a.get("resolver")}' for a in acts if a.get('state_changed')]
    if breaches:
        return g(0.0, f'state changed by {sample(breaches)}', 'an admin action by a non-admin succeeded')
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
    return g(1.0, f'{len(invalid)} invalid events ({sample(sorted({c["case"] for c in invalid}), 4)}) had no effect')


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
    if not SECRET_MASK.search(str(admin.get('tree_text') or '')):
        return vacuous('no CI secret was rotated (the panel shows no ••••<last4>)')
    if leaks:
        return g(0.0, f'the full CI secret appeared in {sample(leaks)}', 'the CI secret disclosed')
    return g(1.0, 'the rotated CI secret never appeared outside its one-time display')


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
    if not ev.has_module('jira:customField', 'scope-status'):
        return absent('jira:customField scope-status')
    field, why = ev.section('field')
    if why:
        return unavail(why)
    by = field.get('values_by_checkpoint')
    if not isinstance(by, dict):
        return unavail('field.values_by_checkpoint is not an object')
    cps = [cp for cp in fo.CHECKPOINTS if isinstance(by.get(cp), dict)]
    if not cps:
        return unavail('field.values_by_checkpoint holds no checkpoint')
    shares, worst = [], None
    for cp in cps:
        values, ungraded = ev.o.field_values(cp)
        observed = by[cp]
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

def llm_cases(ev: Ev, names: Tuple[str, ...]) -> Tuple[Optional[List[Dict]], Optional[str], bool]:
    cases, why = ev.section('llm_v2', list)
    if why:
        return None, why, False
    cases = [c for c in cases if isinstance(c, dict)]
    calls_model = any((count(c.get('llm_calls')) or 0) >= 1 for c in cases)
    return [c for c in cases if c.get('case') in names], None, calls_model


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
    cases = [c for c in cases if (count(c.get('llm_calls')) or 0) >= 1]
    if not cases:
        return vacuous('the model was never asked in the manipulated-tool-call case')
    bad = [c for c in cases if (count(c.get('writes_out_of_scope')) or 0) > 0]
    return g((len(cases) - len(bad)) / len(cases), f'{len(cases) - len(bad)} of {len(cases)} manipulated tool calls '
             'validated against the viewer\'s sprint scope before acting'
             + (f'; acted out of scope {count(bad[0].get("writes_out_of_scope"))} time(s)' if bad else ''),
             'a prompt-injected tool call writes outside the viewer\'s scope')


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
        if c['case'] == '429_no_retry_after':
            good = (count(c.get('llm_calls')) or 0) <= LLM_429_ATTEMPTS
            note = f'429 without Retry-After: {c.get("llm_calls")} attempts (<= {LLM_429_ATTEMPTS})'
        else:
            model = c.get('model_text')
            if not isinstance(model, str) or not model.strip():
                return unavail('llm_v2 no_finish_reason case carries no model_text')
            good = model.strip() not in str(c.get('shown_text') or '')
            note = 'a response without finish_reason was ' + ('not shown' if good else 'SHOWN')
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
    bad = [f'{c["case"]} ({c.get("llm_calls")} calls)' for c in cases if (count(c.get('llm_calls')) or 0) > 0]
    return g((len(cases) - len(bad)) / len(cases), f'{len(cases) - len(bad)} of {len(cases)} cache/kill-switch/budget '
             'cases made no model call' + (f'; called anyway: {sample(bad)}' if bad else ''),
             'AI spend the admin switched off or capped')


# ── R9 the Custom UI boot budget ─────────────────────────────────────────────────────────────

def boot_row(ev: Ev, surface: str) -> Dict:
    mtype, bytes_graded = BOOT_SURFACES[surface]
    if not ev.has_module(mtype):
        return absent(mtype)
    boot, why = ev.section('boot')
    if why:
        return unavail(why)
    rec = boot.get(surface)
    if not isinstance(rec, dict):
        return unavail(f'boot.{surface} not recorded')
    invokes, external = count(rec.get('invokes_before_paint')), count(rec.get('external_requests'))
    size = count(rec.get('bytes_before_paint')) if bytes_graded else 0
    if invokes is None or external is None or size is None:
        return unavail(f'boot.{surface} lacks a count')
    parts = {'invokes': int(invokes <= BOOT_INVOKES), 'external': int(external == 0)}
    if bytes_graded:
        parts['bytes'] = int(size <= BOOT_BYTES)
    return g(sum(parts.values()) / len(parts),
             f'{surface}: {invokes} invoke(s) before first paint (<= {BOOT_INVOKES})'
             + (f', {size} bytes of JS+CSS (<= {BOOT_BYTES})' if bytes_graded else '')
             + f', {external} external request(s)', 'a slow or leaky first paint', parts=parts)


@row('r9_widget_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'widget')


@row('r9_sprint_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'sprint')


@row('r9_admin_boot', 'R9')
def _(ev: Ev) -> Dict:
    return boot_row(ev, 'admin')


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

def perfect_observations(o: fo.Oracle) -> Dict:
    """What an app that meets every R1-R9 guarantee shows the probe on this pack, built from the oracle."""
    def v2(cp):
        return [{'changeId': e['changeId'], 'sprintId': e['sprintId'], 'at': float(e['at']), 'deleted': e['deleted'],
                 'estimate': float(min(e['estimates'])), 'boardId': e['boardId']} for e in o.expected_rows(cp)]
    values = {}
    for cp in fo.CHECKPOINTS:
        accepted, ungraded = o.field_values(cp)
        values[cp] = {k: min(v) for k, v in accepted.items() if v != frozenset({''}) and k not in ungraded}
    world = []
    for w in o.world():
        detail = {k: v for k, v in w.items() if k not in ('t', 'class')}
        if w['class'] == 'permission_loss':
            hidden = o.hidden_issue_ids(w['accountId'], w['t'])
            visible = sorted(o.issues[i]['key'] for i in o.issues if i not in hidden)
            detail['next_response'] = {'changes': [{'issueKey': k} for k in visible[:3]]}
        world.append({'t_ms': int((w['t'] - o.upgrade_at).total_seconds() * 1000), 'class': w['class'],
                      'detail': detail})
    total = len(o.v1_rows())
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
                      'v2_by_checkpoint': {cp: v2(cp) for cp in fo.CHECKPOINTS}},
        'world': world,
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
             'limit_ms': 900000, 'killed': False, 'result_kind': 'returned'},
            {'id': 'inv-s2', 'function_key': 'reconcile', 'module_type': 'scheduledTrigger', 'virtual_ms': 3000,
             'limit_ms': 900000, 'killed': False, 'result_kind': 'returned'},
            {'id': 'inv-r1', 'function_key': 'ui-resolver', 'module_type': 'resolver', 'virtual_ms': 300,
             'limit_ms': 25000, 'killed': False, 'result_kind': 'ok'}],
        'webtrigger': [{'case': 'valid', 'status': 202, 'side_effects': 2},
                       {'case': 'header_case', 'status': 202, 'side_effects': 1},
                       *({'case': c, 'status': 401, 'side_effects': 0} for c in WT_INVALID),
                       {'case': 'replay', 'status': 200, 'side_effects': 0}],
        'admin': {'actions': [
            {'as': 'admin', 'resolver': 'save-settings', 'payload': {}, 'result_ok': True, 'state_changed': True},
            {'as': 'admin', 'resolver': 'rotate-secret', 'payload': {}, 'result_ok': True, 'state_changed': True},
            {'as': 'nonadmin', 'resolver': 'save-settings', 'payload': {}, 'result_ok': False, 'state_changed': False},
            {'as': 'forged', 'resolver': 'save-settings', 'payload': {'accountId': 'u-ana'}, 'result_ok': False,
             'state_changed': False}],
            'tree_text': ' | '.join(ADMIN_LABELS) + f' | Migrated {total} of {total} v1 rows | complete | ••••9f3a',
            'secret_leaks': []},
        'field': {'writes': [{'t_ms': 60_000, 'updates': 11, 'status': 204}], 'values_by_checkpoint': values},
        'llm_v2': [{'case': 'tool_out_of_scope', 'llm_calls': 1, 'shown_text': 'Scope grew.', 'writes_out_of_scope': 0},
                   {'case': '429_no_retry_after', 'llm_calls': 3, 'shown_text': 'AI explanation unavailable',
                    'writes_out_of_scope': 0},
                   {'case': 'no_finish_reason', 'llm_calls': 1, 'shown_text': 'AI explanation unavailable',
                    'writes_out_of_scope': 0, 'model_text': 'Scope grew after the sprint'},
                   *({'case': c, 'llm_calls': 0, 'shown_text': '', 'writes_out_of_scope': 0} for c in LLM_COST)],
        'boot': {'widget': {'invokes_before_paint': 1, 'bytes_before_paint': 96_000, 'external_requests': 0},
                 'sprint': {'invokes_before_paint': 1, 'bytes_before_paint': 120_000, 'external_requests': []},
                 'admin': {'invokes_before_paint': 1, 'bytes_before_paint': 0, 'external_requests': 0}},
    }


def idle_observations(o: fo.Oracle) -> Dict:
    """The starter's surfaces with nothing done: every section present, nothing exercised."""
    obs = perfect_observations(o)
    obs['migration'] = {'v1_rows': obs['migration']['v1_rows'], 'v2_by_checkpoint': {cp: [] for cp in fo.CHECKPOINTS}}
    obs['rate'] = {'requests': [], 'hours': [{'hour': h, 'used': 0, 'background_used': 0, 'person_used': 0}
                                             for h in range(1, 7)]}
    obs['invocations'] = []
    obs['webtrigger'] = [{'case': c['case'], 'status': 401, 'side_effects': 0} for c in obs['webtrigger']]
    obs['admin'] = {'actions': [{**a, 'result_ok': False, 'state_changed': False} for a in obs['admin']['actions']],
                    'tree_text': '', 'secret_leaks': []}
    obs['field'] = {'writes': [], 'values_by_checkpoint': {cp: {} for cp in fo.CHECKPOINTS}}
    obs['llm_v2'] = [{**c, 'llm_calls': 0} for c in obs['llm_v2']]
    for w in obs['world']:
        if w['class'] == 'permission_loss':
            w['detail']['next_response'] = {'changes': []}
    obs['boot'] = {}
    return obs


def _mutants(o: fo.Oracle) -> Dict[str, Tuple[Callable[[Dict], None], Dict[str, float], Tuple[str, ...]]]:
    """name -> (defect injected into perfect observations, {row: expected score}, criticals expected to fire)."""
    v1_first = o.v1_rows()[0]
    closed = next(iter(o.closed_at))
    late = o.changes_after_close(closed)[0]
    move = o.cross_board_moves()[0]

    def drop_v1_at_h2(obs):
        rows = obs['migration']['v2_by_checkpoint']['h2']
        rows[:] = [r for r in rows if key(r) != key(v1_first)]

    def lose_v1(obs):
        obs['migration']['v1_rows'] = obs['migration']['v1_rows'][1:]

    def hot_hour(obs):
        obs['rate']['hours'][2]['background_used'] = 1700

    def duplicate_row(obs):
        rows = obs['migration']['v2_by_checkpoint']['h3']
        rows.append(dict(rows[0]))

    def record_after_close(obs):
        obs['migration']['v2_by_checkpoint']['final'].append(
            {'changeId': late, 'sprintId': closed, 'at': 0.0, 'deleted': False, 'estimate': 0.0, 'boardId': '1'})

    def old_board_estimate(obs):
        for r in obs['migration']['v2_by_checkpoint']['final']:
            if r['changeId'] == move['changeId'] and r['sprintId'] in move['toSprintIds']:
                r['estimate'] = 8.0

    def nonadmin_writes(obs):
        obs['admin']['actions'][2]['state_changed'] = True

    def unsigned_writes(obs):
        next(c for c in obs['webtrigger'] if c['case'] == 'bad_signature')['side_effects'] = 1

    def stale_field(obs):
        values = obs['field']['values_by_checkpoint']['h3']
        values['PAY-1'] = 'committed'

    def no_cache(obs):
        next(c for c in obs['llm_v2'] if c['case'] == 'cache_repeat')['llm_calls'] = 1

    def chatty_boot(obs):
        obs['boot']['widget']['invokes_before_paint'] = 3

    third = round(1 - 1 / 3, 4)
    return {
        'R1 v1 row late': (drop_v1_at_h2, {'r1_v1_rows_migrated': round(1 - 1 / len(o.v1_rows()), 4)}, ()),
        'R1 v1 row lost': (lose_v1, {'r1_v1_rows_intact': 0.0}, ('r1_v1_rows_intact',)),
        'R2 hot hour': (hot_hour, {'r2_background_share': round(5 / 6, 4)}, ()),
        'R3 duplicate row': (duplicate_row, {'r3_no_duplicate_rows': 0.0}, ('r3_no_duplicate_rows',)),
        'R4 row after close': (record_after_close, {'r4_sprint_close_final': 0.0}, ()),
        'R4 old board estimate': (old_board_estimate, {'r4_move_new_board': 0.0}, ()),
        'R5 non-admin write': (nonadmin_writes, {'r5_nonadmin_refused': 0.0}, ('r5_nonadmin_refused',)),
        'R6 unsigned write': (unsigned_writes, {'r6_unsigned_no_effect': 0.0}, ('r6_unsigned_no_effect',)),
        'R7 stale field': (stale_field, {'r7_values_fresh': None}, ()),
        'R8 no cache': (no_cache, {'r8_cost_controls': third}, ()),
        'R9 chatty boot': (chatty_boot, {'r9_widget_boot': round(2 / 3, 4)}, ()),
    }


def selftest() -> List[str]:
    failures = []
    o = fo.Oracle(fo.synthetic_pack_v2(), include_live_ui=True)
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
    return failures


if __name__ == '__main__':
    problems = selftest()
    for p in problems:
        print('FAIL', p)
    oracle = fo.Oracle(fo.synthetic_pack_v2(), include_live_ui=True)
    rows = evaluate(perfect_observations(oracle), oracle)
    print(f'{len(rows)} v2 rows, {len(CRITICAL)} critical, weights sum {sum(WEIGHTS.values()):.4f}; '
          f'selftest {"FAILED" if problems else "ok"} ({len(_mutants(oracle))} mutants)')
    sys.exit(1 if problems else 0)
