"""Controls for worst-site grading retaining actual harm from every scoring site."""
import copy
import unittest
import score_forge2 as s


def row(score, observed=False, **kw):
    return dict(check='r10_shared_permissions', tier='R10', critical='leak', score=score,
                detail='JQL disclosure observed: hidden OPS-SECRET' if observed else 'JQL disclosure not observed: no members',
                parts={'critical_observed': observed}, **kw)


class CrossSiteHarm(unittest.TestCase):
    def merge(self, rows):
        return s.merge_seed_rows([[r] for r in rows], ['site' + str(i) for i in range(len(rows))])[0]

    def test_equal_score_missing_members_cannot_erase_disclosure(self):
        out = self.merge([row(0), row(0, True)])
        self.assertTrue(s.observed_defect(out))
        self.assertTrue(out['detail'].split(': ', 1)[1].startswith('JQL disclosure observed:'))
        self.assertEqual(out['critical_observations'][0]['seed'], 'site1')
        self.assertEqual(s.criticals([out])[0], .6)

    def test_lower_absent_score_cannot_erase_observed_harm(self):
        absent = row(0); absent['parts']['absent_surface'] = True
        out = self.merge([absent, row(.5, True)])
        self.assertEqual(out['score'], 0)
        self.assertEqual(out['score_seed'], 'site0')
        self.assertTrue(s.observed_defect(out))
        self.assertEqual(out['critical_observations'][0]['row']['score'], .5)

    def test_unavailable_stays_a_gap_and_does_not_erase_known_harm(self):
        out = self.merge([row(0, unavailable=True), row(.5, True)])
        self.assertTrue(out['unavailable'])
        self.assertTrue(s.observed_defect(out))
        self.assertEqual(s.criticals([out])[0], .6)

    def test_actual_observations_are_preserved_but_one_class_priced_once(self):
        source = [row(.5, True), row(.3, True), row(1)]
        before = copy.deepcopy(source)
        out = self.merge(source)
        self.assertEqual(out['score'], .3)
        self.assertEqual(len(out['critical_observations']), 2)
        self.assertEqual(s.criticals([out])[0], .6)
        self.assertEqual(source, before)

    def test_numeric_failure_without_measured_disclosure_is_not_a_critical(self):
        out = self.merge([row(0), row(.3), row(1)])
        self.assertFalse(s.observed_defect(out))
        self.assertEqual(s.criticals([out])[0], 1)
        self.assertEqual(out['score'], 0)

    def test_site_order_does_not_change_numeric_score_or_critical(self):
        a, b = self.merge([row(0), row(.5, True)]), self.merge([row(.5, True), row(0)])
        self.assertEqual(a['score'], b['score'])
        self.assertEqual(s.criticals([a])[0], s.criticals([b])[0])

    def test_mismatched_site_registries_are_not_silently_zip_merged(self):
        with self.assertRaises(ValueError): s.merge_seed_rows([[row(1)], []], ['a', 'b'])
        with self.assertRaises(ValueError): s.merge_seed_rows([[row(1)], [row(1)]], ['a', 'a'])


if __name__ == '__main__': unittest.main()
