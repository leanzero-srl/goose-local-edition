from __future__ import annotations

import datetime as dt
import json
import os
import tempfile
import unittest
from pathlib import Path

import quality_observer_aggregate as aggregate


UTC = dt.timezone.utc
NOW = dt.datetime(2026, 8, 24, 21, 10, tzinfo=UTC)


def tick(at: str, seq: int, active_calls: list[dict] | None = None) -> dict:
    return {
        "at": at,
        "goose_pid": 55195,
        "goose_alive": True,
        "last_seq": seq,
        "lifecycle": {
            "permits": seq,
            "terminal": seq - 3,
            "released": seq - 3,
            "active_provider_requests": 3,
            "unreleased_admissions": 3,
        },
        "progress": {
            "jury_units_completed": seq,
            "jury_pairs_completed": seq // 2,
            "adjudication_events": 0,
            "citation_events": 0,
            "corrections_started": 1,
            "corrected_packets_completed": 0,
            "material_gaps_in_completed_packets": 0,
        },
        "active_calls": active_calls or [],
        "recent_corrections": [],
    }


def active(
    activity: str,
    share: float,
    repeated: int,
    thinking_growth: int,
    repeated_growth: int,
) -> dict:
    return {
        "activity": activity,
        "role": "jury-1",
        "model": "model-a",
        "thinking_chars": 1000,
        "thinking_vs_completed_median": 1.2,
        "elapsed_vs_completed_median": 0.9,
        "completed_baseline_n": 4,
        "structured_output_bytes": 0,
        "tool_calls": 1,
        "tool_names": ["recipe__final_output"],
        "errors": 0,
        "malformed": 0,
        "recurrence_share": share,
        "recurrence_repeated_windows": repeated,
        "growth": {
            "thinking_chars": thinking_growth,
            "structured_output_bytes": 0,
            "recurrence_repeated_windows": repeated_growth,
        },
    }


class MorningAggregateTests(unittest.TestCase):
    def test_filters_local_day_and_ignores_first_seen_growth(self) -> None:
        first = active("task-a", 0.01, 10, 1000, 10)
        second = active("task-a", 0.02, 12, 20, 2)
        ticks = [
            tick("2026-08-24T20:59:59+00:00", 1, [first]),
            tick("2026-08-24T21:00:01+00:00", 2, [first]),
            tick("2026-08-24T21:00:31+00:00", 3, [second]),
        ]
        result = aggregate.build_morning_aggregate(
            ticks, {"bytes": 1}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        self.assertEqual(result["ticks"]["count"], 2)
        role = result["same_role_baselines"][0]
        self.assertEqual(role["observed_growth"]["thinking_chars"], 20)
        self.assertEqual(role["observed_growth"]["delta_samples"], 1)

    def test_two_confirmed_recurrence_samples_are_proven(self) -> None:
        rows = [
            active("task-a", 0.31, 1100, 20, 30),
            active("task-a", 0.32, 1150, 20, 50),
        ]
        ticks = [
            tick("2026-08-24T21:00:01+00:00", 2, [rows[0]]),
            tick("2026-08-24T21:00:31+00:00", 3, [rows[1]]),
        ]
        result = aggregate.build_morning_aggregate(
            ticks, {}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        self.assertEqual(result["quality_classification"], "proven")
        self.assertEqual(result["same_role_baselines"][0]["classification"], "proven")

    def test_output_whitelist_does_not_copy_secret_or_raw_argv(self) -> None:
        row = active("task-a", 0.0, 0, 0, 0)
        value = tick("2026-08-24T21:00:01+00:00", 2, [row])
        value["prompt"] = "sk_test_should_not_escape"
        value["raw_argv"] = ["--token", "sk_test_should_not_escape"]
        value["recent_corrections"] = [
            {
                "seq": 7,
                "partition_id": "partition-a",
                "compiler_error": "sk_test_should_not_escape",
            }
        ]
        value["recent_terminal_acceptances"] = [
            {
                "terminal_seq": 8,
                "request_key": {
                    "ordinal": 0,
                    "provider_request_id": "sk_test_should_not_escape",
                },
                "activity": "sk_test_should_not_escape",
                "physical_host_id": "Local",
                "model": "sk_test_should_not_escape",
                "role": "jury-1",
                "terminal_kind": "finished",
                "successful_final_output_calls": 1,
                "accepted": True,
                "errors": 0,
                "malformed": 0,
            }
        ]
        result = aggregate.build_morning_aggregate(
            [value],
            {"unknown": "sk_test_should_not_escape"},
            "Europe/Bucharest",
            dt.date(2026, 8, 25),
            NOW,
        )
        encoded = json.dumps(result)
        self.assertNotIn("sk_test_should_not_escape", encoded)
        self.assertNotIn("raw_argv", encoded)
        self.assertNotIn("compiler_error", encoded)

    def test_schema_two_fields_and_correction_audit_are_aggregated(self) -> None:
        row = active("task-a", 0.0, 0, 0, 0)
        row.pop("role")
        row["semantic_role"] = "jury-2"
        row["physical_host_id"] = "WorksMacStudio.lan"
        row["broker_role"] = "research-target-jury"
        row["provider_request_key"] = {
            "ordinal": 19,
            "provider_request_id": "request-19",
        }
        row["tool_call_names"] = ["recipe__final_output"]
        row["structured_stagnation_secs"] = 41.5
        row["thinking_stagnation_secs"] = 8.0
        row["growth"]["recurrence_observed_windows"] = 11
        value = tick("2026-08-24T21:00:01+00:00", 2, [row])
        value["schema_version"] = 2
        value["classification"] = "observation"
        value["lifecycle"]["active_provider_requests"] = 1
        value["lifecycle"]["unreleased_admissions"] = 1
        value["correction_audits"] = [
            {
                "started_seq": 12,
                "outcome_seq": 19,
                "partition_id": "target-section-ab12",
                "pass": 1,
                "physical_host_id": "WorksMacStudio.lan",
                "correction": 1,
                "compiler_error_sha256": "a" * 64,
                "outcome": "accepted",
                "correction_duration_secs": 12.5,
                "total_packet_duration_secs": 32.5,
                "material_gaps": 0,
                "ledger_corrections": 1,
            }
        ]
        value["open_hosts"] = ["WorksMacStudio.lan"]
        value["open_request_keys"] = [row["provider_request_key"]]
        value["recent_terminal_acceptances"] = [
            {
                "terminal_seq": 11,
                "request_key": {"ordinal": 18, "provider_request_id": "request-18"},
                "activity": "task-completed",
                "physical_host_id": "Local",
                "model": "model-a",
                "role": "jury-1",
                "terminal_kind": "finished",
                "successful_final_output_calls": 1,
                "accepted": True,
                "errors": 0,
                "malformed": 0,
            }
        ]
        result = aggregate.build_morning_aggregate(
            [value], {}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        role = result["same_role_baselines"][0]
        self.assertEqual(role["role"], "jury-2")
        self.assertEqual(role["physical_hosts"], ["WorksMacStudio.lan"])
        self.assertEqual(role["provider_request_keys"], ["19:request-19"])
        self.assertEqual(role["tool_names"], ["recipe__final_output"])
        self.assertEqual(role["stagnation_seconds_max"]["structured"], 41.5)
        self.assertEqual(result["latest_open_join"]["hosts"], ["WorksMacStudio.lan"])
        self.assertTrue(result["latest_open_join"]["request_count_matches_lifecycle"])
        self.assertEqual(result["terminal_acceptances"]["accepted"], 1)
        self.assertEqual(
            result["terminal_acceptances"]["final_output_cardinality_violations"], 0
        )
        self.assertEqual(result["corrections"]["outcome_counts"], {"accepted": 1})
        self.assertEqual(
            result["corrections"]["correction_duration_secs_total"], 12.5
        )

    def test_untrusted_identifiers_are_hashed(self) -> None:
        row = active("task-a", 0.0, 0, 0, 0)
        row["role"] = "sk_test_should_not_escape"
        value = tick("2026-08-24T21:00:01+00:00", 2, [row])
        result = aggregate.build_morning_aggregate(
            [value], {}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        encoded = json.dumps(result)
        self.assertNotIn("sk_test_should_not_escape", encoded)
        self.assertTrue(result["same_role_baselines"][0]["role"].startswith("sha256:"))

    def test_empty_window_fails_loudly(self) -> None:
        with self.assertRaisesRegex(ValueError, "no observer ticks"):
            aggregate.build_morning_aggregate(
                [], {}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
            )

    def test_reader_reports_malformed_and_pending_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "ticks.jsonl"
            source.write_bytes(b'{"at":"2026-08-24T21:00:01+00:00"}\nnot-json\n{"at":')
            ticks, metadata = aggregate.read_tick_prefix(source)
        self.assertEqual(len(ticks), 1)
        self.assertEqual(metadata["malformed_lines"], 1)
        self.assertEqual(metadata["pending_bytes"], 6)

    def test_atomic_output_is_private_and_deterministic(self) -> None:
        value = tick("2026-08-24T21:00:01+00:00", 2, [active("task-a", 0, 0, 0, 0)])
        one = aggregate.build_morning_aggregate(
            [value], {"sha256": "abc"}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        two = aggregate.build_morning_aggregate(
            [value], {"sha256": "abc"}, "Europe/Bucharest", dt.date(2026, 8, 25), NOW
        )
        self.assertEqual(one, two)
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "aggregate.json"
            old_umask = os.umask(0o077)
            try:
                aggregate.atomic_json(destination, one)
            finally:
                os.umask(old_umask)
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)
            self.assertEqual(json.loads(destination.read_text()), one)


if __name__ == "__main__":
    unittest.main()
