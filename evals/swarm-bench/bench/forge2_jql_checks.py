"""Forge 2.0 R10: functional custom JQL and shared precomputation lifecycle.

The probe records actual native-search results, never expected IDs. Oracle.jql_issue_ids supplies
independent truth from the site's history/world at each case's instant.

obs.jql.cases: [{kind, sprintId, operator, accountId, t_ms, applied, precomputationId,
                invoked, issue_ids, expanded_jql, error, search_ok}]
Kinds: initial, cache, shared-user, refresh, empty, future, closed, invalid.
"""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Callable, Dict, List, Optional

WEIGHT = .10
GROUPS = {
    'r10_initial_membership': ('initial', 'cache'),
    'r10_precomputation_refresh': ('refresh',),
    'r10_shared_permissions': ('shared-user',),
    'r10_empty_and_closed': ('empty', 'future', 'closed'),
    'r10_invalid_arguments': ('invalid',),
}
CHECKS = ('r10_manifest', *GROUPS)


def result(score, detail, **extra):
    return {'score': round(score, 4), 'detail': detail[:400], 'consequence': '', **extra}


def unavailable(detail):
    return result(0, f'UNAVAILABLE: {detail}', unavailable=True)


def manifest_row(obs):
    manifest = obs.get('manifest') or {}
    modules = manifest.get('modules') or {}
    candidates = [m for m in modules.get('jira:jqlFunction', []) if m.get('name') == 'scopeChanged']
    if not candidates:
        return result(0, 'scopeChanged jira:jqlFunction absent', parts={'absent_surface': 'jira:jqlFunction'})
    functions = {f.get('key'): f.get('handler') for f in modules.get('function', [])}
    good = any(set(m.get('types', [])) == {'issue'} and set(m.get('operators', [])) == {'in', 'not in'}
               and m.get('arguments') == [{'name': 'sprintId', 'required': True}]
               and isinstance(functions.get(m.get('function')), str) for m in candidates)
    return result(int(good), 'scopeChanged manifest declares issue IN/NOT IN, required sprintId, and bound function' if good
                  else 'scopeChanged manifest declaration does not match the contract')


def clause_key(case):
    return (str(case.get('sprintId')), case.get('operator', 'in'))


def check_group(name, cases, oracle):
    kinds = GROUPS[name]
    selected = [c for c in cases if c.get('kind') in kinds]
    absent = set(kinds) - {c.get('kind') for c in selected}
    if absent:
        return unavailable(f'jql probe omitted cases: {sorted(absent)}')
    initial = {clause_key(c): c for c in cases if c.get('kind') == 'initial'}
    good = 0
    notes = []
    leaks = []
    for case in selected:
        kind = case['kind']
        if name == 'r10_invalid_arguments':
            passed = isinstance(case.get('error'), str) and bool(case['error'].strip()) and not case.get('issue_ids')
        else:
            if not isinstance(case.get('issue_ids'), list) or 'search_ok' not in case:
                return unavailable(f'{kind}: no native-search result recorded')
            expected = set(map(str, oracle.jql_issue_ids(str(case['sprintId']), case['accountId'], case['t_ms'],
                                                       case.get('applied'), case.get('operator', 'in'))))
            actual = list(map(str, case['issue_ids']))
            if name == 'r10_shared_permissions':
                mark = datetime.fromtimestamp(case['t_ms'] / 1000, timezone.utc)
                hidden = oracle.hidden_issue_ids(case['accountId'], mark)
                leaks.extend(sorted(set(actual) & set(map(str, hidden))))
            passed = case.get('search_ok') is True and not case.get('error') and set(actual) == expected and len(actual) == len(set(actual))
            if kind == 'initial':
                passed = passed and case.get('invoked') == 1 and bool(case.get('precomputationId'))
            elif kind in ('cache', 'refresh', 'shared-user'):
                first = initial.get(clause_key(case))
                passed = passed and first is not None and first.get('invoked') == 1 and case.get('invoked') == 0 \
                    and bool(case.get('precomputationId')) and case['precomputationId'] == first.get('precomputationId')
            if not passed:
                missing = sorted(expected - set(actual))[:3]
                extra = sorted(set(actual) - expected)[:3]
                notes.append(f'{kind}: missing={missing}, extra={extra}, invoked={case.get("invoked")}')
        if passed:
            good += 1
        elif name == 'r10_invalid_arguments':
            notes.append(f'{kind}: no explicit refusal or query returned issues')
    graded = result(good / len(selected), f'{good}/{len(selected)} {name.removeprefix("r10_")} cases correct'
                    + ('; ' + '; '.join(notes[:2]) if notes else ''))
    if name == 'r10_shared_permissions':
        graded['critical'] = 'hidden issue data disclosed by a shared JQL precomputation' if leaks else None
        if leaks:
            graded['parts'] = {'observed_hidden_issue_ids': sorted(set(leaks))}
    return graded


def evaluate(obs: Dict, oracle) -> List[Dict]:
    gate = manifest_row(obs)
    section = obs.get('jql')
    cases = section.get('cases') if isinstance(section, dict) else None
    error = (obs.get('sectionErrors') or {}).get('jql')
    out = []
    for name in CHECKS:
        base = {'check': name, 'tier': 'R10', 'weight': round(WEIGHT / len(CHECKS), 6), 'critical': None}
        if name == 'r10_manifest':
            graded = gate
        elif gate['score'] == 0:
            graded = result(0, gate['detail'], **({'parts': gate['parts']} if 'parts' in gate else {}))
        elif error or not isinstance(cases, list):
            graded = unavailable(error or 'jql.cases missing')
        elif not callable(getattr(oracle, 'jql_issue_ids', None)):
            graded = unavailable('oracle.jql_issue_ids is missing')
        else:
            try:
                graded = check_group(name, cases, oracle)
            except Exception as exc:
                graded = unavailable(f'jql scorer error: {type(exc).__name__}: {exc}')
        out.append({**base, **graded})
    return out


@dataclass(frozen=True)
class Row:
    name: str
    family: str
    critical: Optional[str]
    fn: Callable


def _reader(name):
    def read(ev):
        return next(row for row in evaluate(ev.obs, ev.o) if row['check'] == name)
    return read


ROWS = [Row(name, 'R10', 'hidden issue data disclosed by a shared JQL precomputation'
            if name == 'r10_shared_permissions' else None, _reader(name)) for name in CHECKS]
