"""Reference qualification preserves heldout efficiency while refusing dishonest summaries."""
import copy
import unittest

import score_forge2 as s


class ReferenceAdmission(unittest.TestCase):
    def verdict(self, scores=None):
        return s.compose_from_rows(s._scenario(scores or {}))

    def test_known_good_native_shape_and_json_numeric_roundtrip(self):
        verdict = self.verdict()
        self.assertEqual(s.reference_failures(verdict), [])
        verdict['score'] = 1
        verdict['critical']['multiplier'] = 1
        self.assertEqual(s.reference_failures(verdict), [])

    def test_actual_heldout_request_efficiency_stays_scored(self):
        # Native V7 holdout2 measured 597 calls / 202 changes; only the original five fit top2.86.
        score = round(min(1, s.TH['event_economy_top'] / (597 / 202)), 4)
        self.assertEqual(score, 0.9677)
        verdict = self.verdict({'e_event_economy': score})
        row = next(r for r in verdict['checks'] if r['check'] == 'e_event_economy')
        row['parts'] = {'calls': 597, 'changes': 202, 'ratio': round(597 / 202, 4)}
        before = copy.deepcopy(verdict)
        self.assertEqual(verdict['score'], 0.9998)
        self.assertEqual(s.reference_failures(verdict), [])
        self.assertEqual(verdict, before)
        # Reconcile economy is also a continuous reward, separate from product guarantees.
        self.assertEqual(s.reference_failures(self.verdict({'e_reconcile_economy': 0.5})), [])

    def test_actual_three_site_economy_mean_remains_below_perfect(self):
        sites = [self.verdict({'e_event_economy': x})['checks'] for x in (1, 0.9933, 0.9677)]
        result = s.compose_from_rows(s.merge_seed_rows(sites, ['first', 'heldout1', 'heldout2']))
        self.assertEqual(result['score'], 0.9999)
        self.assertEqual(next(r['score'] for r in result['checks'] if r['check'] == 'e_event_economy'), 0.987)
        self.assertEqual(s.reference_failures(result), [])

    def test_every_functional_guarantee_and_ui_budget_remains_required(self):
        for name in sorted(s.REGISTERED - set(s.FITTED_TOPS.values())):
            with self.subTest(check=name):
                failures = s.reference_failures(self.verdict({name: 0.5}))
                self.assertTrue(any(name in reason for reason in failures), failures)

    def test_supplied_reference_tiny_functional_shortfall_is_not_perfection(self):
        result = self.verdict({'r8_cost_controls': 0.9999999999})
        self.assertEqual(result['score'], 1)
        self.assertTrue(any('r8_cost_controls' in reason for reason in s.reference_failures(result)))
        result = self.verdict({'e_ui_round_trips': 0.9999999999})
        self.assertTrue(any('e_ui_round_trips' in reason for reason in s.reference_failures(result)))

    def test_each_missing_summary_field_including_none_hold_is_refused(self):
        for name in s.REFERENCE_COMPOSITION_FIELDS:
            with self.subTest(field=name):
                result = self.verdict(); del result[name]
                self.assertIn('reference composition mismatch: '+name, s.reference_failures(result))

    def test_stale_or_forged_summary_fields_are_refused(self):
        changes = {
            'score': 0.7, 'rawScore': 0.7, 'inner': 0.7, 'v1': {}, 'v2': {}, 'critical': {},
            'reliability': {}, 'tiers': {}, 'admission': {}, 'excellence': {}, 'excellence_gate': False,
            'status': 'held', 'runtime': 'shim', 'publishable': False, 'hold': {'reason': 'derived'},
            'unpublishable_reasons': ['derived evidence'], 'probe_unavailable': ['invented'],
            'vacuous': {'invented': 'pretend'}, 'root_causes': {'invented': []},
        }
        for field, value in changes.items():
            with self.subTest(field=field):
                result = self.verdict(); result[field] = value
                self.assertTrue(s.reference_failures(result))
        for value in (True, False, float('nan'), float('inf'), 10**400):
            with self.subTest(score=repr(value)):
                result = self.verdict(); result['score'] = value
                self.assertIn('reference composition mismatch: score', s.reference_failures(result))
        for path in (('critical', 'multiplier'), ('reliability', 'multiplier'), ('tiers', 'R1', 'mean'),
                     ('excellence', 'fraction'), ('v1', 'score'), ('admission', 'ceiling')):
            with self.subTest(deep_boolean=path):
                result = self.verdict(); cursor = result
                for part in path[:-1]: cursor = cursor[part]
                cursor[path[-1]] = True
                self.assertIn('reference composition mismatch: '+path[0], s.reference_failures(result))

    def test_actual_unavailable_vacuous_runtime_and_held_states_refuse(self):
        for kind in ('unavailable', 'vacuous', 'shim', 'held'):
            with self.subTest(kind=kind):
                rows = s._perfect_rows()
                if kind == 'unavailable': rows[0]['unavailable'] = 'missing observation'
                if kind == 'vacuous': rows[0]['parts'] = {'vacuous_root': 'not exercised'}
                ctx = type('Context', (), {'fixture_seed': '0'*16, 'dev_seed': None,
                    'runtime': 'shim' if kind == 'shim' else 'wrapper',
                    'harness_missing': ['actual unmodeled call'] if kind == 'held' else []})()
                result = s.compose_from_rows(rows, ctx)
                self.assertTrue(s.reference_failures(result))

    def test_reference_row_weights_and_critical_classes_match_native_registry_stamps(self):
        for name in sorted(s.REGISTERED):
            for value in (0, True, '1', float('inf')):
                with self.subTest(check=name, weight=repr(value)):
                    result = self.verdict()
                    row = next(r for r in result['checks'] if r['check'] == name)
                    row['weight'] = value
                    self.assertTrue(any(name+': reference row weight' in reason
                                        for reason in s.reference_failures(result)))
            if name in s.ROW_WEIGHT:
                result = self.verdict()
                next(r for r in result['checks'] if r['check'] == name).pop('weight')
                if s.ROW_WEIGHT[name] != 1:
                    self.assertTrue(any(name+': reference row weight' in reason
                                        for reason in s.reference_failures(result)))
        for name in sorted(s.CRITICAL_OF):
            result = self.verdict()
            next(r for r in result['checks'] if r['check'] == name).pop('critical')
            self.assertTrue(any(name+': reference critical class' in reason for reason in s.reference_failures(result)))
        result = self.verdict()
        plain = next(r for r in result['checks'] if r['check'] not in s.CRITICAL_OF)
        plain['critical'] = 'duplicate'
        self.assertTrue(any(plain['check']+': reference critical class' in reason
                            for reason in s.reference_failures(result)))

    def test_retained_critical_harm_cannot_hide_behind_perfect_row_scores(self):
        for name in sorted(s.CRITICAL_OF):
            with self.subTest(critical=name):
                rows = s._perfect_rows()
                row = next(r for r in rows if r['check'] == name)
                harm = next(r for r in s._scenario({name: 0}) if r['check'] == name)
                row['critical_observations'] = [{'seed': 'prior-site', 'row': harm}]
                result = s.compose_from_rows(rows)
                self.assertEqual(row['score'], 1)
                self.assertLess(result['critical']['multiplier'], 1)
                self.assertTrue(any('observed critical harm' in reason for reason in s.reference_failures(result)))

    def test_derived_unpublishable_artifact_cannot_be_a_native_reference(self):
        result = self.verdict({'e_event_economy': 0.9677})
        result.update(status='derived', publishable=False, hold={'reason': 'stored rows, no fresh native run'},
                      unpublishable_reasons=['derived evidence'])
        self.assertTrue(s.reference_failures(result))


if __name__ == '__main__': unittest.main()
