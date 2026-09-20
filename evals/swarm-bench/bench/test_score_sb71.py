import copy
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path
import tempfile
import json
import score_sb71 as score


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.raw = {'score': 1.0, 'checks': [
            {'check': name, 'tier': 'X', 'score': 1, 'detail': 'observed'}
            for name in (*score.BACKEND_EXCELLENCE, *score.STRUCTURE_CHECKS, *score.QUALITY_CHECKS)], 'tiers': {},
            'harness_missing': [], 'sched_unreached': []}
        self.rows = [{'check': name, 'tier': tier, 'score': 1, 'detail': 'observed pixels'}
                     for name, tier in score.VISUAL_CHECKS.items()]

    def fail(self, name, value=0):
        rows = copy.deepcopy(self.rows)
        next(row for row in rows if row['check'] == name)['score'] = value
        return score.admit(self.raw, rows)

    def test_each_required_leg_independently_prevents_next_band(self):
        for name, ceiling in [('s_visible_surface', .599), ('s_tower_geometry', .699),
                              ('s_currency_collar', .699), ('q_payment_context', .799),
                              ('q_legible_presentation', .799), ('m_committed_event_replay', .899)]:
            with self.subTest(name=name):
                result = self.fail(name)
                self.assertEqual(result['score'], ceiling)
                self.assertLess(round(result['score'] * 100, 1), round((ceiling + .001) * 100, 1))

    def test_backend_and_visual_excellence_both_required(self):
        for name in score.BACKEND_EXCELLENCE:
            raw = copy.deepcopy(self.raw)
            next(r for r in raw['checks'] if r['check'] == name)['score'] = .99
            with self.subTest(name=name):
                self.assertEqual(score.admit(raw, self.rows)['score'], .899)
        self.assertEqual(self.fail('m_committed_event_replay')['score'], .899)
        self.assertEqual(score.admit(self.raw, self.rows)['score'], 1.0)

    def test_pretty_inspector_cannot_hide_broken_full_payment_field(self):
        for names, ceiling in [(score.STRUCTURE_CHECKS, .699), (score.QUALITY_CHECKS, .799)]:
            for name in names:
                raw = copy.deepcopy(self.raw)
                next(r for r in raw['checks'] if r['check'] == name)['score'] = 0
                with self.subTest(name=name):
                    self.assertEqual(score.admit(raw, self.rows)['score'], ceiling)

    def test_gates_never_award_credit_or_mutate_input(self):
        self.raw['score'] = .22
        before = copy.deepcopy(self.raw)
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .22)
        self.assertEqual(self.raw, before)

    def test_unavailable_probe_refuses_official_score(self):
        self.raw['probe_unavailable'] = ['t_pick_real_pass']
        with tempfile.TemporaryDirectory() as tmp, patch.object(score.base, 'evaluate', return_value=self.raw):
            with self.assertRaisesRegex(RuntimeError, 'required benchmark observations unavailable'):
                score.evaluate(SimpleNamespace(probes={}, root=Path(tmp), fixture_seed='123456789abcdef0'))
            diagnostic = json.loads((Path(tmp) / 'scoring-unavailable.json').read_text())
            self.assertNotIn('score', diagnostic)
            self.assertEqual(diagnostic['partial_checks'], self.raw['checks'])

    def test_missing_partial_and_vacuous_evidence_cannot_admit(self):
        self.assertEqual(score.admit(self.raw, [])['score'], .599)
        self.assertEqual(self.fail('s_tower_geometry', .999)['score'], .699)
        self.raw['checks'][0]['parts'] = {'vacuous_root': 'sync_completeness'}
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .899)
        self.raw['checks'][0].pop('parts')
        self.raw['sched_unreached'] = ['outbox_kill']
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .899)


if __name__ == '__main__':
    unittest.main()
