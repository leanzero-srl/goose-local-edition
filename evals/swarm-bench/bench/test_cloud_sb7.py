from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from base64 import b64decode
from pathlib import Path
from unittest import mock

import cloud_sb7


PNG_1X1 = b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
)


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
                            "accepted_reported_models": ["gemini-3.7-flash"],
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
            json.dumps(
                {
                    "status": status,
                    "entrant_manifest": str(manifest),
                    "coordinator": str(root / "instrument/cloud_sb7.py"),
                }
            )
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

    def fixture_verdict(self) -> dict[str, object]:
        return {
            "score": 0.42,
            "scorer_version": "sb-7.0-rc",
            "calibration": "UNCALIBRATED — fixture defaults; rc-grade only",
            "excellent": False,
            "agent": {"secs": 123.4, "timed_out": False},
            "tiers": {
                letter: {"mean": 0.5}
                for letter in ("A", "B", "C", "D", "J", "V", "P", "T", "X", "R", "E")
            },
            "checks": [
                {
                    "check": "fixture_check",
                    "tier": "A",
                    "score": 0.5,
                    "detail": "fixture detail",
                }
            ],
            "rep": 0,
        }

    def make_scored_campaign(self, root: Path) -> tuple[Path, dict[str, object]]:
        entrant_id = "fixture-model"
        secret_file = root / "cloud-providers.env"
        secret_file.write_text("FIXTURE_API_KEY=fixture-provider-secret\n")
        secret_file.chmod(0o600)
        score_dir = root / "scores" / entrant_id / "attempt-1"
        shots = score_dir / "tree" / "sb7-shots"
        shots.mkdir(parents=True)
        (shots / "100-loaded.png").write_bytes(PNG_1X1)
        verdict = self.fixture_verdict()
        verdict_path = score_dir / "verdict.json"
        verdict_path.write_text(json.dumps(verdict))
        (root / "entrants" / entrant_id).mkdir(parents=True)
        (root / "publish").mkdir()
        cloud_sb7.atomic_json(
            cloud_sb7.state_file(root, entrant_id),
            {
                "schema_version": cloud_sb7.CAMPAIGN_SCHEMA,
                "entrant": entrant_id,
                "provider": "fixture",
                "model": entrant_id,
                "status": "SCORED",
                "score_attempts": 1,
                "verdict": str(verdict_path),
                "score": verdict["score"],
                "scorer_version": verdict["scorer_version"],
                "calibration": verdict["calibration"],
                "fixture_seed": "fixture-seed",
                "vendor_port": 9999,
            },
        )
        publisher = {
            "repo": str(root / "site"),
            "entries": {
                entrant_id: {
                    "key": entrant_id,
                    "label": "Fixture Model",
                    "model": entrant_id,
                    "doc_id": "brun-baseline-fixture-model-sb70",
                }
            },
            "website_base_url": "https://example.invalid",
            "revalidate_endpoint": "https://example.invalid/api/revalidate-benchmarks",
            "verify_timeout_seconds": 1,
            "verify_interval_seconds": 0.01,
            "process_timeout_seconds": 1,
            "env_file": str(root / "site/.env.local"),
        }
        campaign = {
            "schema_version": cloud_sb7.CAMPAIGN_SCHEMA,
            "campaign_id": "fixture-campaign",
            "status": "SCORED",
            "binary_sha256": "binary",
            "instrument_set_sha256": "instrument",
            "secret_file": str(secret_file),
            "publisher": publisher,
        }
        cloud_sb7.atomic_json(cloud_sb7.campaign_file(root), campaign)
        cloud_sb7.atomic_json(root / "manager.json", {"status": "IDLE"})
        return verdict_path, verdict

    def publisher_campaign(
        self,
        root: Path,
        row: dict[str, object],
        snapshot: dict[str, object],
    ) -> dict[str, object]:
        manifest = root / "publisher-entrant-manifest.json"
        manifest.write_text(json.dumps({"entrants": [row]}))
        return {
            "entrant_manifest": str(manifest),
            "publisher": snapshot,
        }

    def make_publisher_repo(self, root: Path) -> tuple[Path, dict[str, object]]:
        repo = root / "site"
        (repo / "scripts/lib").mkdir(parents=True)
        (repo / "scripts/data").mkdir(parents=True)
        (repo / "node_modules/@sanity/client").mkdir(parents=True)
        (repo / "node_modules/dotenv").mkdir(parents=True)
        row: dict[str, object] = {
            "id": "fixture-model",
            "provider": "fixture",
            "model": "fixture-model",
            "accepted_reported_models": ["fixture-model"],
            "secret_env": "FIXTURE_API_KEY",
            "provider_lane": "fixture-model",
            "endpoint_family": "fixture",
            "thinking_effort": "medium",
            "context_limit": 100,
            "max_output_tokens": 20,
            "vendor_port": 9999,
            "pricing": {
                "input_per_million": 1,
                "output_per_million": 1,
                "source": "https://example.invalid",
                "verified_at": "now",
            },
        }
        manifest = {
            "expectedChecks": 91,
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
        (repo / "node_modules/@sanity/client/index.js").write_text(
            "export const client = 'fixture';\n"
        )
        (repo / "node_modules/dotenv/package.json").write_text(
            '{"name":"dotenv","version":"fixture"}\n'
        )
        (repo / "node_modules/dotenv/index.js").write_text(
            "export const dotenv = 'fixture';\n"
        )
        (repo / ".env.local").write_text(
            "SANITY_WRITE_TOKEN=publisher-super-secret\n"
            "NEXT_PUBLIC_SANITY_PROJECT_ID=fixture-project\n"
        )
        (repo / ".env.local").chmod(0o600)
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

    def free_port(self) -> int:
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            return int(listener.getsockname()[1])

    def make_supersession_fixture(self, root: Path) -> dict[str, object]:
        publisher_repo, failed_row = self.make_publisher_repo(root)
        failed_row = dict(failed_row)
        failed_row["vendor_port"] = self.free_port()
        carried_row = dict(failed_row)
        carried_row.update(
            {
                "id": "fixture-carried",
                "model": "fixture-carried",
                "accepted_reported_models": ["fixture-carried"],
                "provider_lane": "fixture-carried",
                "vendor_port": self.free_port(),
            }
        )
        publisher_manifest = {
            "expectedChecks": 91,
            "entrants": [
                {
                    "key": row["id"],
                    "label": str(row["id"]).replace("-", " ").title(),
                    "model": row["model"],
                    "docId": f"brun-baseline-{row['id']}-sb70",
                }
                for row in (failed_row, carried_row)
            ],
        }
        (publisher_repo / cloud_sb7.PUBLISHER_MANIFEST).write_text(
            json.dumps(publisher_manifest)
        )
        subprocess.run(["git", "add", "."], cwd=publisher_repo, check=True)
        subprocess.run(
            ["git", "commit", "-qm", "add supersession fixture"],
            cwd=publisher_repo,
            check=True,
        )

        manifest_path = root / "entrants.json"
        manifest_path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "suite": "sb-7.0-rc",
                    "calibration": "uncalibrated fixture",
                    "spend_policy": {
                        "currency": "USD",
                        "total_cap": 20,
                        "provider_caps": {"fixture": 20},
                        "launch_all_entrants_concurrently": True,
                        "max_full_episodes_per_model": 2,
                        "terminal_safe_retry_limit": 0,
                    },
                    "entrants": [failed_row, carried_row],
                }
            )
        )
        secret_path = root / "providers.env"
        secret_path.write_text("FIXTURE_API_KEY=fixture-provider-secret\n")
        secret_path.chmod(0o600)
        old_binary = root / "goose-old"
        old_binary.write_text("old-safe-binary\n")
        old_binary.chmod(0o700)
        replacement = root / "goose-new"
        replacement.write_text("new-safe-binary\n")
        replacement.chmod(0o700)
        publisher = cloud_sb7.publisher_snapshot(
            publisher_repo, [failed_row, carried_row]
        )

        def checked(binary: Path) -> dict[str, object]:
            return {
                "checked_at": "now",
                "binary_sha256": cloud_sb7.sha256_file(binary),
                "models": {},
                "roster_evidence": {},
                "requested_models": [failed_row["model"], carried_row["model"]],
                "ports_free": True,
                "credential_file_mode": "0600",
                "publisher": publisher,
            }

        predecessor_root = root / "predecessor"
        with mock.patch.object(
            cloud_sb7, "preflight", return_value=checked(old_binary)
        ):
            cloud_sb7.init_campaign(
                predecessor_root,
                old_binary,
                manifest_path,
                secret_path,
                publisher_repo,
                True,
            )
        failed_state = cloud_sb7.read_state(predecessor_root, str(failed_row["id"]))
        lifecycle_path = Path(str(failed_state["provider_lifecycle"]))
        lifecycle_path.write_text(
            "\n".join(
                json.dumps(
                    {
                        "schema_version": 1,
                        "timestamp": f"t-{index}",
                        "request_id": "failed-request",
                        "provider": failed_row["provider"],
                        "model": failed_row["model"],
                        "session": "session-1",
                        "state": state,
                        **(
                            {
                                "usage": {
                                    "reported_model": failed_row["model"],
                                    "input_tokens": 10,
                                    "output_tokens": 10,
                                    "total_tokens": 20,
                                }
                            }
                            if state in {"usage_reported", "provider_terminal"}
                            else {}
                        ),
                    }
                )
                for index, state in enumerate(
                    (
                        "queued",
                        "admitted",
                        "first_item",
                        "usage_reported",
                        "provider_terminal",
                    )
                )
            )
            + "\n"
        )
        (Path(str(failed_state["tree"])) / "failed-raw.txt").write_text("sealed failure\n")
        cloud_sb7.update_state(
            predecessor_root,
            str(failed_row["id"]),
            status="STOPPED",
            provider_episode_attempts=1,
            admitted_requests=1,
            provider_terminal_requests=1,
            failure="audited engine infrastructure defect",
        )

        carried_state = cloud_sb7.read_state(predecessor_root, str(carried_row["id"]))
        carried_tree = Path(str(carried_state["tree"]))
        (carried_tree / "successful-raw.txt").write_text("preserve me\n")
        cloud_sb7.update_state(
            predecessor_root,
            str(carried_row["id"]),
            status="BUILD_COMPLETE",
            provider_episode_attempts=1,
            admitted_requests=1,
            provider_terminal_requests=1,
            raw_tree_sha256=cloud_sb7.hash_tree(carried_tree),
        )
        ledger_path = Path(
            str(cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor_root))["budget_ledger"])
        )
        ledger = cloud_sb7.load_json(ledger_path)
        ledger.update(
            {
                "spent_upper_bound": 0.00002,
                "provider_spent_upper_bound": {"fixture": 0.00002},
                "outstanding": {
                    "carried-reservation": {
                        "request_id": "carried-reservation",
                        "provider": carried_row["provider"],
                        "model": carried_row["model"],
                        "reserved_usd": 0.00012,
                        "input_reserve_tokens": 100,
                        "output_reserve_tokens": 20,
                        "created_at_unix_ms": 1,
                    }
                },
                "settled": [
                    {
                        "request_id": "failed-request",
                        "provider": failed_row["provider"],
                        "model": failed_row["model"],
                        "reported_model": failed_row["model"],
                        "input_tokens": 10,
                        "output_tokens": 10,
                        "total_tokens": 20,
                        "charged_upper_bound_usd": 0.00002,
                        "reserved_usd": 0.00012,
                        "settled_at_unix_ms": 1,
                    }
                ],
            }
        )
        cloud_sb7.atomic_json(ledger_path, ledger)
        cloud_sb7.manager_state(
            predecessor_root,
            status="STOPPED",
            pid=None,
            pgid=None,
            identity=None,
        )
        cloud_sb7.update_campaign(predecessor_root, status="STOPPED")

        root_cause = root / "root-cause.txt"
        root_cause.write_text("provider parser accepted an unproven terminal\n")
        regression = root / "regression.txt"
        regression.write_text("terminal-proof regression passed\n")
        predecessor = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor_root))
        evidence_path = root / "defect-evidence.json"
        cloud_sb7.atomic_json(
            evidence_path,
            {
                "schema_version": cloud_sb7.SUPERSESSION_SCHEMA,
                "classification": "infrastructure_defect",
                "defect_id": "provider-terminal-proof-001",
                "summary": "Audited parser defect invalidated the failed full episode.",
                "affected_entrants": [failed_row["id"]],
                "predecessor_campaign_id": predecessor["campaign_id"],
                "predecessor_binary_sha256": predecessor["binary_sha256"],
                "replacement_binary_sha256": cloud_sb7.sha256_file(replacement),
                "fix_source_commit": cloud_sb7.git_value("rev-parse", "HEAD"),
                "artifacts": [
                    {
                        "role": "root_cause",
                        "path": str(root_cause),
                        "sha256": cloud_sb7.sha256_file(root_cause),
                    },
                    {
                        "role": "regression_test",
                        "path": str(regression),
                        "sha256": cloud_sb7.sha256_file(regression),
                    },
                ],
            },
        )
        return {
            "predecessor": predecessor_root,
            "successor": root / "successor",
            "binary": replacement,
            "manifest": manifest_path,
            "secrets": secret_path,
            "publisher": publisher_repo,
            "evidence": evidence_path,
            "checked": checked(replacement),
            "failed_id": str(failed_row["id"]),
            "carried_id": str(carried_row["id"]),
        }

    def supersede_fixture(self, fixture: dict[str, object]) -> dict[str, object]:
        with mock.patch.object(
            cloud_sb7, "preflight", return_value=fixture["checked"]
        ):
            return cloud_sb7.supersede_campaign(
                Path(str(fixture["predecessor"])),
                Path(str(fixture["successor"])),
                Path(str(fixture["binary"])),
                Path(str(fixture["manifest"])),
                Path(str(fixture["secrets"])),
                Path(str(fixture["publisher"])),
                Path(str(fixture["evidence"])),
                True,
            )

    def test_supersession_carries_spend_attempts_and_success_without_rerun(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor_campaign_before = (
                Path(str(fixture["predecessor"])) / "campaign.json"
            ).read_bytes()
            predecessor_ledger = cloud_sb7.load_json(
                Path(
                    str(
                        cloud_sb7.load_json(
                            cloud_sb7.campaign_file(Path(str(fixture["predecessor"])))
                        )["budget_ledger"]
                    )
                )
            )

            successor = self.supersede_fixture(fixture)

            self.assertEqual(successor["lineage"]["generation"], 1)
            self.assertEqual(
                predecessor_campaign_before,
                (Path(str(fixture["predecessor"])) / "campaign.json").read_bytes(),
                "supersession must not mutate predecessor campaign.json",
            )
            self.assertEqual(
                cloud_sb7.stop(Path(str(fixture["predecessor"]))),
                0,
            )
            self.assertEqual(
                predecessor_campaign_before,
                (Path(str(fixture["predecessor"])) / "campaign.json").read_bytes(),
                "a repeated stop must not mutate a sealed predecessor",
            )
            self.assertIsNone(
                cloud_sb7.lineage_failure(Path(str(fixture["successor"])))
            )
            successor_ledger = cloud_sb7.load_json(Path(str(successor["budget_ledger"])))
            self.assertEqual(successor_ledger, predecessor_ledger)
            failed = cloud_sb7.read_state(
                Path(str(fixture["successor"])), str(fixture["failed_id"])
            )
            carried = cloud_sb7.read_state(
                Path(str(fixture["successor"])), str(fixture["carried_id"])
            )
            self.assertEqual(failed["status"], "PLANNED")
            self.assertEqual(failed["provider_episode_attempts"], 1)
            self.assertEqual(failed["lineage_role"], "infrastructure_defect_restart")
            self.assertEqual(carried["status"], "BUILD_COMPLETE")
            self.assertEqual(carried["provider_episode_attempts"], 1)
            self.assertEqual(carried["lineage_role"], "carried_success")
            self.assertEqual(
                (Path(str(carried["tree"])) / "successful-raw.txt").read_text(),
                "preserve me\n",
            )
            self.assertIn(
                "strict all-entrant cloud-smoke proof",
                cloud_sb7.supersession_smoke_gate_failure(
                    Path(str(fixture["successor"]))
                )
                or "",
            )

    def test_supersession_is_idempotent_and_rejects_forks_and_second_hops(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            first = self.supersede_fixture(fixture)
            second = self.supersede_fixture(fixture)
            self.assertEqual(first["lineage"], second["lineage"])

            fork = dict(fixture)
            fork["successor"] = Path(raw) / "fork"
            with self.assertRaisesRegex(SystemExit, "immutable receipt"):
                self.supersede_fixture(fork)

            second_hop = dict(fixture)
            second_hop["predecessor"] = fixture["successor"]
            second_hop["successor"] = Path(raw) / "second-hop"
            with self.assertRaisesRegex(SystemExit, "one hop"):
                self.supersede_fixture(second_hop)

    def test_supersession_rejects_outcome_and_ambiguous_lifecycle_reruns(self) -> None:
        for defect in ("outcome", "ambiguous"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_supersession_fixture(Path(raw))
                predecessor = Path(str(fixture["predecessor"]))
                failed_id = str(fixture["failed_id"])
                state = cloud_sb7.read_state(predecessor, failed_id)
                if defect == "outcome":
                    cloud_sb7.update_state(
                        predecessor,
                        failed_id,
                        status="BUILD_COMPLETE",
                        raw_tree_sha256=cloud_sb7.hash_tree(Path(str(state["tree"]))),
                    )
                    expected = "successful build cannot be rerun"
                else:
                    lifecycle = Path(str(state["provider_lifecycle"]))
                    lines = lifecycle.read_text().splitlines()
                    lifecycle.write_text("\n".join(lines[:-1]) + "\n")
                    expected = "lifecycle is ambiguous"
                with self.assertRaisesRegex(SystemExit, expected):
                    self.supersede_fixture(fixture)

    def test_supersession_crash_boundaries_resume_without_duplicate_root(self) -> None:
        for boundary in (
            "receipt_committed",
            "staged_initialized",
            "lineage_staged",
            "root_committed",
        ):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_supersession_fixture(Path(raw))

                def crash(stage: str) -> None:
                    if stage == boundary:
                        raise RuntimeError(f"crash at {stage}")

                with mock.patch.object(
                    cloud_sb7, "supersession_fault", side_effect=crash
                ), self.assertRaisesRegex(RuntimeError, boundary):
                    self.supersede_fixture(fixture)
                successor = self.supersede_fixture(fixture)
                self.assertEqual(successor["lineage"]["generation"], 1)
                self.assertIsNone(
                    cloud_sb7.lineage_failure(Path(str(fixture["successor"])))
                )

    def test_committed_supersession_recovers_from_its_bundle_without_sources(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))

            def crash(stage: str) -> None:
                if stage == "root_committed":
                    raise RuntimeError("crash at root_committed")

            with mock.patch.object(
                cloud_sb7, "supersession_fault", side_effect=crash
            ), self.assertRaisesRegex(RuntimeError, "root_committed"):
                self.supersede_fixture(fixture)
            evidence = cloud_sb7.load_json(Path(str(fixture["evidence"])))
            for artifact in evidence["artifacts"]:
                Path(str(artifact["path"])).unlink()
            Path(str(fixture["evidence"])).unlink()
            Path(str(fixture["manifest"])).unlink()

            recovered = self.supersede_fixture(fixture)

            self.assertEqual(recovered["lineage"]["generation"], 1)
            self.assertIsNone(
                cloud_sb7.lineage_failure(Path(str(fixture["successor"])))
            )

    def test_supersession_preserves_terminal_unsettled_reserve(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor = Path(str(fixture["predecessor"]))
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            settlement = ledger["settled"].pop()
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            ledger["outstanding"]["failed-request"] = {
                "request_id": "failed-request",
                "provider": settlement["provider"],
                "model": settlement["model"],
                "reserved_usd": settlement["reserved_usd"],
                "input_reserve_tokens": 100,
                "output_reserve_tokens": 20,
                "created_at_unix_ms": 2,
            }
            cloud_sb7.atomic_json(ledger_path, ledger)

            successor = self.supersede_fixture(fixture)

            successor_root = Path(str(fixture["successor"]))
            successor_ledger = cloud_sb7.load_json(Path(str(successor["budget_ledger"])))
            self.assertEqual(
                successor_ledger["outstanding"]["failed-request"],
                ledger["outstanding"]["failed-request"],
            )
            lineage = cloud_sb7.load_json(successor_root / "lineage/lineage.json")
            self.assertEqual(
                lineage["predecessor_terminal_outstanding"][str(fixture["failed_id"])],
                ["failed-request"],
            )
            del successor_ledger["outstanding"]["failed-request"]
            cloud_sb7.atomic_json(Path(str(successor["budget_ledger"])), successor_ledger)
            self.assertIn(
                "reservation changed or disappeared",
                cloud_sb7.lineage_failure(successor_root) or "",
            )

    def test_supersession_rejects_uncorrelated_outstanding_reserve(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor = Path(str(fixture["predecessor"]))
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            settlement = ledger["settled"].pop()
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            ledger["outstanding"]["uncorrelated"] = {
                "request_id": "uncorrelated",
                "provider": settlement["provider"],
                "model": settlement["model"],
                "reserved_usd": settlement["reserved_usd"],
                "input_reserve_tokens": 100,
                "output_reserve_tokens": 20,
                "created_at_unix_ms": 2,
            }
            cloud_sb7.atomic_json(ledger_path, ledger)

            with self.assertRaisesRegex(SystemExit, "no preserved accounting evidence"):
                self.supersede_fixture(fixture)

    def test_supersession_lineage_detects_ledger_artifact_and_attempt_tampering(self) -> None:
        for tamper in ("ledger", "artifact", "attempt"):
            with self.subTest(tamper=tamper), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_supersession_fixture(Path(raw))
                successor = self.supersede_fixture(fixture)
                root = Path(str(fixture["successor"]))
                if tamper == "ledger":
                    ledger_path = Path(str(successor["budget_ledger"]))
                    ledger = cloud_sb7.load_json(ledger_path)
                    ledger["spent_upper_bound"] = 0
                    ledger["provider_spent_upper_bound"] = {"fixture": 0}
                    ledger["settled"] = []
                    cloud_sb7.atomic_json(ledger_path, ledger)
                    expected = "spend decreased"
                elif tamper == "artifact":
                    predecessor = Path(str(fixture["predecessor"]))
                    state = cloud_sb7.read_state(predecessor, str(fixture["failed_id"]))
                    (Path(str(state["tree"])) / "failed-raw.txt").write_text("changed\n")
                    expected = "artifact changed"
                else:
                    cloud_sb7.update_state(
                        root,
                        str(fixture["failed_id"]),
                        provider_episode_attempts=0,
                    )
                    expected = "attempt count reset"
                self.assertIn(expected, cloud_sb7.lineage_failure(root) or "")

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

    def test_reported_model_aliases_must_match_authenticated_roster(self) -> None:
        row = {
            "provider": "google",
            "model": "gemini-3.7-flash",
            "accepted_reported_models": [
                "gemini-3.7-flash",
                "gemini-3.7-flash-08-2026",
            ],
        }
        roster = {
            "models": {"google": {"gemini-3.7-flash"}},
            "accepted_reported_models": {
                "google": {
                    "gemini-3.7-flash": [
                        "gemini-3.7-flash",
                        "gemini-3.7-flash-08-2026",
                    ]
                }
            },
            "evidence": {
                "google": {
                    "gemini-3.7-flash": {
                        "inputTokenLimit": 1_048_576,
                        "outputTokenLimit": 65_536,
                    }
                }
            },
        }
        row["context_limit"] = 1_048_576
        row["max_output_tokens"] = 65_536
        cloud_sb7.validate_rosters([row], roster)
        row["accepted_reported_models"].append("gemini-3.7-flash-anything")
        with self.assertRaises(SystemExit):
            cloud_sb7.validate_rosters([row], roster)

    def test_google_manifest_limits_must_match_authenticated_roster(self) -> None:
        row = {
            "provider": "google",
            "model": "gemini-3.1-pro-preview",
            "accepted_reported_models": [
                "gemini-3.1-pro-preview",
                "gemini-3.1-pro-preview-01-2026",
            ],
            "context_limit": 1_048_576,
            "max_output_tokens": 65_536,
        }
        roster = {
            "models": {"google": {"gemini-3.1-pro-preview"}},
            "accepted_reported_models": {
                "google": {
                    "gemini-3.1-pro-preview": list(row["accepted_reported_models"])
                }
            },
            "evidence": {
                "google": {
                    "gemini-3.1-pro-preview": {
                        "inputTokenLimit": 1_048_576,
                        "outputTokenLimit": 65_536,
                    }
                }
            },
        }

        cloud_sb7.validate_rosters([row], roster)
        row["max_output_tokens"] = 65_535
        with self.assertRaises(SystemExit):
            cloud_sb7.validate_rosters([row], roster)

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

    def test_control_json_rejects_duplicate_object_keys(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "ledger.json"
            path.write_text('{"outstanding": {}, "outstanding": {"forged": {}}}\n')
            with self.assertRaisesRegex(SystemExit, "duplicate object key"):
                cloud_sb7.load_json(path)

    def test_complete_entrant_secret_scan_crosses_chunks_and_blocks_consumers(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            entrant_id = "fixture-model"
            unit = root / "entrants" / entrant_id
            secret = b"fixture-provider-secret"
            targets = (
                unit / "provider-lifecycle.jsonl",
                unit / "vendor-trace-build.jsonl",
                cloud_sb7.state_file(root, entrant_id),
                root / "scores" / entrant_id / "attempt-1/score.log",
                root / "publish" / entrant_id / "publisher.log",
                root / "manager.log",
            )
            for path in targets:
                original = path.read_bytes() if path.is_file() else None
                path.parent.mkdir(parents=True, exist_ok=True)
                boundary_prefix = 1024 * 1024 - len(secret) // 2
                path.write_bytes(b"x" * boundary_prefix + secret + b"\n")
                self.assertIn(
                    str(path),
                    cloud_sb7.persisted_entrant_secret_hits(root, campaign, entrant_id),
                )
                if original is None:
                    path.unlink()
                else:
                    path.write_bytes(original)

            leak = unit / "build-manifest.json"
            leak.write_bytes(secret)
            self.assertFalse(cloud_sb7.publish_one(root, entrant_id))
            self.assertEqual(
                cloud_sb7.read_state(root, entrant_id)["status"], "INCOMPLETE"
            )
            cloud_sb7.update_state(root, entrant_id, status="BUILD_COMPLETE")
            self.assertFalse(cloud_sb7.score_one(root, entrant_id))
            self.assertEqual(
                cloud_sb7.read_state(root, entrant_id)["status"], "INCOMPLETE"
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
        def event(state: str, **extra: object) -> dict[str, object]:
            value: dict[str, object] = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": "request-1",
                "provider": "google",
                "model": "gemini-3.7-flash",
                "session": "session-1",
                "state": state,
            }
            value.update(extra)
            return value

        usage = {
            "reported_model": "gemini-3.7-flash",
            "input_tokens": 1,
            "output_tokens": 2,
            "total_tokens": 3,
        }

        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "lifecycle.jsonl"
            path.write_text(
                "\n".join(
                    [
                        json.dumps(event("queued")),
                        json.dumps(event("admitted")),
                        json.dumps(event("first_item")),
                        json.dumps(event("usage_reported", usage=usage)),
                        json.dumps(event("provider_terminal", usage=usage)),
                    ]
                )
                + "\n"
            )
            summary = cloud_sb7.lifecycle_summary(
                path,
                expected_provider="google",
                expected_model="gemini-3.7-flash",
            )
        self.assertEqual(summary["admitted"], 1)
        self.assertEqual(summary["terminal"], 1)
        self.assertEqual(summary["first_output_at"], "now")
        self.assertEqual(summary["malformed_lines"], 0)
        self.assertEqual(summary["transition_errors"], [])
        self.assertEqual(summary["ambiguous_request_ids"], [])
        self.assertIs(summary["valid"], True)

    def test_lifecycle_equal_counts_cannot_cross_match_request_ids(self) -> None:
        base = {
            "schema_version": 1,
            "timestamp": "now",
            "provider": "google",
            "model": "gemini-3.7-flash",
            "session": "session-1",
        }
        events = [
            {**base, "request_id": "request-a", "state": "queued"},
            {**base, "request_id": "request-a", "state": "admitted"},
            {
                **base,
                "request_id": "request-b",
                "state": "provider_terminal",
                "usage": {
                    "reported_model": "gemini-3.7-flash",
                    "input_tokens": 1,
                    "output_tokens": 2,
                    "total_tokens": 3,
                },
            },
        ]
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "lifecycle.jsonl"
            path.write_text("\n".join(map(json.dumps, events)) + "\n")
            summary = cloud_sb7.lifecycle_summary(
                path,
                expected_provider="google",
                expected_model="gemini-3.7-flash",
            )

        self.assertEqual(summary["admitted"], 1)
        self.assertEqual(summary["terminal"], 0)
        self.assertIn("request-a", summary["ambiguous_request_ids"])
        self.assertTrue(summary["transition_errors"])
        self.assertIs(summary["valid"], False)

    def test_lifecycle_rejects_identity_drift_and_duplicate_terminal(self) -> None:
        def event(state: str, **extra: object) -> dict[str, object]:
            value: dict[str, object] = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": "request-1",
                "provider": "google",
                "model": "gemini-3.7-flash",
                "session": "session-1",
                "state": state,
            }
            value.update(extra)
            return value

        usage = {
            "reported_model": "gemini-3.7-flash",
            "input_tokens": 1,
            "output_tokens": 2,
            "total_tokens": 3,
        }

        events = [
            event("queued"),
            event("admitted"),
            event("usage_reported", usage=usage),
            event("provider_terminal", usage=usage),
            event("provider_terminal", usage=usage),
            event("first_item", model="gemini-3.1-pro-preview"),
        ]
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "lifecycle.jsonl"
            path.write_text("\n".join(map(json.dumps, events)) + "\n")
            summary = cloud_sb7.lifecycle_summary(
                path,
                expected_provider="google",
                expected_model="gemini-3.7-flash",
            )

        self.assertEqual(summary["admitted"], 1)
        self.assertEqual(summary["terminal"], 1)
        self.assertEqual(len(summary["transition_errors"]), 2)
        self.assertIs(summary["valid"], False)

    def test_lifecycle_rejects_consistently_wrong_entrant_identity(self) -> None:
        events = [
            {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": "request-1",
                "provider": "zai_api",
                "model": "glm-5.3",
                "session": "session-1",
                "state": state,
            }
            for state in ("queued", "error")
        ]
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "lifecycle.jsonl"
            path.write_text("\n".join(map(json.dumps, events)) + "\n")
            summary = cloud_sb7.lifecycle_summary(
                path,
                expected_provider="google",
                expected_model="gemini-3.7-flash",
            )

        self.assertEqual(len(summary["transition_errors"]), 2)
        self.assertEqual(summary["request_states"], {})
        self.assertIs(summary["valid"], False)

    def test_lifecycle_rejects_missing_or_changed_terminal_usage(self) -> None:
        base = {
            "schema_version": 1,
            "timestamp": "now",
            "request_id": "request-1",
            "provider": "google",
            "model": "gemini-3.7-flash",
            "session": "session-1",
        }
        usage = {
            "reported_model": "gemini-3.7-flash",
            "input_tokens": 1,
            "output_tokens": 2,
            "total_tokens": 3,
        }
        for terminal_usage in (
            {"reported_model": "gemini-3.7-flash", "total_tokens": 3},
            {**usage, "output_tokens": 3, "total_tokens": 4},
        ):
            with self.subTest(
                terminal_usage=terminal_usage
            ), tempfile.TemporaryDirectory() as raw:
                path = Path(raw) / "lifecycle.jsonl"
                events = [
                    {**base, "state": "queued"},
                    {**base, "state": "admitted"},
                    {**base, "state": "usage_reported", "usage": usage},
                    {
                        **base,
                        "state": "provider_terminal",
                        "usage": terminal_usage,
                    },
                ]
                path.write_text("\n".join(map(json.dumps, events)) + "\n")
                summary = cloud_sb7.lifecycle_summary(
                    path,
                    expected_provider="google",
                    expected_model="gemini-3.7-flash",
                )
                self.assertIs(summary["valid"], False)
                self.assertEqual(summary["terminal"], 0)

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

    def test_dead_manager_during_scoring_stops_scorer_and_keeps_attempt(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "SCORING")
            old_tree = root / "scores/model/attempt-1/tree"
            old_tree.mkdir(parents=True)
            manager = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            scorer = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                cloud_sb7.atomic_json(
                    root / "manager.json",
                    {
                        "status": "SCORING",
                        "pid": manager.pid,
                        "pgid": manager.pid,
                        "identity": cloud_sb7.process_identity(manager.pid),
                    },
                )
                cloud_sb7.update_state(
                    root,
                    "model",
                    status="SCORING",
                    score_attempts=1,
                    score_pid=scorer.pid,
                    score_pgid=scorer.pid,
                    score_identity=cloud_sb7.process_identity(scorer.pid),
                )
                os.kill(manager.pid, 9)
                manager.wait(timeout=5)

                self.assertTrue(cloud_sb7.recover_dead_manager(root))
                scorer.wait(timeout=5)
                state = cloud_sb7.read_state(root, "model")
                self.assertEqual(state["status"], "SCORE_FAILED")
                self.assertEqual(cloud_sb7.next_score_attempt(root, "model", state), 2)
                self.assertTrue(old_tree.is_dir())
                self.assertEqual(
                    cloud_sb7.load_json(root / "manager.json")["status"],
                    "RECOVERED",
                )
            finally:
                if manager.poll() is None:
                    cloud_sb7.stop_group(manager.pid, grace_seconds=0.1)
                if scorer.poll() is None:
                    cloud_sb7.stop_group(scorer.pid, grace_seconds=0.1)

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

    def test_interrupted_publisher_is_stopped_and_requires_remote_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "SCORING")
            publisher = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                cloud_sb7.update_state(
                    root,
                    "model",
                    status="PUBLISHING",
                    publisher_pid=publisher.pid,
                    publisher_pgid=publisher.pid,
                    publisher_identity=cloud_sb7.process_identity(publisher.pid),
                )
                cloud_sb7.recover_interrupted_publication(root)
                publisher.wait(timeout=5)
                state = cloud_sb7.read_state(root, "model")
                self.assertEqual(state["status"], "PUBLISH_FAILED")
                self.assertIn("remote receipt", state["failure"])
                self.assertIsNone(state["publisher_pid"])
                self.assertIsNone(state["publisher_identity"])
            finally:
                if publisher.poll() is None:
                    cloud_sb7.stop_group(publisher.pid, grace_seconds=0.1)

    def test_reused_publisher_pid_is_never_signaled(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "SCORING")
            cloud_sb7.update_state(
                root,
                "model",
                status="PUBLISHING",
                publisher_pid=4321,
                publisher_pgid=4321,
                publisher_identity="recorded-old-process",
            )
            with (
                mock.patch.object(
                    cloud_sb7,
                    "process_identity",
                    return_value="different-reused-process",
                ),
                mock.patch.object(cloud_sb7, "stop_group") as stop,
            ):
                cloud_sb7.recover_interrupted_publication(root)
            stop.assert_not_called()
            state = cloud_sb7.read_state(root, "model")
            self.assertEqual(state["status"], "PUBLISH_FAILED")
            self.assertIsNone(state["publisher_pid"])

    def test_website_base_url_is_an_https_origin_without_credentials(self) -> None:
        self.assertEqual(
            cloud_sb7.normalized_website_base_url("https://leanzero.net/"),
            "https://leanzero.net",
        )
        for value in (
            "http://leanzero.net",
            "https://leanzero.net/path",
            "https://leanzero.net?next=evil",
            "https://token@leanzero.net",
            "https://leanzero.net#fragment",
        ):
            with self.subTest(value=value), self.assertRaises(SystemExit):
                cloud_sb7.normalized_website_base_url(value)

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
                "node_modules/@sanity/client",
                snapshot["runtime_hashes"],
            )
            self.assertEqual(
                snapshot["sanity_target"],
                {"project_id": "fixture-project", "dataset": "production"},
            )
            self.assertEqual(snapshot["env_file_mode"], "0600")
            self.assertNotIn("publisher-super-secret", serialized)

            (repo / cloud_sb7.PUBLISHER_SCRIPT).write_text("console.log('changed')\n")
            with self.assertRaisesRegex(SystemExit, "must be clean"):
                cloud_sb7.publisher_snapshot(repo, [row])

    def test_ignored_runtime_javascript_mutation_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            repo, row = self.make_publisher_repo(root)
            snapshot = cloud_sb7.publisher_snapshot(repo, [row])
            campaign = self.publisher_campaign(root, row, snapshot)
            runtime = repo / "node_modules/@sanity/client/index.js"
            runtime.write_text("export const client = 'mutated';\n")

            mismatch = cloud_sb7.publisher_mismatch(campaign)
            self.assertIn("runtime_hashes", mismatch or "")

    def test_environment_target_or_token_mutation_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            repo, row = self.make_publisher_repo(root)
            snapshot = cloud_sb7.publisher_snapshot(repo, [row])
            campaign = self.publisher_campaign(root, row, snapshot)
            env_file = repo / ".env.local"
            env_file.write_text(
                "SANITY_WRITE_TOKEN=different-publisher-secret\n"
                "NEXT_PUBLIC_SANITY_PROJECT_ID=different-project\n"
                "NEXT_PUBLIC_SANITY_DATASET=staging\n"
            )

            mismatch = cloud_sb7.publisher_mismatch(campaign)
            self.assertIn("env_file_sha256", mismatch or "")
            with self.assertRaisesRegex(
                cloud_sb7.PublicationError,
                "environment changed after freeze",
            ):
                cloud_sb7.pinned_publisher_env_values(campaign)

    def test_group_readable_publisher_environment_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo, row = self.make_publisher_repo(Path(raw))
            (repo / ".env.local").chmod(0o640)
            with self.assertRaisesRegex(SystemExit, "must be mode 0600"):
                cloud_sb7.publisher_snapshot(repo, [row])

    def test_clean_website_commit_drift_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            repo, row = self.make_publisher_repo(root)
            snapshot = cloud_sb7.publisher_snapshot(repo, [row])
            campaign = self.publisher_campaign(root, row, snapshot)
            (repo / "README.md").write_text("new clean commit\n")
            subprocess.run(["git", "add", "README.md"], cwd=repo, check=True)
            subprocess.run(
                ["git", "commit", "-qm", "move website commit"],
                cwd=repo,
                check=True,
            )

            mismatch = cloud_sb7.publisher_mismatch(campaign)
            self.assertIn("commit", mismatch or "")

    def test_frozen_publisher_runtime_is_independent_and_sealed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            repo, row = self.make_publisher_repo(root)
            snapshot = cloud_sb7.publisher_snapshot(repo, [row])
            frozen = cloud_sb7.freeze_publisher_runtime(root / "frozen", snapshot)
            campaign = {"publisher": {**snapshot, "frozen": frozen}}

            (repo / "node_modules/@sanity/client/index.js").write_text(
                "export const client = 'mutated live source';\n"
            )
            self.assertIsNone(cloud_sb7.frozen_publisher_mismatch(campaign))
            (Path(frozen["root"]) / "node_modules/@sanity/client/index.js").unlink()
            self.assertIn(
                "changed after freeze",
                cloud_sb7.frozen_publisher_mismatch(campaign) or "",
            )

    def test_publication_stage_is_sealed_and_never_overwritten(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            runs = cloud_sb7.publication_stage(root, "fixture-model")
            self.assertTrue((runs / "fixture-model.json").is_file())
            self.assertTrue(
                (runs / "fixture-model-r0/sb7-shots/100-loaded.png").is_file()
            )
            state = cloud_sb7.read_state(root, "fixture-model")
            self.assertEqual(state["publish_stage_sha256"], cloud_sb7.hash_tree(runs))

            (runs / "fixture-model.json").write_text("{}")
            with self.assertRaisesRegex(
                cloud_sb7.PublicationError, "changed after sealing"
            ):
                cloud_sb7.publication_stage(root, "fixture-model")

    def test_frozen_instrument_survives_live_source_mutation_and_deletion(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source_repo = root / "live-repo"
            first = source_repo / "evals/swarm-bench/bench/score_sb7.py"
            second = source_repo / "evals/swarm-bench/bench/fixtures_v3.py"
            first.parent.mkdir(parents=True)
            first.write_text("score = 'frozen'\n")
            second.write_text("fixture = 'frozen'\n")
            frozen_root = root / "campaign/instrument/source"
            hashes = cloud_sb7.freeze_instrument(
                frozen_root,
                source_repo=source_repo,
                paths=[first, second],
            )
            entrant_manifest = root / "campaign/instrument/entrants.json"
            entrant_manifest.parent.mkdir(parents=True, exist_ok=True)
            entrant_manifest.write_text('{"entrants": []}\n')
            campaign = {
                "instrument_root": str(frozen_root),
                "instrument_hashes": hashes,
                "entrant_manifest": str(entrant_manifest),
                "entrant_manifest_sha256": cloud_sb7.sha256_file(entrant_manifest),
            }

            first.write_text("score = 'mutated live source'\n")
            second.unlink()
            self.assertIsNone(cloud_sb7.instrument_mismatch(campaign))
            self.assertEqual(
                cloud_sb7.campaign_instrument_path(
                    campaign, "evals/swarm-bench/bench/score_sb7.py"
                ).read_text(),
                "score = 'frozen'\n",
            )

            cloud_sb7.campaign_instrument_path(
                campaign, "evals/swarm-bench/bench/fixtures_v3.py"
            ).unlink()
            mismatch = cloud_sb7.instrument_mismatch(campaign)
            self.assertIn("fixtures_v3.py", mismatch or "")

    def test_frozen_instrument_contains_every_transitive_scorer_import(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            frozen_root = Path(raw) / "source"
            hashes = cloud_sb7.freeze_instrument(frozen_root)
            scorer = frozen_root / "evals/swarm-bench/bench/score_sb7.py"
            proc = subprocess.run(
                [sys.executable, str(scorer), "--help"],
                cwd=frozen_root,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertIn("usage:", proc.stdout.lower())
            self.assertEqual(
                hashes["evals/swarm-bench/bench/score_sb7.py"],
                cloud_sb7.sha256_file(scorer),
            )

    def test_supervisor_launches_the_frozen_coordinator_not_live_source(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            entrant_id = "fixture-model"
            (root / "entrants" / entrant_id / "logs").mkdir(parents=True)
            frozen = root / "instrument/source/evals/swarm-bench/bench/cloud_sb7.py"
            cloud_sb7.atomic_json(
                cloud_sb7.campaign_file(root), {"coordinator": str(frozen)}
            )
            cloud_sb7.atomic_json(
                cloud_sb7.state_file(root, entrant_id),
                {"entrant": entrant_id, "status": "PLANNED"},
            )
            proc = mock.Mock(pid=4321)
            with mock.patch.object(
                cloud_sb7, "launch_detached", return_value=proc
            ) as launch:
                self.assertIs(cloud_sb7.launch_supervisor(root, entrant_id), proc)
            command = launch.call_args.args[0]
            self.assertEqual(command[1], str(frozen))
            self.assertNotEqual(command[1], str(Path(cloud_sb7.__file__).resolve()))

    def test_publisher_process_redacts_secret_output(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            log = root / "publisher.log"
            result = cloud_sb7.run_logged_process(
                [sys.executable, "-c", "print('token=publisher-super-secret')"],
                cwd=root,
                env={"PATH": os.environ.get("PATH", "")},
                log_path=log,
                timeout_seconds=5,
                redactions=["publisher-super-secret"],
            )
            self.assertEqual(result["exit_code"], 0)
            self.assertNotIn("publisher-super-secret", log.read_text())
            self.assertIn("[REDACTED]", log.read_text())

    def test_rendered_verification_requires_exact_board_and_run_evidence(self) -> None:
        verdict = self.fixture_verdict()
        entry = {
            "label": "Fixture Model",
            "model": "fixture-model",
            "doc_id": "brun-baseline-fixture-model-sb70",
        }
        run_url = (
            "https://example.invalid/agentic-benchmarks/run/"
            "brun-baseline-fixture-model-sb70"
        )
        board = (
            '<script type="application/ld+json">'
            + json.dumps(
                {
                    "@type": "ItemList",
                    "itemListElement": [
                        {
                            "@type": "ListItem",
                            "name": "Fixture Model — 0.4200",
                            "url": run_url,
                        }
                    ],
                }
            )
            + "</script>"
        )
        run = (
            '<script type="application/ld+json">'
            + json.dumps(
                {
                    "@type": "Dataset",
                    "name": "fixture-model on sb-7.0-rc",
                    "url": run_url,
                    "variableMeasured": [{"value": 0.42}],
                }
            )
            + "</script>"
            + "<h1>Fixture Model — 0.4200 on sb-7.0-rc</h1>"
            + "<p>fixture-model · scorer sb-7.0-rc</p>"
            + "<p>Scorer calibration · UNCALIBRATED — fixture defaults; rc-grade only</p>"
        )
        matched, evidence = cloud_sb7.rendered_publication_matches(
            board, run, "https://example.invalid", entry, verdict
        )
        self.assertTrue(matched, evidence)
        matched, evidence = cloud_sb7.rendered_publication_matches(
            board.replace("0.4200", "0.4100"),
            run,
            "https://example.invalid",
            entry,
            verdict,
        )
        self.assertFalse(matched)
        self.assertFalse(evidence["board_item_exact"])

    def test_remote_receipt_compares_full_checks_and_screenshot_bytes(self) -> None:
        verdict = self.fixture_verdict()
        entry = {
            "label": "Fixture Model",
            "model": "fixture-model",
            "doc_id": "brun-baseline-fixture-model-sb70",
        }
        shot_sha1 = "a" * 40
        asset_id = f"image-{shot_sha1}-1x1-png"
        document = {
            "_id": entry["doc_id"],
            "_type": "benchmarkRun",
            "_rev": "revision",
            "_updatedAt": "now",
            "label": entry["label"],
            "model": entry["model"],
            "baseline": True,
            "score": 0.42,
            "tierA": 0.5,
            "tierB": 0.5,
            "tierC": 0.5,
            "tierD": 0.5,
            "wallSecs": 123,
            "scorerVersion": verdict["scorer_version"],
            "calibration": verdict["calibration"],
            "excellent": False,
            "checksSummary": [
                {
                    "check": "fixture_check",
                    "tier": "A",
                    "score": 0.5,
                    "detail": "fixture detail",
                }
            ],
            "screenshots": [
                {
                    "caption": "Final render",
                    "asset": {"_ref": asset_id},
                }
            ],
        }
        plan = [{"caption": "Final render", "sha1": shot_sha1}]

        def lookup(_campaign: object, document_id: str) -> dict[str, object] | None:
            if document_id == entry["doc_id"]:
                return document
            if document_id == asset_id:
                return {"_id": asset_id, "sha1hash": shot_sha1}
            return None

        with mock.patch.object(cloud_sb7, "sanity_document", side_effect=lookup):
            receipt = cloud_sb7.remote_publication_receipt({}, entry, verdict, plan)
            self.assertTrue(receipt["matched"], receipt)
            document["checksSummary"][0]["score"] = 0.4
            receipt = cloud_sb7.remote_publication_receipt({}, entry, verdict, plan)
            self.assertFalse(receipt["matched"])
            self.assertIn("document check 0 differs", receipt["reasons"])

    def test_hermetic_verdict_must_match_frozen_scorer_and_check_contract(self) -> None:
        verdict = self.fixture_verdict()
        campaign = {
            "scorer_version": "sb-7.0-rc",
            "publisher": {"expected_checks": 1},
        }
        self.assertIsNone(cloud_sb7.verdict_failure(verdict, campaign))
        wrong = dict(verdict, scorer_version="sb-7.0")
        self.assertIn("scorer version", cloud_sb7.verdict_failure(wrong, campaign) or "")
        wrong = dict(verdict, checks=[])
        self.assertIn("check count", cloud_sb7.verdict_failure(wrong, campaign) or "")

    def test_matching_remote_receipt_resumes_without_second_live_write(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            runs = cloud_sb7.publication_stage(root, "fixture-model")
            state = cloud_sb7.read_state(root, "fixture-model")
            cloud_sb7.update_state(
                root,
                "fixture-model",
                status="PUBLISH_FAILED",
                publisher_plan=[
                    {
                        "name": "loaded",
                        "caption": "Final render",
                        "source": str(
                            runs / "fixture-model-r0/sb7-shots/100-loaded.png"
                        ),
                        "sha1": "a" * 40,
                        "sha256": "b" * 64,
                    }
                ],
                score_attempts=state["score_attempts"],
            )
            rendered = {
                "run_url": (
                    "https://example.invalid/agentic-benchmarks/run/"
                    "brun-baseline-fixture-model-sb70"
                )
            }
            with (
                mock.patch.object(
                    cloud_sb7,
                    "remote_publication_receipt",
                    return_value={"matched": True, "document_sha256": "remote"},
                ),
                mock.patch.object(cloud_sb7, "run_publisher") as publisher,
                mock.patch.object(
                    cloud_sb7,
                    "revalidate_publication",
                    return_value={"status": 200},
                ),
                mock.patch.object(
                    cloud_sb7,
                    "verify_rendered_publication",
                    return_value=rendered,
                ),
            ):
                self.assertTrue(cloud_sb7.publish_one(root, "fixture-model"))
            publisher.assert_not_called()
            final = cloud_sb7.read_state(root, "fixture-model")
            self.assertEqual(final["status"], "PUBLISHED")
            self.assertTrue(final["publisher_write_adopted"])
            self.assertEqual(final["score_attempts"], 1)

    def test_ambiguous_publisher_exit_is_accepted_only_with_matching_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            runs = cloud_sb7.publication_stage(root, "fixture-model")
            cloud_sb7.update_state(
                root,
                "fixture-model",
                publisher_plan=[
                    {
                        "name": "loaded",
                        "caption": "Final render",
                        "source": str(
                            runs / "fixture-model-r0/sb7-shots/100-loaded.png"
                        ),
                        "sha1": "a" * 40,
                        "sha256": "b" * 64,
                    }
                ],
            )
            receipts = [
                {"matched": False, "reasons": ["old document"]},
                {"matched": True, "document_sha256": "new document"},
            ]
            with (
                mock.patch.object(
                    cloud_sb7, "remote_publication_receipt", side_effect=receipts
                ),
                mock.patch.object(
                    cloud_sb7,
                    "run_publisher",
                    return_value={
                        "exit_code": 7,
                        "timed_out": False,
                        "log": "publisher.log",
                        "log_sha256": "log",
                        "pid": 1,
                    },
                ),
                mock.patch.object(
                    cloud_sb7,
                    "revalidate_publication",
                    return_value={"status": 200},
                ),
                mock.patch.object(
                    cloud_sb7,
                    "verify_rendered_publication",
                    return_value={"run_url": "https://example.invalid/run"},
                ),
            ):
                self.assertTrue(cloud_sb7.publish_one(root, "fixture-model"))
            final = cloud_sb7.read_state(root, "fixture-model")
            self.assertEqual(final["status"], "PUBLISHED")
            self.assertTrue(final["publisher_write_adopted"])

    def test_successful_live_process_with_remote_mismatch_is_not_rewritten(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            runs = cloud_sb7.publication_stage(root, "fixture-model")
            cloud_sb7.update_state(
                root,
                "fixture-model",
                publisher_plan=[
                    {
                        "name": "loaded",
                        "caption": "Final render",
                        "source": str(
                            runs / "fixture-model-r0/sb7-shots/100-loaded.png"
                        ),
                        "sha1": "a" * 40,
                        "sha256": "b" * 64,
                    }
                ],
            )
            mismatch = {"matched": False, "reasons": ["remote document differs"]}
            completed = {
                "exit_code": 0,
                "timed_out": False,
                "log": "publisher.log",
                "log_sha256": "log",
                "pid": 1,
            }
            with (
                mock.patch.object(
                    cloud_sb7,
                    "remote_publication_receipt",
                    return_value=mismatch,
                ),
                mock.patch.object(
                    cloud_sb7,
                    "run_publisher",
                    return_value=completed,
                ) as publisher,
            ):
                self.assertFalse(cloud_sb7.publish_one(root, "fixture-model"))
                self.assertFalse(cloud_sb7.publish_one(root, "fixture-model"))
            publisher.assert_called_once()
            final = cloud_sb7.read_state(root, "fixture-model")
            self.assertEqual(final["status"], "PUBLISH_FAILED")
            self.assertIsNotNone(final["publisher_live_succeeded_at"])
            self.assertIn("diverged", final["failure"])

    def test_score_all_publishes_each_entrant_before_scoring_the_next(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            cloud_sb7.atomic_json(
                cloud_sb7.campaign_file(root),
                {"status": "BUILD_COMPLETE", "campaign_id": "fixture"},
            )
            cloud_sb7.atomic_json(root / "manager.json", {"status": "IDLE"})
            calls: list[str] = []

            def score(_root: Path, entrant_id: str) -> bool:
                calls.append(f"score:{entrant_id}")
                return True

            def publish(_root: Path, entrant_id: str) -> bool:
                calls.append(f"publish:{entrant_id}")
                return True

            with (
                mock.patch.object(cloud_sb7, "recover_interrupted_scoring"),
                mock.patch.object(cloud_sb7, "score_one", side_effect=score),
                mock.patch.object(cloud_sb7, "publish_one", side_effect=publish),
            ):
                self.assertTrue(cloud_sb7.score_all(root, ["one", "two"]))
            self.assertEqual(
                calls, ["score:one", "publish:one", "score:two", "publish:two"]
            )
            self.assertEqual(
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                "PUBLISHED",
            )

    def test_manage_publishes_completed_builds_before_reporting_failed_builds(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            manifest = root / "manifest.json"
            manifest.write_text("{}")
            cloud_sb7.atomic_json(
                cloud_sb7.campaign_file(root),
                {
                    "status": "RUNNING",
                    "campaign_id": "fixture",
                    "entrant_manifest": str(manifest),
                },
            )
            cloud_sb7.atomic_json(root / "manager.json", {"status": "RUNNING"})
            for entrant_id, status in (
                ("complete", "BUILD_COMPLETE"),
                ("failed", "INCOMPLETE"),
            ):
                cloud_sb7.atomic_json(
                    cloud_sb7.state_file(root, entrant_id),
                    {"entrant": entrant_id, "status": status},
                )
            with (
                mock.patch.object(
                    cloud_sb7,
                    "entrants",
                    return_value=[{"id": "complete"}, {"id": "failed"}],
                ),
                mock.patch.object(cloud_sb7, "wait_for_builds", return_value=False),
                mock.patch.object(cloud_sb7, "score_all", return_value=True) as score,
            ):
                self.assertEqual(cloud_sb7.manage(root), 1)
            score.assert_called_once_with(
                root, ["complete"], finalize_campaign=False
            )
            self.assertEqual(
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                "ATTENTION",
            )


if __name__ == "__main__":
    unittest.main()
