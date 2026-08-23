from __future__ import annotations

import unittest

from cloud_sb7_recover_terminal_defect import readiness_failure, require_sealed_failure


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

    def test_production_controller_rejects_an_unsealed_failure_label(self) -> None:
        with self.assertRaisesRegex(SystemExit, "sealed descendant-cleanup incident"):
            require_sealed_failure("some other infrastructure failure")

    def test_refuses_ambiguous_or_uncarryable_siblings(self) -> None:
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
            "READY",
        )
        states["carried"]["status"] = "BUILD_RUNNING"
        disposition, reason, _ = readiness_failure(
            campaign,
            manager,
            states,
            {"affected"},
            "fixture failure",
            manager_alive=False,
        )
        self.assertEqual(disposition, "REFUSE")
        self.assertIn("no carryable build outcome", reason)

    def test_discovers_every_exact_terminal_defect_beyond_seed_set(self) -> None:
        campaign = {"status": "ATTENTION"}
        manager = {"status": "ATTENTION"}
        states = self.states()
        states["later-match"] = {
            "status": "INCOMPLETE",
            "failure": "fixture failure",
            "admitted_requests": 97,
            "provider_terminal_requests": 97,
            "budget_outstanding_request_ids": [],
        }
        disposition, _, affected = readiness_failure(
            campaign,
            manager,
            states,
            {"affected"},
            "fixture failure",
            manager_alive=False,
        )
        self.assertEqual(disposition, "READY")
        self.assertEqual(affected, {"affected", "later-match"})

    def test_never_reruns_published_or_active_entrants(self) -> None:
        campaign = {"status": "RUNNING"}
        manager = {"status": "RUNNING"}
        states = self.states()
        states["carried"].update(
            {"failure": "fixture failure", "admitted_requests": 2}
        )
        states["active"] = {"status": "BUILD_RUNNING"}
        disposition, _, affected = readiness_failure(
            campaign,
            manager,
            states,
            {"affected"},
            "fixture failure",
            manager_alive=True,
        )
        self.assertEqual(disposition, "WAIT")
        self.assertEqual(affected, {"affected"})

        campaign["status"] = "ATTENTION"
        manager["status"] = "ATTENTION"
        disposition, reason, affected = readiness_failure(
            campaign,
            manager,
            states,
            {"affected"},
            "fixture failure",
            manager_alive=False,
        )
        self.assertEqual(disposition, "REFUSE")
        self.assertIn("active", reason)
        self.assertEqual(affected, {"affected"})

    def test_waits_for_durable_manager_terminal_state_even_after_process_exit(self) -> None:
        campaign = {"status": "ATTENTION"}
        manager = {"status": "RUNNING"}
        states = self.states()
        disposition, reason, _ = readiness_failure(
            campaign,
            manager,
            states,
            {"affected"},
            "fixture failure",
            manager_alive=False,
        )
        self.assertEqual(disposition, "WAIT")
        self.assertIn("manager=RUNNING", reason)

        manager["status"] = "ATTENTION"
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


if __name__ == "__main__":
    unittest.main()
