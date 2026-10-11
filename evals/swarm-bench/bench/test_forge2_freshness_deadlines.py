"""The same bounded recovery deadline governs custom fields and native JQL membership."""
import copy
import unittest
from datetime import timedelta

import forge2_jql_checks as checks
import forge2_oracle as fo


def fixture(hour=4, dropped=True, older_membership=False, hidden=False, deleted=False):
    pack = fo.synthetic_pack_v2()
    pack['world']['events'] = []
    issue = copy.deepcopy(next(i for i in pack['issues'] if i['id'] == '106'))
    issue.update(id='900', key='OPS-900', hiddenFrom=['u-viewer'] if hidden else [], commentForbiddenFor=[])
    issue['fields'][pack['sprintFieldId']] = [11] if older_membership else []
    pack['issues'].append(issue)
    mark = fo.instant(pack['now']) + timedelta(hours=hour)
    change = {'changelogId': '9401', 'issueId': issue['id'],
              'created': (mark - timedelta(minutes=5)).isoformat(), 'authorId': 'u-ana',
              'items': [{'field': 'Sprint', 'fieldtype': 'custom', 'fieldId': pack['sprintFieldId'],
                         'from': '11' if older_membership else '', 'fromString': '',
                         'to': '' if older_membership else '11', 'toString': ''}],
              'delivery': {'slot': None if dropped else 11, 'duplicates': [], 'dropped': dropped}}
    pack['live'].append(change)
    pack['live'].sort(key=lambda event: event['created'])
    if older_membership:
        older = copy.deepcopy(change)
        older.update(changelogId='9400', created='2026-09-22T11:00:00.000Z')
        older['items'][0].update(**{'from': '', 'to': '11'})
        older.pop('delivery')
        pack['history'].append(older)
    if deleted:
        pack['world']['events'].append({'id': 'deleted-900', 'class': 'issue-delete',
                                       'at': (mark - timedelta(minutes=3)).isoformat(),
                                       'issueId': issue['id'], 'issueKey': issue['key'], 'authorId': 'u-ana'})
    oracle = fo.Oracle(pack)
    applied = {cp: sum(fo.instant(event['created']) <= oracle.checkpoint_mark(cp) for event in pack['live'])
               for cp in fo.CHECKPOINTS if cp != 'final'}
    applied['final'] = len(pack['live'])
    return oracle, applied


def observation(oracle, applied, checkpoint, actual=None, operator='in', account='u-viewer', wall=None,
                kind='refresh', invoked=0):
    def case(cp, case_kind, calls):
        at = fo.epoch_ms(oracle.checkpoint_mark(cp))
        return {'kind': case_kind, 'sprintId': '11', 'accountId': account, 'operator': operator,
                't_ms': at, 'applied': applied[cp], 'issue_ids': sorted(oracle.jql_issue_ids('11', account, at, applied[cp], operator)),
                'invoked': calls, 'precomputationId': 'native-pre-11-' + operator,
                'search_ok': True, 'expanded_jql': 'issue in (...)', 'error': None}
    first = case('h1', 'initial', 1)
    queried = case(checkpoint, kind, invoked)
    if actual is not None:
        queried['issue_ids'] = actual
    return {'jql': {'cases': [first, queried]},
            'clock': {'checkpoints': {cp: fo.epoch_ms(oracle.checkpoint_mark(cp))
                                       for cp in fo.CHECKPOINTS if cp != 'final'}},
            'field': {'applied_by_checkpoint': applied}, 'rate': {'wall': wall}}


def grade(oracle, obs, group='r10_precomputation_refresh'):
    return checks.check_group(group, obs['jql']['cases'], oracle, obs)


class Deadlines(unittest.TestCase):
    def test_lost_event_new_membership_can_heal_next_hour_but_not_later(self):
        oracle, applied = fixture()
        for operator in ('in', 'not in'):
            at4 = fo.epoch_ms(oracle.checkpoint_mark('h4'))
            self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at4, applied['h4'], 'h4', applied), {'900'})
            now = oracle.jql_issue_ids('11', 'u-viewer', at4, applied['h4'], operator)
            stale = now - {'900'} if operator == 'in' else now | {'900'}
            self.assertEqual(grade(oracle, observation(oracle, applied, 'h4', sorted(stale), operator))['score'], 1)
            at5 = fo.epoch_ms(oracle.checkpoint_mark('h5'))
            self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at5, applied['h5'], 'h5', applied), set())
            late = grade(oracle, observation(oracle, applied, 'h5', sorted(stale), operator))
            self.assertEqual(late['score'], 0, late)
            self.assertIn('900', late['detail'])
            self.assertEqual(grade(oracle, observation(oracle, applied, 'h5', sorted(
                oracle.jql_issue_ids('11', 'u-viewer', at5, applied['h5'], operator)), operator))['score'], 1)

    def test_recent_lost_event_never_excuses_older_membership_for_same_issue(self):
        oracle, applied = fixture(older_membership=True)
        at = fo.epoch_ms(oracle.checkpoint_mark('h4'))
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied), set())
        expected = oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4'])
        self.assertIn('900', expected)
        row = grade(oracle, observation(oracle, applied, 'h4', sorted(expected - {'900'})))
        self.assertEqual(row['score'], 0, row)

    def test_unrelated_no_event_world_change_does_not_excuse_delivered_membership(self):
        for world_class in ('sprint-close', 'estimation-field'):
            oracle, applied = fixture(dropped=False)
            h4 = oracle.checkpoint_mark('h4')
            event = {'id': 'unrelated-world', 'class': world_class,
                     'at': (h4 - timedelta(minutes=3)).isoformat(), 'boardId': 1}
            if world_class == 'sprint-close':
                event['sprintId'] = 11
            else:
                event.update(fromFieldId='customfield_10016', toFieldId='customfield_10028')
            oracle.pack['world']['events'].append(event)
            oracle.pack['live'].append({'changelogId': '9402', 'issueId': '900',
                                       'created': (h4 - timedelta(minutes=1)).isoformat(), 'authorId': 'u-ana',
                                       'items': [{'field': 'summary', 'fieldtype': 'jira', 'fieldId': 'summary',
                                                  'from': None, 'fromString': 'Before', 'to': None, 'toString': 'After'}],
                                       'delivery': {'slot': 12, 'dropped': False, 'duplicates': []}})
            oracle.pack['live'].sort(key=lambda change: change['created'])
            oracle = fo.Oracle(oracle.pack)
            applied['h4'] = applied['h5'] = applied['h6'] = applied['final'] = len(oracle.pack['live'])
            at = fo.epoch_ms(h4)
            self.assertIn('900', oracle.freshness_deadline_windows('h4', applied), world_class)
            self.assertNotIn('900', oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied))
            for operator in ('in', 'not in'):
                expected = oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4'], operator)
                stale = expected - {'900'} if operator == 'in' else expected | {'900'}
                row = grade(oracle, observation(oracle, applied, 'h4', sorted(stale), operator))
                self.assertEqual(row['score'], 0, (world_class, operator, row))

    def test_dropped_estimate_change_does_not_excuse_delivered_sprint_membership(self):
        oracle, applied = fixture(dropped=False)
        h4 = oracle.checkpoint_mark('h4')
        oracle.pack['live'].append({'changelogId': '9402', 'issueId': '900',
                                   'created': (h4 - timedelta(minutes=3)).isoformat(), 'authorId': 'u-ana',
                                   'items': [{'field': 'Story Points', 'fieldtype': 'custom',
                                              'fieldId': 'customfield_10016', 'from': '13', 'to': '14'}],
                                   'delivery': {'slot': None, 'dropped': True, 'duplicates': []}})
        oracle.pack['live'].sort(key=lambda change: change['created'])
        oracle = fo.Oracle(oracle.pack)
        applied['h4'] = applied['h5'] = applied['h6'] = applied['final'] = len(oracle.pack['live'])
        at = fo.epoch_ms(h4)
        self.assertIn('900', oracle.freshness_deadline_windows('h4', applied))
        self.assertNotIn('900', oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied))
        stale = oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4']) - {'900'}
        self.assertEqual(grade(oracle, observation(oracle, applied, 'h4', sorted(stale)))['score'], 0)

    def test_each_delivered_contribution_establishes_membership_despite_another_lost_change(self):
        for first_dropped in (False, True):
            oracle, applied = fixture(dropped=first_dropped)
            h4 = oracle.checkpoint_mark('h4')
            oracle.pack['live'].append({'changelogId': '9402', 'issueId': '900',
                                       'created': (h4 - timedelta(minutes=3)).isoformat(), 'authorId': 'u-ana',
                                       'items': [{'field': 'Sprint', 'fieldtype': 'custom', 'fieldId': oracle.pack['sprintFieldId'],
                                                  'from': '11', 'to': ''}],
                                       'delivery': {'slot': 12 if first_dropped else None,
                                                    'dropped': not first_dropped, 'duplicates': []}})
            oracle.pack['live'].sort(key=lambda change: change['created'])
            oracle = fo.Oracle(oracle.pack)
            applied['h4'] = applied['h5'] = applied['h6'] = applied['final'] = len(oracle.pack['live'])
            at = fo.epoch_ms(h4)
            self.assertIn('900', oracle.freshness_deadline_windows('h4', applied))
            self.assertNotIn('900', oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied))
            for operator in ('in', 'not in'):
                expected = oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4'], operator)
                stale = expected - {'900'} if operator == 'in' else expected | {'900'}
                row = grade(oracle, observation(oracle, applied, 'h4', sorted(stale), operator))
                self.assertEqual(row['score'], 0, (first_dropped, operator, row))

    def test_native_quota_wall_carries_only_one_lost_event_deadline(self):
        oracle, applied = fixture(hour=2)
        h2 = oracle.checkpoint_mark('h2')
        wall = {'t_ms': fo.epoch_ms(h2 - timedelta(minutes=10)), 'until_ms': fo.epoch_ms(h2 + timedelta(minutes=10))}
        at3 = fo.epoch_ms(oracle.checkpoint_mark('h3'))
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at3, applied['h3'], 'h3', applied, wall), {'900'})
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at3, applied['h3'], 'h3', applied), set())
        reset_at_mark = {**wall, 'until_ms': fo.epoch_ms(h2)}
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at3, applied['h3'], 'h3', applied, reset_at_mark), set())
        at4 = fo.epoch_ms(oracle.checkpoint_mark('h4'))
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at4, applied['h4'], 'h4', applied, wall), set())

    def test_allocation_exhaustion_does_not_extend_a_delivered_change(self):
        oracle, applied = fixture(dropped=False)
        at = fo.epoch_ms(oracle.checkpoint_mark('h4'))
        allocation = {'background_used': 1680, 'background_cap': 1680}
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied, allocation), set())
        stale = oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4']) - {'900'}
        row = grade(oracle, observation(oracle, applied, 'h4', sorted(stale), wall=allocation))
        self.assertEqual(row['score'], 0, row)

    def test_live_change_under_native_wall_has_no_lost_event_carry(self):
        oracle, applied = fixture(dropped=False)
        h4 = oracle.checkpoint_mark('h4')
        wall = {'t_ms': fo.epoch_ms(h4 - timedelta(minutes=10)), 'until_ms': fo.epoch_ms(h4 + timedelta(minutes=10))}
        at4 = fo.epoch_ms(h4)
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at4, applied['h4'], 'h4', applied, wall), {'900'})
        at5 = fo.epoch_ms(oracle.checkpoint_mark('h5'))
        self.assertEqual(oracle.jql_deferred_issue_ids('11', 'u-viewer', at5, applied['h5'], 'h5', applied, wall), set())

    def test_hidden_or_deleted_ids_and_duplicates_are_still_failures(self):
        for reason, options in (('hidden', {'hidden': True}), ('deleted', {'deleted': True})):
            oracle, applied = fixture(**options)
            at = fo.epoch_ms(oracle.checkpoint_mark('h4'))
            self.assertNotIn('900', oracle.jql_deferred_issue_ids('11', 'u-viewer', at, applied['h4'], 'h4', applied))
            actual = sorted(oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4']) | {'900'})
            row = grade(oracle, observation(oracle, applied, 'h4', actual, kind='shared-user'), 'r10_shared_permissions')
            self.assertEqual(row['score'], 0, (reason, row))
            self.assertEqual(bool(row.get('critical')), reason == 'hidden', row)
        oracle, applied = fixture()
        at = fo.epoch_ms(oracle.checkpoint_mark('h4'))
        actual = sorted(oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4'])) + ['900']
        row = grade(oracle, observation(oracle, applied, 'h4', actual))
        self.assertEqual(row['score'], 0, row)

    def test_deferral_cannot_excuse_broken_native_cache_identity_or_transport(self):
        oracle, applied = fixture()
        at = fo.epoch_ms(oracle.checkpoint_mark('h4'))
        stale = sorted(oracle.jql_issue_ids('11', 'u-viewer', at, applied['h4']) - {'900'})
        for changes in ({'invoked': 1}, {'precomputationId': 'different-native-id'}, {'search_ok': False}, {'error': 'query failed'}):
            obs = observation(oracle, applied, 'h4', stale)
            obs['jql']['cases'][1].update(changes)
            self.assertEqual(grade(oracle, obs)['score'], 0, changes)


if __name__ == '__main__':
    unittest.main()
