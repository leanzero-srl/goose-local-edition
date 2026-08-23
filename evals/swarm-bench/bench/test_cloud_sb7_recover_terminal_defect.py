from __future__ import annotations

import unittest

from cloud_sb7_recover_terminal_defect import readiness_failure


class TerminalDefectRecoveryTest(unittest.TestCase):
    def states(self) -> dict[str, dict[str, object]]:
        return {
            "affected": {
                "status": "INCOMPLETE",
                "failure": "fixture failure",
                "admitted_requests": 4,
                "provider_terminal_requests": 4,
                "budget_outstanding_request_ids": [],
            },
            "carried": {"status": "PUBLISHED"},
        }

    def test_waits_for_manager_then_accepts_exact_terminal_failure(self) -> None:
        campaign = {"status": "ATTENTION"}
        manager = {"status": "ATTENTION"}
        states = self.states()
        self.assertEqual(
            readiness_failure(
                campaign,
                manager,
                states,
                {"affected"},
                "fixture failure",
                manager_alive=True,
            )[0],
            "WAIT",
        )
        self.assertEqual(
            readiness_failure(
                campaign,
                manager,
                states,
                {"affected"},
                "fixture failure",
                manager_alive=False,
            )[0],
            "READY",
        )

    def test_refuses_ambiguous_or_unpublished_siblings(self) -> None:
        campaign = {"status": "ATTENTION"}
        manager = {"status": "ATTENTION"}
        states = self.states()
        states["affected"]["provider_terminal_requests"] = 3
        self.assertEqual(
            readiness_failure(
                campaign,
                manager,
                states,
                {"affected"},
                "fixture failure",
                manager_alive=False,
            )[0],
            "REFUSE",
        )
        states = self.states()
        states["carried"]["status"] = "PUBLISH_FAILED"
        self.assertEqual(
            readiness_failure(
                campaign,
                manager,
                states,
                {"affected"},
                "fixture failure",
                manager_alive=False,
            )[0],
            "REFUSE",
        )


if __name__ == "__main__":
    unittest.main()
