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
                "v24": "405678e0ab35d2859de41f82d0d968af266b84708af47dc3c745e33de93719ad"
            },
        )


class SuccessorBindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.live_root = self.root / successor.LIVE_ROOT_LEAF
        self.run_dir = self.live_root / "swarm-3node-qwen38-brainwaves-r0"
        self.state_dir = self.root / successor.STATE_DIR_LEAF
        self.live_root.mkdir()
        self.run_dir.mkdir()

        self.launcher = self.live_root / "launch_local_v24.py"
        self.launcher.write_text("raise SystemExit('fixture only')\n", encoding="utf-8")
        self.launcher.chmod(0o400)
        self.binary = self.live_root / "bin" / "goose-successor"
        self.binary.parent.mkdir()
        self.binary.write_bytes(b"successor-binary")
        self.binary.chmod(0o500)
        (self.live_root / "fleet-seal.json").write_text("{}\n", encoding="utf-8")

        self.candidate_commit = "1" * 40
        self.candidate_tree = "2" * 40
        self.candidate_branch = "codex/swarm-next-semantic-flow"
        self.candidate_remote_ref = f"origin/{self.candidate_branch}"
        self.wrapper_sha256 = "a" * 64
        instrument_files = {
            "evals/swarm-bench/bench/run_build_local_v20.py": "3" * 64,
            "evals/swarm-bench/spec-build-sb7.md": successor.SPEC_SHA256,
            "evals/swarm-bench/bench/score_sb7.py": successor.SCORER_SHA256,
            "evals/swarm-bench/bench/sb7-thresholds.json": successor.THRESHOLDS_SHA256,
            "evals/swarm-bench/bench/product_probe_v3.mjs": "4" * 64,
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
                    "file_count": 4,
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
                "total_file_count": 5,
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
            "sb7_policy": successor.expected_sb7_policy(
                "swarm-3node-qwen38-brainwaves"
            ),
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
        self.publisher.write_text(
            "\n".join(
                (
                    f"export const TARGET_DOCUMENT_ID = '{successor.PREDECESSOR_TARGET_DOCUMENT_ID}';",
                    "export const PROTECTED_DOCUMENT_IDS = Object.freeze([",
                    *[
                        f"  '{document_id}',"
                        for document_id in successor.PREDECESSOR_PROTECTED_DOCUMENT_ID_ORDER
                    ],
                    "]);",
                    f"export const PUBLIC_PROVENANCE_MARKER = '{successor.PUBLISHER_SOURCE_PROVENANCE_MARKER}';",
                    "`Full sb-7 tier means (the public schema exposes A–D separately): ${tierLine}. The scorer hash differs from the pre-repair scorer only for the authorized notifier reachability classifier repair; all 91 check definitions and weights are unchanged. Exact scorer and calibration provenance remain in the sealed local closure receipt; the public board intentionally stays on its single sb-7.0 era.`,",
                    "'protected-document positive control did not resolve all frozen IDs'",
                    "if (!runHtml.includes(PUBLIC_PROVENANCE_MARKER)) failures.push('run page lacks exact publication provenance');",
                    "  for (const staleMarker of ['Brainwaves v17', 'Brainwaves v18', 'Brainwaves v19', 'Brainwaves v20', 'Brainwaves v21', 'Brainwaves v22']) {",
                    "    if (runHtml.includes(staleMarker)) failures.push(`run page contains stale provenance ${staleMarker}`);",
                    "  }",
                    "  if (state?.initialized) {",
                    "    invariant(state.target_absent_before === true, 'publisher state lacks target-absence proof');",
                    "    invariant(state.target_document_id === TARGET_DOCUMENT_ID, 'publisher state target changed');",
                    "  } else {",
                    "    invariant(!existingTarget, 'new target document already exists without this closure state; refusing to replace it');",
                    "    invariant(",
                    "      JSON.stringify(beforeRows.map((row) => row._id).sort()) === JSON.stringify([...PROTECTED_DOCUMENT_IDS].sort()),",
                    "      'target-absence positive control did not resolve the exact protected set',",
                    "    );",
                    "    state = persistOperationState(state, {",
                    "      schema_version: 1,",
                    "      initialized: true,",
                    "      target_absent_before: true,",
                    "    });",
                    "  }",
                    "  const receipt = {",
                    "    create_only: true,",
                    "    target_absent_before: state.target_absent_before === true,",
                    "    protected_positive_control_ids: state.protected_before.map((row) => row._id),",
                    "    screenshots: [],",
                    "  };",
                )
            )
            + "\n",
            encoding="utf-8",
        )
        self.publisher.chmod(0o400)
        self.usage_policy = self.root / "usage_impairment.py"
        self.usage_policy.write_bytes((ROOT / "usage_impairment.py").read_bytes())
        self.usage_policy.chmod(0o400)
        self.base_config_path = self.root / "stopped-v23-config.json"
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
            {"v24": successor.sha256_file(self.base_config_path)},
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
        old_root = self.root / "local-sb7-engine-v23"
        old_state = self.root / "local-sb7-engine-v23-terminal-closure"
        return {
            "schema_version": 1,
            "armed": True,
            "binding": {"generation": "v23"},
            "closure_generation": "v23",
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
                "launch_controller_path": str(old_root / "launch_local_v23.py"),
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
                "target_document_id": successor.PREDECESSOR_TARGET_DOCUMENT_ID,
                "protected_document_ids": list(
                    successor.PREDECESSOR_PROTECTED_DOCUMENT_ID_ORDER
                ),
                "provenance_marker": successor.PREDECESSOR_PUBLICATION_PROVENANCE_MARKER,
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

    def test_stopped_v23_rejected_exact_successor_accepted_without_mutating_protected_config(
        self,
    ) -> None:
        base_before = self.base_config_path.read_bytes()
        receipt = successor.generate_successor(
            generation="v24",
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
        self.assertEqual(generated.V19_TARGET_DOCUMENT_ID, successor.TARGET_DOCUMENT_ID)
        self.assertEqual(
            generated.V19_PROTECTED_DOCUMENT_ID_ORDER,
            successor.PROTECTED_DOCUMENT_ID_ORDER,
        )
        self.assertEqual(generated.V19_SCORER_SHA256, successor.SCORER_SHA256)
        self.assertEqual(
            generated.V19_PREDECESSOR_SCORER_SHA256,
            successor.PREDECESSOR_SCORER_SHA256,
        )
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
        publisher_path = pathlib.Path(exact["publisher"]["path"])
        publisher_text = publisher_path.read_text(encoding="utf-8")
        self.assertIn(successor.TARGET_DOCUMENT_ID, publisher_text)
        self.assertIn(successor.PUBLICATION_PROVENANCE_MARKER, publisher_text)
        self.assertIn("target_absent_before", publisher_text)
        self.assertEqual(
            exact["publisher"]["source_sha256"],
            successor.sha256_file(self.publisher),
        )
        self.assertEqual(exact["publisher"]["sha256"], receipt["publisher_sha256"])
        self.assertEqual(
            successor.sha256_file(publisher_path), receipt["publisher_sha256"]
        )
        node = successor.shutil.which("node")
        if node is not None:
            checked = successor.subprocess.run(
                [node, "--check", str(publisher_path)],
                check=False,
                stdout=successor.subprocess.PIPE,
                stderr=successor.subprocess.PIPE,
            )
            self.assertEqual(checked.returncode, 0, checked.stderr.decode())
        self.assertEqual(
            exact["expected"]["scorer_change_authorization"],
            successor.SCORER_CHANGE_AUTHORIZATION,
        )
        self.assertFalse((self.state_dir / "closure-instrument").exists())
        self.assertEqual(
            pathlib.Path(exact["publisher"]["path"]).parent.resolve(),
            (self.state_dir / "bootstrap").resolve(),
        )
        frozen = json.loads(json.dumps(exact))
        instrument = self.state_dir / "closure-instrument"
        frozen["publisher"]["path"] = str(
            instrument / successor.PUBLISHER_FILENAME
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
                successor.PUBLISHER_FILENAME: {
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
                successor.PUBLISHER_FILENAME,
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
                "provenance_marker": successor.PUBLICATION_PROVENANCE_MARKER,
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
        self.assertEqual(binding["publisher_sha256"], receipt["publisher_sha256"])
        self.assertEqual(
            binding["publisher_source_sha256"], receipt["publisher_source_sha256"]
        )
        self.assertEqual(
            binding["publication_contract_sha256"],
            receipt["publication_contract_sha256"],
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
            "stale unchanged claim": lambda value: value["sb7_policy"].__setitem__(
                "spec_and_scorer_unchanged_from_v6", True
            ),
            "unauthorized scorer": lambda value: value["sb7_policy"][
                "contract_hashes"
            ].__setitem__("scorer_sha256", successor.PREDECESSOR_SCORER_SHA256),
            "scorer inventory": lambda value: value["files"].__setitem__(
                "evals/swarm-bench/bench/score_sb7.py",
                successor.PREDECESSOR_SCORER_SHA256,
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
                        "v24", self.live_root, self.state_dir
                    )
                self.assertFalse(self.state_dir.exists())
        self._rewrite_manifest(original)

    def test_publisher_rendering_is_exact_and_rejects_anchor_drift(self) -> None:
        source = self.publisher.read_bytes()
        rendered = successor.render_publisher(source)
        text = rendered.decode("utf-8")
        self.assertIn(
            f"export const TARGET_DOCUMENT_ID = '{successor.TARGET_DOCUMENT_ID}';",
            text,
        )
        self.assertIn(
            f"export const PUBLIC_PROVENANCE_MARKER = '{successor.PUBLICATION_PROVENANCE_MARKER}';",
            text,
        )
        for document_id in successor.PROTECTED_DOCUMENT_ID_ORDER:
            self.assertIn(f"  '{document_id}',", text)
        self.assertIn("target-absence positive control", text)
        self.assertIn("target_absent_before", text)
        self.assertIn("authorized notifier reachability classifier repair", text)
        self.assertIn("differs from the pre-repair scorer only", text)
        self.assertNotIn("differs from Brainwaves v21 only", text)
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "document contract anchor"
        ):
            successor.render_publisher(
                source.replace(b"export const TARGET_DOCUMENT_ID", b"const TARGET_DOCUMENT_ID")
            )

    def test_successor_rejects_a_different_entrant_before_creating_state(self) -> None:
        launch = json.loads(self.launch_path.read_text(encoding="utf-8"))
        launch["entrant"] = "different-entrant"
        self.launch_path.write_text(
            json.dumps(launch, sort_keys=True) + "\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "entrant identity"
        ):
            successor.successor_evidence("v24", self.live_root, self.state_dir)
        self.assertFalse(self.state_dir.exists())

    def test_successor_accepts_only_the_exact_v24_root_and_state_leaves(self) -> None:
        stale_root = self.root / "local-sb7-engine-v23"
        stale_root.mkdir()
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "live root does not match"
        ):
            successor.successor_evidence("v24", stale_root, self.state_dir)

        wrong_state = self.root / "local-sb7-engine-v23-terminal-closure"
        with self.assertRaisesRegex(
            successor.SuccessorBindingError, "closure state does not match"
        ):
            successor.successor_evidence("v24", self.live_root, wrong_state)
        self.assertFalse(self.state_dir.exists())
        self.assertFalse(wrong_state.exists())

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
            "v24", self.live_root, self.state_dir
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
                "v24", self.live_root, self.state_dir
            )

    def test_generation_is_append_only_and_rejects_different_successor_bytes(self) -> None:
        kwargs = {
            "generation": "v24",
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
            "generation": "v24",
            "live_root": self.live_root,
            "state_dir": self.state_dir,
            "base_config_path": self.base_config_path,
            "controller_source": ROOT / "terminal_closure.py",
        }
        with mock.patch.object(
            successor,
            "APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION",
            {"v24": "0" * 64},
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
            "v24", self.live_root, self.state_dir
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
                    generation="v24",
                    live_root=self.live_root,
                    state_dir=self.state_dir,
                    base_config_path=self.base_config_path,
                    controller_source=ROOT / "terminal_closure.py",
                )
        self.assertFalse(self.state_dir.exists())


if __name__ == "__main__":
    unittest.main()
