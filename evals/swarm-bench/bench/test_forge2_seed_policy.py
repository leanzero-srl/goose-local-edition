"""Matched episodes use a frozen, domain-separated development and scoring schedule."""
import tempfile
import unittest
from pathlib import Path
import forge2_seed_policy as policy
import forge2_site
import score_forge2


class CommonFixtures(unittest.TestCase):
    def test_actual_dev_and_scorer_entry_points_are_repeatable_and_distinct(self):
        scoring = score_forge2._draw_seed()
        self.assertEqual(scoring, score_forge2._draw_seed())
        dev = forge2_site.draw_dev_seed(scoring)
        self.assertEqual(dev, forge2_site.draw_dev_seed(scoring))
        self.assertNotEqual(dev, scoring)
        self.assertNotIn(dev, score_forge2.scoring_seeds(scoring, dev))
        self.assertEqual(len(set(score_forge2.scoring_seeds(scoring, dev))), 3)

    def test_public_task_change_moves_both_fixture_domains(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in policy.TASK_FILES:
                path = root / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(name)
            before = [policy.site_seed(d, root) for d in ('development', 'scoring')]
            (root / policy.TASK_FILES[0]).write_text('a prospectively changed task')
            after = [policy.site_seed(d, root) for d in ('development', 'scoring')]
            self.assertTrue(all(a != b for a, b in zip(before, after)))
            self.assertNotEqual(after[0], after[1])

    def test_bad_domain_and_equal_dev_control_seed_are_loud(self):
        with self.assertRaises(ValueError): policy.site_seed('typo')
        with self.assertRaises(ValueError): forge2_site.draw_dev_seed(policy.site_seed('development'))


if __name__ == '__main__': unittest.main()
