"""Independent R10 mutations: wrong membership, stale shared cache and disclosed hidden IDs."""
import copy
import unittest

import forge2_jql_checks as checks


class Oracle:
    def jql_issue_ids(self, sprint_id, account_id, t_ms, applied, operator='in'):
        all_ids = {'1', '2', '3'}
        matching = set() if sprint_id in ('42', '43') else {'1', '2'} | ({'3'} if t_ms >= 2000 else set())
        hidden = self.hidden_issue_ids(account_id)
        return (all_ids - matching if operator == 'not in' else matching) - hidden

    def jql_deferred_issue_ids(self, *arguments):
        return set()

    def hidden_issue_ids(self, account_id, mark=None):
        return {'2'} if account_id == 'viewer' else set()


def fixture():
    module = {'key': 'scope-changed', 'name': 'scopeChanged', 'types': ['issue'], 'operators': ['in', 'not in'],
              'arguments': [{'name': 'sprintId', 'required': True}], 'function': 'scope-changed'}
    manifest = {'modules': {'jira:jqlFunction': [module], 'function': [{'key': 'scope-changed', 'handler': 'index.scopeChanged'}]}}
    def case(kind, sprint='41', operator='in', account='viewer', at=1000, **changes):
        ids = sorted(Oracle().jql_issue_ids(sprint, account, at, [], operator))
        return {'kind': kind, 'sprintId': sprint, 'operator': operator, 'accountId': account, 't_ms': at,
                'applied': [], 'precomputationId': f'pre-{sprint}-{operator}', 'invoked': kind not in ('cache', 'refresh', 'shared-user'),
                'issue_ids': ids, 'error': None, 'expanded_jql': 'native clause', 'search_ok': True, **changes}
    cases = [case('initial'), case('initial', operator='not in'), case('cache'),
             case('shared-user', account='peer'), case('refresh', at=2000), case('empty', sprint='42'),
             case('future', sprint='43'), case('closed', at=2000),
             case('invalid', sprint='bad', error='A numeric sprint ID is required.', issue_ids=[], search_ok=False)]
    return {'manifest': manifest, 'jql': {'cases': cases}, 'clock': {'checkpoints': {'h1': 1000, 'h4': 2000}}}


def scores(obs):
    return {row['check']: row for row in checks.evaluate(obs, Oracle())}


class Controls(unittest.TestCase):
    def test_positive_control_reaches_every_category(self):
        rows = scores(fixture())
        self.assertTrue(all(row['score'] == 1 for row in rows.values()), rows)
        self.assertIsNone(rows['r10_shared_permissions']['critical'])

    def test_empty_refusal_does_not_pass_empty_or_future(self):
        obs = fixture()
        for case in obs['jql']['cases']:
            case.update(issue_ids=[], error='not implemented', search_ok=False)
        rows = scores(obs)
        self.assertEqual(rows['r10_initial_membership']['score'], 0)
        self.assertEqual(rows['r10_empty_and_closed']['score'], 0)

    def test_broken_cache_and_stale_refresh_fail_with_correct_initial_search(self):
        obs = fixture()
        next(c for c in obs['jql']['cases'] if c['kind'] == 'cache')['invoked'] = True
        refresh = next(c for c in obs['jql']['cases'] if c['kind'] == 'refresh')
        refresh['issue_ids'] = ['1']
        rows = scores(obs)
        self.assertLess(rows['r10_initial_membership']['score'], 1)
        self.assertEqual(rows['r10_precomputation_refresh']['score'], 0)

    def test_viewer_filtered_precomputation_fails_peer_without_false_critical(self):
        obs = fixture()
        shared = next(c for c in obs['jql']['cases'] if c['kind'] == 'shared-user')
        shared['issue_ids'] = ['1']
        row = scores(obs)['r10_shared_permissions']
        self.assertEqual(row['score'], 0)
        self.assertIsNone(row['critical'])
        self.assertTrue(row['detail'].startswith('JQL disclosure not observed: '))
        self.assertEqual(row['parts']['critical_observed'], False)

    def test_observed_hidden_issue_is_critical_even_when_other_jql_cases_fail(self):
        obs = fixture()
        shared = next(c for c in obs['jql']['cases'] if c['kind'] == 'shared-user')
        shared.update(accountId='viewer', issue_ids=['1', '2'])
        next(c for c in obs['jql']['cases'] if c['kind'] == 'initial')['error'] = 'broken'
        row = scores(obs)['r10_shared_permissions']
        self.assertEqual(row['score'], 0)
        self.assertTrue(row['critical'])
        self.assertTrue(row['detail'].startswith('JQL disclosure observed: '))
        self.assertEqual(row['parts']['observed_hidden_issue_ids'], ['2'])

    def test_missing_probe_is_unavailable_and_absent_module_cannot_earn(self):
        obs = fixture()
        obs['jql']['cases'] = [c for c in obs['jql']['cases'] if c['kind'] != 'refresh']
        self.assertTrue(scores(obs)['r10_precomputation_refresh']['unavailable'])
        obs['manifest']['modules'].pop('jira:jqlFunction')
        self.assertTrue(all(row['score'] == 0 for row in scores(obs).values()))

    def test_wrong_complement_or_duplicate_results_fail(self):
        obs = fixture()
        next(c for c in obs['jql']['cases'] if c['operator'] == 'not in')['issue_ids'] = ['1']
        next(c for c in obs['jql']['cases'] if c['kind'] == 'cache')['issue_ids'] = ['1', '1']
        self.assertLess(scores(obs)['r10_initial_membership']['score'], 1)


if __name__ == '__main__':
    unittest.main()
