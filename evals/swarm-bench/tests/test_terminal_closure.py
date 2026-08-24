from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).parents[1] / "terminal_closure.py"
SPEC = importlib.util.spec_from_file_location("terminal_closure", MODULE_PATH)
assert SPEC and SPEC.loader
closure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(closure)

MODELS = [
    "gabee-qwen3.8-27b-brainwaves-1m-qx86-hi-mlx",
    "mihai-qwen3.8-27b-brainwaves-1m-qx86-hi-mlx",
    "workhorse-qwen3.8-27b-brainwaves-1m-qx86-hi-mlx",
]


def sha(path: pathlib.Path) -> str:
    return closure.sha256_file(path)


def fixture_score(seed: str = "0123456789abcdef") -> dict:
    tiers = {}
    tier_names = sorted(closure.SB7_TIERS)
    for tier in tier_names:
        tiers[tier] = {"mean": 0.75}
    checks = [
        {
            "check": f"fixture_check_{index:02d}",
            "tier": tier_names[index % len(tier_names)],
            "score": 0.75,
            "detail": f"fixture evidence {index}",
        }
        for index in range(91)
    ]
    telemetry_nodes = {
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
        "fixture_seed": seed,
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
        "telemetry": {
            "calls": 9,
            "prompt_tokens": 3603,
            "completion_tokens": 903,
            "prefill_tok_s": 91.5,
            "decode_tok_s": 21.5,
            "nodes": telemetry_nodes,
        },
    }


class TerminalClosureTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def config(self, live_root: pathlib.Path, state_dir: pathlib.Path) -> dict:
        publisher = self.root / "publisher.mjs"
        package_lock = self.root / "package-lock.json"
        package_json = self.root / "package.json"
        if not publisher.exists():
            publisher.write_text("export {};\n", encoding="utf-8")
        if not package_lock.exists():
            package_lock.write_text("{}\n", encoding="utf-8")
        if not package_json.exists():
            package_json.write_text("{}\n", encoding="utf-8")
        node = pathlib.Path(sys.executable).resolve()
        return {
            "schema_version": 1,
            "controller_sha256": "0" * 64,
            "live_root": str(live_root),
            "run_dir": str(live_root / "swarm-3node-qwen38-brainwaves-r0"),
            "state_dir": str(state_dir),
            "score_lock_path": str(self.root / "score.lock"),
            "poll_seconds": 0.01,
            "seal_settle_seconds": 0,
            "max_score_attempts": 2,
            "score_timeout_seconds": 30,
            "max_publish_attempts": 2,
            "publish_timeout_seconds": 30,
            "expected": {
                "vendor_port": 18970,
                "entrant": "swarm-3node-qwen38-brainwaves",
                "raw_scorer_version": "sb-7.0-rc",
                "check_count": 91,
                "telemetry_nodes": ["gabee", "mihai", "workhorse"],
                "run_id": "swarm-fixture",
                "models": MODELS,
                "candidate_commit": "1" * 40,
                "launch_sha256": "2" * 64,
                "instrument_manifest_sha256": "3" * 64,
                "binary_sha256": "4" * 64,
                "instrument_files": {},
            },
            "publication": {
                "target_document_id": "brun-fleet-qwen38-brainwaves-sb70",
                "protected_document_ids": [
                    "brun-fleet-qwen38-sb70",
                    "brun-fleet-qwen-sb70",
                ],
            },
            "publisher": {
                "path": str(publisher),
                "sha256": sha(publisher),
                "node": str(node),
                "node_sha256": sha(node),
                "site_root": str(self.root),
                "env_file": str(self.root / ".env.test"),
                "base_url": "https://example.test",
                "package_lock": str(package_lock),
                "package_lock_sha256": sha(package_lock),
                "package_json": str(package_json),
                "package_json_sha256": sha(package_json),
            },
            "runtime": {"lsof": "/usr/sbin/lsof"},
        }

    def write_terminal_fixture(self, *, seed: str = "0123456789abcdef") -> tuple[pathlib.Path, pathlib.Path, dict]:
        live_root = self.root / "live"
        run_dir = live_root / "swarm-3node-qwen38-brainwaves-r0"
        monitor_dir = run_dir / ".swarm-monitor"
        monitor_dir.mkdir(parents=True)
        state_dir = self.root / "closure-state"
        config = self.config(live_root, state_dir)
        score = fixture_score(seed)
        score.update(
            {
                "entrant": config["expected"]["entrant"],
                "rep": 0,
                "vendor_port": 18970,
                "actual_pool": MODELS,
                "actual_nodes": 3,
                "agent": {"exit": 0, "timed_out": False, "secs": 42.5},
            }
        )
        started = {
            "event": "run_started",
            "run_id": "swarm-fixture",
            "ts": "2026-08-25T10:00:00+00:00",
        }
        finished = {"event": "run_finished", "ts": "2026-08-25T10:01:00+00:00"}
        (run_dir / "run.jsonl").write_text(
            json.dumps(started) + "\n" + json.dumps(finished) + "\n",
            encoding="utf-8",
        )
        monitor_secret = "fixture-monitor-secret-should-never-persist"
        (monitor_dir / "watch.jsonl").write_text(
            json.dumps(
                {
                    "event": "monitor_completed",
                    "outcome": "run_finished",
                    "ps_line": f"goose --api-key {monitor_secret}",
                }
            )
            + "\n",
            encoding="utf-8",
        )
        (run_dir / "verdict.json").write_text(json.dumps(score), encoding="utf-8")
        (live_root / f"{config['expected']['entrant']}.json").write_text(
            json.dumps([score]), encoding="utf-8"
        )
        (live_root / "harness.log").write_text("fixture harness terminal\n", encoding="utf-8")
        launch = {"harness": {"pid": 101}, "goose": {"pid": 102}, "monitor": {"pid": 103}}
        return live_root, state_dir, {"config": config, "launch": launch, "secret": monitor_secret}

    def test_terminal_fixture_reuses_exact_seed_without_persisting_monitor_argv(self) -> None:
        _live, state, fixture = self.write_terminal_fixture()
        config_path = self.root / "config.json"
        config_path.write_text(json.dumps(fixture["config"]), encoding="utf-8")
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        evidence = supervisor.terminal_evidence(fixture["launch"])
        self.assertEqual(evidence["fixture_seed"], "0123456789abcdef")
        self.assertFalse(evidence["harness_exit"]["exit_code_observable"])
        persisted = (state / "terminal-evidence.json").read_text(encoding="utf-8")
        self.assertNotIn(fixture["secret"], persisted)
        self.assertNotIn("ps_line", persisted)
        self.assertEqual(stat.S_IMODE((state / "terminal-evidence.json").stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(state.stat().st_mode), 0o700)

    def test_terminal_fixture_rejects_non_exact_seed(self) -> None:
        _live, _state, fixture = self.write_terminal_fixture(seed="not-a-seed")
        config_path = self.root / "config.json"
        config_path.write_text(json.dumps(fixture["config"]), encoding="utf-8")
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        with self.assertRaisesRegex(closure.ClosureError, "16-hex fixture_seed"):
            supervisor.terminal_evidence(fixture["launch"])

    def test_atomic_receipts_and_events_are_private(self) -> None:
        state = self.root / "private"
        receipt = state / "receipt.json"
        closure.atomic_json(receipt, {"ok": True})
        events = closure.DurableEvents(state / "events.jsonl")
        events.emit("fixture", count=1)
        events.handle.close()
        self.assertEqual(stat.S_IMODE(state.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(receipt.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE((state / "events.jsonl").stat().st_mode), 0o600)

    def test_secret_redaction_covers_literals_headers_and_named_values(self) -> None:
        secret = "fixture-secret-value"
        rendered = closure.redact_text(
            f"Authorization: Bearer abc.def API_KEY={secret} token=xyz {secret}",
            [secret],
        )
        self.assertNotIn(secret, rendered)
        self.assertNotIn("abc.def", rendered)
        self.assertNotIn("xyz", rendered)
        self.assertGreaterEqual(rendered.count("[REDACTED]"), 3)
        with self.assertRaises(closure.ClosureError):
            closure.safe_environment({"SANITY_WRITE_TOKEN": secret})

    def test_process_receipt_never_persists_raw_argv(self) -> None:
        secret_arg = "--token=fixture-raw-argv-secret"
        process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)", secret_arg],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            receipt = closure.safe_process_receipt(process.pid)
            self.assertIsNotNone(receipt)
            path = self.root / "process.json"
            closure.atomic_json(path, receipt)
            persisted = path.read_text(encoding="utf-8")
            self.assertNotIn(secret_arg, persisted)
            self.assertNotIn("command", persisted)
            self.assertEqual(set(receipt), {"pid", "identity_sha256"})
        finally:
            process.terminate()
            process.wait(timeout=5)

    def test_authenticated_identity_allows_only_the_proven_reparenting_transition(self) -> None:
        original = (
            b"55194 55192 55194 Mon Aug 24 23:43:52 2026     "
            b"/opt/python -B frozen_harness.py"
        )
        reparented = (
            b"55194     1 55194 Mon Aug 24 23:43:52 2026     "
            b"/opt/python -B frozen_harness.py"
        )
        expected = closure.sha256_bytes(original)
        self.assertTrue(closure.reparented_process_identity_matches(reparented, expected))
        self.assertFalse(
            closure.reparented_process_identity_matches(
                reparented.replace(b"frozen_harness.py", b"different.py"), expected
            )
        )

    def test_tree_manifest_matches_clone_and_rejects_escaping_symlink(self) -> None:
        tree = self.root / "tree"
        (tree / "nested").mkdir(parents=True)
        (tree / "nested" / "data.txt").write_text("sealed\n", encoding="utf-8")
        (tree / "link").symlink_to("nested/data.txt")
        clone = self.root / "clone"
        shutil.copytree(tree, clone, symlinks=True)
        self.assertTrue(closure.manifests_equal(closure.tree_manifest(tree), closure.tree_manifest(clone)))
        (tree / "escape").symlink_to(self.root / "outside")
        with self.assertRaisesRegex(closure.ClosureError, "escaping symlink"):
            closure.tree_manifest(tree)

    def test_config_refuses_state_or_lock_inside_live_tree(self) -> None:
        live = self.root / "live"
        config = self.config(live, live / "state")
        with self.assertRaisesRegex(closure.ClosureError, "outside the immutable"):
            closure.validate_config(config)
        config["state_dir"] = str(self.root / "state")
        config["score_lock_path"] = str(live / "score.lock")
        with self.assertRaisesRegex(closure.ClosureError, "scorer lock"):
            closure.validate_config(config)

    def test_fake_score_worker_records_seed_port_seals_clone_and_redacts_log(self) -> None:
        instrument = self.root / "instrument"
        instrument.mkdir()
        template = self.root / "score-template.json"
        template.write_text(json.dumps(fixture_score()), encoding="utf-8")
        scorer = instrument / "score_sb7.py"
        scorer.write_text(
            "import argparse, json\n"
            "p=argparse.ArgumentParser(); p.add_argument('--tree'); p.add_argument('--port'); "
            "p.add_argument('--seed'); p.add_argument('--json-out'); a=p.parse_args()\n"
            "print('api key sk_fixture_vendor_secret')\n"
            f"d=json.load(open({str(template)!r})); d['fixture_seed']=a.seed; "
            "json.dump(d,open(a.json_out,'w'))\n",
            encoding="utf-8",
        )
        vendor = instrument / "vendor_service_v3.py"
        vendor.write_text('API_KEY = "sk_fixture_vendor_secret"\n', encoding="utf-8")
        scorer.chmod(0o400)
        vendor.chmod(0o400)
        clone = self.root / "attempt" / "tree"
        clone.mkdir(parents=True)
        (clone / "app.txt").write_text("fixture\n", encoding="utf-8")
        raw_sha = closure.tree_manifest(clone)["tree_sha256"]
        result = self.root / "attempt" / "worker-result.json"
        node = pathlib.Path(sys.executable).resolve()
        job = {
            "schema_version": 1,
            "attempt": 1,
            "clone": str(clone),
            "raw_tree_sha256": raw_sha,
            "seed": "fedcba9876543210",
            "scorer": str(scorer),
            "scorer_sha256": sha(scorer),
            "port": 18970,
            "score_output": str(self.root / "attempt" / "raw-score.json"),
            "score_log": str(self.root / "attempt" / "score.log"),
            "result": str(result),
            "lock": str(self.root / "score.lock"),
            "stop": str(self.root / "STOP"),
            "timeout_seconds": 20,
            "instrument_root": str(instrument),
            "instrument_files": {
                "score_sb7.py": sha(scorer),
                "vendor_service_v3.py": sha(vendor),
            },
            "render_node": str(node),
            "render_node_sha256": sha(node),
            "score_contract": {
                "raw_scorer_version": "sb-7.0-rc",
                "check_count": 91,
                "telemetry_nodes": ["gabee", "mihai", "workhorse"],
            },
            "vendor_source": str(vendor),
        }
        previous_umask = os.umask(0o077)
        try:
            with mock.patch.object(closure, "port_is_available", return_value=True):
                exit_code = closure.score_worker_impl(job)
        finally:
            os.umask(previous_umask)
        self.assertEqual(exit_code, 0)
        worker_result = json.loads(result.read_text(encoding="utf-8"))
        self.assertEqual(worker_result["fixture_seed"], "fedcba9876543210")
        self.assertEqual(worker_result["port"], 18970)
        self.assertTrue(worker_result["descendants_clean"])
        self.assertRegex(worker_result["score_tree_sha256"], r"^[0-9a-f]{64}$")
        self.assertNotIn(
            "sk_fixture_vendor_secret",
            (self.root / "attempt" / "score.log").read_text(encoding="utf-8"),
        )
        for name in ("worker-result.json", "score.log", "score-tree-seal.json", "scorer-state.json"):
            self.assertEqual(stat.S_IMODE((self.root / "attempt" / name).stat().st_mode), 0o600)
        scorer_state = (self.root / "attempt" / "scorer-state.json").read_text(encoding="utf-8")
        self.assertNotIn("--seed", scorer_state)
        self.assertNotIn("fedcba9876543210", scorer_state)

    def test_resume_rejects_abandoned_worker_and_stops_only_authenticated_scorer(self) -> None:
        live = self.root / "live"
        run_dir = live / "swarm-3node-qwen38-brainwaves-r0"
        run_dir.mkdir(parents=True)
        state = self.root / "state"
        config = self.config(live, state)
        config_path = self.root / "resume-config.json"
        config_path.write_text(json.dumps(config), encoding="utf-8")
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        attempt = state / "scoring" / "attempt-1"
        attempt.mkdir(parents=True)
        closure.atomic_json(
            attempt / "scorer-state.json",
            {"process_group_id": 222},
        )
        closure.atomic_json(
            attempt / "scorer.pid.json",
            {"pid": 222, "identity_sha256": "authenticated-scorer"},
        )
        worker_receipt = {"pid": 111, "identity_sha256": "departed-worker"}

        def process_receipt(pid: int) -> dict | None:
            if pid == 222:
                return {"pid": 222, "identity_sha256": "authenticated-scorer"}
            return None

        with (
            mock.patch.object(closure, "safe_process_receipt", side_effect=process_receipt),
            mock.patch.object(closure, "process_group_exists", return_value=True),
            mock.patch.object(closure, "terminate_process_group") as terminate,
            mock.patch.object(closure.time, "sleep"),
        ):
            result = supervisor.wait_score_worker(attempt, worker_receipt)
        terminate.assert_called_once_with(222)
        self.assertEqual(result["exit_code"], 125)
        self.assertEqual(stat.S_IMODE((attempt / "worker-result.json").stat().st_mode), 0o600)

    def test_authoritative_registry_must_match_raw_auto_verdict(self) -> None:
        _live, _state, fixture = self.write_terminal_fixture()
        config_path = self.root / "registry-config.json"
        config_path.write_text(json.dumps(fixture["config"]), encoding="utf-8")
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        score = fixture_score()
        supervisor.validate_score(score, {"fixture_seed": "0123456789abcdef"})
        score["checks"][0]["check"] = "foreign_check"
        with self.assertRaisesRegex(closure.ClosureError, "check registry"):
            supervisor.validate_score(score, {"fixture_seed": "0123456789abcdef"})

    def test_snapshot_is_complete_private_and_hash_guarded(self) -> None:
        live = self.root / "live"
        state = self.root / "state"
        config = self.config(live, state)
        config["controller_sha256"] = sha(MODULE_PATH)
        config_path = self.root / "snapshot-config.json"
        config_path.write_text(json.dumps(config), encoding="utf-8")
        script, frozen_config = closure.snapshot_instrument(config_path)
        instrument = state / "closure-instrument"
        self.assertEqual(stat.S_IMODE(instrument.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(script.stat().st_mode), 0o500)
        self.assertEqual(stat.S_IMODE(frozen_config.stat().st_mode), 0o400)
        self.assertEqual(stat.S_IMODE((instrument / "manifest.json").stat().st_mode), 0o400)
        self.assertEqual(closure.snapshot_instrument(config_path), (script, frozen_config))
        pathlib.Path(config["publisher"]["path"]).write_text("// source drift after snapshot\n")
        self.assertEqual(
            closure.snapshot_instrument(config_path),
            (script, frozen_config),
            "resume must use the durable reviewed snapshot instead of mutable source files",
        )
        script.chmod(0o700)
        with script.open("a", encoding="utf-8") as handle:
            handle.write("# drift\n")
        with self.assertRaisesRegex(closure.ClosureError, "closure instrument changed"):
            closure.snapshot_instrument(config_path)

    def test_detached_start_creates_private_state_before_locking(self) -> None:
        live = self.root / "live"
        state = self.root / "new-state"
        config = self.config(live, state)
        config_path = self.root / "start-config.json"
        config_path.write_text(json.dumps(config), encoding="utf-8")
        frozen_script = self.root / "frozen-controller.py"
        frozen_config = self.root / "frozen-config.json"
        frozen_script.write_text("pass\n", encoding="utf-8")
        frozen_config.write_text("{}\n", encoding="utf-8")
        process = mock.Mock(pid=456)
        with (
            mock.patch.object(
                closure,
                "snapshot_instrument",
                return_value=(frozen_script, frozen_config),
            ),
            mock.patch.object(closure.subprocess, "Popen", return_value=process) as popen,
            mock.patch.object(
                closure,
                "safe_process_receipt",
                return_value={"pid": 456, "identity_sha256": "fixture-process"},
            ),
        ):
            self.assertEqual(closure.spawn_supervisor(config_path, resume=False), 0)
        self.assertEqual(stat.S_IMODE(state.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE((state / "supervisor.lock").stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE((state / "supervisor.pid.json").stat().st_mode), 0o600)
        self.assertTrue(popen.call_args.kwargs["start_new_session"])


if __name__ == "__main__":
    unittest.main()
