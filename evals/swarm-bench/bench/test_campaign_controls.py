import copy
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from bench.campaign_controls import (
    ControlManifestError,
    create_or_resume_arm_receipt,
    create_or_resume_lock,
    parse_behavior_profile,
    prepare_arm_receipt,
    prepare_verification_receipt,
    render_staged_config,
    sha256_value,
    validate_arm_receipt,
    validate_engine_export,
    validate_run_event,
    verify_lock,
)


def engine_export():
    registry = {
        "schema_version": 2,
        "config": [
            {
                "canonical": "default_on",
                "disposition": "retain_enabled",
                "campaign_role": "behavior",
                "source": "config",
                "value_type": "boolean",
                "default": True,
                "effective_echo": True,
            },
            {
                "canonical": "default_off",
                "disposition": "retain_disabled",
                "campaign_role": "behavior",
                "source": "config",
                "value_type": "boolean",
                "default": False,
                "effective_echo": True,
            },
            {
                "canonical": "profile_timeout",
                "disposition": "runtime_profile",
                "campaign_role": "runtime_profile",
                "source": "config",
                "value_type": "integer",
                "default": 900,
                "effective_echo": True,
            },
            {
                "canonical": "old_switch",
                "disposition": "remove_merge",
                "campaign_role": "removal",
                "source": "config",
                "value_type": "boolean",
                "default": False,
                "effective_echo": True,
            },
            {
                "canonical": "occupancy",
                "disposition": "retain_enabled",
                "campaign_role": "telemetry",
                "source": "config",
                "value_type": "boolean",
                "default": True,
                "effective_echo": True,
            },
        ],
        "environment_only": [
            {
                "canonical": "judge",
                "environment": "GOOSE_SWARM_JUDGE",
                "disposition": "modify",
                "campaign_role": "behavior",
                "source": "environment",
                "effective_echo": True,
            }
        ],
        "aliases": [{"alias": "legacy_off", "canonical": "default_off"}],
        "environment_readers": [
            {"environment": "GOOSE_SWARM_DEFAULT_ON", "canonical": "default_on"},
            {"environment": "GOOSE_SWARM_DEFAULT_OFF", "canonical": "default_off"},
            {"environment": "GOOSE_SWARM_JUDGE", "canonical": "judge"},
        ],
    }
    environment = {
        row["environment"]: os.environ.get(row["environment"])
        for row in registry["environment_readers"]
    }
    return {
        "schema_version": 1,
        "engine": {
            "version": "1.2.3",
            "build_sha": "abc123",
            "crate_version": "1.41.0",
        },
        "registry_sha256": sha256_value(registry),
        "control_environment_sha256": sha256_value(environment),
        "control_registry": registry,
    }


def run_event(export, missing=None):
    values = {
        "default_on": True,
        "default_off": False,
        "profile_timeout": 900,
        "old_switch": False,
        "occupancy": True,
        "judge": True,
    }
    if missing:
        values.pop(missing)
    return {
        "event": "levers_resolved",
        "version": export["engine"]["version"],
        "build_sha": export["engine"]["build_sha"],
        "crate_version": export["engine"]["crate_version"],
        "control_registry_sha256": export["registry_sha256"],
        "control_environment_sha256": export["control_environment_sha256"],
        "control_registry": export["control_registry"],
        "levers": values,
    }


class CampaignControlTests(unittest.TestCase):
    def setUp(self):
        self.export = engine_export()
        self.catalog = validate_engine_export(self.export)

    def test_unknown_control_is_rejected(self):
        with self.assertRaisesRegex(ControlManifestError, "unknown engine control"):
            parse_behavior_profile(self.catalog, ["invented=1"])

    def test_alias_and_environment_spelling_resolve_to_the_canonical_control(self):
        self.assertEqual(
            parse_behavior_profile(self.catalog, ["legacy_off=1"]),
            {"default_off": True},
        )
        self.assertEqual(
            parse_behavior_profile(self.catalog, ["GOOSE_SWARM_DEFAULT_OFF=0"]),
            {"default_off": False},
        )
        with self.assertRaisesRegex(ControlManifestError, "assigned twice"):
            parse_behavior_profile(
                self.catalog, ["legacy_off=1", "default_off=false"]
            )

    def test_environment_runtime_removal_and_telemetry_are_not_causal_config_arms(self):
        for assignment, message in (
            ("judge=1", "environment-only"),
            ("profile_timeout=1200", "runtime_profile"),
            ("old_switch=1", "removal"),
            ("occupancy=0", "telemetry"),
        ):
            with self.subTest(assignment=assignment):
                with self.assertRaisesRegex(ControlManifestError, message):
                    parse_behavior_profile(self.catalog, [assignment])

    def test_default_on_must_be_ablated_not_reasserted(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "runtime.yaml"
            baseline.write_text("swarm:\n  profile_timeout: 900\n")
            lock = {
                "engine_binary_sha256": "f" * 64,
                "engine_control_export": self.export,
            }
            with self.assertRaisesRegex(ControlManifestError, r"changed \[\]"):
                prepare_arm_receipt(
                    self.catalog,
                    lock,
                    "no-op",
                    [],
                    ["default_on=true"],
                    baseline,
                )
            receipt = prepare_arm_receipt(
                self.catalog,
                lock,
                "ablate-default-on",
                [],
                ["default_on=false"],
                baseline,
            )
            self.assertEqual(receipt["delta"], ["default_on"])

    def test_changed_control_must_be_explicit_in_the_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            lock = {"engine_binary_sha256": "f" * 64}
            with self.assertRaisesRegex(ControlManifestError, "explicit candidate"):
                prepare_arm_receipt(
                    self.catalog,
                    lock,
                    "implicit-default",
                    ["default_off=true"],
                    [],
                    baseline,
                )

    def test_one_arm_cannot_change_two_controls(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            lock = {
                "engine_binary_sha256": "f" * 64,
                "engine_control_export": self.export,
            }
            with self.assertRaisesRegex(ControlManifestError, "exactly one"):
                prepare_arm_receipt(
                    self.catalog,
                    lock,
                    "confounded",
                    [],
                    ["default_on=false", "default_off=true"],
                    baseline,
                )

    def test_staged_config_removes_behavior_leaks_and_writes_explicit_ablation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "runtime.yaml"
            baseline.write_text(
                "extensions: {}\n"
                "swarm:\n"
                "  profile_timeout: 900\n"
                "  default_on: true\n"
                "  default_off: true\n"
                "  devices:\n"
                "    - id: local\n"
                "      enabled: true\n"
                "active_provider: swarm\n"
            )
            staged = root / "arm.yaml"
            digest = render_staged_config(
                self.catalog,
                baseline,
                {"default_on": False},
                staged,
            )
            rendered = staged.read_text()
            self.assertIn("  default_on: false\n", rendered)
            self.assertNotIn("  default_off:", rendered)
            self.assertIn("  profile_timeout: 900\n", rendered)
            self.assertIn("      enabled: true\n", rendered)
            self.assertEqual(len(digest), 64)

            inline = root / "inline.yaml"
            inline.write_text("swarm: {}\n")
            inline_staged = root / "inline-arm.yaml"
            render_staged_config(
                self.catalog,
                inline,
                {"default_off": True},
                inline_staged,
            )
            self.assertEqual(
                inline_staged.read_text(), "swarm:\n  default_off: true\n"
            )

    def test_arm_receipt_refuses_staged_config_tampering(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            staged = root / "arm.yaml"
            lock = {"engine_binary_sha256": "f" * 64}
            receipt = prepare_arm_receipt(
                self.catalog,
                lock,
                "ablation",
                [],
                ["default_on=false"],
                baseline,
                staged,
            )
            validate_arm_receipt(self.catalog, lock, receipt)
            staged.write_text("swarm:\n  default_on: true\n")
            with self.assertRaisesRegex(ControlManifestError, "staged config changed"):
                validate_arm_receipt(self.catalog, lock, receipt)

    def test_replicate_requires_zero_delta_and_receipt_is_resumable(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            lock = {
                "engine_binary_sha256": "f" * 64,
                "engine_control_export": self.export,
            }
            receipt = prepare_arm_receipt(
                self.catalog,
                lock,
                "replicate-2",
                ["default_off=true"],
                ["legacy_off=1"],
                baseline,
                replicate=True,
            )
            path = root / "arm.json"
            create_or_resume_arm_receipt(path, receipt)
            create_or_resume_arm_receipt(path, receipt)
            changed = copy.deepcopy(receipt)
            changed["arm"] = "different"
            with self.assertRaisesRegex(ControlManifestError, "different evidence"):
                create_or_resume_arm_receipt(path, changed)

    def test_run_event_must_echo_every_registered_effective_control(self):
        validate_run_event(self.catalog, run_event(self.export))
        with self.assertRaisesRegex(ControlManifestError, "default_off"):
            validate_run_event(
                self.catalog, run_event(self.export, missing="default_off")
            )

    def test_run_event_must_match_requested_explicit_values(self):
        receipt = {
            "registry_sha256": self.catalog.registry_sha256,
            "candidate_explicit": {"default_off": True},
        }
        event = run_event(self.export)
        with self.assertRaisesRegex(ControlManifestError, "did not execute"):
            validate_run_event(self.catalog, event, receipt)
        event["levers"]["default_off"] = True
        validate_run_event(self.catalog, event, receipt)

    def test_verified_reference_proves_the_executed_one_control_delta(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            lock = {"engine_binary_sha256": "f" * 64}
            reference_arm = prepare_arm_receipt(
                self.catalog,
                lock,
                "reference",
                [],
                [],
                baseline,
                replicate=True,
            )
            reference_event = run_event(self.export)
            reference_log = root / "reference.jsonl"
            reference_log.write_text(json.dumps(reference_event) + "\n")
            reference = prepare_verification_receipt(
                self.catalog,
                lock,
                reference_arm,
                reference_event,
                reference_log,
            )

            candidate_arm = prepare_arm_receipt(
                self.catalog,
                lock,
                "candidate",
                [],
                ["default_off=true"],
                baseline,
            )
            candidate_event = run_event(self.export)
            candidate_event["levers"]["default_off"] = True
            candidate_log = root / "candidate.jsonl"
            candidate_log.write_text(json.dumps(candidate_event) + "\n")
            verified = prepare_verification_receipt(
                self.catalog,
                lock,
                candidate_arm,
                candidate_event,
                candidate_log,
                reference,
            )
            self.assertEqual(
                verified["executed_delta_from_reference"], ["default_off"]
            )

            candidate_event["levers"]["profile_timeout"] = 901
            candidate_log.write_text(json.dumps(candidate_event) + "\n")
            with self.assertRaisesRegex(ControlManifestError, "declared one-control"):
                prepare_verification_receipt(
                    self.catalog,
                    lock,
                    candidate_arm,
                    candidate_event,
                    candidate_log,
                    reference,
                )

    def test_causal_verification_requires_an_executed_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            baseline = root / "runtime.yaml"
            baseline.write_text("swarm: {}\n")
            arm = prepare_arm_receipt(
                self.catalog,
                {"engine_binary_sha256": "f" * 64},
                "candidate",
                [],
                ["default_off=true"],
                baseline,
            )
            event = run_event(self.export)
            event["levers"]["default_off"] = True
            event_log = root / "candidate.jsonl"
            event_log.write_text(json.dumps(event) + "\n")
            with self.assertRaisesRegex(ControlManifestError, "requires a verified"):
                prepare_verification_receipt(
                    self.catalog,
                    {"engine_binary_sha256": "f" * 64},
                    arm,
                    event,
                    event_log,
                )

    def test_registry_digest_detects_a_missing_manifest_control(self):
        broken = copy.deepcopy(self.export)
        broken["control_registry"]["config"].pop()
        with self.assertRaisesRegex(ControlManifestError, "digest"):
            validate_engine_export(broken)

    def test_stale_binary_mismatch_is_detected_even_when_export_is_unchanged(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / "fake-goose"
            engine.write_text(
                "#!/usr/bin/env python3\n"
                f"print({json.dumps(json.dumps(self.export))})\n"
            )
            engine.chmod(0o755)
            lock = root / "manifest.lock.json"
            create_or_resume_lock(engine, lock)
            verify_lock(engine, lock)
            with engine.open("a") as handle:
                handle.write("# binary changed without changing the claimed build\n")
            with self.assertRaisesRegex(ControlManifestError, "binary digest changed"):
                verify_lock(engine, lock)

    def test_initial_binary_must_match_the_expected_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / "fake-goose"
            engine.write_text(
                "#!/usr/bin/env python3\n"
                f"print({json.dumps(json.dumps(self.export))})\n"
            )
            engine.chmod(0o755)
            with self.assertRaisesRegex(ControlManifestError, "expected build SHA"):
                create_or_resume_lock(
                    engine, root / "manifest.lock.json", expected_build_sha="newer"
                )

    def test_environment_change_invalidates_the_sealed_campaign(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / "fake-goose"
            encoded = json.dumps(self.export)
            engine.write_text(
                "#!/usr/bin/env python3\n"
                "import hashlib, json, os\n"
                f"payload = json.loads({json.dumps(encoded)})\n"
                "names = [row['environment'] for row in "
                "payload['control_registry']['environment_readers']]\n"
                "inputs = {name: os.environ.get(name) for name in names}\n"
                "body = json.dumps(inputs, ensure_ascii=False, sort_keys=True, "
                "separators=(',', ':')).encode()\n"
                "payload['control_environment_sha256'] = hashlib.sha256(body).hexdigest()\n"
                "print(json.dumps(payload))\n"
            )
            engine.chmod(0o755)
            lock = root / "manifest.lock.json"
            with patch.dict(os.environ, {"GOOSE_SWARM_DEFAULT_OFF": "0"}):
                create_or_resume_lock(engine, lock)
            with patch.dict(os.environ, {"GOOSE_SWARM_DEFAULT_OFF": "1"}):
                with self.assertRaisesRegex(ControlManifestError, "environment differs"):
                    verify_lock(engine, lock)

    def test_manifest_rejects_an_alias_whose_target_is_missing(self):
        broken = copy.deepcopy(self.export)
        broken["control_registry"]["aliases"][0]["canonical"] = "absent"
        broken["registry_sha256"] = sha256_value(broken["control_registry"])
        with self.assertRaisesRegex(ControlManifestError, "points at missing"):
            validate_engine_export(broken)

    def test_manifest_rejects_an_alias_that_shadows_a_canonical_control(self):
        broken = copy.deepcopy(self.export)
        broken["control_registry"]["aliases"] = [
            {"alias": "default_on", "canonical": "default_off"}
        ]
        broken["registry_sha256"] = sha256_value(broken["control_registry"])
        with self.assertRaisesRegex(ControlManifestError, "collides"):
            validate_engine_export(broken)


if __name__ == "__main__":
    unittest.main()
