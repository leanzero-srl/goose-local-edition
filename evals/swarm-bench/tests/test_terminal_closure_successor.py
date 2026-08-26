from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).parents[1]
MODULE_PATH = ROOT / "terminal_closure_successor.py"
SPEC = importlib.util.spec_from_file_location("terminal_closure_successor", MODULE_PATH)
assert SPEC and SPEC.loader
successor = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(successor)


class ApprovedAnchorTests(unittest.TestCase):
    def test_checked_in_controller_matches_approved_anchor(self) -> None:
        self.assertEqual(
            successor.sha256_file(ROOT / "terminal_closure.py"),
            successor.APPROVED_CONTROLLER_SOURCE_SHA256,
        )
        self.assertEqual(
            successor.APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION,
            {
                "v21-r5": "0bfa5e8d13708919d69c1cdd26f52baa77b2f47da0ef7a41818477bec3fe3e04"
            },
        )
        self.assertEqual(
            successor.APPROVED_TERMINAL_PREDECESSOR_CONFIG_SHA256_BY_GENERATION,
            {
                "v21-r5": "5941e31cf886ed3ddaab649fe18961c7e68fbb2af38a495a300fa58355735891"
            },
        )


class ScoreAdoptionBindingTests(unittest.TestCase):
    def contract(self, *, failure: str | None = None, port: int = 18970) -> dict[str, object]:
        attempt = pathlib.Path("/private/tmp/source-closure/scoring/attempt-1")
        state = attempt.parent.parent
        evidence = {
            "generation": "v21-r5",
            "live_root": "/private/tmp/live-r5",
            "run_dir": "/private/tmp/live-r5/swarm-3node-qwen38-brainwaves-r0",
            "state_dir": "/private/tmp/new-successor",
            "run_id": "swarm-20260826-123456789",
        }
        publication = {
            "target_document_id": successor.TARGET_DOCUMENT_ID,
            "protected_document_ids": list(successor.PROTECTED_DOCUMENT_ID_ORDER),
            "provenance_marker": "Brainwaves v21",
        }
        expected_failure = (
            "score contains degraded product-probe evidence in probe_unavailable"
        )
        config = {
            "armed": True,
            "closure_generation": "v21-r5",
            "live_root": evidence["live_root"],
            "run_dir": evidence["run_dir"],
            "publication": publication,
            "controller_sha256": "a" * 64,
            "expected": {
                "run_id": evidence["run_id"],
                "fixture_seed": "0123456789abcdef",
            },
        }
        values = {
            state / "config.json": config,
            state / "failure.json": {
                "error_type": "ClosureError",
                "message": "attempt-1 did not prove descendant cleanup; refusing retry",
            },
            attempt / "worker-result.json": {
                "schema_version": 1,
                "attempt": 1,
                "completed_at": "2026-08-26T08:44:58+00:00",
                "exit_code": 70,
                "failure": failure if failure is not None else expected_failure,
                "score_sha256": None,
            },
            attempt / "job.json": {
                "schema_version": 1,
                "attempt": 1,
                "clone": str(attempt / "tree"),
                "score_output": str(attempt / "raw-score.json"),
                "score_log": str(attempt / "score.log"),
                "result": str(attempt / "worker-result.json"),
                "raw_tree": evidence["run_dir"],
                "raw_tree_sha256": "b" * 64,
                "seed": "0123456789abcdef",
                "port": port,
            },
            state / "raw-tree-seal.json": {
                "root": evidence["run_dir"],
                "tree_sha256": "b" * 64,
            },
            attempt / "clone-seal.json": {"tree_sha256": "b" * 64},
        }

        def fake_read_json(path: pathlib.Path, **_: object) -> dict[str, object]:
            return values[path.resolve()]

        with (
            mock.patch.object(successor, "require_regular", side_effect=lambda path, **_: path.resolve()),
            mock.patch.object(successor, "stable_file_sha256", return_value=("a" * 64, 1)),
            mock.patch.object(successor, "stable_tree_content_sha256", return_value="c" * 64),
            mock.patch.object(successor, "read_json", side_effect=fake_read_json),
        ):
            return successor.score_adoption_contract(attempt, evidence, publication)

    def test_exact_post_score_failure_can_be_bound_for_adoption(self) -> None:
        contract = self.contract()
        self.assertEqual(contract["source_clone_tree_sha256"], "c" * 64)
        self.assertEqual(contract["source_raw_tree_sha256"], "b" * 64)

    def test_tampered_job_fails_closed(self) -> None:
        with self.assertRaisesRegex(successor.SuccessorBindingError, "job or initial seal"):
            self.contract(port=18971)

    def test_wrong_failure_fails_closed(self) -> None:
        with self.assertRaisesRegex(successor.SuccessorBindingError, "failure boundary"):
            self.contract(failure="some other failure")

    def test_linked_source_tree_fails_before_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            real = root / "real"
            real.mkdir()
            linked = root / "linked"
            linked.symlink_to(real, target_is_directory=True)
            with self.assertRaisesRegex(
                successor.SuccessorBindingError, "not a real directory"
            ):
                successor.stable_tree_content_sha256(linked)


class PublisherAdoptionBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.state = self.root / "source"
        (self.state / "closure-instrument").mkdir(parents=True)
        self.evidence = {
            "generation": "v21-r5",
            "live_root": str(self.root / "live"),
            "run_dir": str(self.root / "live/run"),
            "state_dir": str(self.root / "successor"),
            "run_id": "swarm-20260826-123456789",
        }
        self.publication = {
            "target_document_id": successor.TARGET_DOCUMENT_ID,
            "protected_document_ids": list(successor.PROTECTED_DOCUMENT_ID_ORDER),
            "provenance_marker": "Brainwaves v21",
        }
        self.authoritative = {
            "entrant": "swarm-3node-qwen38-brainwaves",
            "fixture_seed": "0123456789abcdef",
        }
        self.write("authoritative-verdict.json", self.authoritative)
        authoritative_sha256 = successor.sha256_file(
            self.state / "authoritative-verdict.json"
        )
        self.write(
            "config.json",
            {
                "armed": True,
                "closure_generation": "v21-r5",
                "live_root": self.evidence["live_root"],
                "run_dir": self.evidence["run_dir"],
                "publication": self.publication,
                "expected": {
                    "run_id": self.evidence["run_id"],
                    "fixture_seed": "0123456789abcdef",
                },
                "publisher": {"sha256": "0" * 64},
            },
        )
        self.write("state.json", {"phase": "stopped"})
        self.write("publisher.pid.json", {"pid": 20})
        self.write("supervisor.pid.json", {"pid": 21})
        self.write(
            "scoring-provenance.json",
            {"authoritative_verdict_sha256": authoritative_sha256},
        )
        publisher = self.state / "closure-instrument/seed-fleet-brainwaves-sb70.mjs"
        publisher.write_text("export const guarded = true;\n", encoding="utf-8")
        config = json.loads((self.state / "config.json").read_text(encoding="utf-8"))
        config["publisher"]["sha256"] = successor.sha256_file(publisher)
        self.write("config.json", config)
        self.write(
            "publisher-state.json",
            {
                "schema_version": 1,
                "initialized": True,
                "target_document_id": successor.TARGET_DOCUMENT_ID,
                "authoritative_verdict_sha256": authoritative_sha256,
                "protected_before_sha256": "1" * 64,
                "planned_document_sha256": "2" * 64,
                "document_written": True,
                "document_sha256": "2" * 64,
                "assets": [
                    {
                        "shot_key": "3" * 64,
                        "sha256": "4" * 64,
                        "pixels_sha256": "5" * 64,
                        "asset_id": "image-fixture-1280x800-png",
                        "width": 1280,
                        "height": 800,
                        "filename": "final.png",
                        "caption": "Final render",
                    }
                ],
            },
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write(self, relative: str, value: object) -> None:
        path = self.state / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")

    def contract(self) -> dict[str, object]:
        with mock.patch.object(successor, "process_exists", return_value=False):
            return successor.publisher_adoption_contract(
                self.state / "publisher-state.json",
                self.evidence,
                self.publication,
            )

    def test_exact_stopped_create_only_publication_can_resume(self) -> None:
        contract = self.contract()
        self.assertTrue(contract["create_only_resume"])
        self.assertTrue(contract["document_written"])
        self.assertEqual(contract["target_document_id"], successor.TARGET_DOCUMENT_ID)

    def test_live_writer_fails_closed(self) -> None:
        with (
            mock.patch.object(successor, "process_exists", side_effect=lambda pid: pid == 20),
            self.assertRaisesRegex(successor.SuccessorBindingError, "publisher is still live"),
        ):
            successor.publisher_adoption_contract(
                self.state / "publisher-state.json",
                self.evidence,
                self.publication,
            )

    def test_incomplete_or_already_complete_source_fails_closed(self) -> None:
        publisher_state = json.loads(
            (self.state / "publisher-state.json").read_text(encoding="utf-8")
        )
        publisher_state["document_written"] = False
        self.write("publisher-state.json", publisher_state)
        with self.assertRaisesRegex(successor.SuccessorBindingError, "incomplete"):
            self.contract()
        publisher_state["document_written"] = True
        self.write("publisher-state.json", publisher_state)
        self.write("publication-receipt.json", {"unexpected": True})
        with self.assertRaisesRegex(successor.SuccessorBindingError, "already complete"):
            self.contract()


class SuccessorBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.live_root = self.root / "local-sb7-engine-v21-r5"
        self.run_dir = self.live_root / "swarm-3node-qwen38-brainwaves-r0"
        self.state_dir = self.root / "local-sb7-engine-v21-r5-terminal-closure"
        self.live_root.mkdir()
        self.run_dir.mkdir()

        self.launcher = self.live_root / "launch_local_v21_r5.py"
        self.launcher.write_text("raise SystemExit('fixture only')\n", encoding="utf-8")
        self.launcher.chmod(0o400)
        self.binary = self.live_root / "bin" / "goose-successor"
        self.binary.parent.mkdir()
        self.binary.write_bytes(b"successor-binary")
        self.binary.chmod(0o500)
        (self.live_root / "fleet-seal.json").write_text("{}\n", encoding="utf-8")

        self.candidate_commit = "1" * 40
        self.candidate_tree = "2" * 40
        self.candidate_branch = "codex/swarm-v21-pillar-flow"
        self.candidate_remote_ref = f"origin/{self.candidate_branch}"
        self.wrapper_sha256 = "a" * 64
        instrument_files = {
            "evals/swarm-bench/bench/run_build_local_v20.py": "3" * 64,
            "evals/swarm-bench/bench/score_sb7.py": "4" * 64,
        }
        manifest = {
            "schema_version": 2,
            "prepared_at": "2026-08-26T05:24:32.251777+00:00",
            "candidate_branch": self.candidate_branch,
            "candidate_remote_ref": self.candidate_remote_ref,
            "candidate_commit": self.candidate_commit,
            "candidate_tree": self.candidate_tree,
            "candidate_clean": True,
            "binary": {
                "path": str(self.binary),
                "sha256": successor.sha256_file(self.binary),
                "size_bytes": self.binary.stat().st_size,
                "source_commit": self.candidate_commit,
                "source_tree": self.candidate_tree,
            },
            "instrument_provenance": {
                "candidate_archive": {
                    "commit": self.candidate_commit,
                    "tree": self.candidate_tree,
                    "scope": [
                        "evals/swarm-bench",
                        "scripts/monitor_swarm_run.py",
                    ],
                    "file_count": 1,
                    "inventory_sha256": "b" * 64,
                },
                "inherited_overlay": {
                    "source_run": str(
                        self.root / "local-sb7-engine-v21-r4"
                    ),
                    "source_manifest_sha256": "c" * 64,
                    "files": {
                        "evals/swarm-bench/bench/run_build_local_v20.py": "3"
                        * 64
                    },
                },
                "total_file_count": 2,
                "tracked_only_archive": True,
                "python_cache_debris_forbidden": True,
                "symlinks_forbidden": True,
            },
            "files": instrument_files,
            "wrapper_sha256": self.wrapper_sha256,
            "runtime_policy": {
                "child_environment": "fixed-explicit-allowlist",
                "deferred_live_fleet_seal": True,
                "exact_context_length_by_role": successor.EXACT_CONTEXT_BY_ROLE,
                "lm_studio_cli_path": str(self.root / "lms"),
                "lm_studio_cli_sha256": "d" * 64,
                "minimum_context_length": min(
                    successor.EXACT_CONTEXT_BY_ROLE.values()
                ),
                "monitor_policy": "observation-only",
            },
            "sb7_policy": {
                "spec_and_scorer_unchanged_from_v6": True,
                "website_surface": "stable-sb7",
                "publish_from_run_build_auto_score": False,
                "entrant": "swarm-3node-qwen38-brainwaves",
                "publication_document_id": successor.TARGET_DOCUMENT_ID,
                "protected_document_ids": list(
                    successor.PROTECTED_DOCUMENT_ID_ORDER
                ),
            },
            "publisher_closure": {
                "binding": "deferred-until-authenticated-run-started-and-fixture-seed",
                "protected_publication_untouched_during_freeze": True,
                "publish_from_run_build_auto_score": False,
                "publisher_present_in_main_instrument": False,
            },
            "privacy": {
                "environment_values_persisted": False,
                "raw_argv_persisted": False,
                "secret_fields_persisted": False,
            },
        }
        self.manifest_path = self.live_root / "instrument-manifest.json"
        self.manifest_path.write_text(
            json.dumps(manifest, sort_keys=True) + "\n", encoding="utf-8"
        )
        self.manifest_path.chmod(0o400)
        launch = {
            "schema_version": 1,
            "candidate": {
                "commit": self.candidate_commit,
                "tree": self.candidate_tree,
                "path": str(self.root / "candidate"),
                "remote_commit": self.candidate_commit,
                "remote_ref": self.candidate_remote_ref,
            },
            "binary": {
                "path": str(self.binary),
                "sha256": successor.sha256_file(self.binary),
            },
            "entrant": "swarm-3node-qwen38-brainwaves",
            "publication_document_id": successor.TARGET_DOCUMENT_ID,
            "vendor_port": 18970,
            "launch_controller_sha256": successor.sha256_file(self.launcher),
            "instrument_manifest_sha256": successor.sha256_file(self.manifest_path),
            "wrapper_sha256": self.wrapper_sha256,
            "run_started_identity": {
                "run_id": "swarm-20260826-123456789",
                "working_dir": str(self.run_dir),
            },
            "harness": {"pid": 12001, "identity_sha256": "5" * 64},
            "goose": {"pid": 12002, "identity_sha256": "6" * 64},
            "monitor": {"pid": 12003, "identity_sha256": "7" * 64},
        }
        self.launch_path = self.live_root / "launch.json"
        self.launch_path.write_text(
            json.dumps(launch, sort_keys=True) + "\n", encoding="utf-8"
        )

        self.publisher = self.root / "publisher.mjs"
        self.publisher.write_text("export const guarded = true;\n", encoding="utf-8")
        self.publisher.chmod(0o400)
        self.usage_policy = self.root / "usage_impairment.py"
        self.usage_policy.write_bytes((ROOT / "usage_impairment.py").read_bytes())
        self.usage_policy.chmod(0o400)
        self.base_config_path = self.root / "stopped-r4-config.json"
        self.base_config = self._base_config()
        self.base_config_path.write_text(
            json.dumps(self.base_config, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        self.base_config_path.chmod(0o600)
        self.base_config_mode = self.base_config_path.stat().st_mode & 0o777
        predecessor_anchor = mock.patch.object(
            successor,
            "APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION",
            {"v21-r5": successor.sha256_file(self.base_config_path)},
        )
        controller_anchor = mock.patch.object(
            successor,
            "APPROVED_CONTROLLER_SOURCE_SHA256",
            successor.sha256_file(ROOT / "terminal_closure.py"),
        )
        predecessor_anchor.start()
        controller_anchor.start()
        self.addCleanup(predecessor_anchor.stop)
        self.addCleanup(controller_anchor.stop)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _base_config(self) -> dict[str, object]:
        old_root = self.root / "local-sb7-engine-v21-r4"
        old_state = self.root / "local-sb7-engine-v21-r4-terminal-closure"
        return {
            "schema_version": 1,
            "armed": True,
            "binding": {"generation": "v21-r4"},
            "closure_generation": "v21-r4",
            "live_root": str(old_root),
            "run_dir": str(old_root / "swarm-3node-qwen38-brainwaves-r0"),
            "state_dir": str(old_state),
            "score_lock_path": str(self.root / "old-score.lock"),
            "bound_config_path": str(old_state / "config.json"),
            "controller_sha256": "8" * 64,
            "max_score_attempts": 2,
            "max_publish_attempts": 2,
            "score_timeout_seconds": 60,
            "publish_timeout_seconds": 60,
            "expected": {
                "candidate_commit": "8" * 40,
                "candidate_tree": "9" * 40,
                "binary_path": str(old_root / "bin/goose-old"),
                "binary_sha256": "a" * 64,
                "launch_controller_path": str(old_root / "launch_local_v21_r4.py"),
                "launch_controller_sha256": "b" * 64,
                "instrument_manifest_sha256": "c" * 64,
                "run_id": "swarm-20260826-000000001",
                "fixture_seed": "1234567890abcdef",
                "models": sorted(successor.EXACT_MODEL_ALIASES),
                "instrument_files": {"old": "d" * 64},
                "launch_sha256": "e" * 64,
                "run_started_sha256": "f" * 64,
                "trace_header_sha256": "0" * 64,
                "fleet_seal_sha256": "1" * 64,
                "fleet_binding_sha256": "2" * 64,
                "vendor_port": 18970,
                "entrant": "swarm-3node-qwen38-brainwaves",
                "raw_scorer_version": "sb-7.0-rc",
                "check_count": 91,
                "telemetry_nodes": ["gabee", "mihai", "workhorse"],
            },
            "publication": {
                "target_document_id": successor.TARGET_DOCUMENT_ID,
                "protected_document_ids": list(
                    successor.PROTECTED_DOCUMENT_ID_ORDER
                ),
                "provenance_marker": "Brainwaves v21",
            },
            "publisher": {
                "path": str(self.publisher),
                "sha256": successor.sha256_file(self.publisher),
                "git_commit": "3" * 40,
                "site_root": str(self.root / "website"),
                "node": str(self.root / "node"),
                "node_sha256": "3" * 64,
                "package_lock": str(self.root / "package-lock.json"),
                "package_lock_sha256": "4" * 64,
                "package_json": str(self.root / "package.json"),
                "package_json_sha256": "5" * 64,
                "env_file": str(self.root / ".env.local"),
                "base_url": "https://example.invalid",
            },
            "usage_policy": {
                "path": str(self.usage_policy),
                "sha256": successor.sha256_file(self.usage_policy),
            },
            "runtime": {
                "lsof": "/usr/sbin/lsof",
                "playwright": {
                    "module_root": str(self.root / "playwright-module"),
                    "module_tree_sha256": "6" * 64,
                    "version": "1.0.0",
                    "browsers_json": "browsers.json",
                    "browsers_json_sha256": "7" * 64,
                    "browser_name": "chromium",
                    "browser_revision": "1",
                    "installed_browsers_path": str(self.root / "playwright-browsers"),
                    "browser_directory": "chromium-1",
                    "browser_tree_sha256": "8" * 64,
                    "executable": "chrome",
                    "executable_sha256": "9" * 64,
                },
            },
        }

    def _rewrite_manifest(self, manifest: dict[str, object]) -> None:
        self.manifest_path.chmod(0o600)
        self.manifest_path.write_text(
            json.dumps(manifest, sort_keys=True) + "\n", encoding="utf-8"
        )
        self.manifest_path.chmod(0o400)
        launch = json.loads(self.launch_path.read_text(encoding="utf-8"))
        launch["instrument_manifest_sha256"] = successor.sha256_file(
            self.manifest_path
        )
        self.launch_path.write_text(
            json.dumps(launch, sort_keys=True) + "\n", encoding="utf-8"
        )

    def test_stale_r4_rejected_exact_successor_accepted_without_mutating_protected_config(
        self,
    ) -> None:
        base_before = self.base_config_path.read_bytes()
        receipt = successor.generate_successor(
            generation="v21-r5",
            live_root=self.live_root,
            state_dir=self.state_dir,
            base_config_path=self.base_config_path,
            controller_source=ROOT / "terminal_closure.py",
        )
        self.assertEqual(self.base_config_path.read_bytes(), base_before)
        self.assertEqual(
            self.base_config_path.stat().st_mode & 0o777, self.base_config_mode
        )
        self.assertTrue(receipt["base_config_unchanged"])
        self.assertEqual(
            receipt["protected_document_ids"],
            list(successor.PROTECTED_DOCUMENT_ID_ORDER),
        )

        generated = successor.load_generated_module(pathlib.Path(receipt["controller"]))
        self.assertEqual(
            generated.V19_BOUND_LAUNCH_SHA256,
            successor.sha256_file(self.launch_path),
        )
        self.assertEqual(generated.V19_BOUND_RUN_ID, "swarm-20260826-123456789")
        self.assertEqual(generated.V19_INSTRUMENT_MANIFEST_SCHEMA_VERSION, 2)
        self.assertEqual(
            generated.V19_BOUND_PROCESSES,
            {
                "harness": {"pid": 12001, "identity_sha256": "5" * 64},
                "goose": {"pid": 12002, "identity_sha256": "6" * 64},
                "monitor": {"pid": 12003, "identity_sha256": "7" * 64},
            },
        )
        exact = generated.load_config(pathlib.Path(receipt["template"]))
        generated.validate_config(exact, allow_unarmed=True)
        self.assertFalse((self.state_dir / "closure-instrument").exists())
        self.assertEqual(
            pathlib.Path(exact["publisher"]["path"]).parent.resolve(),
            (self.state_dir / "bootstrap").resolve(),
        )
        frozen = json.loads(json.dumps(exact))
        instrument = self.state_dir / "closure-instrument"
        frozen["publisher"]["path"] = str(
            instrument / "seed-fleet-brainwaves-sb70.mjs"
        )
        frozen["usage_policy"]["path"] = str(instrument / "usage_impairment.py")
        generated.materialize_closure_instrument_snapshot(
            self.state_dir,
            {
                "terminal_closure.py": {
                    "path": receipt["controller"],
                    "sha256": receipt["controller_sha256"],
                    "mode": 0o500,
                },
                "seed-fleet-brainwaves-sb70.mjs": {
                    "path": exact["publisher"]["path"],
                    "sha256": exact["publisher"]["sha256"],
                    "mode": 0o500,
                },
                "usage_impairment.py": {
                    "path": exact["usage_policy"]["path"],
                    "sha256": exact["usage_policy"]["sha256"],
                    "mode": 0o400,
                },
            },
            frozen,
        )
        self.assertEqual(
            {path.name for path in instrument.iterdir()},
            {
                "terminal_closure.py",
                "seed-fleet-brainwaves-sb70.mjs",
                "usage_impairment.py",
                "config.json",
                "manifest.json",
            },
        )
        self.assertEqual(
            pathlib.Path(receipt["binding_receipt"]).parent.resolve(),
            self.state_dir.resolve(),
        )
        self.assertEqual(
            exact["publication"],
            {
                "target_document_id": successor.TARGET_DOCUMENT_ID,
                "protected_document_ids": list(
                    successor.PROTECTED_DOCUMENT_ID_ORDER
                ),
                "provenance_marker": "Brainwaves v21",
            },
        )
        with self.assertRaises(generated.ClosureError):
            generated.validate_config(
                generated.load_config(self.base_config_path), allow_unarmed=True
            )
        self.assertEqual(self.base_config_path.read_bytes(), base_before)
        self.assertEqual(
            self.base_config_path.stat().st_mode & 0o777, self.base_config_mode
        )
        with self.assertRaisesRegex(generated.ClosureError, "generation identity changed"):
            generated.validate_v19_config(self.base_config, allow_unarmed=True)

        binding = json.loads(
            pathlib.Path(receipt["binding_receipt"]).read_text(encoding="utf-8")
        )
        self.assertEqual(binding["candidate_commit"], self.candidate_commit)
        self.assertEqual(binding["candidate_tree"], self.candidate_tree)
        self.assertEqual(binding["binary_sha256"], successor.sha256_file(self.binary))
        self.assertEqual(binding["run_id"], "swarm-20260826-123456789")
        self.assertEqual(
            binding["processes"],
            {
                "harness": {"pid": 12001, "identity_sha256": "5" * 64},
                "goose": {"pid": 12002, "identity_sha256": "6" * 64},
                "monitor": {"pid": 12003, "identity_sha256": "7" * 64},
            },
        )
        self.assertEqual(binding["target_document_id"], successor.TARGET_DOCUMENT_ID)
        self.assertEqual(
            binding["protected_document_ids"],
            list(successor.PROTECTED_DOCUMENT_ID_ORDER),
        )
        self.assertEqual(
            binding["predecessor_config_sha256"], successor.sha256_bytes(base_before)
        )
        self.assertEqual(
            receipt["predecessor_config_sha256"], successor.sha256_bytes(base_before)
        )

        exact["publication"]["protected_document_ids"].append(
            "brun-fleet-qwen38-sb70"
        )
        with self.assertRaisesRegex(generated.ClosureError, "protected"):
            generated.validate_config(exact, allow_unarmed=True)

    def test_schema_two_manifest_rejects_stale_or_tampered_contracts(self) -> None:
        original = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        mutations = {
            "stale schema": lambda value: value.__setitem__("schema_version", 1),
            "extra field": lambda value: value.__setitem__("unexpected", True),
            "binary source": lambda value: value["binary"].__setitem__(
                "source_tree", "f" * 40
            ),
            "binary commit": lambda value: value["binary"].__setitem__(
                "source_commit", "f" * 40
            ),
            "binary size": lambda value: value["binary"].__setitem__(
                "size_bytes", value["binary"]["size_bytes"] + 1
            ),
            "archive count": lambda value: value["instrument_provenance"][
                "candidate_archive"
            ].__setitem__("file_count", 2),
            "total count": lambda value: value["instrument_provenance"].__setitem__(
                "total_file_count", 3
            ),
            "archive flag": lambda value: value["instrument_provenance"].__setitem__(
                "tracked_only_archive", False
            ),
            "overlay digest": lambda value: value["instrument_provenance"][
                "inherited_overlay"
            ]["files"].__setitem__(
                "evals/swarm-bench/bench/run_build_local_v20.py", "e" * 64
            ),
            "protected order": lambda value: value["sb7_policy"].__setitem__(
                "protected_document_ids",
                list(reversed(successor.PROTECTED_DOCUMENT_ID_ORDER)),
            ),
            "privacy": lambda value: value["privacy"].__setitem__(
                "raw_argv_persisted", True
            ),
            "wrapper": lambda value: value.__setitem__("wrapper_sha256", "f" * 64),
        }
        for identity, mutate in mutations.items():
            with self.subTest(identity=identity):
                changed = json.loads(json.dumps(original))
                mutate(changed)
                self._rewrite_manifest(changed)
                with self.assertRaises(successor.SuccessorBindingError):
                    successor.successor_evidence(
                        "v21-r5", self.live_root, self.state_dir
                    )
                self.assertFalse(self.state_dir.exists())
        self._rewrite_manifest(original)

    def test_oversized_release_binary_streams_with_a_cap_and_rejects_tamper(
        self,
    ) -> None:
        oversized_bytes = 64 * 1024 * 1024 + 1
        self.binary.chmod(0o600)
        with self.binary.open("r+b") as handle:
            handle.truncate(oversized_bytes)
        self.binary.chmod(0o500)

        digest, size = successor.stable_file_sha256(self.binary, read_only=True)
        self.assertEqual(size, oversized_bytes)
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "size bound"
        ):
            successor.stable_file_sha256(
                self.binary,
                read_only=True,
                maximum_bytes=64 * 1024 * 1024,
            )

        launch = json.loads(self.launch_path.read_text(encoding="utf-8"))
        launch["binary"]["sha256"] = digest
        self.launch_path.write_text(
            json.dumps(launch, sort_keys=True) + "\n", encoding="utf-8"
        )
        manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        manifest["binary"]["sha256"] = digest
        manifest["binary"]["size_bytes"] = size
        self._rewrite_manifest(manifest)

        evidence = successor.successor_evidence(
            "v21-r5", self.live_root, self.state_dir
        )
        self.assertEqual(evidence["binary_sha256"], digest)
        self.assertFalse(self.state_dir.exists())

        self.binary.chmod(0o600)
        with self.binary.open("ab") as handle:
            handle.write(b"tamper")
        self.binary.chmod(0o500)
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "binary path/hash/mode"
        ):
            successor.successor_evidence(
                "v21-r5", self.live_root, self.state_dir
            )

    def test_generation_is_append_only_and_rejects_different_successor_bytes(self) -> None:
        kwargs = {
            "generation": "v21-r5",
            "live_root": self.live_root,
            "state_dir": self.state_dir,
            "base_config_path": self.base_config_path,
            "controller_source": ROOT / "terminal_closure.py",
        }
        first = successor.generate_successor(**kwargs)
        controller_path = pathlib.Path(first["controller"])
        template_path = pathlib.Path(first["template"])
        first_controller = controller_path.read_bytes()
        first_template = template_path.read_bytes()
        first_modes = (
            controller_path.stat().st_mode & 0o777,
            template_path.stat().st_mode & 0o777,
        )
        second = successor.generate_successor(**kwargs)
        self.assertEqual(first["controller_sha256"], second["controller_sha256"])
        self.assertEqual(first["template_sha256"], second["template_sha256"])
        self.assertEqual(controller_path.read_bytes(), first_controller)
        self.assertEqual(template_path.read_bytes(), first_template)
        self.assertEqual(
            (
                controller_path.stat().st_mode & 0o777,
                template_path.stat().st_mode & 0o777,
            ),
            first_modes,
        )

        launch = json.loads(self.launch_path.read_text(encoding="utf-8"))
        launch["run_started_identity"]["run_id"] = "swarm-20260826-987654321"
        self.launch_path.write_text(json.dumps(launch) + "\n", encoding="utf-8")
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "append-only successor output changed"
        ):
            successor.generate_successor(**kwargs)

    def test_unapproved_predecessor_and_controller_are_rejected(self) -> None:
        kwargs = {
            "generation": "v21-r5",
            "live_root": self.live_root,
            "state_dir": self.state_dir,
            "base_config_path": self.base_config_path,
            "controller_source": ROOT / "terminal_closure.py",
        }
        with mock.patch.object(
            successor,
            "APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION",
            {"v21-r5": "0" * 64},
        ):
            with self.assertRaisesRegex(
                successor.SuccessorBindingError, "predecessor closure config"
            ):
                successor.generate_successor(**kwargs)
        with mock.patch.object(
            successor, "APPROVED_CONTROLLER_SOURCE_SHA256", "0" * 64
        ):
            with self.assertRaisesRegex(
                successor.SuccessorBindingError, "approved controller"
            ):
                successor.generate_successor(**kwargs)
        self.assertFalse(self.state_dir.exists())

    def test_exclusive_commit_does_not_replace_existing_destination(self) -> None:
        source = self.root / "staged"
        target = self.root / "claimed"
        source.mkdir()
        target.mkdir()
        sentinel = target / "sentinel"
        sentinel.write_text("preserve\n", encoding="utf-8")
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "destination already exists"
        ):
            successor.rename_directory_create_only(source, target)
        self.assertTrue(source.is_dir())
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve\n")

    def test_changed_evidence_is_rejected_before_destination_commit(self) -> None:
        first = successor.successor_evidence(
            "v21-r5", self.live_root, self.state_dir
        )
        changed = json.loads(json.dumps(first))
        changed["run_id"] = "swarm-20260826-987654321"
        with mock.patch.object(
            successor, "successor_evidence", side_effect=[first, changed]
        ):
            with self.assertRaisesRegex(
                successor.SuccessorBindingError, "evidence changed before"
            ):
                successor.generate_successor(
                    generation="v21-r5",
                    live_root=self.live_root,
                    state_dir=self.state_dir,
                    base_config_path=self.base_config_path,
                    controller_source=ROOT / "terminal_closure.py",
                )
        self.assertFalse(self.state_dir.exists())


if __name__ == "__main__":
    unittest.main()
