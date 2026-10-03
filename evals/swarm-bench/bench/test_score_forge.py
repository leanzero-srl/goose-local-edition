"""forge-1.0 scorer tests: the registry, the oracle, the composition and the public-text parity (DESIGN §8, §14).

The golden observations below are built from the ORACLE (what a correct app's run would leave in I5), so a
check that disagrees with the oracle on correct behaviour fails here, before any golden app or emulator
exists. Each defect test mutates one fact and asserts the rows it must cost — and the score it must land at.
"""
import copy
from decimal import Decimal
import hashlib
import shutil
import json
import os
from pathlib import Path
import re
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch
import zlib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

import bench_budget  # noqa: E402
import forge_oracle as fo  # noqa: E402
import score_forge as sf  # noqa: E402

TOKENS = {
    'light': {'--ds-text': [23, 43, 77], '--ds-text-subtle': [68, 84, 111], '--ds-link': [12, 102, 228],
              '--ds-surface': [255, 255, 255], '--ds-surface-raised': [255, 255, 255]},
    'dark': {'--ds-text': [182, 194, 207], '--ds-text-subtle': [159, 173, 188], '--ds-link': [87, 157, 255],
             '--ds-surface': [29, 33, 37], '--ds-surface-raised': [34, 39, 43]},
}
ENTITY = 'scope-change'
SCOPES = {'classic': ['read:jira-work'], 'granular': ['read:issue:jira', 'read:issue-meta:jira']}


def _iso(dt):
    return dt.strftime('%Y-%m-%dT%H:%M:%S.000+00:00')


def _row(ch, source):
    return {'key': f'{ch.change_id}:{ch.sprint_id}',
            'value': {'changeId': ch.change_id, 'sprintId': int(ch.sprint_id), 'issueId': ch.issue_id,
                      'issueKey': ch.issue_key, 'kind': ch.kind, 'at': _iso(ch.at), 'by': ch.by, 'source': source}}


def _kvs(rows):
    return {'entities': {ENTITY: rows}, 'keys': []}


def _metrics(n):
    return n.metrics_text()


def golden_tree(root: Path):
    for res in ('widget', 'edit', 'modal'):
        (root / 'static' / res / 'build').mkdir(parents=True)
        (root / 'static' / res / 'build' / 'index.html').write_text('<!doctype html><script src="./main.js"></script>')
    (root / 'src').mkdir()
    (root / 'src' / 'index.js').write_text("import api, { route } from '@forge/api';\nimport { kvs } from '@forge/kvs';\n")
    skill = root / 'skills' / 'sprint-scope-analyst'
    skill.mkdir(parents=True)
    (skill / 'SKILL.md').write_text(
        '---\nname: sprint-scope-analyst\ndescription: Explains what entered an active sprint after it started, who '
        'added it and the scope creep; use it when someone asks about sprint scope changes.\n'
        'allowed-tools: get-sprint-scope\n---\n# Sprint scope analyst\n\nCall `get-sprint-scope` with `sprintId`, '
        'the numeric id of the sprint. Read committed, added, removed and creepPercent; on `error`, say so.\n')


def golden_manifest():
    return {
        'app': {'id': 'ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000', 'runtime': {'name': 'nodejs22.x'},
                'storage': {'entities': [{'name': ENTITY, 'attributes': {'sprintId': {'type': 'integer'},
                                                                         'at': {'type': 'string'}},
                                          'indexes': [{'name': 'by-sprint', 'partition': ['sprintId'], 'range': ['at']}]}]}},
        'modules': {
            'trigger': [{'key': 'issue-updated', 'function': 'on-update', 'events': ['avi:jira:updated:issue']}],
            'consumer': [{'key': 'ledger-consumer', 'queue': 'ledger', 'function': 'consume'}],
            'scheduledTrigger': [{'key': 'reconcile', 'function': 'reconcile-fn', 'interval': 'hour'}],
            'dashboards:widget': [{'key': 'scope-widget', 'title': 'Scope', 'resource': 'widget',
                                   'edit': {'resource': 'edit'}, 'resolver': {'function': 'resolver'}}],
            'jira:sprintAction': [{'key': 'scope-action', 'title': 'Scope ledger', 'resource': 'modal',
                                   'resolver': {'function': 'resolver'}}],
            'action': [{'key': 'get-sprint-scope', 'function': 'rovo-action', 'actionVerb': 'GET',
                        'inputs': {'sprintId': {'title': 'Sprint', 'type': 'string', 'required': True}}}],
            'rovo:skill': [{'key': 'sprint-scope-analyst', 'source': {'dir': 'skills/sprint-scope-analyst'},
                            'dependencies': {'tools': ['get-sprint-scope']}}],
            'rovo:agent': [{'key': 'scope-agent', 'name': 'Scope', 'prompt': 'x', 'skills': ['sprint-scope-analyst']}],
            'rovo:mcp': [{'key': 'scope-mcp', 'name': 'Scope ledger', 'tools': ['get-sprint-scope']}],
            'llm': [{'key': 'scope-llm', 'model': ['claude']}],
            'function': [{'key': 'on-update', 'handler': 'index.onUpdate'}, {'key': 'consume', 'handler': 'index.consume'},
                         {'key': 'reconcile-fn', 'handler': 'index.reconcile', 'timeoutSeconds': 300},
                         {'key': 'resolver', 'handler': 'index.resolver'}, {'key': 'rovo-action', 'handler': 'index.action'}],
        },
        'resources': [{'key': 'widget', 'path': 'static/widget/build'}, {'key': 'edit', 'path': 'static/edit/build'},
                      {'key': 'modal', 'path': 'static/modal/build'}],
        'permissions': {'scopes': ['read:jira-work', 'write:jira-work', 'storage:app']},
    }


def _surface(sid, kind, theme, width=800, extra=None):
    tok = TOKENS[theme]
    s = {'id': sid, 'kind': kind, 'theme': theme, 'width': width, 'rendered': True, 'enableTheming': True,
         'tokens': tok, 'dominant': tok['--ds-surface'], 'blank': False, 'nominal': True,
         'textStyles': [{'role': 'metric', 'color': tok['--ds-text'], 'background': tok['--ds-surface']},
                        {'role': 'link', 'color': tok['--ds-link'], 'background': tok['--ds-surface']}],
         'consoleErrors': [], 'pageErrors': [], 'cspViolations': [], 'failedRequests': [],
         'bridgeOps': [{'op': 'getContext'}, {'op': 'enableTheming'}, {'op': 'invoke'}],
         'invokesBeforePaint': 1, 'texts': [], 'changeIdAttrs': []}
    s.update(extra or {})
    return s


def golden_observations(pack):
    o = fo.Oracle(pack)
    viewer, other = o.viewer, pack['peer']
    jira = lambda t, inv, kind, path, **kw: {'t': t, 'inv': inv, 'kind': kind, 'provider': kw.pop('provider', 'app'),  # noqa: E731
                                             'service': 'jira', 'method': kw.pop('method', 'GET'), 'path': path,
                                             'status': kw.pop('status', 200), 'scopes': SCOPES, **kw}
    kvs_call = lambda t, inv, kind, op, body, status=200: {'t': t, 'inv': inv, 'kind': kind, 'provider': 'app',  # noqa: E731
                                                           'service': 'kvs', 'method': 'POST', 'path': f'/api/v1/{op}',
                                                           'body': body, 'status': status}
    f_rec = next(f for f in pack['faults'] if f['match']['scope'] == 'scheduled-run')
    f_con = next(f for f in pack['faults'] if f['match']['scope'] == 'consumer-of-change')
    backfill_rows = [_row(ch, 'reconcile') for ch in o.changes('backfill')]
    backfill = {
        'calls': [jira(0, 'b1', 'scheduled', '/rest/api/3/field'),
                  jira(1, 'b1', 'scheduled', '/rest/agile/1.0/board', status=429, fault=f_rec['id']),
                  jira(3.5, 'b1', 'scheduled', '/rest/agile/1.0/board', response={'isLast': True, 'values': [{}]}),
                  # the scoring site serves the backfill search in two pages (pack.paging); the golden walks both
                  jira(4, 'b1', 'scheduled', '/rest/api/3/search/jql', method='POST',
                       body={'jql': 'updated >= -30d', 'fields': ['customfield_10020']},
                       response={'issues': [{'id': '100'}], 'nextPageToken': 'p2', 'isLast': False}),
                  jira(4.5, 'b1', 'scheduled', '/rest/api/3/search/jql', method='POST',
                       body={'jql': 'updated >= -30d', 'fields': ['customfield_10020'], 'nextPageToken': 'p2'},
                       response={'issues': [{'id': '101'}], 'isLast': True}),
                  jira(5, 'b1', 'scheduled', '/rest/api/3/changelog/bulkfetch', method='POST',
                       body={'issueIdsOrKeys': ['100']}, response={'issueChangeLogs': []}),
                  jira(6, 'b1', 'scheduled', '/rest/api/3/issue/bulkfetch', method='POST', body={'issueIdsOrKeys': ['100']})]
        + [kvs_call(7, 'b1', 'scheduled', 'entity/set', {'entityName': ENTITY, **r}) for r in backfill_rows],
        'invocations': [{'inv': 'b1', 'kind': 'scheduled', 'moduleKey': 'reconcile', 'ok': True}],
        'deliveries': [], 'kvsAfter': _kvs(backfill_rows)}
    live_rows = backfill_rows + [_row(ch, 'event') for ch in o.changes('live') if ch.phase == 'live']
    relevant = {str(e['changelogId']) for e in o.relevant_live()}
    events, lcalls, linvs, deliveries = [], [], [], []
    t = 100.0
    for e in sorted((e for e in pack['live'] if not e['delivery']['dropped']), key=lambda e: e['delivery']['slot']):
        for dup in range(1 + fo.duplicate_count(e['delivery'])):
            inv = f"trig-{e['changelogId']}-{dup}"
            events.append({'changelogId': e['changelogId'], 'slot': e['delivery']['slot'], 'duplicate': bool(dup),
                           'triggerInvocations': [inv]})
            linvs.append({'inv': inv, 'kind': 'trigger', 'moduleKey': 'issue-updated', 'ok': True})
            if str(e['changelogId']) not in relevant:
                continue
            lcalls.append({'t': t, 'inv': inv, 'kind': 'trigger', 'service': 'queue', 'provider': 'app',
                           'method': 'POST', 'path': '/webhook/queue/publish/ledger', 'status': 201})
            cinv = f"cons-{e['changelogId']}-{dup}"
            if str(e['changelogId']) == f_con['match']['changelogId'] and dup == 0:
                lcalls.append(jira(t + 1, cinv, 'consumer', f"/rest/api/3/issue/{e['issueId']}", status=429, fault=f_con['id']))
                linvs.append({'inv': cinv, 'kind': 'consumer', 'ok': True, 'retryAfter': 30})
                deliveries.append({'eventId': f'ev-{cinv}', 'inv': cinv, 'attempt': 1, 'result': 'retry', 'retryAfter': 30})
                cinv += '-r'
                t += 31
            lcalls.append(jira(t + 1, cinv, 'consumer', f"/rest/api/3/issue/{e['issueId']}"))
            linvs.append({'inv': cinv, 'kind': 'consumer', 'ok': True})
            deliveries.append({'eventId': f'ev-{cinv}', 'inv': cinv, 'attempt': 1, 'result': 'ok'})
            t += 5
    live = {'calls': lcalls, 'invocations': linvs, 'deliveries': deliveries, 'events': events, 'kvsAfter': _kvs(live_rows)}
    heal_rows = live_rows + [_row(ch, 'reconcile') for ch in o.changes('final') if ch.dropped]
    heal = {'calls': [jira(500, 'h1', 'scheduled', '/rest/api/3/search/jql', method='POST', body={'jql': 'x'},
                           response={'issues': []})],
            'invocations': [{'inv': 'h1', 'kind': 'scheduled', 'ok': True}], 'deliveries': [], 'kvsAfter': _kvs(heal_rows)}
    rerun = {'calls': [jira(900, 'r1', 'scheduled', '/rest/api/3/search/jql', method='POST', body={'jql': 'x'},
                            response={'issues': []})],
             'invocations': [{'inv': 'r1', 'kind': 'scheduled', 'ok': True}], 'deliveries': [],
             'kvsAfter': _kvs(copy.deepcopy(heal_rows))}

    def action_json(sid, who):
        want = o.action_result(sid, who)
        return {'sprintId': sid, 'sprintName': want['sprintName'], 'committed': float(want['committed']),
                'added': float(want['added']), 'removed': float(want['removed']),
                'creepPercent': None if want['creepPercent'] is None else float(want['creepPercent']),
                'hiddenChanges': want['hiddenChanges'],
                'changes': [{**ch, 'points': float(ch['points']), 'at': _iso(ch['at'])} for ch in want['changes']]}
    rovo = [{'as': who, 'label': 'sprint', 'sprintId': sid, 'threw': False, 'result': action_json(sid, who)}
            for who in (viewer, other) for sid in o.active_sprints()]
    rovo += [{'as': viewer, 'label': 'unknown', 'sprintId': '999999', 'threw': False, 'result': {'error': 'Unknown sprint'}},
             {'as': viewer, 'label': 'missing', 'sprintId': None, 'threw': False, 'result': {'error': 'sprintId is required'}}]

    surfaces, views = [], []
    ou = fo.Oracle(pack, include_live_ui=True)   # every surface after the live-UI slot
    for bi, board in enumerate(o.scrum_boards()):
        after = bi > 0
        oo = ou if after else o
        sprints = oo.sprints_of_board(board)
        for theme in ('light', 'dark'):
            width = 380
            vid = f'widget-view:{board}:{theme}:{width}'
            surfaces.append(_surface(vid, 'widget-view', theme, width, {'texts': [oo.numbers(s).name for s in sprints]}))
            views.append({'board': board, 'theme': theme, 'width': width, 'afterLive': after,
                          'sprints': [{'id': s, 'metrics': _metrics(oo.numbers(s))} for s in sprints],
                          'chart': {'present': True, 'count': 1, 'rects': [
                              {'sprintId': s, 'series': ser, 'height': float(getattr(oo.numbers(s), ser)) * 4}
                              for s in sprints for ser in ('committed', 'added', 'removed')]},
                          'overflow': {'scrollWidth': width, 'clientWidth': width}, 'sprintsVisible': True})
    live_board = o.scrum_boards()[0]
    live_ui = {'board': live_board, 'subscribed': True, 'idleInvokes': 0, 'reloaded': False,
            'sprintsAfter': [{'id': s, 'metrics': _metrics(ou.numbers(s))} for s in ou.sprints_of_board(live_board)]}
    realtime = {'events': [{'channel': 'scope-ledger', 'isGlobal': True, 'payload': json.dumps({'sprintIds': [s]}),
                            'delivered': ['sub-1']} for s in ou.sprints_of_board(live_board)], 'subscriptions': []}
    for theme in ('light', 'dark'):
        surfaces.append(_surface(f'widget-edit:{theme}', 'widget-edit', theme,
                                 extra={'bridgeOps': [{'op': 'getContext'}, {'op': 'enableTheming'},
                                                      {'op': 'getWidgetEditApi'}, {'op': 'invoke'}]}))
    picks = [{'board': b, 'updateConfigCalls': 1, 'onProductSave': False, 'savedConfig': {'boardId': b},
              'viewSprints': o.sprints_of_board(b), 'reopenPressed': [b]} for b in o.scrum_boards()]
    renders, comments, invokes = [], [], []
    for sid in ou.active_sprints():
        vis = ou.visible_changes(sid, viewer)
        n = ou.numbers(sid)
        for theme in ('light', 'dark'):
            surfaces.append(_surface(f'sprint-action:{sid}:{theme}', 'sprint-action', theme,
                                     extra={'texts': [c.issue_key for c in vis] + [c.by_name for c in vis],
                                            'changeIdAttrs': [c.change_id for c in vis]}))
        render = {'sprintId': sid, 'afterLive': True, 'metrics': _metrics(n), 'hiddenCount': str(ou.hidden_count(sid, viewer)),
                  'headers': list(sf.TABLE_COLS),
                  'rows': [{'changeId': c.change_id, 'cells': {'issue': c.issue_key, 'points': fo.format_points(c.points),
                                                               'kind': c.kind, 'by': c.by_name, 'at': _iso(c.at),
                                                               'source': c.sources[0]}} for c in vis],
                  'sortAt': [{'rows': [c.change_id for c in reversed(vis)],
                              'ariaSort': {'at': 'descending'}},
                             {'rows': [c.change_id for c in vis], 'ariaSort': {'at': 'ascending'}}],
                  'router': [{'issueKey': c.issue_key, 'ops': [{'op': 'open', 'url': f'/browse/{c.issue_key}'}]} for c in vis[:1]],
                  'close': {'closeCalled': True}}
        if vis:
            target = vis[0]
            body = {'type': 'doc', 'version': 1, 'content': [{'type': 'paragraph', 'content': [
                {'type': 'text', 'text': f'{target.issue_key} entered {n.name}; creep {fo.format_creep(n.creep)}'}]}]}
            comments.append({'t': 1002, 'issueKey': target.issue_key, 'provider': 'user', 'accountId': viewer,
                             'status': 201, 'body': body})
            comments.append({'t': 1010, 'issueKey': target.issue_key, 'provider': 'user', 'accountId': viewer,
                             'status': 201, 'body': body})
            comments.append({'t': 1020, 'issueKey': 'OPS-9', 'provider': 'user', 'accountId': viewer, 'status': 400, 'body': body,
                             'errorMessages': ['Vera Viewer, you do not have the permission to comment on this issue.']})
            render.update({'select': {'changeId': target.change_id, 'ariaSelected': True},
                           'post': {'commentsAdded': 1, 'successFlags': 1, 'errorFlags': 0},
                           'doubleClick': {'commentsAdded': 1, 'successFlags': 1, 'errorFlags': 0},
                           'forbidden': {'issueKey': 'OPS-9', 'commentsAdded': 0, 'errorFlags': 1, 'successFlags': 0,
                                         'sortWorksAfter': True}})
        renders.append(render)
        invokes.append({'surface': f'sprint-action:{sid}', 'functionKey': 'ledger',
                        'response': {'rows': [c.change_id for c in vis]}})
    # the five scripted explain answers on the first sprint with visible changes (DESIGN §5.2 llm script)
    ex_sid = next(sid for sid in ou.active_sprints() if ou.visible_changes(sid, viewer))
    ex_vis = [c.change_id for c in ou.visible_changes(ex_sid, viewer)]
    hidden_id = ou.leak_terms(viewer)['changeIds'][0]
    tool = {'type': 'function', 'function': {'name': 'report_scope', 'parameters': {'type': 'object', 'properties': {
        'summary': {'type': 'string'}, 'changeIds': {'type': 'array', 'items': {'type': 'string'}}}}}}
    request = {'messages': [{'role': 'user', 'content': f'Explain sprint {ou.numbers(ex_sid).name}: ' + ', '.join(ex_vis)}],
               'tools': [tool], 'tool_choice': {'type': 'function', 'function': {'name': 'report_scope'}}}

    def answer(args):
        return {'choices': [{'finish_reason': 'tool_use', 'message': {'role': 'assistant', 'content': '',
                'tool_calls': [{'type': 'function', 'function': {'name': 'report_scope', 'arguments': args}}]}}]}
    clean = {'summary': 'Scope grew after the sprint started.', 'changeIds': ex_vis[:2]}
    digits = {'summary': 'Scope grew by 13 points: 5 issues were added.', 'changeIds': [ex_vis[0], hidden_id, '99999999']}
    llm_entries = [{'op': 'list', 'models': []}]
    steps = []
    for kind, args in (('clean', clean), ('digits', digits), ('refusal', None), ('malformed', None), ('error', None)):
        entry = {'op': 'chat', 'phase': f'explain-{ex_sid}', 'step': kind, 'model': 'claude-sonnet-5', 'modelStatus': 'active',
                 'moduleType': 'jira:sprintAction', 'moduleKey': 'scope-action', 'asUser': viewer, 'request': request,
                 'response': answer(args) if args else {'choices': [{'finish_reason': 'refusal', 'message': {'content': 'no'}}]},
                 'invocationId': f'inv-explain-{kind}'}
        llm_entries.append(entry)
        invokes.append({'surface': f'sprint-action:{ex_sid}', 'functionKey': 'explain', 'invocationId': entry['invocationId'],
                        'response': {'ok': args is not None}, 'threw': False})
        if kind == 'clean':
            st = {'explanation': clean['summary'], 'idsShown': clean['changeIds'], 'errorFlags': 0}
        elif kind == 'digits':
            st = {'explanation': f"Committed {fo.format_points(ou.numbers(ex_sid).committed)} points.", 'idsShown': ex_vis[:1], 'errorFlags': 0}
        else:
            st = {'explanation': '', 'idsShown': [], 'errorFlags': 1, 'sortWorksAfter': True}
        steps.append({'step': kind, 'llmCalls': 1, 'llm': entry, **st})
    future = o.future_sprints()[0]
    surfaces.append(_surface(f'not-started:{future}', 'sprint-action', 'light', extra={'texts': ['Not started']}))
    ui_calls = [{'t': 2000, 'inv': 'ui1', 'kind': 'resolver', 'provider': 'app', 'service': 'kvs', 'method': 'POST',
                 'path': '/api/v1/entity/query', 'status': 200,
                 'body': {'entityName': ENTITY, 'indexName': 'by-sprint', 'partition': [int(sid)]}}
                for sid in o.active_sprints()]
    ui_calls += [{'t': c['t'], 'inv': f"post-{c['t']}", 'kind': 'resolver', 'provider': 'user', 'service': 'jira',
                  'method': 'POST', 'path': f"/rest/api/3/issue/{c['issueKey']}/comment", 'status': c['status'],
                  'scopes': {'classic': ['write:jira-work'], 'granular': ['write:comment:jira']}} for c in comments]
    second_board = o.scrum_boards()[-1]
    return {
        'manifest': golden_manifest(),
        'kit': {'pins': {'@forge/api': '8.2.0', '@forge/kvs': '2.0.7'}, 'lockSha256': 'k' * 64, 'wrapperSha256': 'w' * 64},
        'lint': {'runs': [{'counts': {'errors': 0, 'warnings': 0}, 'problems': [], 'stageReached': 3, 'stagesTotal': 3}] * 2},
        'build': {'functions': [{'key': f['key'], 'handler': f['handler'], 'bundled': True, 'loaded': True, 'exported': True,
                                 'forgePackages': {'@forge/api': '8.2.0'}, 'outsideKit': []}
                                for f in golden_manifest()['modules']['function']]},
        'phases': {'backfill': backfill, 'live': live, 'heal': heal, 'rerun': rerun},
        'rovo': {'calls': rovo}, 'comments': comments, 'realtime': realtime, 'llm': {'entries': llm_entries},
        'ui': {'surfaces': surfaces, 'calls': ui_calls,
               'widget': {'noConfig': {'needsConfig': True, 'onlyNeedsConfig': True}, 'views': views,
                          'secondInstance': {'configBoard': second_board, 'sprints': o.sprints_of_board(second_board)}},
               'edit': {'options': [{'boardId': b, 'pressed': False} for b in o.scrum_boards()], 'picks': picks},
               'sprintAction': renders, 'live': live_ui,
               'explain': {'sprintId': ex_sid, 'steps': steps},
               'notStarted': {'sprintId': future, 'onlyNotStarted': True},
               'invokeResponses': invokes},
        'harnessMissing': [], 'sectionErrors': {}, 'shots': [],
    }


class Golden(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        golden_tree(self.root)
        self.pack = fo.synthetic_pack()
        self.obs = golden_observations(self.pack)

    def tearDown(self):
        self.tmp.cleanup()

    def score(self, obs=None, **kw):
        ctx = sf.Ctx(self.root, obs or self.obs, self.pack, fixture_seed=self.pack['seed'], **kw)
        return sf.evaluate(ctx)

    def rows(self, result):
        return {r['check']: r for r in result['checks']}


class GoldenTests(Golden):
    def test_the_golden_run_aces_every_check_and_scores_one(self):
        v = self.score()
        bad = {r['check']: (r['score'], r['detail']) for r in v['checks'] if r['score'] != 1.0}
        self.assertEqual(bad, {})
        self.assertEqual((v['score'], v['status'], v['publishable'], v['critical']['multiplier']), (1.0, 'scored', True, 1.0))
        self.assertTrue(v['excellence_gate'])
        self.assertEqual(sf.reference_failures(v), [])

    def test_the_verdict_names_inner_multiplier_and_unsuppressed_criticals(self):
        v = self.score()
        for key in ('score', 'rawScore', 'inner', 'critical', 'admission', 'excellence', 'tiers', 'checks',
                    'harness_missing', 'publishable', 'runtime', 'fixture_seed', 'dev_seed', 'recall_trace', 'status'):
            self.assertIn(key, v)
        self.assertIn('unsuppressed', v['critical'])
        self.assertIn('Unsuppressed criticals: none', sf.format_report(v, 't'))


class CalibrationTests(Golden):
    SEEDS = ('a5a5a5a5a5a5a5a5', '5eed0123456789ab', '0123456789abcdef', 'fedcba9876543210', '00000000deadbeef')
    RC_ECONOMY = [[1.0, 1.5], [0.75, 3], [0.5, 10], [0.25, 40]]   # DESIGN §8.4 rc rungs, the fit's starting point

    @property
    def rc(self):
        return {**sf.TH, 'calibrated': False, 'economy_rungs': self.RC_ECONOMY, 'event_economy_rungs': self.RC_ECONOMY,
                'receipts': dict(sf.TH['receipts'])}

    def verdicts(self, event_ratios=(1.2, 2.07, 1.9, 2.0, 1.4), reconcile=(1.0, 1.11, 1.05, 1.0, 1.0)):
        base = self.score()
        out = []
        for seed, ev, rec in zip(self.SEEDS, event_ratios, reconcile):
            v = copy.deepcopy(base)
            v['fixture_seed'] = seed
            v['scorerVersion'] = 'forge-1.0-rc'
            for r in v['checks']:
                if r['check'] in ('e_event_economy', 'e_reconcile_economy'):
                    r['parts'] = {'ratio': ev if r['check'] == 'e_event_economy' else rec}
            out.append(v)
        return out

    def test_the_economy_rungs_fit_the_golden_worst_of_five(self):
        fitted = sf.calibrate(self.verdicts(), self.rc)
        self.assertTrue(fitted['calibrated'])
        self.assertEqual(fitted['event_economy_rungs'], [[1.0, 2.07], [0.75, 4.14], [0.5, 13.8], [0.25, 55.2]])
        self.assertEqual(fitted['economy_rungs'], [[1.0, 1.11], [0.75, 2.22], [0.5, 7.4], [0.25, 29.6]])
        self.assertIn('worst-of-5', fitted['receipts']['event_economy_rungs'])
        self.assertEqual(fitted['ui_round_trip_rungs'], self.rc['ui_round_trip_rungs'])
        self.assertEqual(sorted(fitted['calibration']['seeds']), sorted(self.SEEDS))
        # a golden better than the optimum never fits a top rung below the oracle's optimum
        best = sf.calibrate(self.verdicts(event_ratios=(1.0,) * 5, reconcile=(0.9,) * 5), self.rc)
        self.assertEqual(best['economy_rungs'][0], [1.0, 1.0])
        # the fit reads calls/optimum, not the display-rounded ratio: 10/9 = 1.1111 rounds to 1.111 but fits 1.12
        exact = self.verdicts()
        for v in exact:
            next(r for r in v['checks'] if r['check'] == 'e_reconcile_economy')['parts'] = {'calls': 10, 'optimum': 9, 'ratio': 1.111}
        self.assertEqual(sf.calibrate(exact, self.rc)['economy_rungs'][0], [1.0, 1.12])

    def test_calibration_refuses_short_or_failing_evidence(self):
        with self.assertRaisesRegex(ValueError, 'distinct fixture seeds'):
            sf.calibrate(self.verdicts()[:4], self.rc)
        dup = self.verdicts()
        dup[1]['fixture_seed'] = dup[0]['fixture_seed']
        with self.assertRaisesRegex(ValueError, 'distinct fixture seeds'):
            sf.calibrate(dup, self.rc)
        bad = self.verdicts()
        next(r for r in bad[2]['checks'] if r['check'] == 'u_widget_numbers')['score'] = 0.5
        with self.assertRaisesRegex(ValueError, 'u_widget_numbers'):
            sf.calibrate(bad, self.rc)
        miss = self.verdicts()
        next(r for r in miss[3]['checks'] if r['check'] == 'e_ui_round_trips')['score'] = 0.75
        with self.assertRaisesRegex(ValueError, 'misses the floor rung'):
            sf.calibrate(miss, self.rc)
        mixed = self.verdicts()
        mixed[4]['scorer_files_sha256'] = {'score_forge.py': 'other'}
        with self.assertRaisesRegex(ValueError, 'different scorer files'):
            sf.calibrate(mixed, self.rc)


class DefectTests(Golden):
    def mutate(self, fn):
        obs = copy.deepcopy(self.obs)
        fn(obs)
        return self.score(obs)

    def test_a_duplicated_row_fires_the_double_count_critical_and_caps_robustness(self):
        def dup(obs):
            rows = obs['phases']['live']['kvsAfter']['entities'][ENTITY]
            rows.append(copy.deepcopy(rows[-1]))
        v = self.mutate(dup)
        r = self.rows(v)
        self.assertEqual(r['t_no_double_count']['score'], 0)
        self.assertIn('t_no_double_count', v['critical']['unsuppressed'])
        self.assertLessEqual(v['score'], 0.899)

    def test_a_stored_estimate_never_reads_as_a_sprint_id(self):
        def points(obs):
            for ph in ('live', 'heal', 'rerun'):
                for r in obs['phases'][ph]['kvsAfter']['entities'][ENTITY]:
                    if r['value']['changeId'] == '9202' and r['value']['sprintId'] == 11:
                        r['value']['points'] = 12
        self.assertEqual(self.rows(self.mutate(points))['t_no_double_count']['score'], 1)

    def test_a_hidden_key_in_the_modal_is_a_leak(self):
        def leak(obs):
            obs['ui']['surfaces'][-2]['texts'].append('OPS-7 Secret migration plan')
        v = self.mutate(leak)
        self.assertEqual(self.rows(v)['b_no_permission_leak']['score'], 0)
        self.assertIn('b_no_permission_leak', v['critical']['unsuppressed'])

    def test_leak_keys_match_whole_tokens_only(self):
        terms = {'keys': ['OPS-12'], 'summaries': [], 'changeIds': []}
        self.assertEqual(sf._leak_hits('OPS-120 and XOPS-12a', terms), [])
        self.assertEqual(sf._leak_hits('see OPS-12, now', terms), ['OPS-12'])
        self.assertEqual(sf._leak_hits('OPS-12\n', terms), ['OPS-12'])

    def test_a_change_id_counts_only_in_its_attribute_and_field(self):
        hidden = fo.Oracle(self.pack).leak_terms('u-viewer')['changeIds'][0]

        def text(obs):
            obs['ui']['surfaces'][-2]['texts'].append(f'build {hidden} done')
        self.assertEqual(self.rows(self.mutate(text))['b_no_permission_leak']['score'], 1)

        def attr(obs):
            obs['ui']['surfaces'][-2]['changeIdAttrs'].append(hidden)
        self.assertEqual(self.rows(self.mutate(attr))['b_no_permission_leak']['score'], 0)

    def test_a_plain_string_comment_body_fails_adf(self):
        def string_body(obs):
            for x in obs['comments']:
                x['body'] = 'OPS-12 entered OPS Sprint 7'
        v = self.mutate(string_body)
        self.assertEqual(self.rows(v)['b_comment_adf_as_user']['score'], 0)
        self.assertEqual(self.rows(v)['b_comment_exactly_once']['score'], 1)

    def test_the_forbidden_comment_400_is_refused_not_landed(self):
        # Jira's measured answer to a comment without ADD_COMMENTS (WP1 d21b58a53): the fixture's forbidden post.
        self.assertEqual(self.rows(self.score(self.obs))['b_comment_adf_as_user']['score'], 1)

        def invalid_body(obs):   # a 400 without the permission message is an attempt that landed badly
            for x in obs['comments']:
                if x['status'] == 400:
                    x['errorMessages'] = []
        self.assertLess(self.rows(self.mutate(invalid_body))['b_comment_adf_as_user']['score'], 1)

        def legacy_403(obs):   # observations from the earlier dev site (403) are still refused
            for x in obs['comments']:
                if x['status'] == 400:
                    x.update(status=403, errorMessages=None)
        self.assertEqual(self.rows(self.mutate(legacy_403))['b_comment_adf_as_user']['score'], 1)

    def test_a_double_post_is_a_cliff(self):
        def double(obs):
            obs['ui']['sprintAction'][0]['doubleClick']['commentsAdded'] = 2
        v = self.mutate(double)
        self.assertEqual(self.rows(v)['b_comment_exactly_once']['score'], 0)
        self.assertLessEqual(v['score'], 0.6)

    def test_a_throw_on_429_loses_retry_after_but_rows_still_land(self):
        def throw(obs):
            for inv in obs['phases']['live']['invocations']:
                if inv.get('retryAfter'):
                    inv.update(retryAfter=None, threw=True, error='Error: 429')
            for d in obs['phases']['live']['deliveries']:
                if d['result'] == 'retry':
                    d.update(result='throw', retryAfter=None)
        r = self.rows(self.mutate(throw))
        self.assertEqual(r['t_retry_after_honoured']['score'], 0)
        self.assertEqual(r['t_event_rows']['score'], 1)

    def test_an_early_retry_on_the_reconcile_429_fails_rate_limit(self):
        def early(obs):
            calls = obs['phases']['backfill']['calls']
            calls[2]['t'] = 1.5
            calls.insert(2, {**calls[2], 'earlyRetry': 'f-reconcile', 'status': 429})
        self.assertEqual(self.rows(self.mutate(early))['r_rate_limit']['score'], 0)

    def test_a_concurrent_request_refused_in_the_window_is_not_a_retry(self):
        # Luna 2026-10-03: the board list's second page went out 1 ms after the field list's 429 (Promise.all); the
        # site refused it as Jira would, and the app retried each refused request only after Retry-After.
        def concurrent(obs):
            calls = obs['phases']['backfill']['calls']
            calls.insert(2, {**calls[2], 't': 1.001, 'path': '/rest/api/3/field', 'status': 429,
                             'fault': 'f-reconcile', 'earlyRetry': 'f-reconcile', 'response': None})
        rows = self.rows(self.mutate(concurrent))
        self.assertEqual(rows['r_rate_limit']['score'], 1, rows['r_rate_limit']['detail'])
        self.assertIn('concurrent request(s) refused in the window, none a retry', rows['r_rate_limit']['detail'])

        def concurrent_then_retried_early(obs):   # the refused concurrent request, repeated inside the window, is
            concurrent(obs)                        # a retry
            calls = obs['phases']['backfill']['calls']
            calls.insert(3, {**calls[2], 't': 1.2})
        self.assertEqual(self.rows(self.mutate(concurrent_then_retried_early))['r_rate_limit']['score'], 0)

    def test_an_open_sprints_only_backfill_misses_removals_and_multiplies_by_its_severity(self):
        gone = {'9005'}

        def drop(obs):
            for ph in ('backfill', 'live', 'heal', 'rerun'):
                ents = obs['phases'][ph]['kvsAfter']['entities'][ENTITY]
                ents[:] = [r for r in ents if r['value']['changeId'] not in gone]
        v = self.mutate(drop)
        r = self.rows(v)
        self.assertEqual(r['r_removals_found']['score'], 0)
        self.assertLess(r['r_backfill_complete']['score'], 1)
        self.assertAlmostEqual(v['critical']['multiplier'], 0.8)
        self.assertIn('r_backfill_complete', v['root_causes'])

    def test_the_legacy_gadget_holds_the_score_at_0799(self):
        def gadget(obs):
            mods = obs['manifest']['modules']
            mods['jira:dashboardGadget'] = mods.pop('dashboards:widget')
        v = self.mutate(gadget)
        self.assertEqual(self.rows(v)['k_dashboard_widget']['score'], 0)
        self.assertLessEqual(v['score'], 0.799)

    def test_config_saved_through_a_resolver_loses_the_edit_bridge(self):
        def resolver(obs):
            for s in obs['ui']['surfaces']:
                if s['kind'] == 'widget-edit':
                    s['bridgeOps'] = [x for x in s['bridgeOps'] if x['op'] != 'getWidgetEditApi']
            for p in obs['ui']['edit']['picks']:
                p.update(updateConfigCalls=0, savedConfig=None)
        v = self.mutate(resolver)
        self.assertEqual(self.rows(v)['k_widget_edit_bridge']['score'], 0)
        self.assertLessEqual(v['score'], 0.799)

    def test_a_kvs_config_widget_fails_the_second_instance(self):
        def kvs_config(obs):
            obs['ui']['widget']['secondInstance']['sprints'] = obs['ui']['widget']['views'][0]['sprints'] and ['11', '12']
        v = self.mutate(kvs_config)
        self.assertLess(self.rows(v)['u_widget_edit_config']['score'], 1)
        self.assertLessEqual(v['score'], 0.799)

    def test_hard_coded_white_fails_tokens_and_dark_mode(self):
        def white(obs):
            for s in obs['ui']['surfaces']:
                s['dominant'] = [255, 255, 255]
                s['enableTheming'] = False
        r = self.rows(self.mutate(white))
        self.assertEqual(r['v_theme_tokens']['score'], 0)
        dark = [s for s in self.obs['ui']['surfaces'] if s['theme'] == 'dark']
        self.assertAlmostEqual(r['v_dark_mode']['score'], round(1 - len(dark) / len(self.obs['ui']['surfaces']), 4))

    def test_wrong_creep_rounding_is_wrong_numbers(self):
        def bankers(obs):
            for v in obs['ui']['widget']['views']:
                for s in v['sprints']:
                    s['metrics']['creep'] = s['metrics']['creep'].replace('36.4', '36.36')
        self.assertLess(self.rows(self.mutate(bankers))['u_widget_numbers']['score'], 1)

    def test_ids_only_search_without_fields_is_legitimate_for_pagination(self):
        def ids_only(obs):
            for page in obs['phases']['backfill']['calls'][3:5]:
                page['body'].pop('fields')
        self.assertEqual(self.rows(self.mutate(ids_only))['r_pagination']['score'], 1)

    def test_a_walk_that_stops_on_a_next_page_token_is_incomplete(self):
        def stop(obs):
            del obs['phases']['backfill']['calls'][4]
        self.assertLess(self.rows(self.mutate(stop))['r_pagination']['score'], 1)

    def test_one_page_reads_are_not_pagination_evidence(self):
        # Sol 2026-10-03: "83/83 paginated reads walked to their end" while 79 were one page the site answered as
        # the last. A read with nothing after its first page never ran the paging loop: graded on nothing.
        def one_page(obs):
            del obs['phases']['backfill']['calls'][4]
            obs['phases']['backfill']['calls'][3]['response'] = {'issues': [{'id': '100'}], 'isLast': True}
        row = self.rows(self.mutate(one_page))['r_pagination']
        self.assertEqual(row['score'], 0.0)
        self.assertIn('vacuous', row['detail'])

        # beside a real walk, the golden's one-page reads (board list, changelog bulkfetch) ride along ungraded
        row = self.rows(self.score())['r_pagination']
        self.assertEqual(row['score'], 1.0, row['detail'])
        self.assertEqual(row['parts']['paged_walks'], 1)
        self.assertIn('one-page read', row['detail'])

    def test_a_page_that_skips_items_is_incomplete(self):
        def skip(obs):   # offset paging by the REQUESTED maxResults while the site served fewer
            calls = obs['phases']['backfill']['calls']
            calls[2]['response'] = {'startAt': 0, 'maxResults': 1, 'total': 3, 'isLast': False, 'values': [{'id': '1'}]}
            calls.insert(3, {**calls[2], 't': 3.7, 'path': '/rest/agile/1.0/board?startAt=50',
                             'response': {'startAt': 50, 'maxResults': 1, 'total': 3, 'isLast': True, 'values': []}})
        row = self.rows(self.mutate(skip))['r_pagination']
        self.assertLess(row['score'], 1)
        self.assertIn('items skipped', row['detail'])

        def token_mismatch(obs):
            obs['phases']['backfill']['calls'][4]['body']['nextPageToken'] = 'forged'
        self.assertLess(self.rows(self.mutate(token_mismatch))['r_pagination']['score'], 1)

    def test_a_one_page_list_of_many_items_on_the_scoring_site_is_a_harness_gap(self):
        def unpaged(obs):
            obs['phases']['backfill']['calls'][2]['response'] = {'isLast': True, 'values': [{'id': '1'}, {'id': '2'}]}
        self.pack['paging'] = {'rule': 'half'}
        row = self.rows(self.mutate(unpaged))['r_pagination']
        self.assertTrue(row.get('unavailable'), row)

    def test_the_removed_search_endpoint_costs_pagination_and_currency(self):
        def old(obs):
            obs['phases']['backfill']['calls'].append({**obs['phases']['backfill']['calls'][3],
                                                       'path': '/rest/api/3/search?jql=x&startAt=0'})
        r = self.rows(self.mutate(old))
        self.assertLess(r['r_pagination']['score'], 1)
        self.assertLess(r['k_current_apis']['score'], 1)

    def test_storage_from_forge_api_is_a_currency_defect(self):
        (self.root / 'src' / 'old.js').write_text("import { storage } from '@forge/api';\n")
        self.assertLess(self.rows(self.score())['k_current_apis']['score'], 1)

    def test_a_skill_name_unequal_to_its_directory_holds_at_0799(self):
        md = self.root / 'skills' / 'sprint-scope-analyst' / 'SKILL.md'
        md.write_text(md.read_text().replace('name: sprint-scope-analyst', 'name: scope-analyst'))
        v = self.score()
        self.assertLess(self.rows(v)['k_rovo_skill']['score'], 1)
        self.assertLessEqual(v['score'], 0.799)

    def test_an_unmodelled_call_holds_the_verdict_for_rescore(self):
        obs = copy.deepcopy(self.obs)
        obs['harnessMissing'] = ['GET /rest/api/3/issue/OPS-1/worklog']
        v = self.score(obs)
        self.assertEqual(v['status'], 'held')
        self.assertFalse(v['publishable'])
        self.assertEqual(v['score'], 1.0)
        self.assertIn('rescore', v['hold'])
        self.assertTrue(any('harness_missing' in f for f in sf.reference_failures(v)))

    def test_a_harness_section_failure_is_unavailable_never_an_app_zero(self):
        obs = copy.deepcopy(self.obs)
        obs['sectionErrors'] = {'ui': 'chromium failed to launch'}
        v = self.score(obs)
        self.assertIn('u_widget_numbers', v['probe_unavailable'])
        self.assertFalse(v['publishable'])
        self.assertNotIn('a_action_result', v['probe_unavailable'])

    def test_a_run_without_ui_still_scores_and_names_the_missing_recording(self):
        obs = copy.deepcopy(self.obs)
        obs['ui']['surfaces'] = []
        v = self.score(obs)
        self.assertEqual(v['status'], 'scored')
        self.assertEqual(v['media']['videos'], [])
        self.assertIn('graded browser recording absent', v['media']['absent'])
        obs['sectionErrors'] = {'ui': 'chromium failed to launch'}
        self.assertIn('chromium failed to launch', self.score(obs)['media']['absent'])

    def test_the_recording_rides_in_the_verdict(self):
        obs = copy.deepcopy(self.obs)
        clip = {'file': 'bench-media/forge-ui.webm', 'mimeType': 'video/webm', 'caption': 'c', 'sha256': 'a' * 64, 'bytes': 9}
        obs['media'] = {'manifest': 'bench-media/media-manifest.json', 'schemaVersion': 1, 'recording': 'graded-browser',
                        'videos': [clip], 'errors': []}
        (self.root / 'bench-media').mkdir()
        (self.root / 'bench-media' / 'media-manifest.json').write_text('{}')
        v = self.score(obs)
        self.assertEqual(v['media']['videos'], [clip])
        self.assertEqual(v['score'], 1.0)
        written = json.loads((self.root / 'bench-media' / 'media-manifest.json').read_text())
        self.assertEqual((written['scorerVersion'], written['recording'], written['videos']), (sf.VERSION, 'graded-browser', [clip]))

    def test_the_shim_runtime_is_unpublishable(self):
        v = self.score(runtime='shim')
        self.assertFalse(v['publishable'])
        self.assertTrue(any('shim' in f for f in sf.reference_failures(v)))

    def test_scopes_follow_the_oauth2_alternative_each_call_uses(self):
        def granular(obs):
            obs['manifest']['permissions']['scopes'] = ['read:issue:jira', 'read:issue-meta:jira', 'write:comment:jira',
                                                        'storage:app']
        self.assertEqual(self.rows(self.mutate(granular))['l_scopes']['score'], 1)

        def admin(obs):
            obs['manifest']['permissions']['scopes'].append('manage:jira-configuration')
        self.assertEqual(self.rows(self.mutate(admin))['l_scopes']['score'], 0.5)

        def permissions_check(obs, declared):
            obs['manifest']['permissions']['scopes'] = declared
            obs['phases']['rerun']['calls'].append({'t': 950, 'inv': 'r1', 'kind': 'scheduled', 'provider': 'app',
                'service': 'jira', 'method': 'POST', 'path': '/rest/api/3/permissions/check', 'status': 200,
                'scopes': {'chosen': [], 'state': 'Current', 'tolerated': ['read:jira-work', 'read:permission:jira']}})
        base = ['read:jira-work', 'write:jira-work', 'storage:app']
        for extra in ([], ['read:permission:jira']):
            self.assertEqual(self.rows(self.mutate(lambda o: permissions_check(o, base + extra)))['l_scopes']['score'], 1)

        def scripts(obs):
            obs['manifest']['permissions']['content'] = {'scripts': ['unsafe-inline']}
        self.assertEqual(self.rows(self.mutate(scripts))['l_scopes']['score'], 0.75)


class DeployReadinessTests(Golden):
    """Every deploy-readiness rule, each with a passing and a failing case (owner 2026-10-03)."""

    def status(self, rule, obs=None, files=None):
        for rel, text in (files or {}).items():
            path = self.root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        v = self.score(obs)
        found = [f for f in v['deploy_readiness']['rules'] if f['rule'].startswith(rule + ' ')]
        self.assertEqual(len(found), 1, rule)
        for rel in (files or {}):
            (self.root / rel).unlink()
        return found[0]

    def mobs(self, fn):
        obs = copy.deepcopy(self.obs)
        fn(obs['manifest'])
        return obs

    def test_the_golden_fixture_passes_every_rule(self):
        v = self.score()
        self.assertEqual([f['rule'] for f in v['deploy_readiness']['rules'] if f['status'] == 'fail'], [])
        self.assertEqual(v['deploy_readiness']['verdict'], 'would deploy and run')
        self.assertIn('Deploy readiness: would deploy and run', sf.format_report(v))

    def test_m1_trigger_event(self):
        self.assertEqual(self.status('M1')['status'], 'pass')
        bad = self.mobs(lambda m: m['modules']['trigger'][0].update(events=['avi:jira:created:issue']))
        self.assertEqual(self.status('M1', bad)['status'], 'fail')

    def test_m2_m3_queue_pairing(self):
        ok = {'src/q.js': "const QUEUE = 'ledger';\nexport const push = () => new Queue({ key: QUEUE });\n"}
        self.assertEqual(self.status('M2', files=ok)['status'], 'pass')
        self.assertEqual(self.status('M2', files={'src/q.js': "new Queue({ key: 'other' })"})['status'], 'fail')
        spare = self.mobs(lambda m: m['modules']['consumer'].append({'key': 'c2', 'queue': 'unused', 'function': 'consume'}))
        self.assertEqual(self.status('M3', spare, files=ok)['status'], 'fail')
        self.assertEqual(self.status('M3', files=ok)['status'], 'pass')

    def test_m4_interval(self):
        self.assertEqual(self.status('M4')['status'], 'pass')
        self.assertEqual(self.status('M4', self.mobs(lambda m: m['modules']['scheduledTrigger'][0].update(interval='hourly')))['status'], 'fail')

    def test_m5_rovo_references(self):
        self.assertEqual(self.status('M5')['status'], 'pass')
        self.assertEqual(self.status('M5', self.mobs(lambda m: m['modules']['rovo:agent'][0].update(skills=['nope'])))['status'], 'fail')

    def test_m6_relative_resources(self):
        self.assertEqual(self.status('M6')['status'], 'pass')
        self.assertEqual(self.status('M6', self.mobs(lambda m: m['resources'][0].update(path='/static/widget/build')))['status'], 'fail')

    def test_m7_entity_indexes(self):
        self.assertEqual(self.status('M7')['status'], 'pass')
        bad = self.mobs(lambda m: m['app']['storage']['entities'][0]['indexes'][0].update(range=['changedAt']))
        self.assertEqual(self.status('M7', bad)['status'], 'fail')

    def test_m8_custom_ui_only(self):
        self.assertEqual(self.status('M8')['status'], 'pass')
        self.assertEqual(self.status('M8', self.mobs(lambda m: m['modules']['jira:sprintAction'][0].update(render='native')))['status'], 'fail')

    def test_m9_legacy_gadget_is_priced_by_its_row(self):
        self.assertEqual(self.status('M9')['status'], 'pass')
        f = self.status('M9', self.mobs(lambda m: m['modules'].update({'jira:dashboardGadget': [{'key': 'g', 'resource': 'widget'}]})))
        self.assertEqual((f['status'], f['graded_by']), ('fail', 'k_dashboard_widget'))

    def test_m10_runtime(self):
        self.assertEqual(self.status('M10')['status'], 'pass')
        f = self.status('M10', self.mobs(lambda m: m['app']['runtime'].update(name='nodejs18.x')))
        self.assertEqual((f['status'], f['graded_by']), ('fail', 'k_current_apis'))

    def test_m11_function_references(self):
        self.assertEqual(self.status('M11')['status'], 'pass')
        self.assertEqual(self.status('M11', self.mobs(lambda m: m['modules']['trigger'][0].update(function='nope')))['status'], 'fail')

    def test_m12_timeout(self):
        self.assertEqual(self.status('M12')['status'], 'pass')
        self.assertEqual(self.status('M12', self.mobs(lambda m: m['modules']['function'][1].update(timeoutSeconds=1000)))['status'], 'fail')

    def test_p1_documented_routes(self):
        self.assertEqual(self.status('P1', files={'src/r.js': 'route`/rest/api/3/issue/${id}/comment`'})['status'], 'pass')
        self.assertEqual(self.status('P1', files={'src/r.js': 'route`/rest/api/3/no/such/thing`'})['status'], 'fail')

    def test_p2_scopes_cover_the_code(self):
        self.assertEqual(self.status('P2', files={'src/r.js': 'route`/rest/api/3/issue/${id}/comment`'})['status'], 'pass')
        bad = self.mobs(lambda m: m['permissions'].update(scopes=['storage:app']))
        self.assertEqual(self.status('P2', bad, files={'src/r.js': 'route`/rest/agile/1.0/board/${b}/sprint`'})['status'], 'fail')

    def test_p3_least_privilege(self):
        self.assertEqual(self.status('P3')['status'], 'pass')
        f = self.status('P3', self.mobs(lambda m: m['permissions']['scopes'].append('manage:jira-configuration')))
        self.assertEqual((f['status'], f['graded_by']), ('fail', 'l_scopes'))

    def test_p4_egress(self):
        call = {'src/e.js': "await fetch('https://api.example.com/v1/x')"}
        self.assertEqual(self.status('P4', files=call)['status'], 'fail')
        ok = self.mobs(lambda m: m['permissions'].update(external={'fetch': {'backend': [{'address': 'https://api.example.com'}]}}))
        self.assertEqual(self.status('P4', ok, files=call)['status'], 'pass')

    def test_p5_csp_and_prefix(self):
        self.assertEqual(self.status('P5')['status'], 'pass')
        page = self.root / 'static' / 'widget' / 'build' / 'index.html'
        good = page.read_text()
        for bad in ('<button onclick="go()">x</button>', '<script src="/assets/main.js"></script>',
                    '<link href="https://fonts.example.com/a.css" rel="stylesheet">', '<div style="color:red">x</div>'):
            page.write_text(good + bad)
            self.assertEqual(self.status('P5')['status'], 'fail', bad)
        page.write_text(good + '<script>window.x = 1</script>')   # static inline script: hashed by Forge (971cf2b8d)
        self.assertEqual(self.status('P5')['status'], 'pass')
        page.write_text(good)

    def test_r1_route_tag(self):
        self.assertEqual(self.status('R1', files={'src/r.js': 'api.asApp().requestJira(route`/rest/api/3/myself`)'})['status'], 'pass')
        self.assertEqual(self.status('R1', files={'src/r.js': "api.asApp().requestJira('/rest/api/3/myself')"})['status'], 'fail')

    def test_r2_no_as_user_in_background(self):
        body = "export async function consume(e) {{ await api.{}().requestJira(route`/rest/api/3/myself`); }}\n"
        resolver = "export const resolver = async () => { return api.asUser(); };\n"
        self.assertEqual(self.status('R2', files={'src/index.js': body.format('asApp') + resolver})['status'], 'pass')
        self.assertEqual(self.status('R2', files={'src/index.js': body.format('asUser') + resolver})['status'], 'fail')

    def test_r3_r4_removed_apis(self):
        self.assertEqual(self.status('R3')['status'], 'pass')
        self.assertEqual(self.status('R3', files={'src/s.js': "import { storage } from '@forge/api';"})['status'], 'fail')
        self.assertEqual(self.status('R4')['status'], 'pass')
        self.assertEqual(self.status('R4', files={'src/s.js': 'route`/rest/api/3/search?jql=x`'})['status'], 'fail')

    def test_r5_search_pagination(self):
        walk = 'route`/rest/api/3/search/jql`; let nextPageToken;'
        self.assertEqual(self.status('R5', files={'src/s.js': walk})['status'], 'pass')
        self.assertEqual(self.status('R5', files={'src/s.js': 'route`/rest/api/3/search/jql`'})['status'], 'fail')

    def test_r6_retry_after(self):
        self.assertEqual(self.status('R6', files={'src/s.js': "route`/rest/api/3/myself`; h.get('retry-after')"})['status'], 'pass')
        self.assertEqual(self.status('R6', files={'src/s.js': 'route`/rest/api/3/myself`'})['status'], 'fail')
        early = copy.deepcopy(self.obs)
        early['phases']['backfill']['calls'].append({**early['phases']['backfill']['calls'][1], 'earlyRetry': 'f-reconcile'})
        self.assertEqual(self.status('R6', early, files={'src/s.js': "route`/rest/api/3/myself`; h.get('retry-after')"})['status'], 'fail')

    def test_r7_hard_coded_field(self):
        self.assertEqual(self.status('R7')['status'], 'pass')
        self.assertEqual(self.status('R7', files={'src/f.js': "const EST = 'customfield_10016';"})['status'], 'fail')

    def test_r8_bounded_jql(self):
        self.assertEqual(self.status('R8')['status'], 'pass')
        bad = copy.deepcopy(self.obs)
        bad['phases']['backfill']['calls'][3]['status'] = 400
        self.assertEqual(self.status('R8', bad)['status'], 'fail')

    def test_r9_kvs_limits(self):
        self.assertEqual(self.status('R9')['status'], 'pass')
        bad = copy.deepcopy(self.obs)
        bad['phases']['backfill']['calls'][-1]['limitError'] = 'VALUE_TOO_LARGE'
        f = self.status('R9', bad)
        self.assertEqual((f['status'], f['graded_by']), ('fail', 's_limits'))

    def test_a_priced_finding_costs_its_row_and_a_graded_one_costs_nothing_here(self):
        bad = copy.deepcopy(self.obs)
        bad['manifest']['modules']['jira:sprintAction'][0]['render'] = 'native'
        rows = self.rows(self.score(bad))
        self.assertLess(rows['k_manifest_semantics']['score'], 1)
        gadget = copy.deepcopy(self.obs)
        gadget['manifest']['modules']['jira:dashboardGadget'] = [{'key': 'g', 'resource': 'widget'}]
        self.assertEqual(self.rows(self.score(gadget))['k_manifest_semantics']['score'], 1)


class LlmRealtimeMcpTests(Golden):
    """2006de559's rows: each passes on the golden fixture and fails on its one defect."""

    def mutate(self, fn):
        obs = copy.deepcopy(self.obs)
        fn(obs)
        return {r['check']: r for r in self.score(obs)['checks']}, self.score(obs)

    def test_rovo_mcp(self):
        rows, _ = self.mutate(lambda o: o['manifest']['modules']['rovo:mcp'][0].update(name='x' * 31))
        self.assertLess(rows['k_rovo_mcp']['score'], 1)
        rows, _ = self.mutate(lambda o: o['manifest']['modules'].pop('rovo:mcp'))
        self.assertEqual(rows['k_rovo_mcp']['score'], 0)

    def test_llm_model_current(self):
        def unknown(o):
            o['llm']['models'] = [{'model': 'claude-sonnet-5', 'status': 'active'}, {'model': 'claude-opus-5', 'status': 'active'}]
            for e in o['llm']['entries']:
                if e.get('op') == 'chat':
                    e.update(model='claude-3-opus', modelStatus='unknown', status=400)
        rows, v = self.mutate(unknown)
        self.assertEqual(rows['k_llm_model_current']['score'], 0.5)
        self.assertEqual(v['admission']['ceiling'], 1.0)     # points only, never a band (DESIGN 17.2)
        rule = [f for f in v['deploy_readiness']['rules'] if f['rule'].startswith('R11 ')][0]
        self.assertEqual((rule['status'], rule['graded_by']), ('fail', 'k_llm_model_current'))

    def test_sampling_parameters_are_a_predicted_failure(self):
        def sampling(o):
            for e in o['llm']['entries']:
                if e.get('op') == 'chat':
                    e.update(request={**e['request'], 'temperature': 0.7, 'top_p': 0.9}, status=400)
        _rows, v = self.mutate(sampling)
        rule = [f for f in v['deploy_readiness']['rules'] if f['rule'].startswith('R12 ')][0]
        self.assertEqual(rule['status'], 'fail')
        self.assertEqual([f for f in self.score()['deploy_readiness']['rules'] if f['rule'].startswith('R12 ')][0]['status'], 'pass')

    def test_realtime_payload_clean(self):
        def leak(o):
            o['realtime']['events'][0]['payload'] = json.dumps({'sprintId': '11', 'issueKey': 'OPS-12', 'points': 5})
        rows, v = self.mutate(leak)
        self.assertLess(rows['b_realtime_payload_clean']['score'], 1)
        self.assertEqual(v['admission']['ceiling'], 0.899)

    def test_widget_live(self):
        rows, _ = self.mutate(lambda o: o['ui']['live'].update(idleInvokes=3))
        self.assertLess(rows['u_widget_live']['score'], 1)        # polling
        rows, _ = self.mutate(lambda o: o['ui']['live'].update(sprintsAfter=[]))
        self.assertLess(rows['u_widget_live']['score'], 1)        # never showed the new numbers
        rows, _ = self.mutate(lambda o: o['ui'].update(live={'absent': 'the pack has no live-UI changes'}))
        self.assertTrue(rows['u_widget_live'].get('unavailable'))

    def test_llm_explain(self):
        def trusts(o):
            st = o['ui']['explain']['steps'][1]
            st['explanation'] = 'Scope grew by 13 points: 5 issues were added.'
        rows, v = self.mutate(trusts)
        self.assertLess(rows['u_llm_explain']['score'], 1)
        self.assertEqual(v['admission']['ceiling'], 0.899)

        def unfiltered(o):
            o['ui']['explain']['steps'][1]['idsShown'] = o['ui']['explain']['steps'][1]['llm']['response']['choices'][0][
                'message']['tool_calls'][0]['function']['arguments']['changeIds']
        self.assertLess(self.mutate(unfiltered)[0]['u_llm_explain']['score'], 1)

        def unforced(o):
            for st in o['ui']['explain']['steps']:
                st['llm'] = {**st['llm'], 'request': {**st['llm']['request'], 'tool_choice': 'auto'}}
        self.assertLess(self.mutate(unforced)[0]['u_llm_explain']['score'], 1)

        def no_refusal_path(o):
            o['ui']['explain']['steps'][2].update(errorFlags=0, sortWorksAfter=False)
        self.assertLess(self.mutate(no_refusal_path)[0]['u_llm_explain']['score'], 1)

    def test_a_kept_explanation_after_a_failed_attempt_passes_and_a_new_id_does_not(self):
        # DESIGN §17.4 gap 29: clearing or keeping the previous explanation both pass. Sol (cf712137) kept the digits
        # step's one cited id on screen through refusal/malformed/error, each with an error flag, and lost 3 subs.
        def kept(o):
            steps = o['ui']['explain']['steps']
            for st in steps[2:]:
                st.update(explanation=steps[1]['explanation'], idsShown=list(steps[1]['idsShown']))
        rows = self.mutate(kept)[0]
        self.assertEqual(rows['u_llm_explain']['score'], 1.0, rows['u_llm_explain']['detail'])

        def invented(o):   # an id the failed click put on screen is still charged, kept ones beside it or not
            steps = o['ui']['explain']['steps']
            for st in steps[2:]:
                st.update(idsShown=[*steps[1]['idsShown'], 'see the ledger'])
        rows = self.mutate(invented)[0]
        self.assertIn('2:refusal:error_flag', rows['u_llm_explain']['detail'])
        self.assertNotIn('3:malformed:error_flag', rows['u_llm_explain']['detail'])   # same ids as the step before

    def test_a_resolver_that_throws_on_the_refusal_has_no_refusal_path(self):
        def throws(o):   # m_llm_no_refusal_path: the page shows the same error flag, the resolver threw instead of answering
            for r in o['ui']['invokeResponses']:
                if r.get('invocationId') == 'inv-explain-refusal':
                    r.update(threw=True, response=None, error="Cannot read properties of undefined (reading 'function')")
        rows, _ = self.mutate(throws)
        self.assertLess(rows['u_llm_explain']['score'], 1)
        self.assertIn('2:refusal:resolver_answered_refusal', rows['u_llm_explain']['detail'])
        self.assertLess(rows['b_invoke_contract']['score'], 1)

        def predates(o):   # observations without invocation links grade the page outcome only — silent, not charged
            for r in o['ui']['invokeResponses']:
                r.pop('invocationId', None)
                r['threw'] = False
        self.assertEqual(self.mutate(predates)[0]['u_llm_explain']['score'], 1.0)

    def test_explain_is_graded_by_position_when_the_platform_refuses_the_call(self):
        def refused(o):   # an unknown model id: every call is a 400 before the script answers (m_llm_unknown_model)
            for st in o['ui']['explain']['steps']:
                st.update(llm={**st['llm'], 'step': None, 'status': 400, 'response': {'code': 'INVALID_REQUEST'}},
                          explanation='', idsShown=[], errorFlags=1, sortWorksAfter=True)
        rows, v = self.mutate(refused)
        self.assertLessEqual(rows['u_llm_explain']['score'], 0.6)
        self.assertEqual(v['admission']['ceiling'], 0.899)

    def test_an_llm_prompt_with_hidden_data_is_the_critical_leak(self):
        def prompt(o):
            hidden = fo.Oracle(self.pack, include_live_ui=True).leak_terms('u-viewer')['keys'][0]
            for e in o['llm']['entries']:
                if e.get('op') == 'chat':
                    e['request'] = {**e['request'], 'messages': [{'role': 'user', 'content': f'all rows incl. {hidden}'}]}
        rows, v = self.mutate(prompt)
        self.assertEqual(rows['b_no_permission_leak']['score'], 0)
        self.assertIn('b_no_permission_leak', v['critical']['unsuppressed'])

    def test_band_four_is_graded(self):
        rows = sf._scenario({'u_llm_explain': 0.0, 'u_widget_live': 0.0, 'v_console_clean': 0.0})
        self.assertEqual(sf.admit(rows)['ceiling'], 0.839)
        self.assertEqual(sf.GRADED_BAND, ('production robustness', 0.899, 0.03, 0.799))

    def test_deploy_rules_for_llm_and_realtime(self):
        status = lambda v, rid: [f for f in v['deploy_readiness']['rules'] if f['rule'].startswith(rid + ' ')][0]['status']  # noqa: E731
        (self.root / 'src' / 'llm.js').write_text("import { chat } from '@forge/llm';\\n")
        self.assertEqual(status(self.score(), 'M13'), 'pass')
        bare = copy.deepcopy(self.obs)
        bare['manifest']['modules'].pop('llm')
        self.assertEqual(status(self.score(bare), 'M13'), 'fail')
        (self.root / 'src' / 'llm.js').unlink()
        self.assertEqual(status(self.score(), 'R10'), 'pass')
        rejected = copy.deepcopy(self.obs)
        rejected['realtime']['events'][0]['rejected'] = 'PUBLISH_WITHOUT_FRONTEND_CONTEXT'
        self.assertEqual(status(self.score(rejected), 'R10'), 'fail')


class ControlTests(unittest.TestCase):
    def test_empty_starter_and_one_function_app_are_scored_at_most_005(self):
        pack = fo.synthetic_pack()
        for one in (False, True):
            ctx = sf.Ctx(None, sf.selftest_empty_observations(pack, one), pack, fixture_seed=pack['seed'])
            v = sf.evaluate(ctx)
            self.assertEqual(v['status'], 'scored')
            self.assertLessEqual(v['score'], 0.05)
            self.assertEqual(v['probe_unavailable'], [])

    def test_the_severity_selftest_holds(self):
        self.assertEqual(sf.severity_selftest(), [])

    def test_single_defect_cost_table_is_pinned(self):
        # DESIGN §8.6 (10): computed once from the composition; a change here is a scoring change.
        pinned = {
            'l_deployable': 0.499, 'l_bundles_load': 0.499, 'b_no_permission_leak': 0.5894,
            'b_comment_exactly_once': 0.5894, 't_no_double_count': 0.5894, 'r_backfill_complete':
            0.5894, 'u_widget_loads': 0.6, 't_event_rows': 0.699, 'k_dashboard_widget': 0.799,
            'r_pagination': 0.899, 'u_llm_explain': 0.899, 'u_widget_live': 0.899, 'k_rovo_mcp': 0.9845,
            'k_llm_model_current': 0.9845, 'e_reconcile_economy': 0.97, 'b_comment_adf_as_user': 0.9757,
            'u_widget_numbers': 0.9805, 'l_lint_warnings': 0.9816, 't_reestimate_followed': 0.9824,
            'u_widget_chart': 0.9872, 'v_widget_sizes': 0.9859, 'l_scopes': 0.9883,
            'k_manifest_semantics': 0.9845, 'k_runtime_risks': 0.9845}
        costs = sf.single_defect_costs()
        self.assertEqual({k: costs[k] for k in pinned}, pinned)

    def test_a_dead_bundle_multiplies_once(self):
        deps = sf.ROOT_BLOCKS['l_bundles_load']
        rows = sf._scenario({**{n: 0.0 for n in deps}, 'l_bundles_load': 0.0})
        v = sf.compose_from_rows(rows)
        self.assertEqual(v['critical']['unsuppressed'], ['l_bundles_load'])
        self.assertLessEqual(v['score'], 0.30)


class MutantJudgeTests(unittest.TestCase):
    def verdict(self, overrides, crit=()):
        rows = sf._scenario(overrides)
        v = sf.compose_from_rows(rows)
        return v

    def test_a_mutant_must_lose_exactly_its_declared_rows(self):
        import forge_controls as fc
        golden = self.verdict({})
        mutant = self.verdict({'r_backfill_complete': 0.8, 'r_removals_found': 0.0, 'u_widget_numbers': 0.5})
        expect = {'loses': ['r_removals_found', 'r_backfill_complete'], 'critical': True, 'max_final': 0.85}
        self.assertEqual(fc.judge(golden, mutant, expect, sf.ROOT_BLOCKS), [])   # u_widget_numbers: attributed
        fails = fc.judge(golden, mutant, {**expect, 'loses': ['r_removals_found']}, sf.ROOT_BLOCKS)
        self.assertTrue(any('undeclared losses' in f for f in fails))
        fails = fc.judge(golden, mutant, {**expect, 'critical': False, 'max_final': 0.1}, sf.ROOT_BLOCKS)
        self.assertEqual(len(fails), 2)
        self.assertTrue(any('declared losses that did not happen' in f for f in
                            fc.judge(golden, golden, expect, sf.ROOT_BLOCKS)))

    def test_an_unavailable_or_held_mutant_never_passes(self):
        import forge_controls as fc
        golden = self.verdict({})
        held = {**self.verdict({}), 'harness_missing': ['GET /x'], 'probe_unavailable': ['l_deployable']}
        self.assertEqual(len(fc.judge(golden, held, {'loses': []}, sf.ROOT_BLOCKS)), 2)


class OracleTests(unittest.TestCase):
    def setUp(self):
        self.o = fo.Oracle(fo.synthetic_pack())

    def test_the_reconcile_optimum_walks_the_scoring_sites_pages(self):
        pack = fo.synthetic_pack()
        self.assertEqual(fo.Oracle(pack).reconcile_optimum(), (9, []))   # the dev-site formula, unchanged
        pack['paging'] = {'rule': 'half'}
        self.assertEqual(fo.Oracle(pack).reconcile_optimum(), (None, ['agileBoardPage']))
        pack['limits']['agileBoardPage'] = {'value': 50}
        # field 1 + 2 scrum boards in 2 pages + 2 configurations + active sprints {1: 2 -> 2 pages, 2: 1 -> 1}
        # + ids-only search 2 pages + changelog bulkfetch 2 pages + issue bulkfetch 1
        self.assertEqual(fo.Oracle(pack).reconcile_optimum(), (13, []))

    def test_creep_rounds_half_away_from_zero_with_decimal(self):
        self.assertEqual(fo.creep(Decimal(1), Decimal(16)), Decimal('6.3'))   # float round() gives 6.2
        self.assertEqual(round(6.25, 1), 6.2)
        self.assertEqual(fo.creep(Decimal(1), Decimal(32)), Decimal('3.1'))
        self.assertIsNone(fo.creep(Decimal(3), Decimal(0)))
        self.assertEqual(fo.format_creep(Decimal('20')), '20.0%')
        self.assertEqual(fo.format_points(Decimal('34.50')), '34.5')
        self.assertEqual(fo.format_points(Decimal('0.0')), '0')

    def test_numbers_follow_the_contract_definitions(self):
        self.assertEqual(self.o.numbers('11').metrics_text(),
                         {'committed': '22', 'added': '8', 'removed': '3', 'creep': '36.4%'})
        self.assertEqual(self.o.numbers('12').metrics_text(),
                         {'committed': '0', 'added': '10', 'removed': '0', 'creep': '—'})
        self.assertEqual(self.o.numbers('21').metrics_text(),
                         {'committed': '5', 'added': '7.5', 'removed': '0', 'creep': '150.0%'})

    def test_a_multi_id_move_is_a_removed_in_one_sprint_and_an_added_in_the_other(self):
        keys = {(c.change_id, c.sprint_id, c.kind) for c in self.o.changes('final')}
        self.assertIn(('9202', '11', 'removed'), keys)
        self.assertIn(('9202', '12', 'added'), keys)
        self.assertIn(('9007', '12', 'added'), keys)
        self.assertNotIn(('9007', '11', 'added'), keys)

    def test_a_dropped_change_accepts_either_source(self):
        dropped = [c for c in self.o.changes('final') if c.dropped]
        self.assertEqual([(c.change_id, c.sources) for c in dropped], [('9204', ('event', 'reconcile'))])
        self.assertNotIn('9204', {c.change_id for c in self.o.changes('live')})

    def test_visibility_and_leak_terms(self):
        self.assertEqual(self.o.hidden_count('11', 'u-viewer'), 1)
        self.assertEqual(self.o.hidden_count('21', 'u-bob'), 1)
        self.assertEqual(self.o.leak_terms('u-viewer')['keys'], ['OPS-7', 'PAY-3'])

    def test_the_decoy_field_is_never_the_estimate(self):
        self.assertEqual(self.o.board_estimate_field('21'), 'customfield_10028')
        self.assertEqual(self.o.reestimated_sprints(), ['21'])

    def test_colliding_id_classes_are_a_pack_defect(self):
        pack = fo.synthetic_pack()
        pack['history'][0]['changelogId'] = '100'
        with self.assertRaisesRegex(fo.PackDefect, 'collide'):
            fo.Oracle(pack)

    def test_a_pack_without_an_i1_field_is_a_named_defect(self):
        pack = fo.synthetic_pack()
        del pack['sprintFieldId']
        with self.assertRaises(fo.PackDefect):
            fo.Oracle(pack)
        ctx = sf.Ctx(None, {}, pack)
        v = sf.evaluate(ctx)
        self.assertEqual(len(v['probe_unavailable']), len(sf.CHECKS))


class PngTests(unittest.TestCase):
    def test_dominant_colour_of_a_chromium_style_png(self):
        w, h = 4, 3
        raw = b''.join(b'\x00' + bytes([29, 33, 37, 255]) * (w - 1) + bytes([255, 0, 0, 255]) for _ in range(h))

        def chunk(kind, data):
            return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
        png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) \
            + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
        with tempfile.NamedTemporaryFile(suffix='.png') as f:
            f.write(png)
            f.flush()
            color, share = sf.png_dominant(Path(f.name))
        self.assertEqual(color, (29, 33, 37))
        self.assertAlmostEqual(share, 0.75)


class RegistryAndContractTests(unittest.TestCase):
    CONTRACT = (ROOT / sf.CONTRACT).read_text()
    SPEC = (ROOT / sf.SPEC).read_text()

    # DESIGN §14 — every check has a contract anchor (§ = FORGE-CONTRACT.md, P = prompt, S = STARTER).
    ANCHORS = {
        'l_deployable': 'P1', 'l_lint_warnings': 'P1', 'l_bundles_load': 'P1', 'l_real_packages': 'S',
        'l_manifest_rules': '§8', 'l_scopes': '§2', 'k_dashboard_widget': '§2', 'k_widget_edit_bridge': '§4',
        'k_rovo_skill': '§6', 'a_skill_instructions': '§6', 'k_current_apis': '§2', 'k_consumer_shape': '§2',
        'k_entity_declared': '§2', 's_entity_index_used': '§2', 's_index_order': '§2', 't_trigger_handoff': '§3',
        't_event_rows': '§3', 't_no_double_count': '§3', 't_out_of_order': '§3', 't_multi_sprint_parse': '§1',
        't_reestimate_followed': '§3', 't_retry_after_honoured': '§3', 'r_rate_limit': '§3',
        't_no_user_in_async': '§3', 'r_as_app': '§3', 'r_backfill_complete': '§3', 'r_removals_found': '§3',
        'r_heal_dropped': '§3', 'e_idempotent_rerun': '§3', 'r_pagination': 'P3', 'r_completes_in_timeout': '§8',
        's_storage_scope': '§2', 's_limits': '§2', 'b_invoke_contract': '§6', 'b_no_permission_leak': '§1',
        'b_hidden_count': '§5', 'a_action_permissions': '§6', 'b_comment_adf_as_user': '§5',
        'b_comment_exactly_once': '§5', 'u_comment_flow': '§5', 'u_widget_loads': '§4', 'u_widget_numbers': '§4',
        'u_widget_chart': '§4', 'u_widget_edit_config': '§4', 'u_ledger_table': '§5', 'u_ledger_sort': '§5',
        'u_issue_router': '§5', 'u_modal_close': '§5', 'u_not_started': '§5', 'v_theme_tokens': '§7',
        'v_dark_mode': '§7', 'v_csp_clean': '§7', 'v_console_clean': '§7', 'v_widget_sizes': '§4',
        'a_action_result': '§6', 'a_action_errors': '§6', 'e_reconcile_economy': 'P', 'e_event_economy': 'P',
        'e_ui_round_trips': 'P',
        # deploy readiness (owner 2026-10-03; DESIGN §8.2/§14 rows to be added by the orchestrator): the platform
        # rules are discoverable, not restated (DESIGN §3 rule 3); the module and permission duties are §2's.
        'k_manifest_semantics': '§2', 'k_runtime_risks': '§2',
        # 2006de559: Forge LLM, Realtime and rovo:mcp
        'k_rovo_mcp': '§2', 'k_llm_model_current': '§2', 'u_llm_explain': '§5', 'u_widget_live': '§4',
        'b_realtime_payload_clean': '§4',
    }
    # Every hook the contract names, and the check that probes it.
    HOOKS = {
        'board-option': 'u_widget_edit_config', 'scope-widget': 'u_widget_loads', 'needs-config': 'u_widget_edit_config',
        'sprint': 'u_widget_numbers', 'chart': 'u_widget_chart', 'not-started': 'u_not_started',
        'hidden-count': 'b_hidden_count', 'ledger': 'u_ledger_table', 'post-summary': 'b_comment_exactly_once',
        'close': 'u_modal_close', 'committed': 'u_widget_numbers', 'added': 'u_widget_numbers',
        'removed': 'u_widget_numbers', 'creep': 'u_widget_numbers', 'issue': 'u_ledger_table',
        'points': 'u_ledger_table', 'explain': 'u_llm_explain', 'explanation': 'u_llm_explain', 'kind': 'u_ledger_table', 'by': 'u_ledger_table', 'at': 'u_ledger_sort',
        'source': 'u_ledger_table', 'data-board-id': 'u_widget_edit_config', 'aria-pressed': 'u_widget_edit_config',
        'data-sprint-id': 'u_widget_numbers', 'data-series': 'u_widget_chart', 'data-change-id': 'u_ledger_table',
        'aria-sort': 'u_ledger_sort', 'aria-selected': 'u_comment_flow', 'get-sprint-scope': 'a_action_result',
        'sprintId': 'a_action_errors', 'hiddenChanges': 'b_hidden_count', 'creepPercent': 'a_action_result',
        'changeId': 'a_action_result', 'sprint-scope-analyst': 'k_rovo_skill', 'allowed-tools': 'k_rovo_skill',
        'view.theme.enable()': 'v_theme_tokens', '--ds-surface': 'v_dark_mode', '--ds-text': 'v_theme_tokens',
        '--ds-link': 'v_theme_tokens',
    }

    def test_registry_shape(self):
        self.assertEqual(len(sf.CHECKS), 66)
        self.assertEqual(len(sf.CRITICAL_CHECKS), 7)
        counts = {}
        for _n, t, *_ in sf.CHECKS:
            counts[t] = counts.get(t, 0) + 1
        self.assertEqual(counts, {'L': 6, 'K': 10, 'T': 8, 'R': 7, 'S': 4, 'B': 6, 'U': 12, 'V': 5, 'A': 4, 'E': 4})
        self.assertEqual(sf.TIER_WEIGHT, {'L': .08, 'K': .10, 'T': .16, 'R': .14, 'S': .08, 'B': .12, 'U': .16,
                                          'V': .08, 'A': .08})

    def test_every_check_has_a_contract_anchor(self):
        self.assertEqual(set(self.ANCHORS), sf.REGISTERED)
        sections = set(re.findall(r'^## (\d+)\.', self.CONTRACT, re.M))
        for check, anchor in self.ANCHORS.items():
            if anchor.startswith('§'):
                self.assertIn(anchor[1:], sections, check)

    def test_every_contract_hook_has_a_check(self):
        named = set(re.findall(r'data-testid="([^"]+)"', self.CONTRACT))
        named |= set(re.findall(r'data-metric="([^"]+)"', self.CONTRACT))
        named |= {m for m in re.findall(r'`(\w+)`', self.CONTRACT[self.CONTRACT.index('headers `th[data-col]`'):][:120])}
        named |= set(re.findall(r'\b(data-[a-z-]+|aria-[a-z]+)\b', self.CONTRACT)) - {'data-testid', 'data-metric', 'data-col'}
        named |= {'get-sprint-scope', 'sprintId', 'hiddenChanges', 'creepPercent', 'changeId', 'sprint-scope-analyst',
                  'allowed-tools', 'view.theme.enable()', '--ds-surface', '--ds-text', '--ds-link'}
        for hook in named:
            self.assertIn(hook, self.HOOKS, f'contract hook {hook!r} has no check')
            self.assertIn(self.HOOKS[hook], sf.REGISTERED)
            self.assertIn(hook.split('()')[0], self.CONTRACT, hook)

    def test_the_prompt_states_the_four_band_maxima(self):
        bands = re.findall(r'maximum (0\.\d+)', self.SPEC[self.SPEC.index('## Score bands'):])
        self.assertEqual([float(b) for b in bands], [limit for limit, _l, _n in sf.ADMISSION_BANDS])

    def test_the_prompt_states_the_call_budget(self):
        self.assertEqual(bench_budget.stated_budgets(self.SPEC), [bench_budget.CALL_BUDGET])

    def test_thresholds_are_frozen_and_pinned(self):
        raw = sf.THRESHOLDS_FILE.read_bytes()
        data = json.loads(raw)
        self.assertTrue(data['calibrated'])
        self.assertEqual(sf.VERSION, 'forge-1.0')
        self.assertEqual(hashlib.sha256(raw).hexdigest(), sf.CALIB_SHA256)
        self.assertEqual(len(data['calibration']['seeds']), sf.CALIBRATION_SEEDS)
        for key in sf.FITTED_RUNGS:
            self.assertIn('worst-of-5', data['receipts'][key])

    def test_a_calibrated_file_off_its_pin_refuses_to_score(self):
        with tempfile.TemporaryDirectory() as tmp:
            edited = Path(tmp) / 'forge-thresholds.json'
            edited.write_text(sf.THRESHOLDS_FILE.read_text().replace('"contrast_min": 4.5', '"contrast_min": 3.0'))
            with patch.object(sf, 'THRESHOLDS_FILE', edited), self.assertRaisesRegex(SystemExit, 'does not match the pin'):
                sf._load_thresholds()


class CliTests(unittest.TestCase):
    def run_cli(self, *args):
        from io import StringIO
        err = StringIO()
        with patch('sys.stderr', err):
            code = sf.main(list(args))
        return code, err.getvalue()

    def test_refusals(self):
        with tempfile.TemporaryDirectory() as tmp:
            tree = Path(tmp)
            out = str(tree / 'v.json')
            self.assertEqual(self.run_cli('--tree', tmp, '--json-out', out)[0], 2)
            self.assertEqual(self.run_cli('--tree', tmp, '--json-out', out, '--seed', 'xyz')[0], 2)
            (tree / 'trace.jsonl').write_text(json.dumps({'fixture_seed': 'a' * 16, 'dev_seed': 'b' * 16}) + '\n')
            code, err = self.run_cli('--tree', tmp, '--json-out', out, '--seed', 'b' * 16)
            self.assertEqual(code, 2)
            self.assertIn('dev_seed', err)
            with patch.object(sf, '_probe_preflight', return_value='no playwright'):
                self.assertEqual(self.run_cli('--tree', tmp, '--json-out', out, '--seed', 'a' * 16)[0], 3)


class TierWiringTests(unittest.TestCase):
    """forge-1.0 through run_build / isolated_tiers / bench_rescore / release_manifest (DESIGN §12)."""

    def test_the_forge_flag_selects_its_scorer_vendor_and_spec(self):
        import isolated_tiers
        import run_build
        import types
        fake_site = types.ModuleType('forge_site')
        fake_site.DOCS_PATH, fake_site.API_KEY = None, None
        with patch.dict(os.environ, {'BENCH_FORGE10': '1'}, clear=True), patch.dict(sys.modules, {'forge_site': fake_site}):
            scorer, vendor, spec = run_build._regime()
            self.assertIs(scorer, sf)
            self.assertIs(vendor, fake_site)
            self.assertEqual(spec, 'forge/public/spec-build-forge.md')
            tier = isolated_tiers.active()
            self.assertEqual((tier.family, tier.network, tier.kit, tier.reasoning_effort), ('forge', 'fenced', True, 'medium'))
            self.assertFalse(hasattr(tier, 'wallet_usd'))   # owner 2026-10-02: no default dollar stop on any tier
            text = run_build.render_public_contract((ROOT / spec).read_text(), 8850, fake_site)
            self.assertEqual(text, (ROOT / spec).read_text())
            with self.assertRaisesRegex(RuntimeError, 'DOCS_URL'):
                run_build.render_public_contract('see {DOCS_URL}', 8850, fake_site)
        with patch.dict(os.environ, {'BENCH_FORGE10': '1', 'BENCH_SB72': '1'}, clear=True), \
                self.assertRaisesRegex(RuntimeError, 'more than one isolated tier'):
            isolated_tiers.active()

    def test_sb_tiers_keep_their_defaults(self):
        import isolated_tiers
        for tier in (isolated_tiers.SB71, isolated_tiers.SB72):
            self.assertEqual((tier.family, tier.vendor, tier.network, tier.kit, tier.reasoning_effort,
                              tier.own_scoring_site), ('payments', 'vendor_service_v3', 'open', False, None, False))

    def test_the_reasoning_effort_is_pinned_and_recorded(self):
        import isolated_tiers
        import run_build
        tier = isolated_tiers.FORGE10
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(run_build.pinned_reasoning_effort(tier, {'GOOSE_THINKING_EFFORT': 'high'}),
                             {'value': 'medium', 'source': 'forge-1.0 pin', 'env': 'GOOSE_THINKING_EFFORT',
                              'replaced_goose_config': 'high'})
        with patch.dict(os.environ, {'BENCH_REASONING_EFFORT': 'LOW'}, clear=True):
            self.assertEqual(run_build.pinned_reasoning_effort(tier, {})['value'], 'low')
        with patch.dict(os.environ, {'BENCH_REASONING_EFFORT': 'extreme'}, clear=True), \
                self.assertRaisesRegex(RuntimeError, 'REFUSED'):
            run_build.pinned_reasoning_effort(tier, {})

    def test_score_forge_has_every_attribute_its_callers_use(self):
        used = set()
        for name in ('run_build.py', 'bench_rescore.py'):
            used |= set(re.findall(r'\bscorer\.([A-Za-z_][A-Za-z0-9_]*)', (HERE / name).read_text()))
        self.assertEqual(sorted(a for a in used if not hasattr(sf, a)), [])
        self.assertTrue(hasattr(sf, '_kit'))

    def test_a_forge_receipt_carries_the_kit_and_replays_score_forge(self):
        import bench_rescore
        import dataclasses
        import isolated_tiers
        tier = dataclasses.replace(isolated_tiers.FORGE10, scorer_files=('score_forge.py', 'forge_oracle.py'))
        with tempfile.TemporaryDirectory() as tmp, patch.dict(isolated_tiers.BY_VERSION, {'forge-1.0': tier}):
            tree = Path(tmp) / 'tree'
            tree.mkdir()
            (tree / 'manifest.yml').write_text('app: {}\n')
            (tree / 'trace.jsonl').write_text(json.dumps({'fixture_seed': '0123456789abcdef', 'dev_seed': 'f' * 16}) + '\n')
            agent = {'exit': 0, 'secs': 3.0, 'timed_out': False, 'tail': ''}
            args = dict(run_id='r', started_at='t', seed='0123456789abcdef', port=8850, provider='p', model='m', tier=tier)
            with self.assertRaisesRegex(ValueError, 'kit identity'):
                bench_rescore.write_completion(tree, Path(tmp) / 'r0.json', agent, **args)
            receipt = bench_rescore.write_completion(tree, Path(tmp) / 'r.json', agent, kit={'lock_sha256': 'k' * 64,
                                                                                               'wrapper_sha256': 'w' * 64}, **args)
            self.assertEqual((receipt['scorerVersion'], receipt['tier'], receipt['kit_lock_sha256']), ('forge-1.0', 'forge-1.0', 'k' * 64))
            self.assertIn('forge/public/FORGE-CONTRACT.md', receipt['contracts'])
            self.assertIs(bench_rescore.validate_receipt(receipt, tree, 'r'), receipt)

    def test_release_payload_is_per_family(self):
        import release_manifest
        payments, forge = set(release_manifest.payload()), set(release_manifest.payload(family='forge'))
        self.assertFalse({f for f in payments if 'forge' in f}, 'SB7.x pins must not move when forge does')
        self.assertLessEqual({'forge/public/spec-build-forge.md', 'forge/public/FORGE-CONTRACT.md', 'bench/score_forge.py',
                              'bench/forge_oracle.py', 'bench/run_build.py', 'bench/isolated_tiers.py'}, forge)
        self.assertFalse({'bench/score_sb7.py', 'bench/score_sb71.py', 'spec-build-sb72.md'} & forge)
        self.assertFalse([f for f in forge if 'node_modules' in f])
        manifest = json.loads((ROOT / 'forge' / 'release-manifest.json').read_text())
        self.assertEqual((manifest['family'], manifest['scorerVersion']), ('forge', 'forge-1.0'))


# ── probe smoke test against a THIN FAKE of I2 (DESIGN §13.3: until WP1's emulator lands) ─────────────

FAKE_SITE = r"""
const fs = require('fs');
exports.createSite = async ({ seed }) => {
  const pack = JSON.parse(fs.readFileSync(process.env.FAKE_PACK, 'utf8'));
  return { url: 'http://127.0.0.1:0', pack, comments: [], applyChange: async () => {}, flushLive: () => [], stop: async () => {} };
};
"""

FAKE_EMULATOR = r"""
const fs = require('fs');
const path = require('path');
exports.createEmulator = async ({ appDir, site }) => {
  const manifest = JSON.parse(fs.readFileSync(path.join(appDir, 'manifest.json'), 'utf8'));
  const log = [], bridgeLog = [];
  const sprintsOf = (b) => site.pack.sprints.filter((s) => s.state === 'active' && String(s.originBoardId) === String(b));
  const html = (spec) => {
    const x = spec.extension || {};
    if (spec.entry === 'edit') return site.pack.boards.filter((b) => b.type === 'scrum').map((b) =>
      `<button data-testid="board-option" data-board-id="${b.id}" aria-pressed="${String(((x.config) || {}).boardId) === String(b.id)}" onclick="window.__picked='${b.id}';this.setAttribute('aria-pressed','true');window.__op('updateConfig')">${b.name}</button>`).join('');
    if (x.type === 'dashboards:widget') {
      if (!x.config || !x.config.boardId) return '<div data-testid="scope-widget"><p data-testid="needs-config">Pick a board</p></div>';
      return '<div data-testid="scope-widget">' + sprintsOf(x.config.boardId).map((s) => `<section data-testid="sprint" data-sprint-id="${s.id}">
        <b data-metric="committed">1</b><b data-metric="added">0</b><b data-metric="removed">0</b><b data-metric="creep">0.0%</b></section>`).join('')
        + '<svg data-testid="chart" width="100" height="50"></svg></div>';
    }
    if (x.sprint && x.sprint.state !== 'active') return '<p data-testid="not-started">Not started</p>';
    return `<b data-metric="committed">1</b><b data-metric="added">0</b><b data-metric="removed">0</b><b data-metric="creep">0.0%</b>
      <span data-testid="hidden-count">0</span><table data-testid="ledger"><tr>${['issue','points','kind','by','at','source'].map((c) => `<th data-col="${c}">${c}</th>`).join('')}</tr>
      <tr data-change-id="1"><td data-col="issue"><a onclick="window.__op('open')">X-1</a></td><td data-col="points">1</td><td data-col="kind">added</td><td data-col="by">A</td>
      <td data-col="at"><time datetime="2026-10-01T10:00:00+00:00">x</time></td><td data-col="source">event</td></tr></table>
      <button data-testid="post-summary">Post</button><button data-testid="close" onclick="window.__op('close')">Close</button>`;
  };
  return {
    manifest, modules: (t) => manifest.modules[t] || [], log, bridgeLog, harnessMissing: [],
    build: async () => ({ functions: manifest.modules.function.map((f) => ({ key: f.key, handler: f.handler, bundled: true, loaded: true })) }),
    runScheduled: async (key) => {
      log.push({ t_virtual: 1, invocationId: 's-' + log.length, moduleType: 'scheduledTrigger', provider: 'app', method: 'GET',
                 path: '/rest/api/3/field', status: 200, scopes: { classic: ['read:jira-work'], granular: [] } });
      return { ok: true, invocationId: 's-' + log.length };
    },
    deliverProductEvent: async (change) => ({ invocations: [{ ok: true, invocationId: 't-' + change.changelogId }] }),
    deliverNext: (() => {
      let plan = null;
      return async () => {
        plan = plan ?? site.pack.live.filter((c) => !c.delivery.dropped).sort((a, b) => a.delivery.slot - b.delivery.slot);
        const c = plan.shift();
        return c ? { changelogId: c.changelogId, slot: c.delivery.slot, duplicate: false,
                     invocations: [{ ok: true, invocationId: 't-' + c.changelogId }] } : null;
      };
    })(),
    drainQueues: async () => [],
    invokeAction: async (key, inputs) => ({ ok: true, result: inputs.sprintId && site.pack.sprints.some((s) => String(s.id) === inputs.sprintId)
      ? { sprintId: inputs.sprintId } : { error: 'Unknown sprint' } }),
    openSurface: async (page, spec) => {
      await page.exposeFunction('__op', (op) => bridgeLog.push({ op }));
      bridgeLog.push({ op: 'getContext' }, { op: 'enableTheming' }, { op: 'getWidgetEditApi' });
      await page.setContent('<!doctype html><html><body>' + html(spec) + '</body></html>');
    },
    hostSave: async (page) => ({ stored: { boardId: await page.evaluate(() => window.__picked) } }),
    kvs: { snapshot: () => ({ entities: {}, keys: [] }) },
    stop: async () => {},
  };
};
"""

FAKE_LINT = "console.log(JSON.stringify({counts:{errors:0,warnings:0},problems:[],stageReached:3,stagesTotal:3}));\n"


def _node_with_playwright():
    import shutil
    import subprocess
    node = os.environ.get('GOOSE_SWARM_RENDER_NODE') or shutil.which('node')
    if not node:
        return None
    r = subprocess.run([node, str(HERE / 'forge_probe.mjs'), '--preflight'], capture_output=True, text=True)
    return node if r.returncode == 0 else None


class ProbeSmokeTests(unittest.TestCase):
    """The probe runs the whole §8.7 sequence against a thin fake of I2 and its I5 output is gradable: no row
    unavailable for a probe/scorer schema mismatch, the contract hooks the fake renders are read."""

    @unittest.skipUnless(_node_with_playwright(), 'node with playwright and chromium required')
    def test_the_probe_emits_gradable_observations(self):
        import subprocess
        node = _node_with_playwright()
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            pack = fo.synthetic_pack()
            (tmp / 'pack.json').write_text(json.dumps(pack))
            (tmp / 'repo' / 'forge' / 'site').mkdir(parents=True)
            (tmp / 'repo' / 'forge' / 'site' / 'site.cjs').write_text(FAKE_SITE)
            (tmp / 'kit' / 'lib').mkdir(parents=True)
            (tmp / 'kit' / 'bin').mkdir()
            (tmp / 'kit' / 'lib' / 'emulator.cjs').write_text(FAKE_EMULATOR)
            (tmp / 'kit' / 'bin' / 'lint.cjs').write_text(FAKE_LINT)
            app = tmp / 'app'
            golden_tree(app)
            (app / 'manifest.json').write_text(json.dumps(golden_manifest()))
            out = tmp / 'obs.json'
            r = subprocess.run([node, str(HERE / 'forge_probe.mjs'), '--app', str(app), '--kit', str(tmp / 'kit'),
                                '--seed', pack['seed'], '--out', str(out), '--shots', str(tmp / 'shots'),
                                '--repo', str(tmp / 'repo'), '--media', str(app / 'bench-media')],
                               capture_output=True, text=True,
                               env={**os.environ, 'FAKE_PACK': str(tmp / 'pack.json')}, timeout=600)
            self.assertEqual(r.returncode, 0, r.stderr[-2000:])
            obs = json.loads(out.read_text())
            self.assertEqual(obs['sectionErrors'], {}, obs['sectionErrors'])
            self.assertEqual(len(obs['lint']['runs']), 2)
            v = sf.evaluate(sf.Ctx(app, obs, pack, fixture_seed=pack['seed']))
            rows = {r['check']: r for r in v['checks']}
            self.assertEqual(v['probe_unavailable'], [r for r in v['probe_unavailable']
                                                      if r == 'l_real_packages'])  # the fake kit ships no app-modules
            self.assertTrue(obs['ui']['widget']['noConfig']['onlyNeedsConfig'])
            self.assertEqual([p['reopenPressed'] for p in obs['ui']['edit']['picks']], [['1'], ['2']])
            self.assertEqual(obs['ui']['widget']['secondInstance']['sprints'], ['11', '12'])
            self.assertTrue(obs['ui']['notStarted']['onlyNotStarted'])
            self.assertEqual(rows['u_not_started']['score'], 1)
            self.assertEqual(rows['u_modal_close']['score'], 1)
            self.assertEqual(rows['k_widget_edit_bridge']['score'], 1)
            self.assertEqual(rows['a_action_errors']['score'], 1)
            self.assertTrue((tmp / 'shots' / 'contact-sheet.png').is_file())
            media = v['media']
            self.assertEqual(media['recording'], 'graded-browser', media)
            if shutil.which(os.environ.get('BENCH_FFMPEG', 'ffmpeg')):
                self.assertEqual(media['errors'], [])
                (clip,) = media['videos']
                self.assertEqual(clip['file'], 'bench-media/forge-ui.webm')
                data = (app / clip['file']).read_bytes()
                self.assertEqual(data[:4], bytes.fromhex('1a45dfa3'))
                self.assertEqual(hashlib.sha256(data).hexdigest(), clip['sha256'])
                self.assertTrue(clip['publishable'])
                self.assertGreater(len(clip['segments']), 10)
            self.assertLess(v['score'], 0.5)


if __name__ == '__main__':
    unittest.main()
