"""forge-2.0 oracle: every number the Scope Ledger scorer grades, recomputed from the run's pack.

The 1.0 oracle (below, unchanged in behaviour on a 1.0-shaped pack) plus the 2.0 views (forge2/SPEC.md §1-§2): the
larger site (entries indexed per issue, so ~1,000 issues and ~1,000 live changes stay linear), the world timeline
(§2.5), the migration's expected v1 and v2 rows at each checkpoint, the scope-status field values, and the dosing
optimum. A 2.0 pack is the scoring site's `withWorld(facts(seed, {scoring}), {scoring})` (forge2/site/fixtures.cjs +
world.cjs): it carries `v1Preload`, and then must carry `world` too (PackDefect otherwise — the oracle never invents
a timeline). The 2.0 pack keys, as this oracle reads them:
  * `now`: the upgrade, virtual t = 0 of the six scored hours (world.cjs); checkpoint 'hK' is now + K virtual hours,
    'final' is after every scripted change (the held-back live-UI pair excluded: Oracle(pack), include_live_ui=False).
    `issues[].fields` is the site at the upgrade, `history` is before it, `live` holds the changes of the scored hours
    (1.0's `delivery` semantics unchanged; the site applies live changes as a PREFIX of creation order).
  * `v1Preload.entities['scope-change']`: the preloaded v1 rows ([{key, value}] like kvs.snapshot(), or {key: value}).
    Each must be a history ledger change of this oracle with the same `at` (PackDefect otherwise).
  * `world.events` [{class, at (ISO) | atMs, ...}], class one of WORLD_CLASSES with the fields WORLD_FIELDS names. A
    cross-board move is an ordinary Sprint-field change world.cjs inserts into `live`; its `issue-move` event names it.

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
from datetime import datetime, timedelta, timezone
from decimal import Decimal, ROUND_HALF_UP, InvalidOperation
import math
import re
from typing import Dict, FrozenSet, Iterable, List, Optional, Set, Tuple

ONE_DECIMAL = Decimal('0.1')
PHASES = ('backfill', 'live', 'final')

# ── 2.0 constants (forge2/SPEC.md; RATE-MODEL.json carries the §2.1 numbers verbatim) ─────────
SCORED_HOURS = 6                                    # §2.3
CHECKPOINTS = tuple(f'h{k}' for k in range(1, SCORED_HOURS + 1)) + ('final',)
MIGRATION_DEADLINE = 'h2'                           # R1: complete within the first 2 virtual hours
WORLD_CLASSES = ('sprint-close', 'issue-move', 'estimation-field', 'issue-delete', 'browse-revoke')   # world.cjs
WORLD_FIELDS = {'sprint-close': ('sprintId',), 'issue-move': ('changelogId',),
                'estimation-field': ('boardId', 'toFieldId'), 'issue-delete': ('issueId',),
                'browse-revoke': ('accountId', 'projectKey')}
QUOTA_POINTS = 2400                                 # §2.1 per installation per virtual hour
BACKGROUND_SHARE_PCT = 70                           # §2.1 / §2.6 default
SEARCH_ISSUES_PER_POINT = 50                        # §2.1 search/jql: 1 + 1 per 50 issues returned
BULKFETCH_POINTS, BULKFETCH_ISSUES_MAX = 2, 1000    # §2.1 changelog bulkfetch: 2 per call, <= 1,000 issues
FIELD_UPDATES_PER_POINT, FIELD_UPDATES_MAX = 50, 200  # §2.1 app/field/value: 1 + 1 per 50 updates, <= 200
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
JUST_BEFORE = timedelta(microseconds=1)


def epoch_ms(when: datetime) -> int:
    """Exact integer epoch milliseconds (v1's `at`): integer arithmetic, so a float compare with the app's
    `Date.parse` value is exact."""
    return (when - EPOCH) // timedelta(milliseconds=1)


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
    board_id: str = ''         # 2.0: the sprint's board (v2 `boardId`)
    estimate_field: str = ''   # 2.0: the board's estimation field in force at the change (v2 `estimateField`)
    moved_to_board: str = ''   # 2.0: a `removed` row of a cross-board move names the board the issue went to

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
        # The larger site (2.0 §2.3): parse each instant once and index entries per issue, so membership and estimate
        # lookups stop scanning every changelog entry for every issue.
        self._t = {str(e['changelogId']): instant(e['created']) for e in history + live}
        self._live_ids = {str(e['changelogId']) for e in self.live}
        self._entry_cache: Dict[str, List[Dict]] = {}
        self._issue_index: Dict[int, Tuple[List[Dict], Dict[str, List[Dict]]]] = {}
        self._series_cache: Dict[Tuple[int, str], Tuple[List[Dict], List[Tuple[datetime, Set[str], Set[str]]]]] = {}
        self._load_v2()
        self._numbers = self._compute()
        if self.is_v2:
            self._load_v1_rows()

    # ── membership and estimates ───────────────────────────────────────────────────────────

    def ledger_sprints(self) -> List[str]:
        """Sprints active at install (2.0: at the upgrade). The app keeps a ledger for each, also after the world
        closes one (SPEC §2.5: its ledger is final)."""
        active = [sid for sid, s in self.sprints.items() if s.get('state') == 'active']
        return sorted(active, key=lambda sid: (instant(self.sprints[sid]['startDate']), changelog_order(sid)))

    def active_sprints(self) -> List[str]:
        """Sprints active at the END of the run: a sprint the world closes leaves every surface (SPEC §2.5). On a
        1.0 pack (no world) this is `ledger_sprints()`."""
        return [sid for sid in self.ledger_sprints() if sid not in self.closed_at]

    def future_sprints(self) -> List[str]:
        return sorted(sid for sid, s in self.sprints.items() if s.get('state') == 'future')

    def board_estimate_field(self, sprint_id: str) -> str:
        board = self.boards[str(self.sprints[sprint_id]['originBoardId'])]
        fid = board.get('estimationFieldId')
        if not fid:
            raise PackDefect(f"scrum board {board.get('id')} has no estimationFieldId")
        return str(fid)

    def field_in_force(self, board_id: str, when: datetime) -> str:
        """The board's estimation field at `when`: the pack's field, replaced by each world `estimation_field_change`
        of that board at or before `when` (SPEC §2.5: changes after the switch use the new field)."""
        board = self.boards[str(board_id)]
        fid = board.get('estimationFieldId')
        if not fid:
            raise PackDefect(f"scrum board {board.get('id')} has no estimationFieldId")
        for t, new_field in self.field_switches.get(str(board_id), ()):
            if t <= when:
                fid = new_field
        return str(fid)

    def _created(self, entry: Dict) -> datetime:
        return self._t[str(entry['changelogId'])]

    def _start(self, sprint_id: str) -> datetime:
        return instant(self.sprints[sprint_id]['startDate'])

    def _entries(self, live_mode: str) -> List[Dict]:
        """history, plus live entries: 'none' | 'delivered' (not dropped) | 'all'. Memoised per mode: the per-issue
        index below is keyed by the list's identity."""
        if live_mode not in self._entry_cache:
            chosen = list(self.history)
            if live_mode == 'delivered':
                chosen += [e for e in self.live if not (e.get('delivery') or {}).get('dropped')]
            elif live_mode == 'all':
                chosen += list(self.live)
            self._entry_cache[live_mode] = sorted(
                chosen, key=lambda e: (self._created(e), changelog_order(str(e['changelogId']))))
        return self._entry_cache[live_mode]

    def _by_issue(self, entries: List[Dict]) -> Dict[str, List[Dict]]:
        hit = self._issue_index.get(id(entries))
        if hit is None or hit[0] is not entries:
            index: Dict[str, List[Dict]] = {}
            for entry in entries:
                index.setdefault(str(entry['issueId']), []).append(entry)
            hit = (entries, index)
            self._issue_index[id(entries)] = hit
        return hit[1]

    def _sprint_items(self, entry: Dict) -> List[Dict]:
        return [item for item in entry.get('items') or []
                if str(item.get('fieldId') or '') == self.sprint_field
                or (not item.get('fieldId') and item.get('field') == 'Sprint')]

    def _membership_series(self, issue_id: str, entries: List[Dict]) -> List[Tuple[datetime, Set[str], Set[str]]]:
        key = (id(entries), issue_id)
        hit = self._series_cache.get(key)
        if hit is None or hit[0] is not entries:
            out = []
            for entry in self._by_issue(entries).get(issue_id, ()):
                for item in self._sprint_items(entry):
                    out.append((self._created(entry), sprint_ids(item.get('from')), sprint_ids(item.get('to'))))
            hit = (entries, out)
            self._series_cache[key] = hit
        return hit[1]

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
        live = [e for e in self._by_issue(entries).get(issue_id, ()) if str(e['changelogId']) in self._live_ids]
        return self._estimate_from(issue_id, field_id, live)

    def estimate_at(self, issue_id: str, field_id: str, when: Optional[datetime]) -> Decimal:
        """The issue's value of `field_id` at `when` (None: after every scripted change): the value at install, then
        each live change of that field created at or before `when`."""
        live = [e for e in self._by_issue(self._entries('all')).get(issue_id, ())
                if str(e['changelogId']) in self._live_ids and (when is None or self._created(e) <= when)]
        return self._estimate_from(issue_id, field_id, live)

    def _estimate_from(self, issue_id: str, field_id: str, live_entries: List[Dict]) -> Decimal:
        value = self.issues[issue_id].get('fields', {}).get(field_id)
        live_items = [(self._created(e), item) for e in live_entries
                      for item in e.get('items') or [] if str(item.get('fieldId') or '') == field_id]
        for _t, item in sorted(live_items, key=lambda pair: pair[0]):
            value = item.get('to')
        return estimate(value)

    # ── changes and numbers ───────────────────────────────────────────────────────────────

    def _changes(self, live_mode: str, final_entries: List[Dict]) -> Dict[str, List[Change]]:
        entries = self._entries(live_mode)
        by_sprint: Dict[str, List[Change]] = {sid: [] for sid in self.ledger_sprints()}
        for entry in entries:
            created = self._created(entry)
            cid = str(entry['changelogId'])
            issue = self.issues[str(entry['issueId'])]
            delivery = entry.get('delivery') or {}
            for item in self._sprint_items(entry):
                frm, to = sprint_ids(item.get('from')), sprint_ids(item.get('to'))
                for sid in sorted((to - frm) | (frm - to)):
                    if sid not in by_sprint:
                        continue
                    start = self._start(sid)
                    if created <= start:
                        continue
                    closed = self.closed_at.get(sid)
                    if closed is not None and created >= closed:
                        continue   # SPEC §2.5: a closed sprint's ledger is final
                    kind = 'added' if sid in to else 'removed'
                    phase = 'live' if cid in self._live_ids else 'history'
                    dropped = bool(delivery.get('dropped'))
                    sources = ('reconcile',) if phase == 'history' else \
                        (('event', 'reconcile') if dropped else ('event',))
                    board_id = str(self.sprints[sid]['originBoardId'])
                    field_id = self.field_in_force(board_id, created)
                    moved_to = ''
                    if kind == 'removed':   # a move needs a ledger on both sides: a future sprint has no added row
                        boards = {str(self.sprints[t]['originBoardId']) for t in to - frm if t in by_sprint}
                        moved_to = next(iter(sorted(boards - {board_id})), '')
                    by_sprint[sid].append(Change(
                        change_id=cid, sprint_id=sid, issue_id=str(issue['id']), issue_key=issue['key'],
                        kind=kind, at=created, at_text=str(entry['created']), by=str(entry.get('authorId')),
                        by_name=self.users.get(str(entry.get('authorId')), ''),
                        points=self.estimate_of(str(issue['id']), field_id, final_entries),
                        phase=phase, dropped=dropped, duplicates=duplicate_count(delivery),
                        sources=sources, board_id=board_id, estimate_field=field_id, moved_to_board=moved_to))
        for sid in by_sprint:
            by_sprint[sid].sort(key=Change.sort_key)
        return by_sprint

    def _compute(self) -> Dict[str, SprintNumbers]:
        """The §1 numbers per ledger sprint, with the 2.0 world (SPEC §2.5; a no-op on a 1.0 pack): a closed sprint's
        numbers are its state just before the close; a deleted issue counts nowhere; an issue's points are its
        CURRENT value of the field the sprint's board uses now (contract §1; before the close, for a closed sprint)."""
        final_entries = self._entries('all')
        changes = self._changes('all', final_entries)
        gone = set(self.deleted_at)
        out: Dict[str, SprintNumbers] = {}
        for sid in self.ledger_sprints():
            sprint = self.sprints[sid]
            start = self._start(sid)
            board_id = str(sprint['originBoardId'])
            closed = self.closed_at.get(sid)
            at_start, now = set(), set()
            for iid in self.issues:
                if iid in gone:
                    continue
                if sid in self.member_at(iid, start, final_entries):
                    at_start.add(iid)
                members = (self.member_at(iid, closed - JUST_BEFORE, final_entries) if closed is not None
                           else self.member_now(iid, final_entries))
                if sid in members:
                    now.add(iid)
            last: Dict[Tuple[str, str], datetime] = {}
            for ch in changes[sid]:          # sorted by time: the last row of each kind wins
                last[(ch.issue_id, ch.kind)] = ch.at
            ever = at_start | {ch.issue_id for ch in changes[sid] if ch.kind == 'added' and ch.issue_id not in gone}

            # Contract §1: an issue's estimate for S is the value of the field S's board uses NOW (a closed sprint's
            # numbers are its state just before the close) — after a switch, the new field for every issue.
            view_at = closed - JUST_BEFORE if closed is not None else max(
                (t for t, _f in self.field_switches.get(board_id, ())), default=start)
            field_now = self.field_in_force(board_id, max(view_at, start))

            def points(iid: str, _when: datetime) -> Decimal:
                return self.estimate_of(iid, field_now, final_entries)
            committed = sum((points(i, start) for i in at_start), Decimal(0))
            added = sum((points(i, last.get((i, 'added'), start)) for i in now - at_start), Decimal(0))
            removed = sum((points(i, last.get((i, 'removed'), start)) for i in ever - now), Decimal(0))
            out[sid] = SprintNumbers(sprint_id=sid, name=str(sprint.get('name', '')),
                                     board_id=board_id, start=start,
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

    def hidden_issue_ids(self, account_id: str, at: Optional[datetime] = None) -> Set[str]:
        """Issues the person cannot browse: hidden at install, plus every project the world took their browse
        permission on (SPEC §2.5) at or before `at` (None: the end of the run, when the surfaces are graded)."""
        hidden = {iid for iid, issue in self.issues.items() if account_id in (issue.get('hiddenFrom') or [])}
        for loss in self.permission_losses:
            if str(loss['accountId']) == account_id and (at is None or loss['t'] <= at):
                hidden |= {iid for iid, issue in self.issues.items() if issue.get('projectKey') == loss['projectKey']}
        return hidden

    # Contract §1: changes of deleted issues are listed to nobody and counted as hidden for nobody (their ledger rows
    # stay as history, `numbers().changes`).
    def visible_changes(self, sprint_id: str, account_id: str) -> List[Change]:
        hidden = self.hidden_issue_ids(account_id)
        return [c for c in self.numbers(sprint_id).changes if c.issue_id not in hidden and c.issue_id not in self.deleted_at]

    def hidden_count(self, sprint_id: str, account_id: str) -> int:
        hidden = self.hidden_issue_ids(account_id)
        return sum(1 for c in self.numbers(sprint_id).changes if c.issue_id in hidden and c.issue_id not in self.deleted_at)

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
        est_fields |= {fid for switches in self.field_switches.values() for _t, fid in switches}
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
        active = self.ledger_sprints()
        sprint_pages = sum(served(sum(1 for sid in active if str(self.sprints[sid].get('originBoardId')) == str(b)),
                                  need['agileSprintPage']) for b in self.scrum_boards())
        calls = (1 + served(boards, need['agileBoardPage']) + boards + sprint_pages
                 + served(n, need['searchJqlIdsOnlyMax'])
                 + chunks * served(math.ceil(entries / chunks), need['changelogBulkPageMax'])
                 + pages(n, need['issueBulkNamedFields']))
        return calls, []

    # ── 2.0: the world, the migration, the field, the dosing optimum (forge2/SPEC.md) ──────

    def _load_v2(self) -> None:
        """A 2.0 pack carries `v1Preload` and `world`; a 1.0 pack carries neither, every 2.0 view refuses it
        (`_need_v2`), and the world-dependent 1.0 views stay exactly 1.0's."""
        self.is_v2 = 'v1Preload' in self.pack
        self.upgrade_at: Optional[datetime] = None
        self.world_events: List[Dict] = []
        self.closed_at: Dict[str, datetime] = {}
        self.deleted_at: Dict[str, datetime] = {}
        self.field_switches: Dict[str, List[Tuple[datetime, str]]] = {}
        self.permission_losses: List[Dict] = []
        self._v1: List[Dict] = []
        if not self.is_v2:
            return
        self.upgrade_at = instant(self.pack.get('now'))
        if self.upgrade_at is None:
            raise PackDefect('pack.now (the upgrade) is not an ISO-8601 instant')
        world = self.pack.get('world')
        if not isinstance(world, dict) or not isinstance(world.get('events'), list):
            raise PackDefect('a 2.0 pack (it carries v1Preload) needs world.events: build it with world.cjs withWorld')
        projects = {i.get('projectKey') for i in self.issues.values()}
        ledger = set(self.ledger_sprints())
        for raw in world['events']:
            cls = (raw or {}).get('class')
            if cls not in WORLD_CLASSES:
                raise PackDefect(f'world event class {cls!r} is not one of {WORLD_CLASSES}')
            when = instant(raw.get('atMs')) if isinstance(raw.get('atMs'), (int, float)) else instant(raw.get('at'))
            if when is None:
                raise PackDefect(f'world {cls} event has neither atMs nor an ISO `at`')
            missing = [k for k in WORLD_FIELDS[cls] if raw.get(k) in (None, '')]
            if missing:
                raise PackDefect(f'world {cls} event lacks {missing}')
            ev = {**raw, 'class': cls, 't': when}
            if cls == 'sprint-close' and str(raw['sprintId']) not in ledger:
                raise PackDefect(f'world closes sprint {raw["sprintId"]}, which is not active at the upgrade')
            if cls == 'estimation-field' and str(raw['boardId']) not in self.boards:
                raise PackDefect(f'world switches the field of unknown board {raw["boardId"]}')
            if cls == 'issue-delete' and str(raw['issueId']) not in self.issues:
                raise PackDefect(f'world deletes unknown issue {raw["issueId"]}')
            if cls == 'browse-revoke' and raw['projectKey'] not in projects:
                raise PackDefect(f'world revokes browse on unknown project {raw["projectKey"]}')
            if cls == 'issue-move' and str(raw['changelogId']) not in self._live_ids:
                raise PackDefect(f'world move names changelog {raw["changelogId"]}, which is not a live change')
            self.world_events.append(ev)
        self.world_events.sort(key=lambda e: e['t'])
        for ev in self.world_events:
            if ev['class'] == 'sprint-close':
                self.closed_at.setdefault(str(ev['sprintId']), ev['t'])
            elif ev['class'] == 'issue-delete':
                self.deleted_at.setdefault(str(ev['issueId']), ev['t'])
            elif ev['class'] == 'estimation-field':
                self.field_switches.setdefault(str(ev['boardId']), []).append((ev['t'], str(ev['toFieldId'])))
            elif ev['class'] == 'browse-revoke':
                self.permission_losses.append(ev)

    def _load_v1_rows(self) -> None:
        """The preloaded v1 rows, each proven to be a history ledger change of this oracle with the same time — the
        site's preload and the oracle's ledger must agree, or no migration row can be graded (PackDefect)."""
        history = {(c.change_id, c.sprint_id): c for n in self._numbers.values() for c in n.changes
                   if c.phase == 'history'}
        rows = ((self.pack.get('v1Preload') or {}).get('entities') or {}).get('scope-change')
        if isinstance(rows, dict):
            rows = list(rows.values())
        elif isinstance(rows, list):
            rows = [r['value'] if isinstance(r, dict) and isinstance(r.get('value'), dict) else r for r in rows]
        else:
            raise PackDefect("v1Preload.entities['scope-change'] is neither a list nor an object")
        for raw in rows:
            try:
                key = (str(raw['changeId']), str(raw['sprintId']))
                at = float(raw['at'])
            except (KeyError, TypeError, ValueError):
                raise PackDefect(f'v1 row {raw!r} lacks changeId/sprintId/at') from None
            ch = history.get(key)
            if ch is None or at != float(epoch_ms(ch.at)):
                raise PackDefect(f'v1 row {key} is not a history ledger change at its `at` ({at})')
            self._v1.append({**raw, 'changeId': key[0], 'sprintId': key[1], 'at': at})
        keys = [(r['changeId'], r['sprintId']) for r in self._v1]
        if len(set(keys)) != len(keys):
            raise PackDefect("v1Preload's scope-change rows repeat a (changeId, sprintId)")

    def _need_v2(self) -> None:
        if not self.is_v2:
            raise PackDefect('not a 2.0 pack: no v1Preload (the 2.0 views need v1Preload and world.events)')

    def world(self, cls: Optional[str] = None) -> List[Dict]:
        """The scripted world events (SPEC §2.5) in time order; each carries `t` (an aware datetime)."""
        self._need_v2()
        return [dict(e) for e in self.world_events if cls is None or e['class'] == cls]

    def checkpoint_mark(self, checkpoint: str) -> Optional[datetime]:
        """'hK' -> the upgrade (pack.now) + K virtual hours; 'final' -> None (every scripted change applied)."""
        self._need_v2()
        if checkpoint == 'final':
            return None
        m = re.fullmatch(r'h([1-9]\d*)', str(checkpoint))
        if not m:
            raise ValueError(f'unknown checkpoint {checkpoint!r}')
        return self.upgrade_at + timedelta(hours=int(m.group(1)))

    def v1_rows(self) -> List[Dict]:
        """The preloaded `scope-change` rows (R1: each must appear in `scope-ledger` exactly once, original
        changeId and time; the v1 entity stays intact)."""
        self._need_v2()
        return [dict(r) for r in self._v1]

    def _row_estimates(self, ch: Change, mark: Optional[datetime]) -> FrozenSet[Decimal]:
        """The values a v2 row's `estimate` may hold: the field in force at the change, read at the change or at the
        checkpoint (SPEC §2.5 "earlier rows keep their estimate" and §1's current estimates both stay accepted).
        A cross-board move's `removed` row also accepts the new board's field (§2.5 "with the NEW board's
        estimate" names the move, not which of its two rows)."""
        fields = {ch.estimate_field}
        if ch.moved_to_board:
            fields.add(self.field_in_force(ch.moved_to_board, ch.at))
        if ch.phase == 'history':   # a backfill written after its board switched could read only the new field
            fields |= {fid for _t, fid in self.field_switches.get(ch.board_id, ())}
        return frozenset(self.estimate_at(ch.issue_id, f, when) for f in fields for when in (ch.at, mark))

    def expected_rows(self, checkpoint: str) -> List[Dict]:
        """Every `scope-ledger` row that exists at the checkpoint: each ledger change (history + live) created by the
        mark, of every sprint active at the upgrade (a closed sprint keeps its final rows). `at` is the change's
        epoch ms; `deleted` is whether the issue was deleted by the mark; `estimates` the accepted values; `v1`
        whether the row was preloaded in v1."""
        mark = self.checkpoint_mark(checkpoint)
        v1 = {(r['changeId'], r['sprintId']) for r in self._v1}
        out = []
        for sid, n in self._numbers.items():
            for ch in n.changes:
                if mark is not None and ch.at > mark:
                    continue
                gone = self.deleted_at.get(ch.issue_id)
                out.append({'changeId': ch.change_id, 'sprintId': sid, 'issueId': ch.issue_id,
                            'issueKey': ch.issue_key, 'kind': ch.kind, 'at': epoch_ms(ch.at),
                            'boardId': ch.board_id, 'estimateField': ch.estimate_field,
                            'estimates': self._row_estimates(ch, mark),
                            'deleted': gone is not None and (mark is None or gone <= mark),
                            'phase': ch.phase, 'dropped': ch.dropped, 'v1': (ch.change_id, sid) in v1,
                            'created': ch.at})
        return out

    def cross_board_moves(self) -> List[Dict]:
        """Sprint changes that take an issue out of a ledger sprint into a sprint of ANOTHER board (SPEC §2.5)."""
        self._need_v2()
        out = []
        for n in self._numbers.values():
            for ch in n.changes:
                if ch.kind == 'removed' and ch.moved_to_board:
                    targets = [c for m in self._numbers.values() for c in m.changes
                               if c.change_id == ch.change_id and c.kind == 'added' and c.board_id == ch.moved_to_board]
                    out.append({'changeId': ch.change_id, 'issueId': ch.issue_id, 'issueKey': ch.issue_key,
                                'fromSprintId': ch.sprint_id, 'toSprintIds': sorted(c.sprint_id for c in targets),
                                'toBoardId': ch.moved_to_board, 'created': ch.at})
        return out

    def active_sprints_at(self, mark: Optional[datetime]) -> List[str]:
        return [sid for sid in self.ledger_sprints()
                if not (sid in self.closed_at and (mark is None or self.closed_at[sid] <= mark))]

    def applied_cut(self, checkpoint: str, live_applied: Optional[int]) -> Optional[datetime]:
        """The instant the site's Jira state stood at when a checkpoint was read. The site applies live changes as a
        PREFIX of creation order (state.cjs applyThrough), so `live_applied` applied changes means every change created
        up to the last of them and none after — even one created before the mark, when the app's own virtual time
        pushed its delivery past the mark. None (or 'final') reads the nominal mark."""
        mark = self.checkpoint_mark(checkpoint)
        if mark is None or live_applied is None:
            return mark
        order = self.pack['live']
        if not 0 <= live_applied <= len(order):
            raise PackDefect(f'{live_applied} live changes applied at {checkpoint}, but the pack scripts {len(order)}')
        return min(mark, self.upgrade_at if live_applied == 0 else instant(order[live_applied - 1]['created']))

    def field_values(self, checkpoint: str, live_applied: Optional[int] = None
                     ) -> Tuple[Dict[str, FrozenSet[str]], Set[str]]:
        """R7 at a checkpoint: {issueKey: accepted values} and the issue keys not graded there. Live changes count up
        to `applied_cut` (what the site had applied when the probe read the field), world events up to the mark. Per
        active sprint S: in S since its start -> `committed`; in S, added after the start -> `added +<points>` (its
        last added row's estimate); in S's scope since the start but not in S now -> `removed`; every other issue ->
        `` (empty). An issue related to two active sprints accepts either sprint's value. Not graded: a deleted
        issue, and an issue a DROPPED change touched in the hour that ends at the mark (no event exists to make it
        fresh; the next hourly reconcile must, so it is graded at the next mark)."""
        mark = self.checkpoint_mark(checkpoint)
        cut = self.applied_cut(checkpoint, live_applied)
        entries = self._entries('all')
        active = self.active_sprints_at(mark)
        rows: Dict[Tuple[str, str], List[Change]] = {}
        for sid in active:
            for ch in self._numbers[sid].changes:
                if cut is None or ch.at <= cut:
                    rows.setdefault((ch.issue_id, sid), []).append(ch)
        values: Dict[str, FrozenSet[str]] = {}
        ungraded: Set[str] = set()
        for iid, issue in self.issues.items():
            gone = self.deleted_at.get(iid)
            if gone is not None and (mark is None or gone <= mark):
                ungraded.add(issue['key'])
                continue
            accepted: Set[str] = set()
            for sid in active:
                in_start = sid in self.member_at(iid, self._start(sid), entries)
                in_now = sid in (self.member_now(iid, entries) if cut is None else self.member_at(iid, cut, entries))
                mine = rows.get((iid, sid), [])
                if in_now and in_start:
                    accepted.add('committed')
                elif in_now:
                    adds = [ch for ch in mine if ch.kind == 'added']
                    if adds:
                        accepted |= {f'added +{format_points(p)}' for p in self._row_estimates(adds[-1], cut)}
                elif in_start or any(ch.kind == 'added' for ch in mine):
                    accepted.add('removed')
            values[issue['key']] = frozenset(accepted or {''})
        if mark is None:
            lower, upper = self.checkpoint_mark(CHECKPOINTS[-2]), None
        else:
            lower, upper = max(self.upgrade_at, mark - timedelta(hours=1)), cut
        for e in self.relevant_live():
            t = self._created(e)
            if (e.get('delivery') or {}).get('dropped') and t > lower and (upper is None or t <= upper):
                ungraded.add(self.issues[str(e['issueId'])]['key'])
        # The same for the world changes that send NO event (a sprint close, a board's estimation-field switch; SPEC
        # §2.5): an issue whose value they move in the hour ending at the mark is graded at the next mark (SPEC R7:
        # "fresh within the same virtual hour as the change"; the hourly reconcile is the app's only signal).
        world_upper = mark if upper is None else upper
        for sid, t in self.closed_at.items():
            if t > lower and (world_upper is None or t <= world_upper):
                ungraded |= {issue['key'] for iid, issue in self.issues.items()
                             if sid in self.member_at(iid, t - JUST_BEFORE, entries)
                             or any(ch.issue_id == iid for ch in self._numbers[sid].changes)}
        for board_id, switches in self.field_switches.items():
            if any(t > lower and (world_upper is None or t <= world_upper) for t, _f in switches):
                board_sprints = {sid for sid in active if str(self.sprints[sid]['originBoardId']) == board_id}
                ungraded |= {issue['key'] for iid, issue in self.issues.items()
                             if board_sprints & self.member_now(iid, entries)}
        return values, ungraded

    def dosing_optimum(self) -> Dict:
        """The fewest background points (SPEC §2.1 costs) an app spends over the scored hours: the agile reads (board
        list, one configuration and one sprint page per scrum board), ONE backfill over the ledger's issues with the
        bulk endpoints (an ids search, 1 + 1 per 50; changelog bulkfetch, 2 per 1,000 issues), one read per relevant
        change delivered as an event, an hourly reconcile search (1, plus a bulkfetch when a dropped change needs
        healing that hour), and the field writes in bulk each hour (requests of <= 200 updates, 1 + 1 per 50). A
        floor to report beside the measured points and a feasibility proof — no app can be dosed if it exceeds the
        background share of the scored hours (PackDefect) — never a graded target."""
        self._need_v2()
        ledger = set(self._numbers)
        earliest = min((self._start(sid) for sid in ledger), default=None)
        install_entries = self._entries('none')
        touched = {str(e['issueId']) for e in self.history if earliest and self._created(e) > earliest}
        touched |= {iid for iid in self.issues if self.member_now(iid, install_entries) & ledger}
        n = len(touched)
        agile = 1 + 2 * len(self.scrum_boards())
        backfill = (1 + math.ceil(n / SEARCH_ISSUES_PER_POINT)) + BULKFETCH_POINTS * max(1, math.ceil(n / BULKFETCH_ISSUES_MAX))
        events = sum(1 for e in self.relevant_live() if not (e.get('delivery') or {}).get('dropped'))
        marks = [self.checkpoint_mark(c) for c in CHECKPOINTS[:-1]]
        reconcile = 0
        lower = self.upgrade_at
        for upper in marks:
            healing = any((e.get('delivery') or {}).get('dropped') and lower < self._created(e) <= upper
                          for e in self.relevant_live())
            reconcile += 1 + (BULKFETCH_POINTS if healing else 0)
            lower = upper
        writes, previous = [], {}
        for c in CHECKPOINTS[:-1]:
            values, _ungraded = self.field_values(c)
            current = {k: min(v) for k, v in values.items()}
            changed = sum(1 for k, v in current.items() if v != previous.get(k, ''))
            previous = current
            full, rest = divmod(changed, FIELD_UPDATES_MAX)
            pts = full * (1 + FIELD_UPDATES_MAX // FIELD_UPDATES_PER_POINT)
            pts += (1 + math.ceil(rest / FIELD_UPDATES_PER_POINT)) if rest else 0
            writes.append(pts)
        total = agile + backfill + events + reconcile + sum(writes)
        cap = QUOTA_POINTS * BACKGROUND_SHARE_PCT // 100
        if total > cap * SCORED_HOURS:
            raise PackDefect(f'the dosing optimum ({total} points) exceeds {SCORED_HOURS} hours of the background '
                             f'share ({cap} each): no app could be dosed on this pack')
        return {'points': total, 'background_cap_per_hour': cap, 'issues_backfilled': n,
                'parts': {'agile': agile, 'backfill': backfill, 'events': events, 'reconcile': reconcile,
                          'field_writes': sum(writes)}, 'field_writes_by_hour': writes}

    def leak_terms_for(self, issue_ids: Iterable[str]) -> Dict[str, List[str]]:
        """What must not show for these issues: their keys, and their summaries that no OTHER issue shares (the
        generator repeats summaries across ~1,000 issues, so a shared one proves nothing)."""
        chosen = set(issue_ids)
        others = {i.get('summary') for iid, i in self.issues.items() if iid not in chosen}
        return {'keys': sorted(self.issues[i]['key'] for i in chosen),
                'summaries': sorted({self.issues[i].get('summary') for i in chosen
                                     if self.issues[i].get('summary') and self.issues[i].get('summary') not in others})}


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


def synthetic_pack_v2(seed: str = '00000000000000bb') -> Dict:
    """The 1.0 synthetic pack in the 2.0 shape the scoring site builds (module docstring: fixtures.cjs `v1Preload`,
    world.cjs `world.events`): the upgrade (`now`) at 09:30, every world class once with a change that tells the right
    reading from the wrong one, and the v1 rows of each active sprint's first two days (SPEC §2.3). The selftest
    fixture of forge2_checks.py; deterministic, every value written out."""
    pack = synthetic_pack(seed)
    sf = pack['sprintFieldId']
    for e in pack['live']:
        e['delivery'].pop('liveUi', None)       # the selftest grades the scored hours only

    def move(cid, iid, created, frm, to, slot):
        return {'changelogId': cid, 'issueId': iid, 'created': created, 'authorId': 'u-ana',
                'items': [{'field': 'Sprint', 'fieldtype': 'custom', 'fieldId': sf, 'from': frm, 'fromString': '',
                           'to': to, 'toString': ''}],
                'delivery': {'slot': slot, 'duplicates': [], 'dropped': False}}
    pack['live'] += [
        move('9301', '200', '2026-10-01T12:00:00.000Z', '21', '11', 9),           # PAY-1: board 2 -> board 1
        move('9302', '101', '2026-10-01T12:20:00.000Z', '11, 12', '11, 12, 21', 10),  # after board 2's switch: 5, not 8
    ]
    pack['live'].sort(key=lambda e: e['created'])
    base = Oracle(pack)
    v1 = []
    for sid in base.ledger_sprints():
        start = base.numbers(sid).start
        for ch in base.numbers(sid).changes:
            if ch.phase == 'history' and ch.at < start + timedelta(days=2):
                v1.append({'key': f'{ch.change_id}:{sid}', 'value': {
                    'sprintId': sid, 'changeId': ch.change_id, 'at': epoch_ms(ch.at), 'created': ch.at_text,
                    'issueId': ch.issue_id, 'issueKey': ch.issue_key, 'kind': ch.kind, 'authorId': ch.by,
                    'authorName': ch.by_name, 'source': 'event'}})

    def at(hours: float) -> str:
        return (instant('2026-10-01T09:30:00.000Z') + timedelta(hours=hours)).isoformat().replace('+00:00', 'Z')
    pack.update({
        'now': '2026-10-01T09:30:00.000Z',
        'v1Preload': {'entities': {'scope-change': v1}},
        'world': {'window': {'start': at(0), 'end': at(6)}, 'injection': None, 'events': [
            {'id': 'world-bb-1', 'class': 'issue-move', 'at': at(2.5), 'issueId': '200', 'issueKey': 'PAY-1',
             'fromSprintId': 21, 'toSprintId': 11, 'fromBoardId': 2, 'toBoardId': 1, 'changelogId': '9301',
             'viaLive': True},
            {'id': 'world-bb-2', 'class': 'estimation-field', 'at': at(2.75), 'boardId': 2,
             'fromFieldId': 'customfield_10028', 'toFieldId': 'customfield_10016'},
            {'id': 'world-bb-3', 'class': 'sprint-close', 'at': at(3.5), 'sprintId': 12, 'boardId': 1},
            {'id': 'world-bb-4', 'class': 'issue-delete', 'at': at(3.8), 'issueId': '201', 'issueKey': 'PAY-2',
             'authorId': 'u-ana'},
            {'id': 'world-bb-5', 'class': 'browse-revoke', 'at': at(4.5), 'accountId': 'u-bob', 'projectKey': 'PAY'},
        ]},
    })
    return pack


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
