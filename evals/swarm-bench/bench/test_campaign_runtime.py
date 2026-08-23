import copy
import datetime as dt
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from bench.campaign_controls import ControlManifestError, sha256_value
from bench.campaign_runtime import (
    CampaignRuntimeError,
    _arm_dir,
    activate_arm,
    generate_campaign,
    measure_arm,
    next_arm,
)
from bench import campaign_runtime
from bench.test_campaign_controls import engine_export, run_event


class CampaignRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.state = self.root / "state"
        self.build_root = self.root / "builds"
        self.build_root.mkdir()
        self.baseline = self.root / "runtime.yaml"
        self.baseline.write_text(
            "extensions: {}\n"
            "swarm:\n"
            "  profile_timeout: 900\n"
            "active_provider: swarm\n"
        )
        self.spec = self.root / "spec.txt"
        self.spec.write_text("Build the fixture.\n")
        self.global_config = self.root / "config.yaml"
        self.global_config.write_text("extensions:\n  original: true\nswarm: {}\n")
        self.export = engine_export()
        self.engine = self.root / "fake-goose"
        self._write_engine(self.export)

    def tearDown(self):
        self.tmp.cleanup()

    def _write_engine(self, payload):
        self.engine.write_text(
            "#!/usr/bin/env python3\n"
            "import json\n"
            f"print({json.dumps(json.dumps(payload))})\n"
        )
        self.engine.chmod(0o755)

    def _generate(self, **kwargs):
        return generate_campaign(
            self.state,
            self.engine,
            self.baseline,
            self.spec,
            9897,
            **kwargs,
        )

    def _queue(self):
        return [
            json.loads(line)
            for line in (self.state / "QUEUE.schema2.jsonl").read_text().splitlines()
        ]

    def _row(self, arm):
        return next(row for row in self._queue() if row["arm"] == arm)

    def _heartbeat(self, arm, seconds_old=0):
        swarm = self.build_root / arm / ".swarm"
        swarm.mkdir(parents=True, exist_ok=True)
        timestamp = dt.datetime.now(dt.timezone.utc) - dt.timedelta(seconds=seconds_old)
        (swarm / "heartbeat").write_text(timestamp.isoformat())
        return swarm

    def _event_log(self, arm, values=None, finished=True):
        swarm = self.build_root / arm / ".swarm"
        swarm.mkdir(parents=True, exist_ok=True)
        event = run_event(self.export)
        if values:
            event["levers"].update(values)
        events = [event]
        if finished:
            events.append(
                {
                    "event": "run_finished",
                    "report": {"done": ["one"], "failed": [], "bonus": []},
                    "phases": {
                        "total_min": 1,
                        "planning_min": 0.2,
                        "execute_min": 0.8,
                        "research_min": 0.1,
                    },
                }
            )
        path = swarm / "run-swarm-20260823-000000000.jsonl"
        path.write_text("".join(json.dumps(event) + "\n" for event in events))
        return path

    def test_generation_is_manifest_driven_explicit_and_resumable(self):
        plan = self._generate()
        self.assertEqual(plan["arm_count"], 5)
        rows = self._queue()
        self.assertEqual(
            [row["arm"] for row in rows],
            [
                "reference-1",
                "reference-2",
                "reference-3",
                "arm-default_off",
                "arm-default_on",
            ],
        )
        default_on = json.loads(
            (_arm_dir(self.state, "arm-default_on") / "candidate.json").read_text()
        )
        self.assertIs(default_on["default_on"], False)
        inventory = json.loads((self.state / "CAMPAIGN.inventory.json").read_text())
        statuses = {
            (row["canonical"], row["source"]): row["campaign_status"]
            for row in inventory["controls"]
        }
        self.assertEqual(
            statuses[("profile_timeout", "config")],
            "classified_not_queued:runtime_profile",
        )
        self.assertEqual(
            statuses[("judge", "environment")],
            "unreachable_from_persisted_config",
        )
        before = {
            path.relative_to(self.state): path.read_bytes()
            for path in self.state.rglob("*")
            if path.is_file() and path.name != ".campaign-state.lock"
        }
        self._generate()
        after = {
            path.relative_to(self.state): path.read_bytes()
            for path in self.state.rglob("*")
            if path.is_file() and path.name != ".campaign-state.lock"
        }
        self.assertEqual(before, after)

    def test_missing_numeric_variant_fails_before_queue_creation(self):
        payload = copy.deepcopy(self.export)
        payload["control_registry"]["config"].append(
            {
                "canonical": "thinking_depth",
                "disposition": "modify",
                "campaign_role": "behavior",
                "source": "config",
                "value_type": "integer",
                "default": 3,
                "effective_echo": True,
            }
        )
        payload["registry_sha256"] = sha256_value(payload["control_registry"])
        self._write_engine(payload)
        with self.assertRaisesRegex(CampaignRuntimeError, "needs an explicit variant"):
            self._generate()
        self.assertFalse((self.state / "QUEUE.schema2.jsonl").exists())

    def test_unknown_and_alias_collision_are_rejected(self):
        variants = self.root / "variants.json"
        variants.write_text(json.dumps({"invented": True}))
        with self.assertRaisesRegex(ControlManifestError, "unknown engine control"):
            self._generate(variants_path=variants)

        reference = self.root / "reference.profile"
        reference.write_text(
            "default_on=true\nlegacy_off=true default_off=false\n"
        )
        with self.assertRaisesRegex(ControlManifestError, "assigned twice"):
            self._generate(reference_profile=reference)

    def test_missing_reference_control_is_rejected(self):
        reference = self.root / "reference.json"
        reference.write_text(json.dumps({"default_on": True}))
        with self.assertRaisesRegex(CampaignRuntimeError, r"missing=\['default_off'\]"):
            self._generate(reference_profile=reference)

    def test_live_heartbeat_blocks_before_and_during_the_locked_recheck(self):
        self._generate()
        original = self.global_config.read_bytes()
        self._heartbeat("already-running")
        with self.assertRaisesRegex(CampaignRuntimeError, "heartbeat is live"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
            )
        self.assertEqual(self.global_config.read_bytes(), original)
        self.assertFalse((self.state / "CAMPAIGN.active.json").exists())

        shutil.rmtree(self.build_root / "already-running")

        def race():
            self._heartbeat("raced-in")

        with self.assertRaisesRegex(CampaignRuntimeError, "heartbeat is live"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
                pre_commit_hook=race,
            )
        self.assertEqual(self.global_config.read_bytes(), original)
        self.assertFalse((self.state / "CAMPAIGN.active.json").exists())

    def test_activation_recovers_crashes_without_rederiving_the_arm(self):
        self._generate()
        original = self.global_config.read_bytes()
        with self.assertRaisesRegex(RuntimeError, "after activation intent"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
                failure_point="after_intent",
            )
        self.assertEqual(self.global_config.read_bytes(), original)
        activation = activate_arm(
            self.state,
            "reference-1",
            self.global_config,
            self.build_root,
            recovery=True,
        )
        self.assertEqual(activation["state"], "activated")
        staged = Path(self._row("reference-1")["staged_config"]).read_bytes()
        self.assertEqual(self.global_config.read_bytes(), staged)
        self.assertEqual(
            (self.build_root / "reference-1" / ".arm-config.yaml").read_bytes(),
            staged,
        )

    def test_global_config_divergence_requires_explicit_same_arm_recovery(self):
        self._generate()
        activate_arm(
            self.state,
            "reference-1",
            self.global_config,
            self.build_root,
        )
        staged = self.global_config.read_bytes()
        self.global_config.write_text("swarm: {}\n# app rewrote it\n")
        with self.assertRaisesRegex(CampaignRuntimeError, "explicit crash recovery"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
            )
        with self.assertRaisesRegex(CampaignRuntimeError, "another campaign arm owns"):
            activate_arm(
                self.state,
                "reference-2",
                self.global_config,
                self.build_root,
                recovery=True,
            )
        activate_arm(
            self.state,
            "reference-1",
            self.global_config,
            self.build_root,
            recovery=True,
        )
        self.assertEqual(self.global_config.read_bytes(), staged)

    def test_two_process_activation_race_has_one_owner_and_one_config(self):
        self._generate()
        command = [
            sys.executable,
            str(Path(campaign_runtime.__file__).resolve()),
            "activate",
            "--state",
            str(self.state),
            "--global-config",
            str(self.global_config),
            "--build-root",
            str(self.build_root),
        ]
        first = subprocess.Popen(
            [*command, "--arm", "reference-1"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        second = subprocess.Popen(
            [*command, "--arm", "reference-2"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        first_output = first.communicate(timeout=10)
        second_output = second.communicate(timeout=10)
        self.assertEqual(
            sorted([first.returncode, second.returncode]),
            [0, 2],
            msg=f"first={first_output!r}; second={second_output!r}",
        )
        active = json.loads((self.state / "CAMPAIGN.active.json").read_text())
        queued = self._row(active["arm"])
        self.assertEqual(
            self.global_config.read_bytes(), Path(queued["staged_config"]).read_bytes()
        )

    def test_terminal_arm_cannot_be_relaunched_during_measurement_race(self):
        self._generate()
        activate_arm(
            self.state,
            "reference-1",
            self.global_config,
            self.build_root,
        )
        self._event_log("reference-1")
        with self.assertRaisesRegex(CampaignRuntimeError, "awaiting verification"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
                recovery=True,
            )

    def test_stale_engine_and_tampered_queue_fail_before_global_mutation(self):
        self._generate()
        original = self.global_config.read_bytes()
        with self.engine.open("a") as handle:
            handle.write("# changed binary\n")
        with self.assertRaisesRegex(ControlManifestError, "binary digest changed"):
            activate_arm(
                self.state,
                "reference-1",
                self.global_config,
                self.build_root,
            )
        self.assertEqual(self.global_config.read_bytes(), original)

        self._write_engine(self.export)
        queue = self.state / "QUEUE.schema2.jsonl"
        queue.write_text(queue.read_text() + "{}\n")
        with self.assertRaisesRegex(CampaignRuntimeError, "queue is missing or changed"):
            next_arm(self.state)

    def test_measure_fails_closed_then_verifies_appends_releases_and_resumes(self):
        self._generate()
        activate_arm(
            self.state,
            "reference-1",
            self.global_config,
            self.build_root,
        )
        incomplete = self._event_log("reference-1", finished=False)
        ledger = self.state / "LEDGER.tsv"
        with self.assertRaisesRegex(CampaignRuntimeError, "exactly one run_finished"):
            measure_arm(
                self.state,
                "reference-1",
                self.build_root / "reference-1",
                ledger,
                incomplete,
            )
        self.assertFalse(ledger.exists())
        self.assertTrue((self.state / "CAMPAIGN.active.json").exists())

        complete = self._event_log("reference-1")
        row, appended = measure_arm(
            self.state,
            "reference-1",
            self.build_root / "reference-1",
            ledger,
            complete,
        )
        self.assertTrue(appended)
        self.assertEqual(row["executed_delta"], "")
        self.assertFalse((self.state / "CAMPAIGN.active.json").exists())
        self.assertEqual(next_arm(self.state)["arm"], "reference-2")

        row2, appended2 = measure_arm(
            self.state,
            "reference-1",
            self.build_root / "reference-1",
            ledger,
            complete,
        )
        self.assertFalse(appended2)
        self.assertEqual(row2["event_log_sha256"], row["event_log_sha256"])
        self.assertEqual(len(ledger.read_text().splitlines()), 2)


if __name__ == "__main__":
    unittest.main()
