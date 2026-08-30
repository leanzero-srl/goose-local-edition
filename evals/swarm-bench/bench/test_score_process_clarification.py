"""THE DEAD FIELD THAT FLATTERED EVERY RUN, pinned so it can never hand out credit again.

From the P1-5 rewire to cfcd32908 (2026-08-30), `low_confidence_ask` carried
open_decisions_total=0 / open_decisions_not_asked=0 UNCONDITIONALLY — the only live call site
passed breakdown=None (swarm.rs:27124-27132 vs the computation at :36704-36710). The old ruler
computed asked/(asked+not_asked) with not_asked from that dead field, so every run scored 1.0 on
asked_when_unsure regardless of how many open decisions it silently guessed. r5's truth, from
primary data (the opener's final output in the durable .swarm/activity/open.log): FIVE open
decisions, three asked (ask_max_q truncation) — the honest score is 0.6, and the old code
reported 1.0. cfcd32908 killed both the dead breakdown arg and the truncation: a post-fix engine
emits the real total/not_asked and asks EVERYTHING the opener opened, so an honest 1.0 is what a
healthy post-fix run is EXPECTED to earn here. The fixtures below pin the PRE-fix shapes so
archived runs keep scoring truthfully.

The rules pinned here:
  * dead 0/0 fields + primary present → the primary's denominator (0.6, never 1.0);
  * primary absent + total=0 → CANNOT-MEASURE, named loudly — zero is the dead field's only
    possible output, so it can never license credit (defaulting to 1.0 IS the flattery class);
  * primary absent + total NON-zero → usable evidence (a live-field engine: pre-P1-5, or
    cfcd32908 onward — bedA-1541 really carries total=5/questions=5 and has no open lane log;
    erasing its measurement would rewrite honest history);
  * primary present AND a non-zero event total → the primary wins and the disagreement is flagged.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import score_process as sp


FIVE = [
    "D1 (DECISIONS.md): brushed record on streamed mutation",
    "D2 (DECISIONS.md): rejected draft terminal or resubmittable",
    "D3 (DECISIONS.md): the third corner in section 9",
    "Python HTTP stack for ledgerd and notifierd",
    "Notifications feed interface",
]

# r5's real shape: three questions surfaced, both engine totals dead at 0.
DEAD_ASK = {"event": "low_confidence_ask", "plan_confidence": None,
            "questions": [{"question": "D1"}, {"question": "D2"}, {"question": "D3"}],
            "open_decisions_total": 0, "open_decisions_not_asked": 0}

PRIMARY = (FIVE, "primary open.log: 5 open decision(s) in the opener's output")
ABSENT = (None, "opener lane log absent (tried .swarm/activity/open.log)")


class AskedWhenUnsure(unittest.TestCase):
    def test_dead_zero_fields_with_primary_present_score_three_of_five(self):
        checks = sp.axis_clarification([DEAD_ASK], PRIMARY)
        self.assertAlmostEqual(checks["asked_when_unsure"]["score"], 0.6)
        self.assertIn("3 of 5", checks["asked_when_unsure"]["detail"])

    def test_primary_absent_and_dead_zero_is_cannot_measure_never_full_credit(self):
        checks = sp.axis_clarification([DEAD_ASK], ABSENT)
        self.assertIsNone(checks["asked_when_unsure"]["score"],
                          "a dead 0 must never become a denominator OR a free 1.0")
        self.assertIn("CANNOT-MEASURE", checks["asked_when_unsure"]["detail"])
        self.assertIn("absent", checks["asked_when_unsure"]["detail"])

    def test_a_live_event_total_disagreeing_with_the_primary_loses_and_is_flagged(self):
        live = dict(DEAD_ASK, open_decisions_total=4, open_decisions_not_asked=1)
        checks = sp.axis_clarification([live], PRIMARY)
        self.assertAlmostEqual(checks["asked_when_unsure"]["score"], 0.6)
        self.assertIn("DISAGREES", checks["asked_when_unsure"]["detail"])

    def test_primary_absent_but_nonzero_event_total_stays_measurable(self):
        """bedA-1541's real shape: total=5, questions=5, no open lane log ever written."""
        live = {"event": "low_confidence_ask", "plan_confidence": 30,
                "questions": [{"question": f"q{i}"} for i in range(5)],
                "open_decisions_total": 5, "open_decisions_not_asked": 0}
        checks = sp.axis_clarification([live], ABSENT)
        self.assertAlmostEqual(checks["asked_when_unsure"]["score"], 1.0)
        self.assertIn("live-field", checks["asked_when_unsure"]["detail"])

    def test_no_ask_stays_not_measurable(self):
        checks = sp.axis_clarification([], PRIMARY)
        self.assertIsNone(checks["asked_when_unsure"]["score"])


class OpenerPrimaryExtraction(unittest.TestCase):
    def test_extracts_the_last_open_decisions_array_from_the_durable_log(self):
        with tempfile.TemporaryDirectory() as d:
            lane = Path(d) / ".swarm" / "activity"
            lane.mkdir(parents=True)
            (lane / "open.log").write_text(
                'the model muses: "open_decisions": should list the corners\n'
                + json.dumps({"slices": [], "open_decisions": FIVE}))
            decisions, src = sp.opener_open_decisions(Path(d) / "run.jsonl")
            self.assertEqual(decisions, FIVE)
            self.assertIn("5 open decision(s)", src)

    def test_absent_log_is_a_named_absence_not_an_empty_list(self):
        with tempfile.TemporaryDirectory() as d:
            decisions, src = sp.opener_open_decisions(Path(d) / "run.jsonl")
            self.assertIsNone(decisions)
            self.assertIn("absent", src)

    def test_a_log_without_the_key_is_named_distinctly_from_an_absent_file(self):
        with tempfile.TemporaryDirectory() as d:
            lane = Path(d) / ".swarm" / "activity"
            lane.mkdir(parents=True)
            (lane / "open.log").write_text("a pre-OPEN-phase lane log with nothing relevant")
            decisions, src = sp.opener_open_decisions(Path(d) / "run.jsonl")
            self.assertIsNone(decisions)
            self.assertIn("no open_decisions key", src)


class EndToEnd(unittest.TestCase):
    def test_evaluate_reads_the_primary_from_the_run_dir(self):
        with tempfile.TemporaryDirectory() as d:
            run_log = Path(d) / "run.jsonl"
            run_log.write_text(json.dumps(DEAD_ASK) + "\n")
            lane = Path(d) / ".swarm" / "activity"
            lane.mkdir(parents=True)
            (lane / "open.log").write_text(json.dumps({"open_decisions": FIVE}))
            result = sp.evaluate(run_log, [], None)
            check = result["axes"]["clarification"]["asked_when_unsure"]
            self.assertAlmostEqual(check["score"], 0.6)


if __name__ == "__main__":
    unittest.main()
