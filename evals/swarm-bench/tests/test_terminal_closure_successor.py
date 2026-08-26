from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest


ROOT = pathlib.Path(__file__).parents[1]
MODULE_PATH = ROOT / "terminal_closure_successor.py"
SPEC = importlib.util.spec_from_file_location("terminal_closure_successor", MODULE_PATH)
assert SPEC and SPEC.loader
successor = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(successor)


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
        manifest = {
            "schema_version": 1,
            "candidate_commit": self.candidate_commit,
            "candidate_tree": self.candidate_tree,
            "binary": {"path": "/frozen/instrument/goose", "sha256": "3" * 64},
            "sb7_policy": {
                "spec_and_scorer_unchanged_from_v6": True,
                "website_surface": "stable-sb7",
                "publish_from_run_build_auto_score": False,
                "entrant": "swarm-3node-qwen38-brainwaves",
                "publication_document_id": successor.TARGET_DOCUMENT_ID,
                "protected_document_ids": sorted(successor.PROTECTED_DOCUMENT_IDS),
            },
            "files": {"evals/swarm-bench/bench/score_sb7.py": "4" * 64},
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
        self.usage_policy = ROOT / "usage_impairment.py"
        self.base_config_path = self.root / "stopped-r4-config.json"
        self.base_config = self._base_config()
        self.base_config_path.write_text(
            json.dumps(self.base_config, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        self.base_config_path.chmod(0o600)
        self.base_config_mode = self.base_config_path.stat().st_mode & 0o777

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
                "protected_document_ids": sorted(successor.PROTECTED_DOCUMENT_IDS),
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
            sorted(successor.PROTECTED_DOCUMENT_IDS),
        )

        generated = successor.load_generated_module(pathlib.Path(receipt["controller"]))
        self.assertEqual(
            generated.V19_BOUND_LAUNCH_SHA256,
            successor.sha256_file(self.launch_path),
        )
        self.assertEqual(generated.V19_BOUND_RUN_ID, "swarm-20260826-123456789")
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
        self.assertEqual(
            exact["publication"],
            {
                "target_document_id": successor.TARGET_DOCUMENT_ID,
                "protected_document_ids": sorted(successor.PROTECTED_DOCUMENT_IDS),
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
            sorted(successor.PROTECTED_DOCUMENT_IDS),
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


if __name__ == "__main__":
    unittest.main()
