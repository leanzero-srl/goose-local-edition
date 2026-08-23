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

    def test_seed_without_one_final_output_is_rejected(self):
        gate = MONITOR.EventGate(3, True)
        incident = gate.observe(
            self.record(
                {
                    "event": "research_pod_role_completed",
                    "role": "seed-requirement-evidence-mapper",
                    "tool_calls": 0,
                }
            )
        )
        self.assertIn("exactly one final_output", incident["reason"])

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


if __name__ == "__main__":
    unittest.main()
