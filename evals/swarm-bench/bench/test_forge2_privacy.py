"""Displayed LLM cache evidence is graded under its actual viewer, even after a failed earlier surface."""
import copy
import unittest
import forge2_checks as fc
import forge2_oracle as fo
import score_forge2 as s


class DisplayedPrivacy(unittest.TestCase):
    def setUp(self):
        self.pack = fo.synthetic_pack_v2()
        oracle = fo.Oracle(self.pack)
        self.obs = s.selftest_empty_observations(self.pack)
        self.obs['llm_v2'] = fc.perfect_observations(oracle)['llm_v2']
        self.case = next(c for c in self.obs['llm_v2'] if c['case'] == 'cache')
        self.cross = self.case['cross_viewer']
        self.peer = self.cross['as']
        hidden = oracle.hidden_issue_ids(self.peer)
        self.hidden = next(i for iid, i in oracle.issues.items() if iid in hidden)
        self.hidden_change = next(cid for cid in oracle.leak_terms(self.peer)['changeIds'])
        self.visible = next(i for iid, i in oracle.issues.items() if iid not in hidden)

    def privacy(self):
        ctx = s.Ctx(None, self.obs, self.pack)
        return s._run_check(ctx, *next(e for e in s.CHECKS if e[0] == 'b_no_permission_leak'))

    def test_actual_cross_viewer_hidden_key_summary_or_citation_fires(self):
        for display in ({'shown_text': self.hidden['key']}, {'shown_own': self.hidden['summary']},
                        {'ids_shown': [self.hidden_change]}):
            with self.subTest(display=display):
                old = copy.deepcopy(self.cross['click_results'])
                self.cross['click_results'][0].update(display)
                row = self.privacy()
                self.assertTrue(s.observed_defect(row), row)
                self.assertEqual(s.criticals([row])[0], .6)
                self.cross['click_results'] = old

    def test_known_display_harm_survives_unrelated_ui_or_rovo_gap(self):
        self.cross['click_results'][0]['shown_text'] = self.hidden['key']
        for section in ('ui', 'rovo'):
            self.obs['sectionErrors'] = {section: 'earlier surface failed'}
            self.assertTrue(s.observed_defect(self.privacy()))

    def test_permission_is_the_actual_viewers_not_the_first_viewers(self):
        self.cross['click_results'][0]['shown_text'] = self.hidden['key']
        self.assertTrue(s.observed_defect(self.privacy()))
        self.cross['as'] = fo.Oracle(self.pack).viewer
        self.assertFalse(s.observed_defect(self.privacy()))

    def test_visible_key_and_unknown_outside_sprint_citation_are_not_privacy_harm(self):
        self.cross['click_results'][0].update(shown_text=self.visible['key'], ids_shown=['unknown-not-hidden'])
        row = self.privacy()
        self.assertEqual(row['score'], 1, row)
        self.assertFalse(s.observed_defect(row))

    def test_undisplayed_bad_model_answer_is_not_a_display_leak(self):
        self.cross['click_results'][0]['model_calls'] = [{'returned_tools': [{'arguments': {'summary': self.hidden['key']}}]}]
        self.assertFalse(s.observed_defect(self.privacy()))

    def test_blank_idle_app_cannot_earn_privacy_credit(self):
        self.obs['llm_v2'] = []
        row = self.privacy()
        self.assertEqual(row['score'], 0)
        self.assertFalse(s.observed_defect(row))

    def test_unknown_viewer_is_missing_evidence_not_permission_to_invent_leak(self):
        del self.cross['as']
        self.cross['click_results'][0]['shown_text'] = self.hidden['key']
        row = self.privacy()
        self.assertTrue(row.get('unavailable'), row)
        self.assertFalse(s.observed_defect(row))


if __name__ == '__main__': unittest.main()
