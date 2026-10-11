"""Complete reference-registry admission at the real calibration API boundary."""
import copy
import unittest

import score_forge2 as sf
import test_forge2_calibration_provenance as provenance

class RegistryGuard(unittest.TestCase):
    def verdicts(self):
        return provenance.CalibrationProvenance().verdicts()

    def refused(self, change, named):
        verdicts = self.verdicts()
        for v in verdicts: change(v)
        with self.assertRaisesRegex(ValueError, 'scored registry refused:.*'+named):
            sf.calibrate(verdicts, sf.TH)
        with self.assertRaisesRegex(ValueError, 'scored registry refused:.*'+named):
            sf.reference_failures(verdicts[0])
        with self.assertRaisesRegex(ValueError, 'scored registry refused:.*'+named):
            sf.recompose(verdicts[0])

    def test_full_105_rows_are_admitted_with_correct_fit_and_composition(self):
        v = self.verdicts()
        self.assertEqual(len(v[0]['checks']), 105)
        fitted = sf.calibrate(v, sf.TH)
        self.assertEqual({key:fitted[key] for key in sf.FITTED_TOPS}, {'event_economy_top':2.5,'reconcile_economy_top':2.5})
        self.assertEqual(sf.reference_failures(v[0]), [])
        composed = sf.compose_from_rows(v[0]['checks'])
        self.assertEqual(composed['score'], 1)
        self.assertEqual(composed['critical']['multiplier'], 1)
        self.assertEqual(composed['critical']['unsuppressed'], [])

    def test_sparse_three_economy_rows_are_named_missing(self):
        self.refused(lambda v:v.update(checks=[r for r in v['checks'] if r['check'] in sf.CALIBRATION_OWNED]), 'missing:')

    def test_every_missing_registered_row_is_named_including_missing_economy(self):
        for name in sorted(sf.REGISTERED):
            with self.subTest(name=name):
                self.refused(lambda v:v.update(checks=[r for r in v['checks'] if r['check'] != name]), 'missing: '+name)

    def test_every_duplicate_registered_row_is_named(self):
        for name in sorted(sf.REGISTERED):
            with self.subTest(name=name):
                self.refused(lambda v:v['checks'].append(copy.deepcopy(next(r for r in v['checks'] if r['check']==name))), 'duplicate: '+name)

    def test_unknown_addition_and_replacement_are_named(self):
        self.refused(lambda v:v['checks'].append({'check':'invented','tier':'E','score':1}), 'unknown: invented')
        self.refused(lambda v:v['checks'][0].update(check='invented'), 'unknown: invented')

    def test_every_named_wrong_tier_is_refused(self):
        for name in sorted(sf.REGISTERED):
            with self.subTest(name=name):
                self.refused(lambda v:next(r for r in v['checks'] if r['check']==name).update(tier='invented'), 'tier mismatch: '+name)

    def test_malformed_checks_and_unnamed_rows_are_refused(self):
        for value in (None, {}, 'checks', [None], [{'score':1}], [{'check':3,'tier':'E','score':1}]):
            with self.subTest(value=value):
                self.refused(lambda v:v.update(checks=value), 'checks must be')

        for name in sorted(sf.REGISTERED):
            for value in (float('nan'), float('inf'), -float('inf'), True, False, None, -0.01, 1.01, 10**400, '1'):
                with self.subTest(name=name, invalid_score=repr(value)):
                    self.refused(lambda v:next(r for r in v['checks'] if r['check']==name).update(score=value), 'invalid score: '+name)
            with self.subTest(name=name, missing_score=True):
                self.refused(lambda v:next(r for r in v['checks'] if r['check']==name).pop('score'), 'invalid score: '+name)

    def test_complete_failed_reference_remains_failure_not_accepted(self):
        v = self.verdicts()
        target = next(r for r in v[0]['checks'] if r['tier']=='R8')
        target['score']=0.5
        self.assertTrue(any(target['check'] in reason for reason in sf.reference_failures(v[0])))
        with self.assertRaisesRegex(ValueError, 'calibration refused'):
            sf.calibrate(v, sf.TH)

    def test_foreign_scorer_family_is_refused_before_any_valid_recomposition(self):
        native = self.verdicts()[0]
        native['scorerVersion'] = 'unrelated-1.0'
        with self.assertRaisesRegex(ValueError, 'the verdict was scored by'):
            sf.recompose(native)

if __name__ == '__main__':
    unittest.main()
