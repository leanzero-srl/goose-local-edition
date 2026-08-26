from __future__ import annotations

import copy
import importlib.util
import json
import pathlib
import tempfile
import unittest


MODULE_PATH = pathlib.Path(__file__).parents[1] / "terminal_closure.py"
SPEC = importlib.util.spec_from_file_location("terminal_closure", MODULE_PATH)
assert SPEC and SPEC.loader
closure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(closure)

RUN_ID = "swarm-20260826-123456789"
FIXTURE_SEED = "0123456789abcdef"


def score_payload() -> dict[str, object]:
    tiers = {tier: {"mean": 0.75} for tier in sorted(closure.SB7_TIERS)}
    tier_names = sorted(closure.SB7_TIERS)
    checks = [
        {
            "check": f"fixture_check_{index:02d}",
            "tier": tier_names[index % len(tier_names)],
            "score": 0.75,
            "detail": f"fixture evidence {index}",
        }
        for index in range(91)
    ]
    nodes = {
        name: {
            "calls": 3,
            "prompt_tokens": 1200 + index,
            "completion_tokens": 300 + index,
            "prefill_tok_s": 90.5 + index,
            "decode_tok_s": 20.5 + index,
        }
        for index, name in enumerate(("gabee", "mihai", "workhorse"))
    }
    return {
        "score": 0.75,
        "inner": 0.75,
        "scorer_version": "sb-7.0-rc",
        "fixture_seed": FIXTURE_SEED,
        "calibration": "UNCALIBRATED — fixture; rc-grade only",
        "tiers": tiers,
        "checks": checks,
        "excellent": False,
        "excellence_gate": False,
        "excellence": {
            "fraction": 0.75,
            "e_mean": 0.75,
            "conditions": [
                {"name": "fixture_condition", "ok": False, "value": 0.75}
            ],
        },
        "critical": {
            "floor": 0.6,
            "multiplier": 1.0,
            "pre_severity_score": 0.75,
            "rows": [],
        },
        "solid": True,
        "probe_unavailable": [],
        "harness_missing": [],
        "sched_unreached": [],
        "telemetry": {
            "calls": 9,
            "prompt_tokens": 3603,
            "completion_tokens": 903,
            "prefill_tok_s": 91.5,
            "decode_tok_s": 21.5,
            "nodes": nodes,
        },
    }


def score_contract() -> dict[str, object]:
    return {
        "raw_scorer_version": "sb-7.0-rc",
        "check_count": 91,
        "telemetry_nodes": ["gabee", "mihai", "workhorse"],
    }


def engine_row(event: str) -> dict[str, object]:
    row: dict[str, object] = {"event": event}
    if event == "run_started":
        row["run_id"] = RUN_ID
    return row


def recurrence_monitor_rows(incident_dir: pathlib.Path) -> list[dict[str, object]]:
    return [
        {
            "event": "monitor_started",
            "recurrence_source": closure.RECURRENCE_OBSERVATION_SOURCE,
            "recurrence_share": 0.3,
            "repeated_windows": 512,
            "confirmations": 2,
            "stop_on_incident": False,
        },
        {
            "event": "incident_detected",
            "reason": closure.RECURRENCE_OBSERVATION_REASON,
            "evidence": {
                "source": closure.RECURRENCE_OBSERVATION_SOURCE,
                "tail_reasoning_used": False,
                "repeat_share": 0.31,
                "repeat_share_gate": 0.3,
                "repeated_windows": 513,
                "required_corroborations": 2,
                "corroboration_streak": 2,
                "thinking_growth": 48,
                "structured_output_growth": 0,
            },
        },
        {"event": "incident_captured", "incident_dir": str(incident_dir)},
    ]


def write_complete_capture(
    run_dir: pathlib.Path, rows: list[dict[str, object]]
) -> None:
    incident_dir = pathlib.Path(str(rows[-1]["incident_dir"]))
    incident_dir.mkdir(parents=True)
    incident = {
        "reason": closure.RECURRENCE_OBSERVATION_REASON,
        "evidence": rows[-2]["evidence"],
        "run_dir": str(run_dir.resolve()),
        "pid": 4321,
    }
    incident_path = incident_dir / "incident.json"
    incident_path.write_text(json.dumps(incident) + "\n", encoding="utf-8")
    (incident_dir / "manifest.sha256").write_text(
        f"{closure.sha256_file(incident_path)}  incident.json\n",
        encoding="utf-8",
    )
    (incident_dir / "CAPTURE_COMPLETE").write_text(
        json.dumps({"captured_at": "2026-08-26T02:02:08+00:00", "pid": 4321})
        + "\n",
        encoding="utf-8",
    )


class TerminalTransitionTests(unittest.TestCase):
    def test_frozen_overview_precedes_the_terminal_pair(self) -> None:
        rows = [
            engine_row("run_started"),
            engine_row("run_overview"),
            engine_row("complete_result"),
            engine_row("run_finished"),
        ]
        assessment = closure.terminal_completion_assessment(rows, RUN_ID)
        self.assertTrue(assessment["terminal_complete"])
        self.assertTrue(assessment["marker_order_exact"])

    def test_missing_frozen_overview_is_rejected(self) -> None:
        rows = [
            engine_row("run_started"),
            engine_row("complete_result"),
            engine_row("run_finished"),
        ]
        self.assertFalse(
            closure.terminal_completion_assessment(rows, RUN_ID)[
                "terminal_complete"
            ]
        )

    def test_every_other_terminal_transition_order_is_rejected(self) -> None:
        invalid_orders = {
            "legacy_closer_order": [
                "run_started",
                "complete_result",
                "run_overview",
                "run_finished",
            ],
            "overview_after_finished": [
                "run_started",
                "complete_result",
                "run_finished",
                "run_overview",
            ],
            "started_after_complete": [
                "complete_result",
                "run_started",
                "run_finished",
            ],
            "finished_before_complete": [
                "run_started",
                "run_finished",
                "complete_result",
            ],
            "duplicate_overview": [
                "run_started",
                "run_overview",
                "run_overview",
                "complete_result",
                "run_finished",
            ],
            "duplicate_complete": [
                "run_started",
                "run_overview",
                "complete_result",
                "complete_result",
                "run_finished",
            ],
            "duplicate_finished": [
                "run_started",
                "run_overview",
                "complete_result",
                "run_finished",
                "run_finished",
            ],
        }
        for name, events in invalid_orders.items():
            with self.subTest(name=name):
                rows = [engine_row(event) for event in events]
                self.assertFalse(
                    closure.terminal_completion_assessment(rows, RUN_ID)[
                        "terminal_complete"
                    ]
                )

    def test_terminal_identity_must_match_the_bound_run(self) -> None:
        rows = [
            engine_row("run_started"),
            engine_row("run_overview"),
            engine_row("complete_result"),
            engine_row("run_finished"),
        ]
        self.assertFalse(
            closure.terminal_completion_assessment(rows, "swarm-20260826-000000000")
            ["terminal_complete"]
        )


class RawAndHermeticScorePolicyTests(unittest.TestCase):
    def test_nonzero_raw_agent_exit_is_terminal_evidence_not_build_success(self) -> None:
        closure.validate_raw_agent_terminal(
            {"exit": 1, "timed_out": False, "secs": 9096.1}
        )

    def test_timeout_or_signal_cannot_authenticate_raw_terminal(self) -> None:
        invalid = (
            {"exit": 0, "timed_out": True, "secs": 10.0},
            {"exit": -9, "timed_out": False, "secs": 10.0},
        )
        for agent in invalid:
            with self.subTest(agent=agent):
                with self.assertRaisesRegex(
                    closure.ClosureError, "completed Goose process terminal"
                ):
                    closure.validate_raw_agent_terminal(agent)

    def test_degraded_raw_can_continue_into_clean_hermetic_scoring(self) -> None:
        raw = score_payload()
        raw["probe_unavailable"] = ["t_labels_culling"]
        raw["checks"][0].update(
            {
                "unavailable": True,
                "detail": "PROBE UNAVAILABLE: raw Playwright module unresolved",
            }
        )
        closure.validate_raw_sb7_terminal_payload(raw, score_contract(), FIXTURE_SEED)

        hermetic = score_payload()
        closure.validate_sb7_score_payload(hermetic, score_contract(), FIXTURE_SEED)

    def test_degraded_hermetic_score_cannot_reach_publication(self) -> None:
        hermetic = score_payload()
        hermetic["probe_unavailable"] = ["t_labels_culling"]
        with self.assertRaisesRegex(
            closure.ClosureError, "degraded product-probe evidence"
        ):
            closure.validate_sb7_score_payload(
                hermetic, score_contract(), FIXTURE_SEED
            )


class MonitorIncidentPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.run_dir = self.root / "run"
        self.incident_dir = (
            self.run_dir / ".swarm-monitor" / "incidents" / "recurrence-observation"
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def supervisor(self, rows: list[dict[str, object]]) -> object:
        watch = self.run_dir / ".swarm-monitor" / "watch.jsonl"
        watch.parent.mkdir(parents=True, exist_ok=True)
        watch.write_text(
            "".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8"
        )
        supervisor = object.__new__(closure.TerminalClosure)
        supervisor.run_dir = self.run_dir.resolve()
        return supervisor

    def test_clean_monitor_terminal_is_accepted(self) -> None:
        rows = [
            {"event": "monitor_started", "stop_on_incident": False},
            {"event": "monitor_completed", "outcome": "run_finished"},
        ]
        receipt = self.supervisor(rows).monitor_terminal_row(
            {"terminal_complete": True}
        )
        self.assertEqual(receipt["classification"], "clean_terminal")

    def test_captured_non_signalling_recurrence_waits_for_engine_terminal(self) -> None:
        rows = recurrence_monitor_rows(self.incident_dir)
        write_complete_capture(self.run_dir, rows)
        supervisor = self.supervisor(rows)
        with self.assertRaisesRegex(
            closure.ClosureError, "requires authenticated engine terminal"
        ):
            supervisor.monitor_terminal_row({"terminal_complete": False})
        receipt = supervisor.monitor_terminal_row({"terminal_complete": True})
        self.assertEqual(receipt["classification"], "observation_only")
        self.assertEqual(receipt["reason"], closure.RECURRENCE_OBSERVATION_REASON)
        self.assertRegex(receipt["incident_manifest_sha256"], r"^[0-9a-f]{64}$")

    def test_observation_without_a_complete_capture_is_fatal(self) -> None:
        rows = recurrence_monitor_rows(self.incident_dir)
        supervisor = self.supervisor(rows)
        with self.assertRaisesRegex(closure.ClosureError, "capture escaped"):
            supervisor.monitor_terminal_row({"terminal_complete": True})

    def test_material_incidents_are_never_reclassified_as_observations(self) -> None:
        base = recurrence_monitor_rows(self.incident_dir)
        cases: dict[str, list[dict[str, object]]] = {}

        integrity = copy.deepcopy(base)
        integrity[1]["reason"] = "event log was truncated during a live run"
        cases["integrity_reason"] = integrity

        signalled = copy.deepcopy(base)
        signalled.insert(
            -1,
            {"event": "stop_after_capture", "signalled": True},
        )
        cases["stop_requested"] = signalled

        mixed = copy.deepcopy(base)
        mixed.insert(
            -1,
            {
                "event": "incident_detected",
                "reason": "Goose pid identity changed during the run",
            },
        )
        cases["mixed_incident"] = mixed

        relabelled = copy.deepcopy(base)
        relabelled[1]["evidence"]["source"] = "event-log-integrity-gate"
        cases["relabelled_evidence"] = relabelled

        uncaptured = copy.deepcopy(base[:-1])
        cases["uncaptured_observation"] = uncaptured

        for name, rows in cases.items():
            with self.subTest(name=name):
                assessment = closure.monitor_completion_assessment(rows)
                self.assertEqual(assessment["classification"], "publication_fatal")
                with self.assertRaisesRegex(closure.ClosureError, "material incident"):
                    self.supervisor(rows).monitor_terminal_row(
                        {"terminal_complete": True}
                    )

    def test_capture_manifest_tampering_is_publication_fatal(self) -> None:
        rows = recurrence_monitor_rows(self.incident_dir)
        write_complete_capture(self.run_dir, rows)
        (self.incident_dir / "incident.json").write_text(
            json.dumps({"reason": closure.RECURRENCE_OBSERVATION_REASON}) + "\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(closure.ClosureError, "manifest hash differs"):
            self.supervisor(rows).monitor_terminal_row({"terminal_complete": True})


if __name__ == "__main__":
    unittest.main()
