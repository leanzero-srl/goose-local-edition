"""forge-1.0 scorer tests: the registry, the oracle, the composition and the public-text parity (DESIGN §8, §14).

The golden observations below are built from the ORACLE (what a correct app's run would leave in I5), so a
check that disagrees with the oracle on correct behaviour fails here, before any golden app or emulator
exists. Each defect test mutates one fact and asserts the rows it must cost — and the score it must land at.
"""
import copy
from decimal import Decimal
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
                  jira(4, 'b1', 'scheduled', '/rest/api/3/search/jql', method='POST',
                       body={'jql': 'updated >= -30d', 'fields': ['customfield_10020']},
                       response={'issues': [{'id': '100'}]}),
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
    for board in o.scrum_boards():
        sprints = o.sprints_of_board(board)
        for theme in ('light', 'dark'):
            for width in (380, 1180):
                vid = f'widget-view:{board}:{theme}:{width}'
                surfaces.append(_surface(vid, 'widget-view', theme, width,
                                         {'texts': [o.numbers(s).name for s in sprints]}))
                views.append({'board': board, 'theme': theme, 'width': width,
                              'sprints': [{'id': s, 'metrics': _metrics(o.numbers(s))} for s in sprints],
                              'chart': {'present': True, 'count': 1, 'rects': [
                                  {'sprintId': s, 'series': ser, 'height': float(getattr(o.numbers(s), ser)) * 4}
                                  for s in sprints for ser in ('committed', 'added', 'removed')]},
                              'overflow': {'scrollWidth': width, 'clientWidth': width}, 'sprintsVisible': True})
    for theme in ('light', 'dark'):
        surfaces.append(_surface(f'widget-edit:{theme}', 'widget-edit', theme,
                                 extra={'bridgeOps': [{'op': 'getContext'}, {'op': 'enableTheming'},
                                                      {'op': 'getWidgetEditApi'}, {'op': 'invoke'}]}))
    picks = [{'board': b, 'updateConfigCalls': 1, 'onProductSave': False, 'savedConfig': {'boardId': b},
              'viewSprints': o.sprints_of_board(b), 'reopenPressed': [b]} for b in o.scrum_boards()]
    renders, comments, invokes = [], [], []
    first = True
    for sid in o.active_sprints():
        vis = o.visible_changes(sid, viewer)
        n = o.numbers(sid)
        for theme in ('light', 'dark'):
            surfaces.append(_surface(f'sprint-action:{sid}:{theme}', 'sprint-action', theme,
                                     extra={'texts': [c.issue_key for c in vis] + [c.by_name for c in vis],
                                            'changeIdAttrs': [c.change_id for c in vis]}))
        render = {'sprintId': sid, 'metrics': _metrics(n), 'hiddenCount': str(o.hidden_count(sid, viewer)),
                  'headers': list(sf.TABLE_COLS),
                  'rows': [{'changeId': c.change_id, 'cells': {'issue': c.issue_key, 'points': fo.format_points(c.points),
                                                               'kind': c.kind, 'by': c.by_name, 'at': _iso(c.at),
                                                               'source': c.sources[0]}} for c in vis],
                  'sortAt': [{'rows': [c.change_id for c in sorted(vis, key=lambda c: (-c.at.timestamp(), fo.changelog_order(c.change_id)))],
                              'ariaSort': {'at': 'descending'}},
                             {'rows': [c.change_id for c in vis], 'ariaSort': {'at': 'ascending'}}],
                  'sortPoints': {'rows': [c.change_id for c in sorted(vis, key=lambda c: (-c.points, c.at, fo.changelog_order(c.change_id)))],
                                 'ariaSort': {'points': 'descending'}},
                  'router': [{'issueKey': c.issue_key, 'ops': [{'op': 'open', 'url': f'/browse/{c.issue_key}'}]} for c in vis[:1]],
                  'close': {'closeCalled': True}}
        if vis:
            target = vis[0]
            body = {'type': 'doc', 'version': 1, 'content': [{'type': 'paragraph', 'content': [
                {'type': 'text', 'text': f'{target.issue_key} entered {n.name}; creep {fo.format_creep(n.creep)}'}]}]}
            if first:
                comments.append({'t': 1000, 'issueKey': target.issue_key, 'provider': 'user', 'accountId': viewer,
                                 'status': 429, 'body': body, 'fault': 'f-comment'})
            comments.append({'t': 1002, 'issueKey': target.issue_key, 'provider': 'user', 'accountId': viewer,
                             'status': 201, 'body': body})
            comments.append({'t': 1010, 'issueKey': target.issue_key, 'provider': 'user', 'accountId': viewer,
                             'status': 201, 'body': body})
            comments.append({'t': 1020, 'issueKey': 'OPS-9', 'provider': 'user', 'accountId': viewer, 'status': 403, 'body': body})
            render.update({'select': {'changeId': target.change_id, 'ariaSelected': True},
                           'post': {'commentsAdded': 1, 'successFlags': 1, 'errorFlags': 0},
                           'doubleClick': {'commentsAdded': 1, 'successFlags': 1, 'errorFlags': 0},
                           'forbidden': {'issueKey': 'OPS-9', 'commentsAdded': 0, 'errorFlags': 1, 'successFlags': 0,
                                         'sortWorksAfter': True}})
            first = False
        renders.append(render)
        invokes.append({'surface': f'sprint-action:{sid}', 'functionKey': 'ledger',
                        'response': {'rows': [c.change_id for c in vis]}})
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
        'rovo': {'calls': rovo}, 'comments': comments,
        'ui': {'surfaces': surfaces, 'calls': ui_calls,
               'widget': {'noConfig': {'needsConfig': True, 'onlyNeedsConfig': True}, 'views': views,
                          'secondInstance': {'configBoard': second_board, 'sprints': o.sprints_of_board(second_board)}},
               'edit': {'options': [{'boardId': b, 'pressed': False} for b in o.scrum_boards()], 'picks': picks},
               'sprintAction': renders,
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
            obs['phases']['backfill']['calls'][3]['body'].pop('fields')
        self.assertEqual(self.rows(self.mutate(ids_only))['r_pagination']['score'], 1)

    def test_a_walk_that_stops_on_a_next_page_token_is_incomplete(self):
        def stop(obs):
            obs['phases']['backfill']['calls'][3]['response']['nextPageToken'] = 'p2'
        self.assertLess(self.rows(self.mutate(stop))['r_pagination']['score'], 1)

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

        def scripts(obs):
            obs['manifest']['permissions']['content'] = {'scripts': ['unsafe-inline']}
        self.assertEqual(self.rows(self.mutate(scripts))['l_scopes']['score'], 0.75)


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
        pinned = {'l_deployable': 0.499, 'l_bundles_load': 0.499, 'b_no_permission_leak': 0.5873,
                  'b_comment_exactly_once': 0.5873, 't_no_double_count': 0.5894, 'r_backfill_complete': 0.5894,
                  'u_widget_loads': 0.6, 't_event_rows': 0.699, 'k_dashboard_widget': 0.799, 'r_pagination': 0.899,
                  'e_reconcile_economy': 0.97, 'b_comment_adf_as_user': 0.9703, 'u_widget_numbers': 0.9758,
                  'l_lint_warnings': 0.9797, 't_reestimate_followed': 0.9824, 'u_widget_chart': 0.9844,
                  'v_widget_sizes': 0.9859, 'l_scopes': 0.9883}
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
    }
    # Every hook the contract names, and the check that probes it.
    HOOKS = {
        'board-option': 'u_widget_edit_config', 'scope-widget': 'u_widget_loads', 'needs-config': 'u_widget_edit_config',
        'sprint': 'u_widget_numbers', 'chart': 'u_widget_chart', 'not-started': 'u_not_started',
        'hidden-count': 'b_hidden_count', 'ledger': 'u_ledger_table', 'post-summary': 'b_comment_exactly_once',
        'close': 'u_modal_close', 'committed': 'u_widget_numbers', 'added': 'u_widget_numbers',
        'removed': 'u_widget_numbers', 'creep': 'u_widget_numbers', 'issue': 'u_ledger_table',
        'points': 'u_ledger_sort', 'kind': 'u_ledger_table', 'by': 'u_ledger_table', 'at': 'u_ledger_sort',
        'source': 'u_ledger_table', 'data-board-id': 'u_widget_edit_config', 'aria-pressed': 'u_widget_edit_config',
        'data-sprint-id': 'u_widget_numbers', 'data-series': 'u_widget_chart', 'data-change-id': 'u_ledger_table',
        'aria-sort': 'u_ledger_sort', 'aria-selected': 'u_comment_flow', 'get-sprint-scope': 'a_action_result',
        'sprintId': 'a_action_errors', 'hiddenChanges': 'b_hidden_count', 'creepPercent': 'a_action_result',
        'changeId': 'a_action_result', 'sprint-scope-analyst': 'k_rovo_skill', 'allowed-tools': 'k_rovo_skill',
        'view.theme.enable()': 'v_theme_tokens', '--ds-surface': 'v_dark_mode', '--ds-text': 'v_theme_tokens',
        '--ds-link': 'v_theme_tokens',
    }

    def test_registry_shape(self):
        self.assertEqual(len(sf.CHECKS), 59)
        self.assertEqual(len(sf.CRITICAL_CHECKS), 7)
        counts = {}
        for _n, t, *_ in sf.CHECKS:
            counts[t] = counts.get(t, 0) + 1
        self.assertEqual(counts, {'L': 6, 'K': 6, 'T': 8, 'R': 7, 'S': 4, 'B': 5, 'U': 10, 'V': 5, 'A': 4, 'E': 4})
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

    def test_thresholds_are_uncalibrated_until_the_freeze(self):
        data = json.loads(sf.THRESHOLDS_FILE.read_text())
        self.assertFalse(data['calibrated'])
        self.assertEqual(sf.VERSION, 'forge-1.0-rc')
        self.assertEqual(sf.CALIB_SHA256, 'TBD-AT-FREEZE')


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


# ── probe smoke test against a THIN FAKE of I2 (DESIGN §13.3: until WP1's emulator lands) ─────────────

FAKE_SITE = r"""
const fs = require('fs');
exports.createSite = async ({ seed }) => {
  const pack = JSON.parse(fs.readFileSync(process.env.FAKE_PACK, 'utf8'));
  return { url: 'http://127.0.0.1:0', pack, comments: [], applyChange: async () => {}, stop: async () => {} };
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
                                '--repo', str(tmp / 'repo')], capture_output=True, text=True,
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
            self.assertLess(v['score'], 0.5)


if __name__ == '__main__':
    unittest.main()
