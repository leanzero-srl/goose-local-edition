from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import cloud_sb7


class CloudSb7HarnessTest(unittest.TestCase):
    def make_recovery_campaign(self, root: Path, status: str) -> None:
        (root / "entrants/model/tree").mkdir(parents=True)
        (root / "scores/model").mkdir(parents=True)
        (root / "locks").mkdir()
        manifest = root / "instrument.json"
        manifest.write_text(
            json.dumps(
                {
                    "entrants": [
                        {
                            "id": "model",
                            "provider": "google",
                            "model": "gemini-3.7-flash",
                            "secret_env": "GOOGLE_API_KEY",
                            "provider_lane": "google",
                            "endpoint_family": "google",
                            "thinking_effort": "medium",
                            "context_limit": 100,
                            "max_output_tokens": 20,
                            "vendor_port": 9901,
                            "pricing": {
                                "input_per_million": 1,
                                "output_per_million": 1,
                                "source": "https://example.test",
                                "verified_at": "now",
                            },
                        }
                    ]
                }
            )
        )
        (root / "campaign.json").write_text(
            json.dumps({"status": status, "entrant_manifest": str(manifest)})
        )
        (root / "manager.json").write_text(
            json.dumps({"status": status, "pid": None, "pgid": None})
        )
        (root / "entrants/model/state.json").write_text(
            json.dumps(
                {
                    "entrant": "model",
                    "provider": "google",
                    "model": "gemini-3.7-flash",
                    "status": "BUILD_COMPLETE",
                    "tree": str(root / "entrants/model/tree"),
                }
            )
        )

    def make_publisher_repo(self, root: Path) -> tuple[Path, dict[str, object]]:
        repo = root / "site"
        (repo / "scripts/lib").mkdir(parents=True)
        (repo / "scripts/data").mkdir(parents=True)
        (repo / "node_modules/@sanity/client").mkdir(parents=True)
        (repo / "node_modules/dotenv").mkdir(parents=True)
        row: dict[str, object] = {
            "id": "fixture-model",
            "model": "fixture-model",
        }
        manifest = {
            "entrants": [
                {
                    "key": "fixture-model",
                    "label": "Fixture Model",
                    "model": "fixture-model",
                    "docId": "brun-baseline-fixture-model-sb70",
                }
            ]
        }
        (repo / cloud_sb7.PUBLISHER_SCRIPT).write_text("console.log('fixture')\n")
        (repo / "scripts/lib/sb7-cloud-publisher.mjs").write_text(
            "export const fixture = true;\n"
        )
        (repo / cloud_sb7.PUBLISHER_MANIFEST).write_text(json.dumps(manifest))
        (repo / "package.json").write_text('{"type":"module"}\n')
        (repo / "package-lock.json").write_text('{"lockfileVersion":3}\n')
        (repo / "node_modules/@sanity/client/package.json").write_text(
            '{"name":"@sanity/client","version":"fixture"}\n'
        )
        (repo / "node_modules/dotenv/package.json").write_text(
            '{"name":"dotenv","version":"fixture"}\n'
        )
        (repo / ".env.local").write_text(
            "SANITY_WRITE_TOKEN=publisher-super-secret\n"
            "NEXT_PUBLIC_SANITY_PROJECT_ID=fixture-project\n"
        )
        (repo / ".gitignore").write_text(".env.local\nnode_modules/\n")
        subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
        subprocess.run(
            ["git", "config", "user.email", "fixture@example.invalid"],
            cwd=repo,
            check=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Fixture"], cwd=repo, check=True
        )
        subprocess.run(["git", "add", "."], cwd=repo, check=True)
        subprocess.run(
            ["git", "commit", "-qm", "fixture publisher"], cwd=repo, check=True
        )
        return repo, row

    def test_manifest_has_exact_unique_models_and_ports(self) -> None:
        manifest = cloud_sb7.load_json(cloud_sb7.DEFAULT_ENTRANTS)
        rows = cloud_sb7.entrants(manifest)
        self.assertEqual(
            [row["model"] for row in rows],
            [
                "glm-5.3",
                "gemini-3.7-flash",
                "gemini-3.1-pro-preview",
                "deepseek-v4-flash",
                "deepseek-v4-pro",
            ],
        )
        self.assertEqual(len({row["vendor_port"] for row in rows}), 5)
        self.assertEqual(len({row["provider_lane"] for row in rows}), 5)
        self.assertEqual(rows[0]["provider"], "zai_api")
        policy = cloud_sb7.spend_policy(manifest, rows)
        self.assertEqual(policy["total_cap"], 400.0)
        self.assertEqual(policy["provider_caps"]["google"], 250.0)
        self.assertIs(policy["launch_all_entrants_concurrently"], True)

    def test_secret_parser_rejects_group_readable_file(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "secrets.env"
            path.write_text("GOOGLE_API_KEY=secret\n")
            path.chmod(0o640)
            with self.assertRaises(SystemExit):
                cloud_sb7.parse_secret_file(path)
            path.chmod(0o600)
            self.assertEqual(
                cloud_sb7.parse_secret_file(path), {"GOOGLE_API_KEY": "secret"}
            )

    def test_child_environment_contains_only_active_credential(self) -> None:
        row = {
            "id": "gemini-3.7-flash",
            "provider": "google",
            "model": "gemini-3.7-flash",
            "secret_env": "GOOGLE_API_KEY",
            "thinking_effort": "medium",
            "context_limit": 100,
            "max_output_tokens": 20,
        }
        state = {
            "profile": "/tmp/profile",
            "tree": "/tmp/campaign/entrant/tree",
            "provider_lifecycle": "/tmp/campaign/entrant/provider-lifecycle.jsonl",
            "budget_config_sha256": "abc123",
        }
        with mock.patch.dict(
            os.environ,
            {
                "ANTHROPIC_API_KEY": "must-not-leak",
                "DEEPSEEK_API_KEY": "must-not-leak",
                "PATH": "/bin",
            },
            clear=True,
        ):
            env = cloud_sb7.child_env(row, state, "active-secret")
        self.assertEqual(env["GOOGLE_API_KEY"], "active-secret")
        self.assertNotIn("ANTHROPIC_API_KEY", env)
        self.assertNotIn("DEEPSEEK_API_KEY", env)
        self.assertEqual(env["GOOSE_THINKING_EFFORT"], "medium")
        self.assertEqual(env["GOOSE_PROVIDER_LIFECYCLE_STRICT"], "true")
        self.assertEqual(env["GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES"], "true")
        self.assertEqual(env["GOOSE_BENCH_BUDGET_CONFIG_SHA256"], "abc123")
        self.assertEqual(env["PATH"], "/usr/bin:/bin:/usr/sbin:/sbin")
        self.assertEqual(env["TMPDIR"], "/tmp/profile/tool-home/tmp")

    def test_admitted_failure_is_never_retryable(self) -> None:
        self.assertEqual(cloud_sb7.classify_build_exit(0, 3), ("BUILD_COMPLETE", None))
        self.assertEqual(
            cloud_sb7.classify_build_exit(7, 0)[0], "PRE_ADMISSION_FAILURE"
        )
        status, reason = cloud_sb7.classify_build_exit(7, 1)
        self.assertEqual(status, "INCOMPLETE")
        self.assertIn("never retried", reason or "")

    def test_campaign_path_is_derived_from_tree(self) -> None:
        row = {
            "id": "glm-5.3",
            "provider": "zai_api",
            "model": "glm-5.3",
            "secret_env": "ZHIPU_API_KEY",
            "thinking_effort": "max",
            "context_limit": 100,
            "max_output_tokens": 20,
        }
        state = {
            "profile": "/tmp/campaign/entrants/glm/profile",
            "tree": "/tmp/campaign/entrants/glm/tree",
            "provider_lifecycle": "/tmp/campaign/entrants/glm/provider-lifecycle.jsonl",
            "budget_config_sha256": "def456",
        }
        with mock.patch.dict(os.environ, {"PATH": "/bin"}, clear=True):
            env = cloud_sb7.child_env(row, state, "secret")
        self.assertEqual(env["GOOSE_BENCH_CAMPAIGN"], "/tmp/campaign")
        self.assertEqual(
            env["GOOSE_BENCH_BUDGET_LEDGER"], "/tmp/campaign/budget-ledger.json"
        )

    def test_lifecycle_summary_requires_matching_admission_and_terminal(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "lifecycle.jsonl"
            path.write_text(
                "\n".join(
                    [
                        json.dumps({"state": "queued"}),
                        json.dumps({"state": "admitted"}),
                        json.dumps({"state": "first_item", "at": "now"}),
                        json.dumps({"state": "provider_terminal"}),
                    ]
                )
                + "\n"
            )
            summary = cloud_sb7.lifecycle_summary(path)
        self.assertEqual(summary["admitted"], 1)
        self.assertEqual(summary["terminal"], 1)
        self.assertEqual(summary["first_output_at"], "now")
        self.assertEqual(summary["malformed_lines"], 0)

    def test_outstanding_budget_reservation_makes_ambiguous_work_visible(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            ledger = Path(raw) / "budget-ledger.json"
            ledger.write_text(
                json.dumps(
                    {
                        "outstanding": {
                            "req-match": {
                                "provider": "google",
                                "model": "gemini-3.7-flash",
                            },
                            "req-other": {
                                "provider": "zai_api",
                                "model": "glm-5.3",
                            },
                        }
                    }
                )
            )
            ids, error = cloud_sb7.entrant_outstanding_reservations(
                {"budget_ledger": str(ledger)},
                {"provider": "google", "model": "gemini-3.7-flash"},
            )
        self.assertIsNone(error)
        self.assertEqual(ids, ["req-match"])

    def test_binary_marker_scan_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "goose"
            binary.write_bytes(b"prefix GOOSE_PROVIDER_LIFECYCLE_FILE suffix")
            missing = cloud_sb7.binary_missing_markers(binary)
        self.assertIn("GOOSE_BENCH_BUDGET_LEDGER", missing)
        self.assertNotIn("GOOSE_PROVIDER_LIFECYCLE_FILE", missing)

    def test_hash_tree_changes_with_content_not_mtime(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            target = root / "a.txt"
            target.write_text("one")
            first = cloud_sb7.hash_tree(root)
            os.utime(target, (1, 1))
            self.assertEqual(first, cloud_sb7.hash_tree(root))
            target.write_text("two")
            self.assertNotEqual(first, cloud_sb7.hash_tree(root))

    def test_owned_process_group_is_terminated(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            log_path = Path(raw) / "detached.log"
            proc = cloud_sb7.launch_detached(
                [sys.executable, "-c", "import time; time.sleep(120)"], log_path
            )
            try:
                self.assertTrue(cloud_sb7.process_alive(proc.pid))
                self.assertTrue(cloud_sb7.stop_group(proc.pid, grace_seconds=0.1))
                proc.wait(timeout=5)
                self.assertFalse(cloud_sb7.process_alive(proc.pid))
            finally:
                if proc.poll() is None:
                    os.killpg(proc.pid, 9)

    def test_atomic_json_never_leaves_partial_state(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "state.json"
            cloud_sb7.atomic_json(path, {"status": "PLANNED"})
            self.assertEqual(json.loads(path.read_text()), {"status": "PLANNED"})
            self.assertEqual(list(path.parent.glob(".state.json.*")), [])

    def test_dead_manager_recovery_does_not_kill_live_supervisor(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "RUNNING")
            manager = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            supervisor = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                cloud_sb7.atomic_json(
                    root / "manager.json",
                    {
                        "status": "RUNNING",
                        "pid": manager.pid,
                        "pgid": manager.pid,
                        "identity": cloud_sb7.process_identity(manager.pid),
                    },
                )
                cloud_sb7.update_state(
                    root,
                    "model",
                    status="BUILD_RUNNING",
                    supervisor_pid=supervisor.pid,
                    supervisor_pgid=supervisor.pid,
                    supervisor_identity=cloud_sb7.process_identity(supervisor.pid),
                )
                os.kill(manager.pid, 9)
                manager.wait(timeout=5)

                self.assertTrue(cloud_sb7.recover_dead_manager(root))
                self.assertTrue(cloud_sb7.process_alive(supervisor.pid))
                self.assertEqual(
                    cloud_sb7.load_json(root / "manager.json")["status"], "RECOVERED"
                )
                self.assertEqual(
                    cloud_sb7.read_state(root, "model")["status"], "BUILD_RUNNING"
                )
            finally:
                cloud_sb7.stop_group(supervisor.pid, grace_seconds=0.1)
                supervisor.wait(timeout=5)
                if manager.poll() is None:
                    cloud_sb7.stop_group(manager.pid, grace_seconds=0.1)

    def test_interrupted_scorer_is_stopped_and_next_attempt_is_immutable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "SCORING")
            old_tree = root / "scores/model/attempt-1/tree"
            old_tree.mkdir(parents=True)
            scorer = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                cloud_sb7.update_state(
                    root,
                    "model",
                    status="SCORING",
                    score_attempts=1,
                    score_pid=scorer.pid,
                    score_pgid=scorer.pid,
                    score_identity=cloud_sb7.process_identity(scorer.pid),
                )
                cloud_sb7.recover_interrupted_scoring(root)
                scorer.wait(timeout=5)
                state = cloud_sb7.read_state(root, "model")
                self.assertEqual(state["status"], "SCORE_FAILED")
                self.assertEqual(cloud_sb7.next_score_attempt(root, "model", state), 2)
                self.assertTrue(old_tree.is_dir())
            finally:
                if scorer.poll() is None:
                    cloud_sb7.stop_group(scorer.pid, grace_seconds=0.1)

    def test_restart_accepts_build_complete_and_scoring_campaigns(self) -> None:
        class Launched:
            pid = 99999999

        for status in ("BUILD_COMPLETE", "SCORING"):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                self.make_recovery_campaign(root, status)
                with mock.patch.object(
                    cloud_sb7, "launch_detached", return_value=Launched()
                ) as launch:
                    self.assertEqual(cloud_sb7.start(root), 0)
                launch.assert_called_once()
                self.assertEqual(
                    cloud_sb7.load_json(root / "manager.json")["status"], "STARTING"
                )

    def test_publisher_snapshot_pins_commit_inputs_runtime_without_secrets(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo, row = self.make_publisher_repo(Path(raw))
            snapshot = cloud_sb7.publisher_snapshot(repo, [row])
            serialized = json.dumps(snapshot)
            self.assertEqual(snapshot["repo"], str(repo.resolve()))
            self.assertEqual(
                snapshot["entries"]["fixture-model"]["doc_id"],
                "brun-baseline-fixture-model-sb70",
            )
            self.assertIn(str(cloud_sb7.PUBLISHER_SCRIPT), snapshot["tracked_hashes"])
            self.assertIn(
                "node_modules/@sanity/client/package.json",
                snapshot["runtime_hashes"],
            )
            self.assertNotIn("publisher-super-secret", serialized)
            self.assertNotIn("fixture-project", serialized)

            (repo / cloud_sb7.PUBLISHER_SCRIPT).write_text("console.log('changed')\n")
            with self.assertRaisesRegex(SystemExit, "must be clean"):
                cloud_sb7.publisher_snapshot(repo, [row])


if __name__ == "__main__":
    unittest.main()
