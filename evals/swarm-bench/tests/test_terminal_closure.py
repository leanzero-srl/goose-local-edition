from __future__ import annotations

import importlib.util
import contextlib
import json
import os
import pathlib
import shutil
import signal
import socket
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
        "probe_unavailable": [],
        "harness_missing": [],
        "sched_unreached": [],
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

    def playwright_runtime(self) -> dict:
        module_root = self.root / "node-runtime" / "lib" / "node_modules" / "playwright"
        browsers_json = module_root / "node_modules" / "playwright-core" / "browsers.json"
        package_json = module_root / "package.json"
        module_entry = module_root / "index.js"
        installed_browsers = self.root / "installed-playwright-browsers"
        browser_directory = "chromium_headless_shell-1200"
        browser_root = installed_browsers / browser_directory
        executable_relative = pathlib.Path("fixture-platform") / "chrome-headless-shell"
        executable = browser_root / executable_relative
        if not module_entry.exists():
            browsers_json.parent.mkdir(parents=True)
            browser_root.mkdir(parents=True)
            package_json.write_text(
                json.dumps({"name": "playwright", "version": "1.57.0"}),
                encoding="utf-8",
            )
            browsers_json.write_text(
                json.dumps(
                    {
                        "browsers": [
                            {
                                "name": "chromium-headless-shell",
                                "revision": "1200",
                                "installByDefault": True,
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            module_entry.write_text(
                "const { existsSync } = require('node:fs');\n"
                "const { join } = require('node:path');\n"
                "module.exports = { chromium: { launch: async () => {\n"
                "  const root = process.env.PLAYWRIGHT_BROWSERS_PATH || "
                "join(process.env.HOME || '', 'Library', 'Caches', 'ms-playwright');\n"
                "  const executable = join(root, 'chromium_headless_shell-1200', "
                "'fixture-platform', 'chrome-headless-shell');\n"
                "  if (!existsSync(executable)) throw new Error('fixture browser executable missing');\n"
                "  return { newPage: async () => ({ setContent: async () => {}, "
                "title: async () => 'terminal-closure-playwright-smoke' }), close: async () => {} };\n"
                "} } };\n",
                encoding="utf-8",
            )
            executable.parent.mkdir(parents=True)
            executable.write_text("fixture browser runtime\n", encoding="utf-8")
            executable.chmod(0o500)
        return {
            "module_root": str(module_root),
            "module_tree_sha256": closure.tree_manifest(module_root)["tree_sha256"],
            "version": "1.57.0",
            "browsers_json": "node_modules/playwright-core/browsers.json",
            "browsers_json_sha256": sha(browsers_json),
            "browser_name": "chromium-headless-shell",
            "browser_revision": "1200",
            "installed_browsers_path": str(installed_browsers),
            "browser_directory": browser_directory,
            "browser_tree_sha256": closure.tree_manifest(browser_root)["tree_sha256"],
            "executable": executable_relative.as_posix(),
            "executable_sha256": sha(executable),
        }

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
        node_path = shutil.which("node")
        if node_path is None:
            raise RuntimeError("Node is required for terminal-closure tests")
        node = pathlib.Path(node_path).resolve()
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
            "runtime": {
                "lsof": "/usr/sbin/lsof",
                "playwright": self.playwright_runtime(),
            },
        }

    def v18_binding_fixture(
        self, *, fixture_seed: str | None = "0123456789abcdef"
    ) -> tuple[pathlib.Path, dict, contextlib.ExitStack]:
        live_root = self.root / "local-sb7-engine-v18"
        run_dir = live_root / "swarm-3node-qwen38-brainwaves-r0"
        state_dir = self.root / "local-sb7-engine-v18-terminal-closure"
        publisher_root = self.root / "LeanZero-website"
        publisher = publisher_root / "scripts" / "seed-fleet-brainwaves-sb70.mjs"
        publisher.parent.mkdir(parents=True)
        publisher.write_text("export const generation = 'Brainwaves v18';\n", encoding="utf-8")
        launcher = live_root / "launch_local_v18.py"
        binary = live_root / "bin" / "goose-fixture"
        instrument_file = live_root / "instrument" / "evals/swarm-bench/bench/score_sb7.py"
        run_dir.mkdir(parents=True)
        launcher.write_text("raise SystemExit('fixture launcher must not run')\n", encoding="utf-8")
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture goose binary\n")
        instrument_file.parent.mkdir(parents=True)
        instrument_file.write_text("# frozen scorer fixture\n", encoding="utf-8")
        for path in (launcher, binary, instrument_file):
            path.chmod(0o400)
        config = self.config(live_root, state_dir)
        config.update(
            {
                "closure_generation": "v18",
                "armed": False,
                "binding": None,
                "bound_config_path": str(state_dir / "config.json"),
                "controller_sha256": sha(MODULE_PATH),
                "score_lock_path": str(self.root / "local-sb7-engine-v18-score.lock"),
            }
        )
        config["publication"]["provenance_marker"] = "Brainwaves v18"
        config["publisher"].update(
            {
                "path": str(publisher),
                "sha256": sha(publisher),
                "site_root": str(publisher_root),
                "git_commit": closure.V18_PUBLISHER_COMMIT,
            }
        )
        config["expected"].update(
            {
                "candidate_commit": closure.V18_CANDIDATE_COMMIT,
                "candidate_tree": closure.V18_CANDIDATE_TREE,
                "binary_path": str(binary),
                "binary_sha256": sha(binary),
                "launch_controller_path": str(launcher),
                "launch_controller_sha256": sha(launcher),
                "run_id": None,
                "fixture_seed": None,
                "models": None,
                "launch_sha256": None,
                "instrument_manifest_sha256": None,
                "run_started_sha256": None,
                "trace_header_sha256": None,
                "fleet_seal_sha256": None,
                "instrument_files": {},
            }
        )
        models = list(MODELS)
        model_rows = [
            {
                "identifier": identifier,
                "deviceIdentifier": None if identifier.startswith("mihai-") else identifier,
                "role": "local"
                if identifier.startswith("mihai-")
                else "workhorse"
                if identifier.startswith("workhorse-")
                else "mac",
                "path": "/fixture/model.gguf",
                "contextLength": 262144,
                "parallel": 2,
                "quantization": {"bits": 6, "name": "fixture"},
            }
            for identifier in models
        ]
        planner = models[2]
        fleet_seal = {
            "schema_version": 1,
            "sealed_at": "2026-08-25T06:00:00+00:00",
            "source": "authenticated-live-lm-studio-preflight",
            "local_device_identifier": "fixture-local",
            "preferred_device_identifier": "fixture-workhorse",
            "models": sorted(model_rows, key=lambda row: row["role"]),
            "model_ids": sorted(models),
            "planner_model": planner,
            "api_model_ids": sorted(models),
            "protected_prior_aliases_reused": True,
        }
        fleet_path = live_root / "fleet-seal.json"
        closure.atomic_json(fleet_path, fleet_seal)
        manifest = {
            "schema_version": 1,
            "candidate_commit": closure.V18_CANDIDATE_COMMIT,
            "candidate_tree": closure.V18_CANDIDATE_TREE,
            "binary": {"path": str(binary), "sha256": sha(binary)},
            "files": {
                "evals/swarm-bench/bench/score_sb7.py": sha(instrument_file)
            },
            "sb7_policy": {
                "spec_and_scorer_unchanged_from_v6": True,
                "website_surface": "stable-sb7",
                "publish_from_run_build_auto_score": False,
                "entrant": config["expected"]["entrant"],
                "publication_document_id": closure.V18_TARGET_DOCUMENT_ID,
                "protected_document_ids": config["publication"][
                    "protected_document_ids"
                ],
            },
        }
        manifest_path = live_root / "instrument-manifest.json"
        closure.atomic_json(manifest_path, manifest, 0o400)
        run_started = {
            "event": "run_started",
            "seq": 0,
            "assured": False,
            "run_id": "swarm-20260825-123456789",
            "working_dir": str(run_dir),
            "telemetry_file": str(run_dir / ".swarm/telemetry.jsonl"),
            "endpoint": "http://localhost:1234",
            "max_attempts": 3,
            "max_turns": 100000,
            "planner_model": planner,
            "pool": [{"model_id": model} for model in models],
        }
        (run_dir / "run.jsonl").write_text(
            json.dumps(run_started) + "\n", encoding="utf-8"
        )
        trace_path = live_root / "trace-swarm-3node-qwen38-brainwaves-r0.jsonl"
        trace_path.write_text(
            json.dumps(
                {
                    "trace_header": "meridian-v3",
                    "seq": 1,
                    "fixture_seed": fixture_seed,
                }
            )
            + "\n",
            encoding="utf-8",
        )
        for path in (fleet_path, run_dir / "run.jsonl", trace_path):
            path.chmod(0o600)
        launch = {
            "schema_version": 1,
            "candidate": {
                "commit": closure.V18_CANDIDATE_COMMIT,
                "tree": closure.V18_CANDIDATE_TREE,
            },
            "binary": {"path": str(binary), "sha256": sha(binary)},
            "launch_controller_sha256": sha(launcher),
            "instrument_manifest_sha256": sha(manifest_path),
            "vendor_port": 18970,
            "entrant": config["expected"]["entrant"],
            "publication_document_id": closure.V18_TARGET_DOCUMENT_ID,
            "run_started_identity": {
                "run_id": run_started["run_id"],
                "planner_model": planner,
                "pool_models": sorted(models),
            },
            "fleet_seal": {
                "path": str(fleet_path),
                "sha256": sha(fleet_path),
                "model_ids": sorted(models),
                "planner_model": planner,
            },
            "harness": {"pid": 101, "identity_sha256": "1" * 64},
            "goose": {"pid": 102, "identity_sha256": "2" * 64},
            "monitor": {"pid": 103, "identity_sha256": "3" * 64},
        }
        closure.atomic_json(live_root / "launch.json", launch)
        template_path = self.root / "terminal-closure-v18.unarmed.json"
        template_path.write_text(json.dumps(config, indent=2), encoding="utf-8")
        patches = contextlib.ExitStack()
        patches.enter_context(mock.patch.object(closure, "V18_LIVE_ROOT", live_root))
        patches.enter_context(mock.patch.object(closure, "V18_RUN_DIR", run_dir))
        patches.enter_context(mock.patch.object(closure, "V18_STATE_DIR", state_dir))
        patches.enter_context(
            mock.patch.object(closure, "V18_BOUND_CONFIG", state_dir / "config.json")
        )
        patches.enter_context(
            mock.patch.object(
                closure,
                "V18_SCORE_LOCK",
                self.root / "local-sb7-engine-v18-score.lock",
            )
        )
        patches.enter_context(mock.patch.object(closure, "V18_LAUNCHER", launcher))
        patches.enter_context(mock.patch.object(closure, "V18_FLEET_SEAL", fleet_path))
        patches.enter_context(mock.patch.object(closure, "V18_BINARY", binary))
        patches.enter_context(
            mock.patch.object(closure, "V18_BINARY_SHA256", sha(binary))
        )
        patches.enter_context(
            mock.patch.object(closure, "V18_PUBLISHER_ROOT", publisher_root)
        )
        patches.enter_context(mock.patch.object(closure, "V18_PUBLISHER_PATH", publisher))
        patches.enter_context(
            mock.patch.object(closure, "V18_PUBLISHER_SHA256", sha(publisher))
        )
        patches.enter_context(
            mock.patch.object(closure, "git_head", return_value=closure.V18_PUBLISHER_COMMIT)
        )
        patches.enter_context(
            mock.patch.object(closure, "validate_authenticated_process", return_value=True)
        )
        return template_path, config, patches

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

    def test_scorer_spawn_tracking_does_not_add_an_observation_window(self) -> None:
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        fake_ps = fake_bin / "ps"
        fake_ps.write_text("#!/bin/sh\nprintf 'fixture-process-identity\\n'\n", encoding="utf-8")
        fake_ps.chmod(0o500)
        subprocess.run([fake_ps], check=True, stdout=subprocess.DEVNULL)
        elapsed_path = self.root / "spawn-elapsed"
        scorer = self.root / "timed-scorer.py"
        scorer.write_text(
            "import subprocess,sys,time\n"
            "started=time.monotonic()\n"
            "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(5)'])\n"
            f"open({str(elapsed_path)!r},'w').write(str(time.monotonic()-started))\n"
            "child.terminate()\n"
            "child.wait()\n",
            encoding="utf-8",
        )
        journal = self.root / "spawn-journal.txt"
        journal.touch(mode=0o600)
        gate_source = closure.SCORER_GATE_SOURCE.replace("os.fsync(output)", "None")
        self.assertNotEqual(gate_source, closure.SCORER_GATE_SOURCE)
        read_gate, write_gate = os.pipe()
        try:
            gate = subprocess.Popen(
                [
                    sys.executable,
                    "-c",
                    gate_source,
                    str(read_gate),
                    str(journal),
                    str(scorer),
                ],
                pass_fds=(read_gate,),
                env={**os.environ, "PATH": f"{fake_bin}:{os.environ['PATH']}"},
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        finally:
            os.close(read_gate)
        try:
            os.write(write_gate, b"1")
        finally:
            os.close(write_gate)
        stdout, stderr = gate.communicate(timeout=10)
        self.assertEqual(gate.returncode, 0, f"stdout={stdout!r} stderr={stderr!r}")
        self.assertLess(
            float(elapsed_path.read_text(encoding="utf-8")),
            0.15,
            "spawn tracking delayed the scorer after Popen returned",
        )
        receipts = closure.read_spawn_journal_receipts(journal)
        self.assertEqual(len(receipts), 1)

    def test_spawn_tracking_failure_reaps_a_detached_child(self) -> None:
        fake_bin = self.root / "failing-ps-bin"
        fake_bin.mkdir()
        fake_ps = fake_bin / "ps"
        fake_ps.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        fake_ps.chmod(0o500)
        subprocess.run([fake_ps], check=False, stdout=subprocess.DEVNULL)
        for failure in ("ps", "journal"):
            with self.subTest(failure=failure):
                with socket.socket(socket.AF_INET6) as reservation:
                    reservation.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                    reservation.bind(("::1", 0))
                    port = int(reservation.getsockname()[1])
                child_pid_path = self.root / f"{failure}-child.pid"
                child_source = (
                    "import os,socket,time\n"
                    "listener=socket.socket(socket.AF_INET6); "
                    "listener.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1); "
                    f"listener.bind(('::1',{port})); listener.listen(1)\n"
                    f"open({str(child_pid_path)!r},'w').write(str(os.getpid()))\n"
                    "time.sleep(30)\n"
                )
                scorer = self.root / f"{failure}-tracking-scorer.py"
                scorer.write_text(
                    "import os,subprocess,sys\n"
                    f"pid_path={str(child_pid_path)!r}\n"
                    "def record_pid():\n"
                    "    descriptor=os.open(pid_path,os.O_CREAT|os.O_WRONLY,0o600)\n"
                    "    os.write(descriptor,str(os.getpid()).encode())\n"
                    "    os.close(descriptor)\n"
                    f"subprocess.Popen([sys.executable,'-c',{child_source!r}], "
                    "start_new_session=True,preexec_fn=record_pid)\n",
                    encoding="utf-8",
                )
                journal = self.root / f"{failure}-spawn-journal.txt"
                if failure == "ps":
                    journal.touch(mode=0o600)
                else:
                    journal = self.root / "missing-journal-parent" / "journal.txt"
                read_gate, write_gate = os.pipe()
                environment = dict(os.environ)
                if failure == "ps":
                    environment["PATH"] = f"{fake_bin}:{environment['PATH']}"
                try:
                    gate = subprocess.Popen(
                        [
                            sys.executable,
                            "-c",
                            closure.SCORER_GATE_SOURCE,
                            str(read_gate),
                            str(journal),
                            str(scorer),
                        ],
                        pass_fds=(read_gate,),
                        env=environment,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        text=True,
                    )
                finally:
                    os.close(read_gate)
                try:
                    os.write(write_gate, b"1")
                finally:
                    os.close(write_gate)
                stdout, stderr = gate.communicate(timeout=10)
                self.assertNotEqual(
                    gate.returncode, 0, f"stdout={stdout!r} stderr={stderr!r}"
                )
                self.assertTrue(child_pid_path.is_file())
                child_pid = int(child_pid_path.read_text(encoding="utf-8"))

                def cleanup_child() -> None:
                    try:
                        os.killpg(child_pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass

                self.addCleanup(cleanup_child)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child_pid, 0)
                with socket.socket(socket.AF_INET6) as listener:
                    listener.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                    listener.bind(("::1", port))

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

    def test_empty_home_playwright_fails_closed_until_pinned_runtime_is_mounted(self) -> None:
        runtime = self.playwright_runtime()
        node_path = shutil.which("node")
        self.assertIsNotNone(node_path)
        node = pathlib.Path(str(node_path)).resolve()
        runtime_info = closure.validate_playwright_runtime(runtime, node, sha(node))
        runtime_root = self.root / "isolated-playwright"
        runtime_home = runtime_root / "empty-home"
        runtime_tmp = runtime_root / "tmp"
        empty_view = runtime_root / "empty-browser-view"
        closure.ensure_secure_dir(empty_view)
        with self.assertRaisesRegex(closure.ClosureError, "smoke launch failed"):
            closure.smoke_playwright_runtime(
                runtime_info,
                node,
                runtime_home,
                runtime_tmp,
                empty_view,
                MODULE_PATH,
                timeout_seconds=10,
            )
        pinned_view = closure.prepare_playwright_browser_view(
            runtime_info, runtime_root / "pinned-browser-view"
        )
        receipt = closure.smoke_playwright_runtime(
            runtime_info,
            node,
            runtime_home,
            runtime_tmp,
            pinned_view,
            MODULE_PATH,
            timeout_seconds=10,
        )
        self.assertTrue(receipt["ok"])
        self.assertTrue(
            (pinned_view / runtime["browser_directory"]).is_symlink(),
            "the worker must expose only the pinned browser revision",
        )
        shadow_root = self.root / "shadow-probe"
        shadow_probe = shadow_root / "product_probe_v3.mjs"
        shadow_package = shadow_root / "node_modules" / "playwright"
        shadow_package.mkdir(parents=True)
        shadow_probe.write_text("export {};\n", encoding="utf-8")
        (shadow_package / "package.json").write_text(
            json.dumps(
                {
                    "name": "playwright",
                    "version": runtime["version"],
                    "main": "index.js",
                }
            ),
            encoding="utf-8",
        )
        (shadow_package / "index.js").write_text(
            "module.exports = { chromium: { launch: async () => ({}) } };\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(closure.ClosureError, "unpinned Playwright package"):
            closure.smoke_playwright_runtime(
                runtime_info,
                node,
                runtime_home,
                runtime_tmp,
                pinned_view,
                shadow_probe,
                timeout_seconds=10,
            )

    def test_config_refuses_state_or_lock_inside_live_tree(self) -> None:
        live = self.root / "live"
        config = self.config(live, live / "state")
        with self.assertRaisesRegex(closure.ClosureError, "outside the immutable"):
            closure.validate_config(config)
        config["state_dir"] = str(self.root / "state")
        config["score_lock_path"] = str(live / "score.lock")
        with self.assertRaisesRegex(closure.ClosureError, "scorer lock"):
            closure.validate_config(config)

    def test_v18_unarmed_config_and_null_evidence_refuse_binding(self) -> None:
        template_path, config, patches = self.v18_binding_fixture(fixture_seed=None)
        with patches:
            closure.validate_config(config, allow_unarmed=True)
            with self.assertRaisesRegex(closure.ClosureError, "unarmed"):
                closure.validate_config(config)
            for operation in (closure.status, closure.results, closure.stop):
                with self.subTest(operation=operation.__name__), self.assertRaisesRegex(
                    closure.ClosureError, "unarmed"
                ):
                    operation(template_path)
            self.assertFalse(closure.V18_STATE_DIR.exists())
            with self.assertRaisesRegex(closure.ClosureError, "original exact fixture_seed"):
                closure.bind_v18(template_path)
            (closure.V18_LIVE_ROOT / "launch.json").unlink()
            with self.assertRaisesRegex(closure.ClosureError, "missing or linked"):
                closure.bind_v18(template_path)

    def test_v18_binding_is_create_only_private_and_exactly_once(self) -> None:
        template_path, _config, patches = self.v18_binding_fixture()
        with patches:
            target, created = closure.bind_v18(template_path)
            self.assertTrue(created)
            self.assertEqual(target, closure.V18_BOUND_CONFIG)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o400)
            armed = closure.read_json(target)
            self.assertTrue(armed["armed"])
            self.assertEqual(armed["expected"]["fixture_seed"], "0123456789abcdef")
            self.assertEqual(
                armed["expected"]["run_id"], "swarm-20260825-123456789"
            )
            self.assertNotIn("local-sb7-engine-v17", target.read_text(encoding="utf-8"))
            same_target, second_created = closure.bind_v18(template_path)
            self.assertEqual(same_target, target)
            self.assertFalse(second_created)
            _script, frozen_config = closure.snapshot_instrument(target)
            frozen = closure.load_config(frozen_config)
            self.assertIn(
                pathlib.Path(frozen["publisher"]["path"]).resolve(),
                {
                    closure.V18_PUBLISHER_PATH.resolve(),
                    (
                        closure.V18_STATE_DIR
                        / "closure-instrument/seed-fleet-brainwaves-sb70.mjs"
                    ).resolve(),
                },
            )
            closure.validate_config(frozen)
            self.assertEqual(
                pathlib.Path(frozen["publisher"]["path"]).resolve(),
                (
                    closure.V18_STATE_DIR
                    / "closure-instrument/seed-fleet-brainwaves-sb70.mjs"
                ).resolve(),
            )
            target.chmod(0o600)
            armed["expected"]["fixture_seed"] = "fedcba9876543210"
            closure.atomic_json(target, armed, 0o400)
            with self.assertRaises(closure.ClosureError):
                closure.bind_v18(template_path)

    def test_v18_binding_rejects_stale_v17_provenance(self) -> None:
        template_path, config, patches = self.v18_binding_fixture()
        with patches:
            config["live_root"] = "/Users/mihaiperdum/goose-builds/local-sb7-engine-v17"
            config["run_dir"] = (
                "/Users/mihaiperdum/goose-builds/local-sb7-engine-v17/"
                "swarm-3node-qwen38-brainwaves-r0"
            )
            template_path.write_text(json.dumps(config), encoding="utf-8")
            with self.assertRaisesRegex(closure.ClosureError, "stale v17"):
                closure.bind_v18(template_path)

    def test_v18_fixture_seed_is_checked_in_each_closure_representation(self) -> None:
        template_path, _config, patches = self.v18_binding_fixture()
        with patches:
            bound_path, _created = closure.bind_v18(template_path)
            supervisor = closure.TerminalClosure(bound_path)
            self.addCleanup(supervisor.events.handle.close)
            expected_seed = "0123456789abcdef"
            wrong_seed = "fedcba9876543210"

            trace_path = closure.V18_LIVE_ROOT / (
                "trace-swarm-3node-qwen38-brainwaves-r0.jsonl"
            )
            trace_path.write_text(
                json.dumps(
                    {
                        "trace_header": "meridian-v3",
                        "seq": 1,
                        "fixture_seed": wrong_seed,
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            trace_path.chmod(0o600)
            with self.assertRaisesRegex(closure.ClosureError, "first trace evidence"):
                supervisor.bound_trace_header()

            attempt = supervisor.state_dir / "scoring/attempt-1"
            (attempt / "tree").mkdir(parents=True)
            for name in (
                "raw-score.json",
                "score-tree-seal.json",
                "descendants.json",
                "spawn-journal.txt",
            ):
                (attempt / name).write_text("{}\n", encoding="utf-8")
            runtime = attempt / "runtime"
            runtime.mkdir()
            (runtime / "playwright-node").write_text("fixture\n", encoding="utf-8")
            closure.atomic_json(
                attempt / "worker-result.json",
                {
                    "exit_code": 0,
                    "scorer_exit_code": 0,
                    "fixture_seed": wrong_seed,
                },
            )
            with self.assertRaisesRegex(closure.ClosureError, "score worker.*fixture_seed"):
                supervisor.successful_score_attempt(attempt)

            score = fixture_score(wrong_seed)
            with self.assertRaisesRegex(closure.ClosureError, "fixture_seed"):
                supervisor.validate_score(score, {"fixture_seed": expected_seed})

            authoritative_path = supervisor.state_dir / "authoritative-verdict.json"
            provenance_path = supervisor.state_dir / "scoring-provenance.json"
            publication_receipt = {
                "target_document_id": closure.V18_TARGET_DOCUMENT_ID,
                "protected_document_ids": sorted(closure.V18_PROTECTED_DOCUMENT_IDS),
                "protected_before_sha256": "0" * 64,
                "protected_after_sha256": "0" * 64,
            }
            closure.atomic_json(authoritative_path, fixture_score(wrong_seed))
            closure.atomic_json(
                provenance_path,
                {"fixture_seed": expected_seed},
            )
            with self.assertRaisesRegex(closure.ClosureError, "fixture_seed"):
                supervisor.validate_publication_receipt(publication_receipt)

            closure.atomic_json(authoritative_path, fixture_score(expected_seed))
            closure.atomic_json(provenance_path, {"fixture_seed": wrong_seed})
            with self.assertRaisesRegex(closure.ClosureError, "provenance"):
                supervisor.validate_publication_receipt(publication_receipt)

    def test_v18_terminal_rejects_auto_verdict_seed_different_from_binding(self) -> None:
        _live, _state, fixture = self.write_terminal_fixture(seed="fedcba9876543210")
        fixture["config"]["expected"]["fixture_seed"] = "0123456789abcdef"
        config_path = self.root / "bound-seed-config.json"
        config_path.write_text(json.dumps(fixture["config"]), encoding="utf-8")
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        with self.assertRaisesRegex(closure.ClosureError, "armed closure identity"):
            supervisor.terminal_evidence(fixture["launch"])

    def score_worker_fixture(
        self, scorer_extra_source: str = ""
    ) -> tuple[dict, pathlib.Path]:
        instrument = self.root / "instrument"
        instrument.mkdir()
        template = self.root / "score-template.json"
        template.write_text(json.dumps(fixture_score()), encoding="utf-8")
        scorer = instrument / "score_sb7.py"
        scorer.write_text(
            "import argparse, json, os, subprocess\n"
            "p=argparse.ArgumentParser(); p.add_argument('--tree'); p.add_argument('--port'); "
            "p.add_argument('--seed'); p.add_argument('--json-out'); a=p.parse_args()\n"
            "assert 'NODE_PATH' not in os.environ and 'PLAYWRIGHT_BROWSERS_PATH' not in os.environ\n"
            "subprocess.run([os.environ['GOOSE_SWARM_RENDER_NODE'], '-e', "
            "\"const fs=require('fs'),p=require('path'); "
            "if(!require.resolve('playwright').includes('node-runtime'))process.exit(2); "
            "if(!fs.existsSync(p.join(process.env.PLAYWRIGHT_BROWSERS_PATH,"
            "'chromium_headless_shell-1200','fixture-platform','chrome-headless-shell')))"
            "process.exit(3)\"], check=True)\n"
            "print('api key sk_fixture_vendor_secret')\n"
            f"{scorer_extra_source}"
            f"d=json.load(open({str(template)!r})); d['fixture_seed']=a.seed; "
            "json.dump(d,open(a.json_out,'w'))\n",
            encoding="utf-8",
        )
        vendor = instrument / "vendor_service_v3.py"
        vendor.write_text('API_KEY = "sk_fixture_vendor_secret"\n', encoding="utf-8")
        probe = instrument / "product_probe_v3.mjs"
        probe.write_text("export {};\n", encoding="utf-8")
        scorer.chmod(0o400)
        vendor.chmod(0o400)
        probe.chmod(0o400)
        clone = self.root / "attempt" / "tree"
        clone.mkdir(parents=True)
        (clone / "app.txt").write_text("fixture\n", encoding="utf-8")
        raw_sha = closure.tree_manifest(clone)["tree_sha256"]
        result = self.root / "attempt" / "worker-result.json"
        node_path = shutil.which("node")
        self.assertIsNotNone(node_path)
        node = pathlib.Path(str(node_path)).resolve()
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
                "product_probe_v3.mjs": sha(probe),
            },
            "render_node": str(node),
            "render_node_sha256": sha(node),
            "playwright_runtime": self.playwright_runtime(),
            "playwright_probe": str(probe),
            "playwright_probe_sha256": sha(probe),
            "score_contract": {
                "raw_scorer_version": "sb-7.0-rc",
                "check_count": 91,
                "telemetry_nodes": ["gabee", "mihai", "workhorse"],
            },
            "vendor_source": str(vendor),
        }
        return job, result

    def test_fake_score_worker_records_seed_port_seals_clone_and_redacts_log(self) -> None:
        job, result = self.score_worker_fixture()
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
        self.assertEqual(
            worker_result["playwright_browser_tree_sha256"],
            job["playwright_runtime"]["browser_tree_sha256"],
        )
        render_wrapper = self.root / "attempt" / "runtime" / "playwright-node"
        self.assertEqual(stat.S_IMODE(render_wrapper.stat().st_mode), 0o500)
        self.assertEqual(
            worker_result["playwright_node_wrapper_sha256"], sha(render_wrapper)
        )
        self.assertNotIn(
            "sk_fixture_vendor_secret",
            (self.root / "attempt" / "score.log").read_text(encoding="utf-8"),
        )
        for name in (
            "worker-result.json",
            "score.log",
            "score-tree-seal.json",
            "scorer-state.json",
            "descendants.json",
            "spawn-journal.txt",
        ):
            self.assertEqual(stat.S_IMODE((self.root / "attempt" / name).stat().st_mode), 0o600)
        scorer_state = (self.root / "attempt" / "scorer-state.json").read_text(encoding="utf-8")
        self.assertNotIn("--seed", scorer_state)
        self.assertNotIn("fedcba9876543210", scorer_state)

    def test_score_worker_terminates_detached_session_descendant_on_port_18970(
        self,
    ) -> None:
        ready = self.root / "detached-ready"
        child_source = (
            "import socket,time\n"
            "listener=socket.socket(socket.AF_INET6); "
            "listener.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1); "
            "listener.bind(('::1',18970)); listener.listen(1)\n"
            f"open({str(ready)!r},'w').write('ready')\n"
            "time.sleep(120)\n"
        )
        scorer_extra = (
            f"child=subprocess.Popen([{sys.executable!r},'-c',{child_source!r}], "
            "start_new_session=True)\n"
            "import time\n"
            f"deadline=time.time()+5\nwhile not os.path.exists({str(ready)!r}):\n"
            "    assert time.time() < deadline\n    time.sleep(0.01)\n"
        )
        job, result = self.score_worker_fixture(scorer_extra)
        with mock.patch.object(closure, "port_is_available", return_value=True):
            exit_code = closure.score_worker_impl(job)
        self.assertEqual(exit_code, 70)
        worker_result = json.loads(result.read_text(encoding="utf-8"))
        self.assertFalse(worker_result["descendants_clean"])
        self.assertTrue(worker_result["descendant_cleanup_proven"])
        self.assertGreaterEqual(worker_result["descendants_observed"], 2)
        self.assertGreaterEqual(worker_result["descendants_survived_scorer"], 1)
        self.assertEqual(worker_result["descendants_live_after_cleanup"], 0)
        inventory_text = (self.root / "attempt" / "descendants.json").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("--port", inventory_text)
        self.assertNotIn("fedcba9876543210", inventory_text)
        with socket.socket(socket.AF_INET6) as listener:
            listener.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
            listener.bind(("::1", 18970))

    def test_stale_spawn_journal_pid_never_authorizes_an_unrelated_process(
        self,
    ) -> None:
        unrelated = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        self.addCleanup(
            lambda: unrelated.poll() is None
            and (unrelated.terminate(), unrelated.wait(timeout=5))
        )
        journal = self.root / "spawn-journal.txt"
        closure.atomic_write(
            journal,
            (
                json.dumps(
                    {
                        "pid": unrelated.pid,
                        "identity_sha256s": ["0" * 64],
                        "birth_sha256s": ["0" * 64],
                    },
                    separators=(",", ":"),
                )
                + "\n"
            ).encode(),
        )
        with mock.patch.object(closure, "port_is_available", return_value=True):
            cleanup = closure.cleanup_persisted_attempt_processes(
                self.root / "missing-inventory.json", journal, 18970
            )
        self.assertTrue(cleanup["cleanup_proven"])
        self.assertEqual(cleanup["signals_sent"], 0)
        self.assertIsNone(unrelated.poll(), "stale PID authority signalled an unrelated process")
        bare_pid_journal = self.root / "bare-pid-journal.txt"
        closure.atomic_write(bare_pid_journal, f"{unrelated.pid}\n".encode())
        with self.assertRaisesRegex(closure.ClosureError, "malformed"):
            closure.read_spawn_journal_receipts(bare_pid_journal)

    def test_live_descendant_birth_probe_failure_makes_cleanup_unproven(self) -> None:
        process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )

        def cleanup_process() -> None:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)

        self.addCleanup(cleanup_process)
        identity = closure.safe_process_receipt(process.pid)
        birth = closure.process_birth_sha256(process.pid)
        self.assertIsNotNone(identity)
        self.assertIsNotNone(birth)
        inventory = self.root / "descendants.json"
        closure.atomic_json(
            inventory,
            {
                "schema_version": 1,
                "root_pid": process.pid,
                "processes": [
                    {
                        "pid": process.pid,
                        "identity_sha256s": [identity["identity_sha256"]],
                        "birth_sha256s": [birth],
                    }
                ],
            },
        )
        journal = self.root / "spawn-journal.txt"
        journal.touch(mode=0o600)
        with (
            mock.patch.object(closure, "process_birth_sha256", return_value=None),
            mock.patch.object(closure, "port_is_available", return_value=True),
        ):
            cleanup = closure.cleanup_persisted_attempt_processes(
                inventory, journal, 18970, grace_seconds=0.01
            )
        self.assertTrue(cleanup["identity_probe_unavailable"])
        self.assertFalse(cleanup["cleanup_proven"])
        self.assertEqual(cleanup["signals_sent"], 0)
        self.assertIsNone(process.poll())

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

    def test_retry_refuses_any_unproven_descendant_cleanup(self) -> None:
        live = self.root / "live"
        run_dir = live / "swarm-3node-qwen38-brainwaves-r0"
        run_dir.mkdir(parents=True)
        state = self.root / "state"
        config_path = self.root / "retry-config.json"
        config_path.write_text(
            json.dumps(self.config(live, state)), encoding="utf-8"
        )
        supervisor = closure.TerminalClosure(config_path)
        self.addCleanup(supervisor.events.handle.close)
        attempt = state / "scoring" / "attempt-1"
        attempt.mkdir(parents=True)
        with self.assertRaisesRegex(closure.ClosureError, "refusing retry"):
            supervisor.prove_attempt_cleanup_before_retry(
                attempt,
                {"exit_code": 70, "descendant_cleanup_proven": False},
            )
        with (
            mock.patch.object(
                closure,
                "cleanup_persisted_attempt_processes",
                return_value={"cleanup_proven": False},
            ),
            self.assertRaisesRegex(closure.ClosureError, "contaminated port"),
        ):
            supervisor.prove_attempt_cleanup_before_retry(
                attempt,
                {"exit_code": 70, "descendant_cleanup_proven": True},
            )

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

    def test_score_payload_rejects_all_product_probe_degradation_forms(self) -> None:
        expected = {
            "raw_scorer_version": "sb-7.0-rc",
            "check_count": 91,
            "telemetry_nodes": ["gabee", "mihai", "workhorse"],
        }
        for field, value in (
            ("probe_unavailable", ["t_labels_culling"]),
            ("harness_missing", ["fixtures_v3"]),
        ):
            score = fixture_score()
            score[field] = value
            with self.subTest(field=field), self.assertRaisesRegex(
                closure.ClosureError, "degraded product-probe"
            ):
                closure.validate_sb7_score_payload(
                    score, expected, "0123456789abcdef"
                )
        missing = fixture_score()
        missing.pop("probe_unavailable")
        with self.assertRaisesRegex(closure.ClosureError, "missing or malformed"):
            closure.validate_sb7_score_payload(
                missing, expected, "0123456789abcdef"
            )
        valid_low_score = fixture_score()
        valid_low_score["sched_unreached"] = [
            {"sched-unreached": "partition_after_event"}
        ]
        valid_low_score["checks"][0]["detail"] = (
            "sync2 produced too few list responses (sched-unreached, R1)"
        )
        closure.validate_sb7_score_payload(
            valid_low_score, expected, "0123456789abcdef"
        )
        for mutation in (
            {"unavailable": True},
            {
                "detail": "PROBE UNAVAILABLE: browser exited",
                "consequence": "harness failure, not app evidence",
            },
            {"detail": "_probe_error product_probe_v3 failed"},
        ):
            score = fixture_score()
            score["checks"][0].update(mutation)
            with self.subTest(mutation=mutation), self.assertRaisesRegex(
                closure.ClosureError, "probe-unavailable|degraded product-probe"
            ):
                closure.validate_sb7_score_payload(
                    score, expected, "0123456789abcdef"
                )

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
