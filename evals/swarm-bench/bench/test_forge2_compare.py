import copy
import unittest
from forge2_compare import compare


def build(model, episode, score):
    return {'model': model, 'episode': episode, 'engine_sha256': 'same-engine', 'settings': {'reasoning': 'medium'},
            'verdict': {'model': model, 'scorerVersion': 'forge-2.0', 'status': 'scored', 'publishable': True,
                'scorer_files_sha256': {'scorer': 'same-bytes'}, 'spec_sha256': 'same-task', 'kit_lock_sha256': 'same-kit',
                'wrapper_sha256': 'same-wrapper', 'fixture_seeds': ['a', 'b', 'c'], 'seed_scores': {'a': score, 'b': score, 'c': score},
                'score': score, 'checks': [{'check': 'native-query', 'score': score}], 'critical': {'classes': []}},
            'cost': {'status': 'complete', 'billed_usd_is_lower_bound': False, 'billed_usd': 0.5,
                     'tokens': {'prompt': 1000, 'cached': 900}}}


class IndependentComparison(unittest.TestCase):
    def test_three_fixtures_of_one_build_cannot_establish_a_direction(self):
        out = compare([build('mimo', 1, .8), build('sol', 1, .7)])
        self.assertIsNone(out['screening_direction'])
        self.assertEqual(out['models']['mimo']['independent_builds'], 1)

    def test_repeated_nonoverlapping_builds_are_only_a_screening_signal(self):
        out = compare([build('mimo', 1, .8), build('sol', 1, .7), build('mimo', 2, .82), build('sol', 2, .72)])
        self.assertEqual(out['screening_direction'], 'mimo')
        self.assertAlmostEqual(out['models']['mimo']['median'], .81)
        self.assertEqual(out['billed_usd'], 2)
        self.assertIn('not a model-population confidence interval', out['conclusion'])

    def test_overlapping_build_noise_is_unresolved(self):
        out = compare([build('mimo', 1, .8), build('sol', 1, .7), build('mimo', 2, .6), build('sol', 2, .72)])
        self.assertIsNone(out['screening_direction'])

    def test_wrong_fixture_task_settings_or_billing_cannot_enter_a_comparison(self):
        for mutation in ('task', 'seed', 'settings', 'price', 'wallet', 'rc', 'duplicate'):
            a, b = build('mimo', 1, .8), build('sol', 1, .7)
            if mutation == 'task': b['verdict']['spec_sha256'] = 'changed'
            if mutation == 'seed': b['verdict']['fixture_seeds'] = ['x', 'y', 'z']
            if mutation == 'settings': b['settings']['reasoning'] = 'different'
            if mutation == 'price': b['cost']['billed_usd_is_lower_bound'] = True
            if mutation == 'wallet': b['verdict']['agent'] = {'budget': {'stopped_by': 'wallet_guard'}}
            if mutation == 'rc': b['verdict']['scorerVersion'] = 'forge-2.0-rc'
            if mutation == 'duplicate': b = copy.deepcopy(a)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError): compare([a,b])


if __name__ == '__main__': unittest.main()
