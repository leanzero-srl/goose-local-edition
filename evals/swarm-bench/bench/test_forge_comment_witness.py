import unittest

from forge_comment_witness import comment_duplicate_witness


def observation(post=1, double=0):
    return {'runtimePublishable': True, 'sectionErrors': {},
            'ui': {'sprintAction': [{'post': {'commentsAdded': post},
                                     'doubleClick': {'commentsAdded': double}}]}}


class CommentWitnessTests(unittest.TestCase):
    def test_failed_action_has_no_duplicate_witness(self):
        result = comment_duplicate_witness(observation())
        self.assertIs(result['observed'], False)
        self.assertEqual([r['commentsAdded'] for r in result['scenarios']], [1, 0])

    def test_actual_duplicate_still_has_a_witness(self):
        self.assertIs(comment_duplicate_witness(observation(double=2))['observed'], True)

    def test_missing_or_refused_evidence_is_not_a_negative(self):
        for value in [None, {}, {'ui': {}}, observation(double=None), observation(double=True)]:
            self.assertIsNone(comment_duplicate_witness(value)['observed'])
        incomplete = observation(); incomplete['runtimePublishable'] = False
        self.assertIsNone(comment_duplicate_witness(incomplete)['observed'])
        failed = observation(); failed['sectionErrors'] = {'ui': 'unavailable'}
        self.assertIsNone(comment_duplicate_witness(failed)['observed'])

    def test_positive_harm_survives_a_separate_missing_scenario(self):
        value = observation(double=2); del value['ui']['sprintAction'][0]['post']
        result = comment_duplicate_witness(value)
        self.assertIs(result['observed'], True)
        self.assertFalse(result['complete'])


if __name__ == '__main__':
    unittest.main()
