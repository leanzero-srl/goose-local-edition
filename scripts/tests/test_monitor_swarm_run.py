import importlib.util
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = pathlib.Path(__file__).parents[1] / "monitor_swarm_run.py"
RESPONSE_ONLY_RETRY_FIXTURE = (
    pathlib.Path(__file__).parent / "fixtures/response_only_schema_retry.json"
)
SPEC = importlib.util.spec_from_file_location("monitor_swarm_run", SCRIPT)
MONITOR = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MONITOR)


def activity(
    path,
    thinking,
    observed,
    repeated,
    structured_bytes=0,
    structured_active=False,
    phase="processing",
):
    value = {
        "model": "node-qwen/qwen3.8-27b",
        "phase": phase,
        "thinking_chars": thinking,
        "tool_calls": 0,
        "malformed": 0,
        "reasoning_recurrence": {
            "window_chars": 48,
            "observed_windows": observed,
            "repeated_windows": repeated,
            "repeat_share": repeated / observed if observed else 0.0,
            "earlier_reasoning": "preserved full-stream excerpt",
        },
        "provider_stream": {
            "revision": thinking,
            "bytes": thinking,
            "structured_output_bytes": structured_bytes,
            "structured_output_chunks": 1 if structured_bytes else 0,
            "structured_output_active": structured_active,
        },
    }
    raw = json.dumps(value).encode()
    return MONITOR.ActivitySample.from_bytes(path, raw)


def activity_bytes(thinking, observed, repeated):
    value = {
        "model": "node-qwen/qwen3.8-27b",
        "phase": "processing",
        "thinking_chars": thinking,
        "tool_calls": 0,
        "malformed": 0,
        "reasoning_recurrence": {
            "window_chars": 48,
            "observed_windows": observed,
            "repeated_windows": repeated,
            "repeat_share": repeated / observed,
            "earlier_reasoning": "preserved full-stream excerpt",
        },
        "provider_stream": {
            "revision": thinking,
            "bytes": thinking,
            "structured_output_bytes": 0,
            "structured_output_chunks": 0,
            "structured_output_active": False,
        },
    }
    return json.dumps(value).encode()


class RecurrenceGateTests(unittest.TestCase):
    def test_f924_shape_requires_growth_and_corroboration(self):
        gate = MONITOR.RecurrenceGate(confirmations=2)
        path = pathlib.Path("detail.json")
        self.assertFalse(gate.observe(activity(path, 9000, 8000, 3200)).incident)
        self.assertFalse(gate.observe(activity(path, 10000, 9000, 3700)).incident)
        decision = gate.observe(activity(path, 11000, 10000, 4100))
        self.assertTrue(decision.incident)
        self.assertFalse(decision.evidence["tail_reasoning_used"])
        self.assertEqual(
            decision.evidence["source"], "full-stream-reasoning-recurrence-meter"
        )

    def test_structured_output_progress_disproves_reasoning_only_incident(self):
        gate = MONITOR.RecurrenceGate(confirmations=2)
        path = pathlib.Path("seed.json")
        gate.observe(activity(path, 9000, 8000, 3200))
        first = gate.observe(activity(path, 10000, 9000, 3700, 8000, True))
        second = gate.observe(activity(path, 11000, 10000, 4100, 16000, True))
        self.assertFalse(first.incident)
        self.assertFalse(second.incident)

    def test_stale_structured_active_flag_does_not_hide_growing_recurrence(self):
        gate = MONITOR.RecurrenceGate(confirmations=1)
        path = pathlib.Path("seed.json")
        gate.observe(activity(path, 9000, 8000, 3200, 8000, True))
        decision = gate.observe(activity(path, 10000, 9000, 3700, 8000, True))
        self.assertTrue(decision.incident)

    def test_saturated_rolling_window_still_detects_recurrence(self):
        gate = MONITOR.RecurrenceGate(confirmations=1)
        path = pathlib.Path("detail.json")
        gate.observe(activity(path, 190000, 65536, 30000))
        decision = gate.observe(activity(path, 191000, 65536, 30000))
        self.assertTrue(decision.incident)

    def test_unchanged_poll_does_not_erase_corroboration(self):
        gate = MONITOR.RecurrenceGate(confirmations=2)
        path = pathlib.Path("detail.json")
        gate.observe(activity(path, 9000, 8000, 3200))
        first = activity(path, 10000, 9000, 3700)
        self.assertFalse(gate.observe(first).incident)
        self.assertFalse(gate.observe(first).incident)
        decision = gate.observe(activity(path, 11000, 10000, 4100))
        self.assertTrue(decision.incident)

    def test_healthy_v4_repeat_share_does_not_arm(self):
        gate = MONITOR.RecurrenceGate(confirmations=1)
        path = pathlib.Path("healthy.json")
        gate.observe(activity(path, 54000, 53900, 3800))
        decision = gate.observe(activity(path, 56062, 56015, 4056))
        self.assertAlmostEqual(decision.evidence["repeat_share"], 0.0724091761)
        self.assertFalse(decision.incident)


class ResponseOnlyEventGateTests(unittest.TestCase):
    def record(self, tool_calls):
        value = {
            "event": "research_pod_role_completed",
            "role": "seed-requirement-evidence-mapper",
            "partition_id": "seed-4",
            "model": "workhorse-qwen/qwen3.8-27b",
            "tool_calls": tool_calls,
            "seq": 168,
        }
        raw = json.dumps(value).encode()
        return MONITOR.EventRecord(value, 10, 10 + len(raw), "abc")

    def observe(self, activity):
        with tempfile.TemporaryDirectory() as temporary:
            activity_dir = pathlib.Path(temporary) / ".swarm" / "activity"
            activity_dir.mkdir(parents=True)
            path = activity_dir / "research-pod-seed-4:pre-scheduler:11.json"
            MONITOR.atomic_write(path, json.dumps(activity).encode())
            return MONITOR.ResponseOnlyEventGate(activity_dir).observe(
                self.record(activity["tool_calls"])
            )

    def captured_retry(self):
        return json.loads(RESPONSE_ONLY_RETRY_FIXTURE.read_text(encoding="utf-8"))

    def test_schema_rejected_attempt_before_one_success_is_accepted(self):
        activity = self.captured_retry()
        self.assertEqual(
            activity["capture_provenance"]["source_activity_sha256"],
            "51592a4d10713024e17117b9f82b201df290a9e55a16c27dd2a51af384544f09",
        )
        self.assertEqual([call["ok"] for call in activity["calls"]], [False, True])
        self.assertIsNone(self.observe(activity))

    def test_two_successful_final_outputs_are_rejected(self):
        activity = self.captured_retry()
        successful = activity["calls"][1]
        activity["calls"] = [successful, dict(successful)]
        activity["tool_calls"] = 2
        incident = self.observe(activity)
        self.assertIn("exactly one successful final_output", incident["reason"])

    def test_forbidden_tool_type_is_rejected(self):
        activity = self.captured_retry()
        activity["calls"] = [
            activity["calls"][1],
            {"name": "developer__shell", "ok": True, "is_mcp": False},
        ]
        activity["tool_calls"] = 2
        incident = self.observe(activity)
        self.assertIn("forbidden tool types", incident["reason"])

    def test_zero_successful_final_outputs_are_rejected(self):
        activity = self.captured_retry()
        activity["calls"] = [activity["calls"][0]]
        activity["tool_calls"] = 1
        incident = self.observe(activity)
        self.assertIn("exactly one successful final_output", incident["reason"])

    def test_rejected_attempt_after_success_is_rejected(self):
        activity = self.captured_retry()
        activity["calls"].reverse()
        incident = self.observe(activity)
        self.assertIn("continued after", incident["reason"])


class EventGateTests(unittest.TestCase):
    def record(self, value):
        raw = json.dumps(value).encode()
        return MONITOR.EventRecord(value, 10, 10 + len(raw), "abc")

    def test_compliant_seed_assignment_is_accepted(self):
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(
            self.record(
                {
                    "event": "research_seed_roles_assigned",
                    "available_nodes": 3,
                    "assigned_nodes": 3,
                    "all_nodes_assigned_before_first_model_call": True,
                    "coordinator_calls_started": 0,
                    "roles": [{}, {}, {}, {}, {}, {}, {}, {}, {}],
                    "initial_node_roles": [
                        {"model": "gabee"},
                        {"model": "mihai"},
                        {"model": "workhorse"},
                    ],
                }
            )
        )
        self.assertIsNone(incident)

    def test_coordinator_before_seed_merge_is_rejected(self):
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(
            self.record(
                {
                    "event": "research_pod_role_started",
                    "role": "evidence-saturation-coordinator",
                }
            )
        )
        self.assertIn("before every seed", incident["reason"])

    def test_saturation_pod_requires_seed_merge_and_distinct_initial_nodes(self):
        event = {
            "event": "research_saturation_pod_started",
            "available_nodes": 3,
            "requirements": 197,
            "partitions": 9,
            "initial_node_roles": [
                {"model": "gabee"},
                {"model": "mihai"},
                {"model": "workhorse"},
            ],
        }
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(self.record(event))
        self.assertIn("before every seed", incident["reason"])
        gate.seed_merged = True
        self.assertIsNone(gate.observe(self.record(event)))

    def test_saturation_pod_rejects_duplicate_initial_node_assignment(self):
        gate = MONITOR.EventGate(3, True)
        gate.seed_merged = True
        incident = gate.observe(
            self.record(
                {
                    "event": "research_saturation_pod_started",
                    "available_nodes": 3,
                    "requirements": 197,
                    "partitions": 9,
                    "initial_node_roles": [
                        {"model": "gabee"},
                        {"model": "gabee"},
                        {"model": "workhorse"},
                    ],
                }
            )
        )
        self.assertIn("distinct authority packet", incident["reason"])

    def test_saturation_pod_with_fewer_requirements_than_nodes_is_valid(self):
        gate = MONITOR.EventGate(3, True)
        gate.seed_merged = True
        incident = gate.observe(
            self.record(
                {
                    "event": "research_saturation_pod_started",
                    "available_nodes": 3,
                    "requirements": 2,
                    "partitions": 2,
                    "initial_node_roles": [
                        {"model": "gabee"},
                        {"model": "mihai"},
                    ],
                }
            )
        )
        self.assertIsNone(incident)

    def test_saturation_retry_must_move_to_a_distinct_node(self):
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(
            self.record(
                {
                    "event": "research_saturation_packet_reassigned",
                    "attempt": 2,
                    "model": "gabee",
                    "prior_failed_nodes": ["gabee"],
                }
            )
        )
        self.assertIn("distinct roster device", incident["reason"])

    def test_research_cannot_precede_physical_snapshot(self):
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(self.record({"event": "research_pod_started"}))
        self.assertIn("physical snapshot", incident["reason"])

    def test_seed_merge_requires_observed_three_node_occupancy(self):
        gate = MONITOR.EventGate(3, True)
        gate.seed_roles = 3
        incident = gate.observe(
            self.record(
                {"event": "research_seed_merged", "completed_node_roles": 3}
            )
        )
        self.assertIn("all nodes active concurrently", incident["reason"])

    def test_lms_snapshot_proves_three_node_seed_occupancy(self):
        gate = MONITOR.EventGate(3, True)
        gate.seed_roles = 9
        gate.observe_lms(
            {
                "ok": True,
                "models": [
                    {"status": "processing"},
                    {"status": "generating"},
                    {"status": "processing"},
                ],
            }
        )
        self.assertTrue(gate.seed_concurrency_observed)

    def test_lms_processing_prompt_status_variants_prove_seed_occupancy(self):
        for status in ("processingPrompt", "processing_prompt", "processing-prompt"):
            with self.subTest(status=status):
                gate = MONITOR.EventGate(3, True)
                gate.seed_roles = 9
                gate.observe_lms(
                    {
                        "ok": True,
                        "models": [
                            {"status": status},
                            {"status": "generating"},
                            {"status": "processingPrompt"},
                        ],
                    }
                )
                self.assertTrue(gate.seed_concurrency_observed)


class ProcessParsingTests(unittest.TestCase):
    def test_rustc_build_is_not_a_live_goose_process(self):
        output = """\
28474 /path/rustc rustc --crate-name goose_cli --extern goose=/tmp/libgoose.rlib
29000 /tmp/bin/goose /tmp/bin/goose swarm run --prompt x
"""
        processes = MONITOR.parse_goose_processes(output)
        self.assertEqual([process["pid"] for process in processes], [29000])


class EndToEndMonitorTests(unittest.TestCase):
    def test_incident_is_durably_captured_before_exact_process_is_signalled(self):
        sleep = pathlib.Path(shutil.which("sleep")).resolve()
        binary_sha = hashlib.sha256(sleep.read_bytes()).hexdigest()
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = pathlib.Path(temporary) / "run"
            activity_dir = run_dir / ".swarm" / "activity"
            activity_dir.mkdir(parents=True)
            activity_path = activity_dir / "detail.json"
            MONITOR.atomic_write(activity_path, activity_bytes(9000, 8000, 3200))
            pid_file = run_dir / "goose.pid"
            sleeper = subprocess.Popen([str(sleep), "30"])
            pid_file.write_text(str(sleeper.pid) + "\n", encoding="utf-8")
            watcher = subprocess.Popen(
                [
                    sys.executable,
                    str(SCRIPT),
                    "watch",
                    "--run-dir",
                    str(run_dir),
                    "--pid-file",
                    str(pid_file),
                    "--binary",
                    str(sleep),
                    "--sha256",
                    binary_sha,
                    "--stop-on-incident",
                    "--poll-secs",
                    "0.05",
                    "--lms-poll-secs",
                    "1000",
                    "--confirmations",
                    "2",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            try:
                heartbeat = run_dir / ".swarm-monitor" / "heartbeat"
                deadline = time.monotonic() + 5
                while not heartbeat.exists() and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertTrue(heartbeat.exists(), "monitor did not establish its baseline")
                MONITOR.atomic_write(activity_path, activity_bytes(10000, 9000, 3700))
                time.sleep(0.2)
                MONITOR.atomic_write(activity_path, activity_bytes(11000, 10000, 4100))
                time.sleep(0.2)
                MONITOR.atomic_write(activity_path, activity_bytes(12000, 11000, 4600))
                stdout, stderr = watcher.communicate(timeout=25)
                self.assertEqual(
                    watcher.returncode,
                    20,
                    (stdout.decode("utf-8", "replace"), stderr.decode("utf-8", "replace")),
                )
                sleeper.wait(timeout=5)
                incidents = list((run_dir / ".swarm-monitor" / "incidents").iterdir())
                self.assertEqual(len(incidents), 1)
                incident = incidents[0]
                self.assertTrue((incident / "CAPTURE_COMPLETE").is_file())
                self.assertTrue((incident / "manifest.sha256").is_file())
                self.assertTrue((incident / "SIGNAL_SENT.json").is_file())
                self.assertTrue(
                    (run_dir / ".swarm-monitor" / "expected-stop.json").is_file()
                )
                self.assertLessEqual(
                    (incident / "CAPTURE_COMPLETE").stat().st_mtime_ns,
                    (incident / "SIGNAL_SENT.json").stat().st_mtime_ns,
                )
                payload = json.loads((incident / "incident.json").read_text())
                self.assertEqual(
                    payload["evidence"]["source"],
                    "full-stream-reasoning-recurrence-meter",
                )
                self.assertFalse(payload["evidence"]["tail_reasoning_used"])
            finally:
                if watcher.poll() is None:
                    watcher.terminate()
                    watcher.wait(timeout=5)
                if sleeper.poll() is None:
                    sleeper.terminate()
                    sleeper.wait(timeout=5)

    def test_external_expected_stop_does_not_create_a_second_exit_incident(self):
        sleep = pathlib.Path(shutil.which("sleep")).resolve()
        binary_sha = hashlib.sha256(sleep.read_bytes()).hexdigest()
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = pathlib.Path(temporary) / "run"
            run_dir.mkdir(parents=True)
            pid_file = run_dir / "goose.pid"
            sleeper = subprocess.Popen([str(sleep), "30"])
            pid_file.write_text(str(sleeper.pid) + "\n", encoding="utf-8")
            watcher = subprocess.Popen(
                [
                    sys.executable,
                    str(SCRIPT),
                    "watch",
                    "--run-dir",
                    str(run_dir),
                    "--pid-file",
                    str(pid_file),
                    "--binary",
                    str(sleep),
                    "--sha256",
                    binary_sha,
                    "--poll-secs",
                    "0.05",
                    "--lms-poll-secs",
                    "1000",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            try:
                heartbeat = run_dir / ".swarm-monitor" / "heartbeat"
                deadline = time.monotonic() + 5
                while not heartbeat.exists() and time.monotonic() < deadline:
                    time.sleep(0.05)
                self.assertTrue(heartbeat.exists(), "monitor did not establish its baseline")

                identity = MONITOR.process_identity(sleeper.pid)
                self.assertIsNotNone(identity)
                incident = MONITOR.capture_incident(
                    run_dir,
                    run_dir / ".swarm-monitor",
                    sleeper.pid,
                    identity,
                    sleep,
                    binary_sha,
                    "operator-requested stop",
                    {"classification": "expected"},
                    run_dir / "run.jsonl",
                    run_dir / "engine-console.log",
                )
                self.assertTrue(
                    MONITOR.stop_after_capture(
                        sleeper.pid,
                        identity,
                        incident,
                        initiator="operator",
                    )
                )
                sleeper.wait(timeout=5)
                stdout, stderr = watcher.communicate(timeout=10)
                self.assertEqual(
                    watcher.returncode,
                    0,
                    (stdout.decode("utf-8", "replace"), stderr.decode("utf-8", "replace")),
                )
                incidents = list((run_dir / ".swarm-monitor" / "incidents").iterdir())
                self.assertEqual(incidents, [incident])
                events = [
                    json.loads(line)
                    for line in (run_dir / ".swarm-monitor" / "watch.jsonl")
                    .read_text(encoding="utf-8")
                    .splitlines()
                ]
                self.assertEqual(events[-1]["event"], "monitor_completed")
                self.assertEqual(events[-1]["outcome"], "expected_stop")
                self.assertEqual(
                    events[-1]["expected_stop"]["initiator"], "operator"
                )
            finally:
                if watcher.poll() is None:
                    watcher.terminate()
                    watcher.wait(timeout=5)
                if sleeper.poll() is None:
                    sleeper.terminate()
                    sleeper.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
