"""forge-1.0 oracle: every number the Scope Ledger scorer grades, recomputed from the run's pack.

The pack is interface I1 (forge/DESIGN.md §5.2): `node forge/site/fixtures.cjs --seed S --out pack.json`.
Nothing here is hand-written expectation: the scorer feeds the pack of the run's own `fixture_seed`.

Pack semantics this oracle relies on (WP1 builds the generator to them; a pack that breaks one is a harness
defect the oracle names, never an app zero):
  * `issues[].fields` is the site AT INSTALL: every `history` entry applied, no `live` entry applied.
  * The site applies every `live` entry when its slot comes up, whether or not its delivery is `dropped`
    (a dropped change happened in Jira; only its product event never arrives).
  * Sprint changelog `from`/`to` are comma-separated sprint id lists (`"12, 15"`), possibly empty.
  * An estimate item names the field by `fieldId`; its `to` is the new value as a string, or empty/null.
  * The live-UI slot (DESIGN §5.2, what forge_probe.mjs's live step needs): exactly two `live` entries carry
    `delivery: {liveUi: true, slot: null, duplicates: [], dropped: false}`. They are the two LATEST-created live
    entries (the site applies live changes in creation order, so nothing may follow them), each a relevant change
    (a Sprint-field or estimation-field item) that moves the numbers of at least one ACTIVE sprint of the FIRST scrum
    board in `pack.boards` order (the probe opens the live step on that board's widget). They are left out of the
    site's delivery plan (`deliverNext`) and out of `flushLive`; the probe delivers them in the UI phase through
    `emu.deliverProductEvent(change)`, which must apply exactly that change. Before the slot, KVS snapshots and the
    Rovo action grade against `Oracle(pack)`; every surface opened after it against `Oracle(pack, True)`.

The rules are FORGE-CONTRACT.md §1 verbatim:
  * a change is a Sprint-field changelog entry created strictly after the sprint's `startDate` that puts the
    issue into S (`added`) or takes it out of S (`removed`), keyed by changelog id + sprint;
  * committed / added / removed sum CURRENT estimates (the sprint's board estimation field; no value is 0);
  * creep = 100 x added / committed, rounded half away from zero to one decimal (Decimal ROUND_HALF_UP — the
    values are non-negative, so half-up IS half-away-from-zero), `None` when committed is 0.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from decimal import Decimal, ROUND_HALF_UP, InvalidOperation
import math
import re
from typing import Dict, Iterable, List, Optional, Set, Tuple

ONE_DECIMAL = Decimal('0.1')
PHASES = ('backfill', 'live', 'final')


def duplicate_count(delivery: Dict) -> int:
    """I1 `delivery.duplicates` is the list of extra slots the change is redelivered at (WP1's generator); an
    integer count is accepted too."""
    dup = (delivery or {}).get('duplicates')
    if isinstance(dup, list):
        return len(dup)
    return int(dup) if isinstance(dup, int) and not isinstance(dup, bool) else 0


class PackDefect(ValueError):
    """The pack breaks an I1 invariant the oracle needs: a harness defect, never app evidence."""


def instant(value) -> Optional[datetime]:
    """An ISO-8601 instant (offset required) or epoch milliseconds, as an aware UTC datetime."""
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, (int, float)):
        if not math.isfinite(value):
            return None
        return datetime.fromtimestamp(value / 1000.0, tz=timezone.utc)
    if not isinstance(value, str) or not value.strip():
        return None
    text = value.strip()
    if text.endswith('Z'):
        text = text[:-1] + '+00:00'
    # Jira writes offsets without the colon (`+0000`); fromisoformat accepts both on 3.11+.
    m = re.fullmatch(r'(.*[T ]\d\d:\d\d(?::\d\d(?:\.\d+)?)?)([+-]\d\d)(\d\d)', text)
    if m:
        text = f'{m.group(1)}{m.group(2)}:{m.group(3)}'
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.astimezone(timezone.utc)


def sprint_ids(value) -> Set[str]:
    """Sprint membership in any of the shapes Jira uses: `"12, 15"`, `[12, 15]`, `[{id: 12}, …]`."""
    if value is None:
        return set()
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return {str(int(value))}
    if isinstance(value, str):
        return {part.strip() for part in value.split(',') if part.strip()}
    if isinstance(value, dict):
        return {str(value['id'])} if value.get('id') is not None else set()
    if isinstance(value, (list, tuple)):
        out: Set[str] = set()
        for item in value:
            out |= sprint_ids(item)
        return out
    return set()


def estimate(value) -> Decimal:
    if value is None or isinstance(value, bool):
        return Decimal(0)
    if isinstance(value, (int, float)):
        return Decimal(str(value))
    if isinstance(value, str):
        if not value.strip():
            return Decimal(0)
        try:
            return Decimal(value.strip())
        except InvalidOperation:
            raise PackDefect(f'estimate value {value!r} is not a number') from None
    raise PackDefect(f'estimate value {value!r} has an unknown shape')


def creep(added: Decimal, committed: Decimal) -> Optional[Decimal]:
    if committed == 0:
        return None
    return (Decimal(100) * added / committed).quantize(ONE_DECIMAL, rounding=ROUND_HALF_UP)


def format_points(value: Decimal) -> str:
    text = format(value.normalize(), 'f')
    return '0' if text in ('-0', '0') else text


def format_creep(value: Optional[Decimal]) -> str:
    return '—' if value is None else f'{value.quantize(ONE_DECIMAL, rounding=ROUND_HALF_UP)}%'


def changelog_order(change_id: str):
    return (0, int(change_id), '') if str(change_id).isdigit() else (1, 0, str(change_id))


@dataclass(frozen=True)
class Change:
    change_id: str
    sprint_id: str
    issue_id: str
    issue_key: str
    kind: str
    at: datetime
    at_text: str
    by: str
    by_name: str
    points: Decimal
    phase: str          # 'history' | 'live'
    dropped: bool
    duplicates: int
    sources: Tuple[str, ...]   # sources the contract accepts for this row (P7)

    @property
    def key(self) -> Tuple[str, str]:
        return (self.change_id, self.sprint_id)

    def sort_key(self):
        return (self.at, changelog_order(self.change_id))


@dataclass
class SprintNumbers:
    sprint_id: str
    name: str
    board_id: str
    start: datetime
    committed: Decimal
    added: Decimal
    removed: Decimal
    creep: Optional[Decimal]
    changes: List[Change] = field(default_factory=list)

    def metrics_text(self) -> Dict[str, str]:
        return {'committed': format_points(self.committed), 'added': format_points(self.added),
                'removed': format_points(self.removed), 'creep': format_creep(self.creep)}


class Oracle:
    """All graded expectations for one pack. Construction validates the pack's I1 invariants."""

    def __init__(self, pack: Dict, include_live_ui: bool = False):
        """`include_live_ui`: the state after the live-UI slot (DESIGN §5.2: two live changes held back from the
        script and delivered while the widget is open, `delivery.liveUi: true`). Without it the oracle is the site
        before the UI phase: what the KVS snapshots, the Rovo action and the widget views opened first must show."""
        self.pack = pack
        try:
            self.viewer = str(pack['viewer'])
            self.sprint_field = str(pack['sprintFieldId'])
            self.users = {str(u['accountId']): u.get('displayName') or '' for u in pack['users']}
            self.boards = {str(b['id']): b for b in pack['boards']}
            self.sprints = {str(s['id']): s for s in pack['sprints']}
            self.issues = {str(i['id']): i for i in pack['issues']}
            history = list(pack['history'])
            live = list(pack['live'])
        except (KeyError, TypeError) as error:
            raise PackDefect(f'pack lacks an I1 field: {error}') from None
        self.issue_by_key = {i['key']: i for i in self.issues.values()}
        self.history = history
        self.live_ui = [e for e in live if (e.get('delivery') or {}).get('liveUi')]
        self.live = live if include_live_ui else [e for e in live if not (e.get('delivery') or {}).get('liveUi')]
        self.include_live_ui = include_live_ui
        for entry in history + live:
            if str(entry.get('issueId')) not in self.issues:
                raise PackDefect(f"changelog {entry.get('changelogId')} names unknown issue {entry.get('issueId')}")
            if instant(entry.get('created')) is None:
                raise PackDefect(f"changelog {entry.get('changelogId')} has no ISO-8601 created instant")
        cids = {str(e.get('changelogId')) for e in history + live}
        if len(cids) != len(history) + len(live):
            raise PackDefect('changelog ids repeat across history and live')
        clash = cids & (set(self.issues) | set(self.sprints) | set(self.boards))
        if clash:
            # Ledger rows are matched by exact leaves; an id shared across classes would make them ambiguous.
            raise PackDefect(f'changelog ids collide with issue/sprint/board ids: {sorted(clash)[:5]}')
        for sid, sprint in self.sprints.items():
            if sprint.get('state') == 'active' and instant(sprint.get('startDate')) is None:
                raise PackDefect(f'active sprint {sid} has no startDate instant')
            if str(sprint.get('originBoardId')) not in self.boards:
                raise PackDefect(f'sprint {sid} names unknown board {sprint.get("originBoardId")}')
        self._numbers = self._compute()

    # ── membership and estimates ───────────────────────────────────────────────────────────

    def active_sprints(self) -> List[str]:
        active = [sid for sid, s in self.sprints.items() if s.get('state') == 'active']
        return sorted(active, key=lambda sid: (instant(self.sprints[sid]['startDate']), changelog_order(sid)))

    def future_sprints(self) -> List[str]:
        return sorted(sid for sid, s in self.sprints.items() if s.get('state') == 'future')

    def board_estimate_field(self, sprint_id: str) -> str:
        board = self.boards[str(self.sprints[sprint_id]['originBoardId'])]
        fid = board.get('estimationFieldId')
        if not fid:
            raise PackDefect(f"scrum board {board.get('id')} has no estimationFieldId")
        return str(fid)

    def _entries(self, live_mode: str) -> List[Dict]:
        """history, plus live entries: 'none' | 'delivered' (not dropped) | 'all'."""
        chosen = list(self.history)
        if live_mode == 'delivered':
            chosen += [e for e in self.live if not (e.get('delivery') or {}).get('dropped')]
        elif live_mode == 'all':
            chosen += list(self.live)
        return sorted(chosen, key=lambda e: (instant(e['created']), changelog_order(str(e['changelogId']))))

    def _sprint_items(self, entry: Dict) -> List[Dict]:
        return [item for item in entry.get('items') or []
                if str(item.get('fieldId') or '') == self.sprint_field
                or (not item.get('fieldId') and item.get('field') == 'Sprint')]

    def _membership_series(self, issue_id: str, entries: List[Dict]) -> List[Tuple[datetime, Set[str], Set[str]]]:
        out = []
        for entry in entries:
            if str(entry['issueId']) != issue_id:
                continue
            for item in self._sprint_items(entry):
                out.append((instant(entry['created']), sprint_ids(item.get('from')), sprint_ids(item.get('to'))))
        return out

    def member_at(self, issue_id: str, when: datetime, entries: List[Dict]) -> Set[str]:
        series = self._membership_series(issue_id, entries)
        before = [to for t, _frm, to in series if t <= when]
        if before:
            return before[-1]
        if series:
            return series[0][1]
        return sprint_ids(self.issues[issue_id].get('fields', {}).get(self.sprint_field))

    def member_now(self, issue_id: str, entries: List[Dict]) -> Set[str]:
        series = self._membership_series(issue_id, entries)
        if series:
            return series[-1][2]
        return sprint_ids(self.issues[issue_id].get('fields', {}).get(self.sprint_field))

    def estimate_of(self, issue_id: str, field_id: str, entries: List[Dict]) -> Decimal:
        value = self.issues[issue_id].get('fields', {}).get(field_id)
        install_value = value
        live_items = [(instant(e['created']), item) for e in entries if e in self.live and str(e['issueId']) == issue_id
                      for item in e.get('items') or [] if str(item.get('fieldId') or '') == field_id]
        for _t, item in sorted(live_items, key=lambda pair: pair[0]):
            value = item.get('to')
        return estimate(value if live_items else install_value)

    # ── changes and numbers ───────────────────────────────────────────────────────────────

    def _changes(self, live_mode: str, final_entries: List[Dict]) -> Dict[str, List[Change]]:
        entries = self._entries(live_mode)
        live_ids = {str(e['changelogId']) for e in self.live}
        by_sprint: Dict[str, List[Change]] = {sid: [] for sid in self.active_sprints()}
        for entry in entries:
            created = instant(entry['created'])
            cid = str(entry['changelogId'])
            issue = self.issues[str(entry['issueId'])]
            delivery = entry.get('delivery') or {}
            for item in self._sprint_items(entry):
                frm, to = sprint_ids(item.get('from')), sprint_ids(item.get('to'))
                for sid in sorted((to - frm) | (frm - to)):
                    if sid not in by_sprint:
                        continue
                    start = instant(self.sprints[sid]['startDate'])
                    if created <= start:
                        continue
                    kind = 'added' if sid in to else 'removed'
                    phase = 'live' if cid in live_ids else 'history'
                    dropped = bool(delivery.get('dropped'))
                    sources = ('reconcile',) if phase == 'history' else \
                        (('event', 'reconcile') if dropped else ('event',))
                    field_id = self.board_estimate_field(sid)
                    by_sprint[sid].append(Change(
                        change_id=cid, sprint_id=sid, issue_id=str(issue['id']), issue_key=issue['key'],
                        kind=kind, at=created, at_text=str(entry['created']), by=str(entry.get('authorId')),
                        by_name=self.users.get(str(entry.get('authorId')), ''),
                        points=self.estimate_of(str(issue['id']), field_id, final_entries),
                        phase=phase, dropped=dropped, duplicates=duplicate_count(delivery),
                        sources=sources))
        for sid in by_sprint:
            by_sprint[sid].sort(key=Change.sort_key)
        return by_sprint

    def _compute(self) -> Dict[str, SprintNumbers]:
        final_entries = self._entries('all')
        changes = self._changes('all', final_entries)
        out: Dict[str, SprintNumbers] = {}
        for sid in self.active_sprints():
            sprint = self.sprints[sid]
            start = instant(sprint['startDate'])
            field_id = self.board_estimate_field(sid)
            at_start, now, ever = set(), set(), set()
            for iid in self.issues:
                if sid in self.member_at(iid, start, final_entries):
                    at_start.add(iid)
                if sid in self.member_now(iid, final_entries):
                    now.add(iid)
            ever = set(at_start) | {c.issue_id for c in changes[sid] if c.kind == 'added'}
            est = {iid: self.estimate_of(iid, field_id, final_entries) for iid in at_start | now | ever}
            committed = sum((est[i] for i in at_start), Decimal(0))
            added = sum((est[i] for i in now - at_start), Decimal(0))
            removed = sum((est[i] for i in ever - now), Decimal(0))
            out[sid] = SprintNumbers(sprint_id=sid, name=str(sprint.get('name', '')),
                                     board_id=str(sprint['originBoardId']), start=start,
                                     committed=committed, added=added, removed=removed,
                                     creep=creep(added, committed), changes=changes[sid])
        return out

    # ── public views ──────────────────────────────────────────────────────────────────────

    def numbers(self, sprint_id: str) -> SprintNumbers:
        return self._numbers[str(sprint_id)]

    def all_numbers(self) -> Dict[str, SprintNumbers]:
        return dict(self._numbers)

    def changes(self, phase: str = 'final') -> List[Change]:
        """Ledger rows expected after a phase: 'backfill' (history), 'live' (history + delivered live
        changes; dropped ones are not yet expected), 'final' (everything)."""
        if phase not in PHASES:
            raise ValueError(phase)
        final_rows = [c for sid in self._numbers for c in self._numbers[sid].changes]
        if phase == 'final':
            return final_rows
        if phase == 'backfill':
            return [c for c in final_rows if c.phase == 'history']
        return [c for c in final_rows if c.phase == 'history' or not c.dropped]

    def hidden_issue_ids(self, account_id: str) -> Set[str]:
        return {iid for iid, issue in self.issues.items() if account_id in (issue.get('hiddenFrom') or [])}

    def visible_changes(self, sprint_id: str, account_id: str) -> List[Change]:
        hidden = self.hidden_issue_ids(account_id)
        return [c for c in self.numbers(sprint_id).changes if c.issue_id not in hidden]

    def hidden_count(self, sprint_id: str, account_id: str) -> int:
        hidden = self.hidden_issue_ids(account_id)
        return sum(1 for c in self.numbers(sprint_id).changes if c.issue_id in hidden)

    def action_result(self, sprint_id: str, account_id: str) -> Dict:
        n = self.numbers(sprint_id)
        return {'sprintId': sprint_id, 'sprintName': n.name,
                'committed': n.committed, 'added': n.added, 'removed': n.removed,
                'creepPercent': n.creep, 'hiddenChanges': self.hidden_count(sprint_id, account_id),
                'changes': [{'changeId': c.change_id, 'issueKey': c.issue_key, 'kind': c.kind,
                             'points': c.points, 'at': c.at, 'by': c.by_name}
                            for c in self.visible_changes(sprint_id, account_id)]}

    def sprints_of_board(self, board_id: str) -> List[str]:
        return [sid for sid in self.active_sprints() if str(self.sprints[sid]['originBoardId']) == str(board_id)]

    def scrum_boards(self) -> List[str]:
        return [bid for bid, b in self.boards.items() if b.get('type') == 'scrum']

    def leak_terms(self, account_id: str) -> Dict[str, List[str]]:
        """What a person must never see: keys and summaries of issues hidden from them, and the
        changelog ids of changes to those issues."""
        hidden = self.hidden_issue_ids(account_id)
        keys = sorted(self.issues[i]['key'] for i in hidden)
        summaries = sorted({self.issues[i].get('summary') for i in hidden if self.issues[i].get('summary')})
        change_ids = sorted({c.change_id for n in self._numbers.values() for c in n.changes if c.issue_id in hidden})
        return {'keys': keys, 'summaries': summaries, 'changeIds': change_ids}

    def reestimated_sprints(self) -> List[str]:
        """Active sprints whose numbers move with the live estimate changes: the sprints
        `t_reestimate_followed` grades."""
        moved = []
        install_entries = self._entries('none')
        final_entries = self._entries('all')
        for sid, n in self._numbers.items():
            field_id = self.board_estimate_field(sid)
            issues = {c.issue_id for c in n.changes}
            for iid in self.issues:
                if sid in self.member_now(iid, final_entries) or sid in self.member_at(iid, n.start, final_entries):
                    issues.add(iid)
            if any(self.estimate_of(i, field_id, final_entries) != self.estimate_of(i, field_id, install_entries)
                   for i in issues):
                moved.append(sid)
        return sorted(moved, key=changelog_order)

    def relevant_live(self) -> List[Dict]:
        """Live entries that touch the Sprint field or an estimation field (the events that warrant Jira or
        queue work); the rest are the irrelevant updates `t_trigger_handoff` grades."""
        est_fields = {str(b.get('estimationFieldId')) for b in self.boards.values() if b.get('estimationFieldId')}
        return [e for e in self.live
                if self._sprint_items(e) or any(str(i.get('fieldId') or '') in est_fields for i in e.get('items') or [])]

    # ── economy optimum (§8.4; rungs are ratios of these) ─────────────────────────────────

    def event_optimum(self) -> int:
        """Jira reads an optimal event path makes in the live phase: one per relevant scripted change that is
        delivered (its first delivery; a duplicate is recognised from KVS), none for a dropped one."""
        return sum(1 for e in self.relevant_live() if not (e.get('delivery') or {}).get('dropped'))

    def limit(self, name: str) -> Optional[int]:
        entry = (self.pack.get('limits') or {}).get(name)
        value = entry.get('value') if isinstance(entry, dict) else entry
        return value if isinstance(value, int) and value > 0 else None

    def reconcile_optimum(self) -> Tuple[Optional[int], List[str]]:
        """Jira calls of an optimal first scheduled run: field list + board list + one configuration and one
        sprint page per scrum board + the ids-only search pages + changelog bulkfetch pages + issue
        bulkfetch pages, over the issues touched since the earliest active start (plus every issue
        currently in an active sprint). Returns (calls, missing limit names)."""
        paged = (self.pack.get('paging') or {}).get('rule') == 'half'
        need = {name: self.limit(name) for name in
                ('searchJqlIdsOnlyMax', 'changelogBulkIssues', 'changelogBulkPageMax', 'issueBulkNamedFields')
                + (('agileBoardPage', 'agileSprintPage') if paged else ())}
        missing = [name for name, value in need.items() if value is None]
        if missing:
            return None, missing
        earliest = min(self.numbers(sid).start for sid in self._numbers) if self._numbers else None
        install_entries = self._entries('none')
        touched = {str(e['issueId']) for e in self.history if earliest and instant(e['created']) > earliest}
        touched |= {iid for iid in self.issues
                    if self.member_now(iid, install_entries) & set(self._numbers)}
        n = len(touched)
        entries = sum(1 for e in self.history if str(e['issueId']) in touched)
        boards = len(self.scrum_boards())
        pages = lambda total, size: max(1, math.ceil(total / size))  # noqa: E731
        if not paged:
            changelog_pages = max(pages(n, need['changelogBulkIssues']), pages(entries, need['changelogBulkPageMax']))
            calls = (1 + 1 + boards + boards + pages(n, need['searchJqlIdsOnlyMax']) + changelog_pages
                     + pages(n, need['issueBulkNamedFields']))
            return calls, []
        # The scoring site's page rule (pack.paging, forge/site/limits.cjs): a list of >= 2 items is served in pages
        # of at most ceil(total / 2), so the optimal run walks those pages too. The sprint list is the board's active
        # sprints (the cheapest filter the API offers).
        served = lambda total, size: pages(total, min(size, math.ceil(total / 2)) if total >= 2 else size)  # noqa: E731
        chunks = pages(n, need['changelogBulkIssues'])
        active = self.active_sprints()
        sprint_pages = sum(served(sum(1 for sid in active if str(self.sprints[sid].get('originBoardId')) == str(b)),
                                  need['agileSprintPage']) for b in self.scrum_boards())
        calls = (1 + served(boards, need['agileBoardPage']) + boards + sprint_pages
                 + served(n, need['searchJqlIdsOnlyMax'])
                 + chunks * served(math.ceil(entries / chunks), need['changelogBulkPageMax'])
                 + pages(n, need['issueBulkNamedFields']))
        return calls, []


def synthetic_pack(seed: str = '00000000000000aa') -> Dict:
    """A small hand-made pack following I1 exactly. It is the severity selftest's fixture (the empty-starter
    and one-function controls run through the REAL checks on it) and the unit tests' pack until WP1's
    generator lands. Deterministic, no clock, no randomness: every value below is written out."""
    users = [{'accountId': 'u-viewer', 'displayName': 'Vera Viewer'},
             {'accountId': 'u-ana', 'displayName': 'Ana Lead'},
             {'accountId': 'u-bob', 'displayName': 'Bob Dev'},
             {'accountId': 'u-app', 'displayName': 'Scope Ledger'}]
    fields = [{'id': 'customfield_10020', 'name': 'Sprint', 'custom': True, 'schema': {'custom': 'gh-sprint'}},
              {'id': 'customfield_10016', 'name': 'Story point estimate', 'custom': True, 'schema': {'type': 'number'}},
              {'id': 'customfield_10028', 'name': 'Story Points', 'custom': True, 'schema': {'type': 'number'}},
              {'id': 'customfield_10031', 'name': 'Team', 'custom': True, 'schema': {'type': 'string'}}]
    projects = [{'id': '10000', 'key': 'OPS', 'name': 'Operations'}, {'id': '10001', 'key': 'PAY', 'name': 'Payments'}]
    boards = [{'id': '1', 'name': 'OPS board', 'type': 'scrum', 'projectKey': 'OPS', 'estimationFieldId': 'customfield_10016'},
              {'id': '2', 'name': 'PAY board', 'type': 'scrum', 'projectKey': 'PAY', 'estimationFieldId': 'customfield_10028'},
              {'id': '3', 'name': 'OPS flow', 'type': 'kanban', 'projectKey': 'OPS', 'estimationFieldId': None}]
    sprints = [{'id': '11', 'name': 'OPS Sprint 7', 'state': 'active', 'originBoardId': '1',
                'startDate': '2026-09-21T09:00:00.000Z', 'endDate': '2026-10-05T09:00:00.000Z', 'completeDate': None},
               {'id': '12', 'name': 'OPS Hotfix 7b', 'state': 'active', 'originBoardId': '1',
                'startDate': '2026-09-24T09:00:00.000Z', 'endDate': '2026-10-08T09:00:00.000Z', 'completeDate': None},
               {'id': '13', 'name': 'OPS Sprint 8', 'state': 'future', 'originBoardId': '1',
                'startDate': None, 'endDate': None, 'completeDate': None},
               {'id': '10', 'name': 'OPS Sprint 6', 'state': 'closed', 'originBoardId': '1',
                'startDate': '2026-09-07T09:00:00.000Z', 'endDate': '2026-09-21T09:00:00.000Z',
                'completeDate': '2026-09-21T08:00:00.000Z'},
               {'id': '21', 'name': 'PAY Sprint 3', 'state': 'active', 'originBoardId': '2',
                'startDate': '2026-09-22T09:00:00.000Z', 'endDate': '2026-10-06T09:00:00.000Z', 'completeDate': None},
               {'id': '22', 'name': 'PAY Sprint 4', 'state': 'future', 'originBoardId': '2',
                'startDate': None, 'endDate': None, 'completeDate': None}]
    sf, ops_est, pay_est = 'customfield_10020', 'customfield_10016', 'customfield_10028'
    issues = []

    def issue(iid, key, summary, sprints_now, ops=None, pay=None, hidden=(), forbidden=()):
        issues.append({'id': iid, 'key': key, 'projectKey': key.split('-')[0], 'summary': summary,
                       'fields': {sf: [int(s) for s in sprints_now], ops_est: ops, pay_est: pay},
                       'hiddenFrom': list(hidden), 'commentForbiddenFor': list(forbidden)})
    # install-time state (history applied)
    issue('100', 'OPS-1', 'Rotate the pager schedule', ['11'], ops=3)
    issue('101', 'OPS-2', 'Alert on queue depth', ['11', '12'], ops=5, pay=8)      # carry-over, decoy value
    issue('102', 'OPS-12', 'Patch the kernel fleet', ['11'], ops=2)
    issue('103', 'OPS-120', 'Archive old runbooks', [], ops=1)                        # removed to the backlog
    issue('104', 'OPS-7', 'Secret migration plan', ['11'], ops=8, hidden=('u-viewer',))
    issue('105', 'OPS-8', 'Incident review template', ['12'], ops=None)
    issue('106', 'OPS-9', 'Disk alarms on the edge', ['11'], ops=13, forbidden=('u-viewer',))
    issue('200', 'PAY-1', 'Refund webhook retries', ['21'], pay=5, ops=2)
    issue('201', 'PAY-2', 'Ledger export to CSV', ['21'], pay=3)
    issue('202', 'PAY-3', 'Payout holiday calendar', ['21'], pay=0.5, hidden=('u-viewer', 'u-bob'))
    issue('203', 'PAY-4', 'Chargeback evidence upload', [], pay=2)

    def change(cid, iid, created, author, frm, to):
        return {'changelogId': cid, 'issueId': iid, 'created': created, 'authorId': author,
                'items': [{'field': 'Sprint', 'fieldtype': 'custom', 'fieldId': sf,
                           'from': frm, 'fromString': '', 'to': to, 'toString': ''}]}
    history = [
        change('9001', '100', '2026-09-20T10:00:00.000Z', 'u-ana', '', '11'),            # before start: committed
        change('9002', '101', '2026-09-20T11:00:00.000Z', 'u-ana', '10', '10, 11'),      # carry-over, before start
        change('9003', '102', '2026-09-22T10:00:00.000Z', 'u-bob', '', '11'),            # added after start
        change('9004', '103', '2026-09-20T12:00:00.000Z', 'u-ana', '', '11'),            # committed ...
        change('9005', '103', '2026-09-23T15:00:00.000Z', 'u-bob', '11', ''),            # ... removed to backlog
        change('9006', '104', '2026-09-22T16:30:00.000Z', 'u-ana', '', '11'),            # hidden from viewer
        change('9007', '101', '2026-09-25T08:00:00.000Z', 'u-bob', '10, 11', '11, 12'),  # multi-id: added to 12
        change('9008', '106', '2026-09-20T09:00:00.000Z', 'u-ana', '', '11'),
        change('9101', '200', '2026-09-21T10:00:00.000Z', 'u-ana', '', '21'),
        change('9102', '201', '2026-09-23T10:00:00.000Z', 'u-bob', '', '21'),            # added after start
        change('9103', '202', '2026-09-23T11:00:00.000Z', 'u-ana', '', '21'),            # hidden added
        change('9104', '105', '2026-09-26T10:00:00.000Z', 'u-ana', '', '12'),
    ]
    live = [
        {**change('9201', '203', '2026-10-01T10:00:00.000Z', 'u-bob', '', '21'),
         'delivery': {'slot': 1, 'duplicates': [7], 'dropped': False}},
        {**change('9202', '102', '2026-10-01T10:05:00.000Z', 'u-ana', '11', '12'),
         'delivery': {'slot': 3, 'duplicates': [], 'dropped': False}},
        {**change('9203', '105', '2026-10-01T10:06:00.000Z', 'u-ana', '12', ''),
         'delivery': {'slot': 2, 'duplicates': [], 'dropped': False}},                      # permuted pair
        {**change('9204', '100', '2026-10-01T10:10:00.000Z', 'u-bob', '11', '11, 12'),
         'delivery': {'slot': None, 'duplicates': [], 'dropped': True}},
        {'changelogId': '9205', 'issueId': '201', 'created': '2026-10-01T10:20:00.000Z', 'authorId': 'u-ana',
         'items': [{'field': 'Story Points', 'fieldtype': 'custom', 'fieldId': pay_est,
                    'from': '3', 'fromString': '3', 'to': '5', 'toString': '5'}],
         'delivery': {'slot': 5, 'duplicates': [], 'dropped': False}},
        {**change('9207', '106', '2026-10-01T10:40:00.000Z', 'u-bob', '11', '11, 12'),
         'delivery': {'slot': 7, 'duplicates': [], 'dropped': False, 'liveUi': True}},
        {'changelogId': '9208', 'issueId': '200', 'created': '2026-10-01T10:45:00.000Z', 'authorId': 'u-ana',
         'items': [{'field': 'Story Points', 'fieldtype': 'custom', 'fieldId': pay_est,
                    'from': '5', 'fromString': '5', 'to': '8', 'toString': '8'}],
         'delivery': {'slot': 8, 'duplicates': [], 'dropped': False, 'liveUi': True}},
        {'changelogId': '9206', 'issueId': '100', 'created': '2026-10-01T10:30:00.000Z', 'authorId': 'u-ana',
         'items': [{'field': 'summary', 'fieldtype': 'jira', 'fieldId': 'summary',
                    'from': None, 'fromString': 'Rotate the pager', 'to': None, 'toString': 'Rotate the pager schedule'}],
         'delivery': {'slot': 6, 'duplicates': [], 'dropped': False}},
    ]
    faults = [{'id': 'f-consumer', 'match': {'scope': 'consumer-of-change', 'changelogId': '9201', 'nth': 1},
               'status': 429, 'retryAfter': 30, 'reason': 'jira-quota-tenant-based'},
              {'id': 'f-reconcile', 'match': {'scope': 'scheduled-run', 'nth': 2},
               'status': 429, 'retryAfter': 2, 'reason': 'jira-burst-based'}]   # the comment-path 429 was dropped (DESIGN §17.2 E)
    limits = {name: {'value': value, 'receipt': 'synthetic (WP1 names, forge/site/limits.cjs)'} for name, value in
              (('searchJqlIdsOnlyMax', 5000), ('searchJqlFieldsMax', 100), ('changelogBulkIssues', 1000),
               ('changelogBulkFields', 10), ('changelogBulkPageMax', 10000), ('issueBulkNamedFields', 1000),
               ('agileSprintPage', 50))}
    return {'seed': seed, 'now': '2026-10-01T12:00:00.000Z', 'cloudId': 'cloud-synthetic',
            'siteUrl': 'https://synthetic.atlassian.net', 'appAccountId': 'u-app', 'viewer': 'u-viewer', 'peer': 'u-bob',
            'users': users, 'fields': fields, 'sprintFieldId': sf, 'projects': projects, 'boards': boards,
            'sprints': sprints, 'issues': issues, 'history': history, 'live': live, 'faults': faults,
            'limits': limits}


def iter_leaves(value, depth: int = 0) -> Iterable:
    """Scalar leaves of an app-defined JSON value (entity rows, action results), for exact-leaf matching."""
    if depth > 31:
        return
    if isinstance(value, dict):
        for v in value.values():
            yield from iter_leaves(v, depth + 1)
    elif isinstance(value, (list, tuple)):
        for v in value:
            yield from iter_leaves(v, depth + 1)
    elif value is not None:
        yield value
