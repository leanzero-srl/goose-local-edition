from __future__ import annotations

import copy
import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).parents[1] / "terminal_closure.py"
SPEC = importlib.util.spec_from_file_location("terminal_closure", MODULE_PATH)
assert SPEC and SPEC.loader
closure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(closure)

USAGE_MODULE_PATH = pathlib.Path(__file__).parents[1] / "usage_impairment.py"
USAGE_SPEC = importlib.util.spec_from_file_location(
    "usage_impairment_under_test", USAGE_MODULE_PATH
)
assert USAGE_SPEC and USAGE_SPEC.loader
usage = importlib.util.module_from_spec(USAGE_SPEC)
USAGE_SPEC.loader.exec_module(usage)

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


def notification_unavailable_score() -> dict[str, object]:
    score = score_payload()
    score["probe_unavailable"] = [closure.NOTIFICATION_MULTISET_CHECK]
    score["checks"][0] = {
        "check": closure.NOTIFICATION_MULTISET_CHECK,
        "tier": "R",
        "score": 0.0,
        "detail": closure.NOTIFICATION_MULTISET_UNAVAILABLE_DETAIL,
        "consequence": closure.NOTIFICATION_MULTISET_UNAVAILABLE_CONSEQUENCE,
        "unavailable": True,
    }
    return score


def notification_schema_receipt(score: dict[str, object]) -> dict[str, object]:
    return {
        "schema_version": closure.SUPPLEMENTAL_NOTIFICATION_SCHEMA_VERSION,
        "classification": "reachable-json-product-schema-mismatch",
        "method": "GET",
        "endpoint": "/notify/notifications?limit=200&offset=0",
        "reachable": True,
        "http_status": 200,
        "content_type": "application/json",
        "json_object": True,
        "observed_keys": list(closure.NOTIFICATION_SCHEMA_OBSERVED_KEYS),
        "required_keys": list(closure.NOTIFICATION_SCHEMA_REQUIRED_KEYS),
        "missing_required_keys": list(closure.NOTIFICATION_SCHEMA_REQUIRED_KEYS),
        "notifications_is_list": True,
        "response_body_bytes": 42,
        "response_body_sha256": "a" * 64,
        "notifier_source_sha256": "b" * 64,
        "probe_process_identity_sha256": "c" * 64,
        "probe_process_exit_proven": True,
        "score_sha256": closure.sha256_bytes(closure.canonical_json(score)),
        "clone_tree_sha256": "d" * 64,
        "raw_tree_sha256": "e" * 64,
        "fixture_seed": FIXTURE_SEED,
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

    def test_exact_notification_product_schema_failure_keeps_frozen_unavailable_row(self) -> None:
        score = notification_unavailable_score()
        receipt = notification_schema_receipt(score)
        closure.validate_sb7_score_payload(
            score,
            score_contract(),
            FIXTURE_SEED,
            supplemental_product_schema_failure=receipt,
        )
        self.assertTrue(score["checks"][0]["unavailable"])
        self.assertEqual(score["checks"][0]["score"], 0.0)

    def test_notification_supplement_rejects_unreachable_or_non_json(self) -> None:
        for field, value in (("reachable", False), ("json_object", False)):
            score = notification_unavailable_score()
            receipt = notification_schema_receipt(score)
            receipt[field] = value
            with self.subTest(field=field):
                with self.assertRaisesRegex(
                    closure.ClosureError, "reachable exact product mismatch"
                ):
                    closure.validate_sb7_score_payload(
                        score,
                        score_contract(),
                        FIXTURE_SEED,
                        supplemental_product_schema_failure=receipt,
                    )


class AdoptedScoreProcessProofTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.attempt = pathlib.Path(self.temporary.name)
        receipt = {
            "pid": 43210,
            "identity_sha256s": ["a" * 64],
            "birth_sha256s": ["b" * 64],
        }
        (self.attempt / "descendants.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "root_pid": 43210,
                    "updated_at": "2026-08-26T00:00:00+00:00",
                    "processes": [receipt],
                }
            )
            + "\n",
            encoding="utf-8",
        )
        (self.attempt / "spawn-journal.txt").write_text(
            json.dumps(receipt) + "\n", encoding="utf-8"
        )
        for name, pid in (("scorer.pid.json", 43210), ("worker.pid.json", 43211)):
            (self.attempt / name).write_text(
                json.dumps({"pid": pid, "identity_sha256": "c" * 64}) + "\n",
                encoding="utf-8",
            )
        (self.attempt / "scorer-state.json").write_text(
            json.dumps({"process_group_id": 43210}) + "\n", encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_all_authenticated_descendants_absent_is_accepted(self) -> None:
        with (
            mock.patch.object(closure, "process_receipt_status", return_value="absent"),
            mock.patch.object(closure, "process_group_exists", return_value=False),
            mock.patch.object(closure, "port_is_available", return_value=True),
        ):
            receipts, journal = closure.prove_adoption_process_absence(
                self.attempt, 18970
            )
        self.assertEqual(len(receipts), 1)
        self.assertEqual(len(journal), 1)

    def test_active_descendant_fails_closed_without_signalling(self) -> None:
        with (
            mock.patch.object(closure, "process_receipt_status", return_value="match"),
            mock.patch.object(closure, "process_group_exists", return_value=True),
            mock.patch.object(closure, "port_is_available", return_value=False),
            self.assertRaisesRegex(closure.ClosureError, "active or unproven"),
        ):
            closure.prove_adoption_process_absence(self.attempt, 18970)

    def test_occupied_port_fails_closed_with_absent_descendants(self) -> None:
        with (
            mock.patch.object(closure, "process_receipt_status", return_value="absent"),
            mock.patch.object(closure, "process_group_exists", return_value=False),
            mock.patch.object(closure, "port_is_available", return_value=False),
            self.assertRaisesRegex(closure.ClosureError, "active or unproven"),
        ):
            closure.prove_adoption_process_absence(self.attempt, 18970)

    def test_tampered_adoption_receipt_fails_closed(self) -> None:
        contract = {"source_clone_tree_sha256": "a" * 64}
        result = {
            "score_sha256": "b" * 64,
            "supplemental_product_schema_failure_sha256": "c" * 64,
        }
        receipt = {
            "schema_version": 1,
            "source_state_sha256": closure.sha256_bytes(
                closure.canonical_json(contract)
            ),
            "source_score_sha256": "b" * 64,
            "source_clone_tree_sha256": "a" * 64,
            "descendant_count": 30,
            "all_descendants_absent": True,
            "port_isolated": True,
            "supplemental_product_schema_failure_sha256": "c" * 64,
        }
        closure.validate_score_adoption_receipt(contract, result, receipt)
        receipt["source_score_sha256"] = "d" * 64
        with self.assertRaisesRegex(closure.ClosureError, "provenance differs"):
            closure.validate_score_adoption_receipt(contract, result, receipt)

    def test_notification_supplement_rejects_other_check(self) -> None:
        score = notification_unavailable_score()
        score["checks"][0]["check"] = "r_other"
        score["probe_unavailable"] = ["r_other"]
        receipt = notification_schema_receipt(score)
        with self.assertRaisesRegex(closure.ClosureError, "exactly one frozen check"):
            closure.validate_sb7_score_payload(
                score,
                score_contract(),
                FIXTURE_SEED,
                supplemental_product_schema_failure=receipt,
            )

    def test_notification_supplement_rejects_nonzero_row(self) -> None:
        score = notification_unavailable_score()
        score["checks"][0]["score"] = 0.1
        receipt = notification_schema_receipt(score)
        with self.assertRaisesRegex(closure.ClosureError, "row differs"):
            closure.validate_sb7_score_payload(
                score,
                score_contract(),
                FIXTURE_SEED,
                supplemental_product_schema_failure=receipt,
            )

    def test_notification_supplement_rejects_multiple_unavailable_rows(self) -> None:
        score = notification_unavailable_score()
        score["checks"][1]["unavailable"] = True
        receipt = notification_schema_receipt(score)
        with self.assertRaisesRegex(closure.ClosureError, "multiple unavailable"):
            closure.validate_sb7_score_payload(
                score,
                score_contract(),
                FIXTURE_SEED,
                supplemental_product_schema_failure=receipt,
            )


class UsageQuarantineReasonTests(unittest.TestCase):
    def quarantine(self, reason: str) -> dict[str, object]:
        admission = {
            "admission_id": "admission-15",
            "physical_host_id": "Local",
            "model_instance_id": "mihai-model",
        }
        return {
            "event": "broker_admission_quarantined",
            "run_id": RUN_ID,
            "receipt": {
                "reason": reason,
                "admission": admission,
                "unresolved": {
                    "admission": dict(admission),
                    "provider_requests_started": 1,
                    "provider_requests_terminal": 0,
                    "provider_request_pending": False,
                    "provider_turn_permit_held": True,
                    "provider_starts_closed": True,
                    "local_completion": "error",
                },
            },
        }

    def test_exact_dispatcher_wrapper_preserves_unproven_request_identity(self) -> None:
        request_id = "engine-provider-request:" + "a" * 32
        reason = (
            "provider dispatcher failed (content-retry: owned file missing) "
            "without terminal proof: outstanding provider request `"
            f"{request_id}` has no proven cancelled terminal"
        )
        identity = usage._quarantine_identity(self.quarantine(reason), RUN_ID)
        self.assertEqual(identity["provider_request_id"], request_id)

    def test_unrelated_or_loosely_similar_reason_remains_rejected(self) -> None:
        request_id = "engine-provider-request:" + "b" * 32
        reasons = (
            f"dispatcher failed: outstanding provider request `{request_id}` has no proven cancelled terminal",
            f"provider dispatcher failed (content-retry) without proof: outstanding provider request `{request_id}` has no proven cancelled terminal",
            "provider dispatcher failed (content-retry\nforged) without terminal proof: "
            f"outstanding provider request `{request_id}` has no proven cancelled terminal",
        )
        for reason in reasons:
            with self.subTest(reason=reason):
                with self.assertRaisesRegex(
                    usage.UsageEvidenceError,
                    "quarantine reason is not exact unproven-terminal evidence",
                ):
                    usage._quarantine_identity(self.quarantine(reason), RUN_ID)


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
