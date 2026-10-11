"""Control-catalog and judge proofs; no emulator, browser, model call or scoring subprocess."""
from contextlib import redirect_stdout
from io import StringIO
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import forge2_controls as controls


def verdict(scores, classes=(), unavailable=(), missing=()):
    return {'score': 0.8, 'status': 'scored',
            'checks': [{'check': name, 'score': score, 'detail': f'actual {score}', 'parts': {}}
                       for name, score in scores.items()],
            'critical': {'unsuppressed': list(classes), 'classes': list(classes)},
            'probe_unavailable': list(unavailable), 'harness_missing': list(missing)}


class JudgeTests(unittest.TestCase):
    def setUp(self):
        self.golden = verdict({'primary': 1.0, 'secondary': 1.0, 'other': 1.0})
        self.expect = {'loses': ['primary'], 'may_lose': ['secondary'], 'critical': False,
                       'why': 'A skipped history page also omits its dependent person-view row on affected seeds.'}

    def test_named_secondary_loss_is_optional_and_reported_only_when_observed(self):
        primary_only = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 1.0})
        with_secondary = verdict({'primary': 0.0, 'secondary': 0.7, 'other': 1.0})
        self.assertEqual(controls.judge(self.golden, primary_only, self.expect, {}), [])
        self.assertEqual(controls.loss_evidence(self.golden, primary_only, self.expect), {})
        self.assertEqual(controls.judge(self.golden, with_secondary, self.expect, {}), [])
        proof = controls.loss_evidence(self.golden, with_secondary, self.expect)['secondary']
        self.assertEqual(proof['cause'], self.expect['why'])
        self.assertEqual((proof['golden']['score'], proof['mutant']['score']), (1.0, 0.7))
        self.assertEqual(proof['mutant']['detail'], 'actual 0.7')

    def test_optional_loss_cannot_replace_required_loss_or_authorize_unrelated_loss(self):
        secondary_only = verdict({'primary': 1.0, 'secondary': 0.0, 'other': 1.0})
        fails = controls.judge(self.golden, secondary_only, self.expect, {})
        self.assertTrue(any('declared losses that did not happen' in fail for fail in fails))
        unrelated = verdict({'primary': 0.0, 'secondary': 0.5, 'other': 0.0})
        fails = controls.judge(self.golden, unrelated, self.expect, {})
        self.assertTrue(any("undeclared losses: ['other']" in fail for fail in fails))

    def test_optional_whitelist_requires_cause_and_registered_names(self):
        mutant = verdict({'primary': 0.0, 'secondary': 0.0, 'other': 1.0})
        fails = controls.judge(self.golden, mutant, {**self.expect, 'why': ''}, {})
        self.assertTrue(any('documented causal explanation' in fail for fail in fails))
        self.assertTrue(any('undeclared losses' in fail for fail in fails))
        fails = controls.judge(self.golden, mutant, {**self.expect, 'may_lose': ['typo']}, {})
        self.assertTrue(any('unregistered checks' in fail for fail in fails))

    def test_missing_rows_and_unavailable_runtime_cannot_pass(self):
        mutant = verdict({'primary': 0.0, 'other': 1.0}, unavailable=['secondary'], missing=['GET /unknown'])
        fails = controls.judge(self.golden, mutant, self.expect, {})
        self.assertTrue(any('omits registered checks' in fail for fail in fails))
        self.assertTrue(any('unavailable rows' in fail for fail in fails))
        self.assertTrue(any('harness_missing' in fail for fail in fails))

    def test_one_declared_critical_cannot_hide_an_unrelated_critical(self):
        mutant = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 1.0}, classes=['duplicate', 'leak'])
        fails = controls.judge(self.golden, mutant, {**self.expect, 'critical': 'duplicate'}, {})
        self.assertTrue(any("unexpected critical classes ['leak']" in fail for fail in fails))


class FamilyCoverageTests(unittest.TestCase):
    def test_only_passing_required_loss_in_owned_family_covers_that_family(self):
        rows = {'primary': 'R1', 'optional': 'R2', 'jql': 'R10'}
        entries = {
            'legacy_incidental': {'family': None, 'lost': ['primary'], 'required_losses': ['primary'], 'fails': []},
            'optional_neighbor': {'family': 'R1', 'lost': ['optional'], 'required_losses': [], 'fails': []},
            'wrong_family': {'family': 'R2', 'lost': ['primary'], 'required_losses': ['primary'], 'fails': []},
            'failed': {'family': 'R10', 'lost': ['jql'], 'required_losses': ['jql'], 'fails': ['wrong harm']},
        }
        self.assertEqual(controls.family_gaps(entries, rows, ('R1', 'R2', 'R10')), ['R1', 'R2', 'R10'])
        entries['real'] = {'family': 'R1', 'lost': ['primary', 'optional'],
                           'required_losses': ['primary'], 'fails': []}
        self.assertEqual(controls.family_gaps(entries, rows, ('R1', 'R2', 'R10')), ['R2', 'R10'])

    def test_all_three_control_sources_are_loaded_with_explicit_modern_families(self):
        candidates = controls.candidates(include_legacy=True)
        self.assertEqual({candidate['origin'] for candidate in candidates},
                         {'v1-regression', 'v2-catalog', 'native-jql'})
        self.assertEqual({candidate['family'] for candidate in candidates if candidate['family']},
                         {f'R{i}' for i in range(1, 11)})
        catalog = json.loads(controls.FAMILY_CATALOG.read_text())
        self.assertEqual({entry['name'] for entry in catalog},
                         {candidate['name'] for candidate in candidates if candidate['origin'] == 'v2-catalog'})
        self.assertEqual(len(candidates), len({candidate['name'] for candidate in candidates}))

    def test_default_gate_contains_only_13_actual_modern_family_controls(self):
        candidates = controls.candidates()
        self.assertEqual(len(candidates), 13)
        self.assertEqual({entry['origin'] for entry in candidates}, {'v2-catalog', 'native-jql'})

    def test_catalog_missing_a_family_is_rejected_before_running_any_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            catalog = directory / 'MUTANTS.json'
            catalog.write_text(json.dumps([{'name': 'r1', 'family': 'R1', 'patch': 'r1.patch',
                                            'loses': ['row'], 'why': 'Only this one family exists.'}]))
            (directory / 'r1.patch').write_text('not applied in catalog discovery')
            with self.assertRaisesRegex(RuntimeError, 'no registered controls'):
                controls.candidates(controls.MUTANTS, catalog)


class CommonPathTests(unittest.TestCase):
    def test_catalog_mutant_reaches_the_same_scoring_and_judging_loop(self):
        import score_forge2
        calls = []

        def score(tree, seed, out):
            calls.append(tree.name)
            if tree.name in ('starter_untouched', 'one_function'):
                return {'score': 0.0, 'status': 'scored', 'checks': [], 'critical': {}}
            return verdict({'r5_nonadmin_refused': 0.0 if tree.name == 'r5_save_unguarded' else 1.0},
                           classes=['admin'] if tree.name == 'r5_save_unguarded' else [])

        with patch.object(score_forge2, '_kit', return_value=({'app_modules': None}, None)), \
                patch.object(score_forge2, 'reference_failures', return_value=[]), \
                patch.object(controls, '_rebuild', return_value=None), patch.object(controls, 'score', side_effect=score), \
                redirect_stdout(StringIO()):
            result = controls.main(['--seed', '0123456789abcdef', '--only', 'r5_save_unguarded'])
        self.assertEqual(result, 0)
        self.assertEqual(calls, ['golden', 'r5_save_unguarded', 'starter_untouched', 'one_function'])

    def test_preparation_never_scores(self):
        import score_forge2
        with patch.object(score_forge2, '_kit', return_value=({'app_modules': None, 'kit_dir': '/unused'}, None)), \
                patch.object(controls, '_rebuild', return_value=None), \
                patch.object(controls, 'offline_lint', return_value={'errors': [], 'warnings': []}), \
                patch.object(controls, 'score', side_effect=AssertionError('preparation called scorer')), \
                redirect_stdout(StringIO()):
            result = controls.main(['--prepare-families', '--only', 'r5_save_unguarded'])
        self.assertEqual(result, 0)


if __name__ == '__main__':
    unittest.main()
