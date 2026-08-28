"""THE ENGINE MAY RETRACT ITS OWN GREEN, AND THE SCORER MUST READ THE RETRACTION.

`complete_result_revised` shipped with NO consumer anywhere — a grep across crates/, ui/desktop/src
and evals/ returned only the emit site in swarm.rs. So this scorer read `verified` off the original
claim and reported `verified=True` for a run whose own engine had already printed
"NOT VERIFIED - dead code shipped". The demote is ON by default (SwarmConfig::default
unwired_demotes_verified) and `review: true` ships in the user config, so that was the DEFAULT
reading of an ordinary run.

The second test is the one that matters for the ruler: a run with no retraction must score EXACTLY
as it did before this change. A new check that hands out a free 1.0 would silently re-grade every
archived run, which is a different kind of dishonesty from the one being fixed.
"""

from __future__ import annotations

import unittest

import score_process as sp


CLAIM = {"event": "complete_result", "passed": True, "verified": True, "remaining_findings": 0}
FINISH = {"event": "run_finished", "phases": {"total_min": 12}}
REVISED = {"event": "complete_result_revised", "verified": False,
           "reason": "unwired-module-unfixed", "evidence": ["kanban/db.py"]}


class FinalClaim(unittest.TestCase):
    def test_no_revision_leaves_the_claim_alone(self):
        self.assertEqual(sp.final_claim([CLAIM, FINISH])["verified"], True)

    def test_a_revision_overrides_verified_and_never_touches_passed(self):
        merged = sp.final_claim([CLAIM, REVISED, FINISH])
        self.assertEqual(merged["verified"], False)
        self.assertEqual(merged["passed"], True)
        self.assertEqual(merged["revised_reason"], "unwired-module-unfixed")
        self.assertEqual(merged["revised_evidence"], ["kanban/db.py"])

    def test_no_claim_at_all_is_still_none(self):
        self.assertIsNone(sp.final_claim([FINISH]))


class DeliveryAxis(unittest.TestCase):
    def test_the_honesty_line_reports_the_retracted_value(self):
        checks = sp.axis_delivery([CLAIM, REVISED, FINISH], 0.95)
        self.assertIn("verified=False", checks["claim_was_honest"]["detail"])

    def test_a_retraction_scores_zero_and_names_the_dead_module(self):
        checks = sp.axis_delivery([CLAIM, REVISED, FINISH], 0.95)
        self.assertEqual(checks["claim_stood"]["score"], 0.0)
        self.assertIn("kanban/db.py", checks["claim_stood"]["detail"])

    def test_a_run_that_never_retracted_is_not_measured_here(self):
        """The ruler is unchanged for every run recorded before the demote existed."""
        checks = sp.axis_delivery([CLAIM, FINISH], 0.95)
        self.assertIsNone(checks["claim_stood"]["score"])
        scored = {k for k, c in checks.items() if c["score"] is not None}
        self.assertNotIn("claim_stood", scored,
                         "an un-fired retraction check must never contribute a score")
        self.assertEqual(scored, {"run_finished", "phase_timings", "claim_was_honest"},
                         "exactly the checks this axis scored before the retraction was folded in")


if __name__ == "__main__":
    unittest.main()
