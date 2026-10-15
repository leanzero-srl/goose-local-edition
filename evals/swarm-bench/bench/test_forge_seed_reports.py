"""Pure report controls. Importing this test cannot launch an app, probe or model."""
import copy
import sys
import unittest


def forbid_execution(event, _args):
    if event in {'subprocess.Popen', 'os.system', 'os.posix_spawn', 'socket.connect', 'socket.bind'}:
        raise AssertionError('No execution or network permitted: ' + event)


sys.addaudithook(forbid_execution)
import score_forge
import score_forge2


class SeedReports(unittest.TestCase):
    def test_distinct_economy_measurements_are_not_presented_as_an_aggregate(self):
        for scorer in [score_forge, score_forge2]:
            rows = [[{'check': 'economy', 'tier': 'E', 'score': score,
                      'detail': f'{calls} calls / optimum 10', 'parts': {'calls': calls}}]
                    for score, calls in [(.2, 50), (.8, 12), (.5, 20)]]
            original = copy.deepcopy(rows)
            row = scorer.merge_seed_rows(rows, ['a', 'b', 'c'])[0]
            self.assertEqual(row['score'], .5)
            self.assertEqual(row['aggregation'], 'mean')
            self.assertNotIn('50 calls', row['detail'])
            self.assertEqual([x['detail'] for x in row['seed_evidence']],
                             ['50 calls / optimum 10', '12 calls / optimum 10', '20 calls / optimum 10'])
            self.assertEqual(row['parts_seed'], 'a')
            self.assertEqual(rows, original)
            row['seed_evidence'][0]['parts']['calls'] = -1
            self.assertEqual(rows, original)

    def test_worst_correctness_and_separate_harm_evidence_are_preserved(self):
        rows = [[{'check': 'permission', 'tier': 'B', 'score': score, 'detail': detail,
                  'critical': 'leak', 'parts': parts}]
                for score, detail, parts in [(0, 'absent', {'absent_surface': 'panel'}),
                                            (.3, 'observed disclosure', {})]]
        row = score_forge2.merge_seed_rows(rows, ['a', 'b'])[0]
        self.assertEqual(row['score'], 0)
        self.assertEqual(row['score_seed'], 'a')
        self.assertEqual(row['evidence_seed'], 'b')
        self.assertEqual(row['parts_seed'], 'b')
        self.assertEqual(row['detail'], 'worst of 2 scoring sites (seed b): observed disclosure')
        self.assertEqual(len(row['critical_observations']), 1)

    def test_unavailable_economy_keeps_the_named_absence(self):
        rows = [[{'check': 'economy', 'tier': 'E', 'score': 0, 'detail': 'missing trace', 'unavailable': True}],
                [{'check': 'economy', 'tier': 'E', 'score': 1, 'detail': 'observed'}]]
        for scorer in [score_forge, score_forge2]:
            row = scorer.merge_seed_rows(rows, ['a', 'b'])[0]
            self.assertTrue(row['unavailable'])
            self.assertEqual(row['detail'], 'seed a: missing trace')


if __name__ == '__main__':
    unittest.main()
