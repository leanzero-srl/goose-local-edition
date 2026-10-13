"""Actual retained idle accounting plus missing, partial and contradictory controls; no scoring run."""
import copy
import json
from pathlib import Path
import unittest

import forge2_checks as fc
import forge2_oracle as fo


class EmptyAccounting(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads((Path(__file__).parent / 'fixtures/forge2-idle-accounting.json').read_text())
        self.obs = copy.deepcopy(self.fixture['observation'])
        self.oracle = fo.Oracle(fo.synthetic_pack_v2())
        self.check = next(r for r in fc.ROWS if r.name == 'r2_background_share')

    def result(self):
        return self.check.fn(fc.Ev(self.obs, self.oracle))

    def test_actual_completed_noop_accounting_is_vacuous_zero_never_full_credit(self):
        self.assertEqual(self.fixture['obs_sha256'], '394bdad8a8b99217cd09d6e60c25d25f9577c05d9aa6d0f46e10aed71ae6d7ae')
        row = self.result()
        self.assertEqual(row['score'], 0)
        self.assertIn('vacuous_root', row['parts'])
        self.assertFalse(row.get('unavailable'), row)
        self.assertFalse(fc.critical_fired(row), row)

    def test_kvs_only_work_does_not_invent_spent_jira_points(self):
        self.obs['phases']['backfill']['calls'] = [{'service': 'kvs', 'status': 200}]
        self.assertFalse(self.result().get('unavailable'))
        self.assertEqual(self.result()['score'], 0)

    def test_absent_or_malformed_accounting_remains_unavailable(self):
        for section, key, value in [('rate', 'requests', None), ('rate', 'hours', None), ('clock', 'checkpoints', None), ('phases', 'live', None)]:
            with self.subTest(section=section, key=key):
                saved = copy.deepcopy(self.obs)
                self.obs[section][key] = value
                self.assertTrue(self.result().get('unavailable'), self.result())
                self.obs = saved
        del self.obs['rate']
        self.assertTrue(self.result().get('unavailable'))

    def test_nonempty_requests_without_hours_are_inconsistent_and_unavailable(self):
        self.obs['rate']['requests'] = [{'kind': 'background', 'cost': 0, 'status': 429}]
        self.assertTrue(self.result().get('unavailable'))

    def test_missing_checkpoint_or_incomplete_elapsed_world_is_unavailable(self):
        for mutate in (lambda: self.obs['clock']['checkpoints'].pop('h2'),
                       lambda: self.obs['clock']['checkpoints'].update(h6=self.obs['clock']['upgrade_t_ms']+1000),
                       lambda: self.obs['clock']['checkpoints'].update(h1=0),
                       lambda: self.obs['clock']['checkpoints'].update(final=0)):
            saved = copy.deepcopy(self.obs)
            mutate()
            self.assertTrue(self.result().get('unavailable'))
            self.obs = saved

    def test_harness_gap_section_failure_or_contradictory_jira_call_is_unavailable(self):
        for mutate in (lambda: self.obs.update(harnessMissing=['rate ledger unavailable']),
                       lambda: self.obs['sectionErrors'].update(rate='capture failed'),
                       lambda: self.obs['sectionErrors'].update(live='phase failed'),
                       lambda: self.obs['phases']['live']['calls'].append({'service':'jira', 'status':200})):
            saved = copy.deepcopy(self.obs)
            mutate()
            self.assertTrue(self.result().get('unavailable'))
            self.obs = saved

    def test_nonphase_jira_evidence_also_contradicts_empty_request_accounting(self):
        sdk_call = {'service': 'jira', 'method': 'GET', 'path': '/rest/api/3/myself', 'status': 200}
        for mutate in (lambda: self.obs['upgrade']['calls'].append(sdk_call),
                       lambda: self.obs['ui']['calls'].append(sdk_call),
                       lambda: self.obs['ui']['adminCalls'].append(sdk_call),
                       lambda: self.obs['ui'].update(llmCalls=[sdk_call]),
                       lambda: self.obs['webtriggerCalls'].append(sdk_call),
                       lambda: self.obs['rovo']['calls'].append({'calls': [sdk_call]})):
            saved = copy.deepcopy(self.obs)
            mutate()
            self.assertTrue(self.result().get('unavailable'))
            self.obs = saved

    def test_missing_or_malformed_nonphase_call_capture_cannot_prove_empty_accounting(self):
        for mutate in (lambda: self.obs.pop('upgrade'),
                       lambda: self.obs.update(ui='malformed'),
                       lambda: self.obs['ui'].pop('calls'),
                       lambda: self.obs['ui'].update(adminCalls=None),
                       lambda: self.obs['rovo'].update(calls=None),
                       lambda: self.obs['rovo']['calls'].append({'calls': None}),
                       lambda: self.obs.pop('webtriggerCalls'),
                       lambda: self.obs.pop('sectionErrors'),
                       lambda: self.obs.update(sectionErrors=None),
                       lambda: self.obs.update(sectionErrors=[]),
                       lambda: self.obs['ui']['calls'].append({'service': None}),
                       lambda: self.obs['ui']['calls'].append({'service': ''})):
            saved = copy.deepcopy(self.obs)
            mutate()
            self.assertTrue(self.result().get('unavailable'))
            self.obs = saved

    def test_known_proxy_only_refusals_remain_zero_but_forwarded_jira_calls_contradict_it(self):
        for call in ({'service': 'jira', 'provider': 'app', 'path': '/not-a-jira-resource', 'status': 404, 'op': None},
                     {'service': 'jira', 'provider': 'user', 'path': '/rest/api/3/myself', 'status': 401,
                      'needsAuthentication': True, 'op': None}):
            self.obs['phases']['live']['calls'] = [call]
            row = self.result()
            self.assertFalse(row.get('unavailable'), row)
            self.assertEqual(row['score'], 0)
            self.assertIn('vacuous_root', row['parts'])
        for call in ({'service': 'jira', 'path': '/rest/api/3/missing', 'status': 404, 'op': None},
                     {'service': 'jira', 'status': 404, 'op': None},
                     {'service': 'jira', 'path': '/not-a-resource', 'status': 404, 'op': 'GET /rest/api/3/myself'},
                     {'service': 'jira', 'provider': 'user', 'path': '/rest/api/3/myself', 'status': 401,
                      'needsAuthentication': False},
                     {'service': 'jira', 'provider': 'app', 'path': '/rest/api/3/myself', 'status': 401,
                      'needsAuthentication': True}):
            self.obs['phases']['live']['calls'] = [call]
            self.assertTrue(self.result().get('unavailable'), self.result())

    def test_normal_accounted_dosing_and_overbudget_outcomes_are_unchanged(self):
        self.obs = fc.perfect_observations(self.oracle)
        self.assertEqual(self.result()['score'], 1)
        self.obs['rate']['hours'][2]['background_used'] = 1700
        self.assertEqual(self.result()['score'], round(5/6, 4))


if __name__ == '__main__':
    unittest.main()
