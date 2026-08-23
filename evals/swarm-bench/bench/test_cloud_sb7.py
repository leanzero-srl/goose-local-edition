from __future__ import annotations

import json
import os
import signal
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
    def install_live_monitor_lease(self, root: Path) -> str:
        campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
        lease_id = "fixture-monitor-lease"
        pid = os.getpid()
        cloud_sb7.monitor_state(
            root,
            status="RUNNING",
            pid=pid,
            pgid=pid,
            identity=cloud_sb7.process_identity(pid),
            parent_pid=1,
            session_id=pid,
            detached_session=True,
            smoke_contract_sha256=campaign.get("smoke_contract_sha256"),
            lease_id=lease_id,
        )
        cloud_sb7.manager_state(
            root,
            status="RUNNING",
            monitor_lease_id=lease_id,
        )
        return lease_id

    def make_smoke_campaign(
        self, root: Path, *, entrant_count: int = 5
    ) -> list[dict[str, object]]:
        source_manifest = cloud_sb7.load_json(cloud_sb7.DEFAULT_ENTRANTS)
        rows = cloud_sb7.entrants(source_manifest)[:entrant_count]
        (root / "instrument/source/fixture").mkdir(parents=True)
        frozen_file = root / "instrument/source/fixture/instrument.txt"
        frozen_file.write_text("frozen instrument\n")
        instrument_hashes = {
            "fixture/instrument.txt": cloud_sb7.sha256_file(frozen_file)
        }
        manifest = root / "instrument/cloud-sb7-entrants.json"
        manifest.write_text(
            json.dumps(
                {
                    "suite": source_manifest["suite"],
                    "smoke_max_turns": cloud_sb7.SMOKE_MAX_TURNS,
                    "spend_policy": source_manifest["spend_policy"],
                    "entrants": rows,
                }
            )
        )
        binary = root / "instrument/goose"
        binary.write_bytes(b"fixture binary")
        binary.chmod(0o700)
        budget_config = root / "instrument/budget-config.json"
        budget_config.write_text('{"schema_version":1}\n')
        ledger = root / "budget-ledger.json"
        cloud_sb7.atomic_json(
            ledger,
            {
                "schema_version": 1,
                "outstanding": {},
                "settled": [],
                "spent_upper_bound": 0,
            },
        )
        secret_file = root / "cloud.env"
        secret_file.write_text(
            "\n".join(
                f"{row['secret_env']}=fixture-secret-{index}"
                for index, row in enumerate(rows)
            )
            + "\n"
        )
        secret_file.chmod(0o600)
        campaign: dict[str, object] = {
            "schema_version": cloud_sb7.CAMPAIGN_SCHEMA,
            "campaign_id": root.name,
            "status": "INITIALIZED",
            "smoke_status": "PLANNED",
            "binary": str(binary),
            "binary_sha256": cloud_sb7.sha256_file(binary),
            "entrant_manifest": str(manifest),
            "entrant_manifest_sha256": cloud_sb7.sha256_file(manifest),
            "budget_config": str(budget_config),
            "budget_config_sha256": cloud_sb7.sha256_file(budget_config),
            "budget_ledger": str(ledger),
            "instrument_root": str(root / "instrument/source"),
            "instrument_hashes": instrument_hashes,
            "instrument_set_sha256": cloud_sb7.sha256_bytes(
                json.dumps(instrument_hashes, sort_keys=True).encode()
            ),
            "coordinator": str(root / "instrument/source/cloud_sb7.py"),
            "secret_file": str(secret_file),
            "smoke_max_turns": cloud_sb7.SMOKE_MAX_TURNS,
            "lineage": {
                "generation": 0,
                "predecessor_campaign_id": None,
                "predecessor_contract_sha256": None,
            },
        }
        campaign = cloud_sb7.bind_smoke_contract(campaign, rows)
        cloud_sb7.atomic_json(cloud_sb7.campaign_file(root), campaign)
        (root / "locks").mkdir()
        (root / "smoke").mkdir()
        (root / "entrants").mkdir()
        cloud_sb7.atomic_json(root / "manager.json", {"status": "IDLE"})
        for row in rows:
            entrant_id = str(row["id"])
            (root / "entrants" / entrant_id / "tree").mkdir(parents=True)
            (root / "smoke" / entrant_id / "attempts").mkdir(parents=True)
            cloud_sb7.atomic_json(
                cloud_sb7.state_file(root, entrant_id),
                {
                    "entrant": entrant_id,
                    "status": "PLANNED",
                    "provider_episode_attempts": 0,
                    "admitted_requests": 0,
                    "tree": str(root / "entrants" / entrant_id / "tree"),
                },
            )
            cloud_sb7.atomic_json(
                cloud_sb7.smoke_state_file(root, entrant_id),
                {
                    "schema_version": cloud_sb7.CAMPAIGN_SCHEMA,
                    "entrant": entrant_id,
                    "provider": row["provider"],
                    "model": row["model"],
                    "provider_lane": row["provider_lane"],
                    "status": "PLANNED",
                    "launch_attempts": 0,
                    "admitted_episodes": 0,
                    "active_attempt": False,
                    "attempt_evidence_sha256": {},
                    "smoke_contract_sha256": campaign["smoke_contract_sha256"],
                    "budget_settled_baseline_request_ids": campaign[
                        "smoke_budget_settled_baselines"
                    ][entrant_id],
                    "budget_outstanding_baseline_request_ids": campaign[
                        "smoke_budget_outstanding_baselines"
                    ][entrant_id],
                    "budget_config_sha256": campaign["budget_config_sha256"],
                    "thinking_effort": row["thinking_effort"],
                    "context_limit": row["context_limit"],
                    "max_output_tokens": row["max_output_tokens"],
                    "endpoint_family": row["endpoint_family"],
                },
            )
        return rows

    def smoke_stream_events(self, state: dict[str, object]) -> list[dict[str, object]]:
        request_id = "tool-request-1"
        return [
            {
                "type": "message",
                "message": {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "toolRequest",
                            "id": request_id,
                            "toolCall": {
                                "status": "success",
                                "value": {
                                    "name": "shell",
                                    "arguments": {"command": state["expected_command"]},
                                },
                            },
                            "_meta": {"goose_extension": "developer"},
                        }
                    ],
                },
            },
            {
                "type": "message",
                "message": {
                    "role": "user",
                    "content": [
                        {
                            "type": "toolResponse",
                            "id": request_id,
                            "toolResult": {
                                "status": "success",
                                "value": {
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": (
                                                "xcodebuild sandbox warning\n"
                                                + str(state["expected_tool_output"])
                                            ),
                                        }
                                    ],
                                    "structuredContent": {
                                        "stdout": state["expected_tool_output"],
                                        "stderr": "xcodebuild sandbox warning",
                                        "exit_code": 0,
                                    },
                                    "isError": False,
                                },
                            },
                        }
                    ],
                },
            },
            {
                "type": "message",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": state["final_marker"]}],
                },
            },
            {"type": "complete", "total_tokens": 10},
        ]

    def progress_observation(
        self,
        text: str,
        *,
        recorded_at: str = "2026-08-23T00:00:00Z",
        build_log_bytes: int | None = None,
        tree_bytes: int = 0,
        telemetry_bytes: int = 0,
        active_provider: bool = True,
        process_generation: str = "a" * 64,
        provider_generation: str = "b" * 64,
    ) -> dict[str, object]:
        repetition = cloud_sb7.monitor_progress_repetition_evidence(text)
        semantic_sha = repetition["semantic_sha256"]
        build_log_evidence = {
            "regular": True,
            "bytes": build_log_bytes if build_log_bytes is not None else len(text),
            "sha256": semantic_sha,
            "stable_read": True,
            "read_error": None,
        }
        return {
            "schema_version": cloud_sb7.MONITOR_PROGRESS_SCHEMA,
            "recorded_at": recorded_at,
            "campaign_id": "fixture-campaign",
            "smoke_contract_sha256": "fixture-contract",
            "entrant": "fixture-entrant",
            "provider": "fixture-provider",
            "model": "fixture-model",
            "provider_lane": "fixture-lane",
            "status": "BUILD_RUNNING",
            "provider_episode_attempts": 1,
            "process_generation": process_generation,
            "provider_generation": provider_generation,
            "processes": {
                "supervisor_pid": 101,
                "supervisor_pgid": 101,
                "supervisor_identity": "supervisor-generation",
                "supervisor_alive": True,
                "goose_pid": 102,
                "goose_identity": "goose-generation",
                "goose_alive": True,
            },
            "evidence": {
                "lifecycle": {
                    "regular": True,
                    "bytes": 2,
                    "sha256": cloud_sb7.sha256_bytes(b"lifecycle"),
                    "stable_read": True,
                    "read_error": None,
                    "events": 2,
                    "admitted": 1,
                    "terminal": 0,
                    "active_provider_request_ids": (
                        ["provider-request"] if active_provider else []
                    ),
                },
                "build_log": build_log_evidence,
                "tree": {
                    "directory": True,
                    "files": 1 if tree_bytes else 0,
                    "bytes": tree_bytes,
                    "sha256": cloud_sb7.sha256_bytes(
                        f"tree:{tree_bytes}".encode()
                    ),
                    "read_error": None,
                },
                "telemetry": {
                    "regular": True,
                    "bytes": telemetry_bytes,
                    "sha256": cloud_sb7.sha256_bytes(
                        f"telemetry:{telemetry_bytes}".encode()
                    ),
                    "stable_read": True,
                    "read_error": None,
                },
                "repetition": repetition,
            },
        }

    def test_smoke_attempt_prepares_after_provider_lane_admission(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            cloud_sb7.update_smoke_state(
                root,
                entrant_id,
                status="WAITING_PROVIDER_LANE",
                supervisor_pid=os.getpid(),
                supervisor_pgid=os.getpgrp(),
            )

            state = cloud_sb7.prepare_smoke_attempt(root, entrant_id, row)

            self.assertEqual(state["status"], "PREPARING")
            self.assertEqual(state["launch_attempts"], 1)
            self.assertEqual(state["admitted_episodes"], 0)
            self.assertTrue(Path(str(state["attempt_root"])).is_dir())

    def test_smoke_supervisor_crosses_provider_lane_before_local_preflight(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            with mock.patch.object(
                cloud_sb7,
                "persist_listener_isolation",
                side_effect=SystemExit("controlled local preflight stop"),
            ):
                self.assertEqual(
                    cloud_sb7.smoke_supervise_claimed(root, entrant_id),
                    2,
                )

            state = cloud_sb7.read_smoke_state(root, entrant_id)
            self.assertEqual(state["launch_attempts"], 1)
            self.assertEqual(state["admitted_episodes"], 0)
            self.assertNotIn("cannot launch from WAITING_PROVIDER_LANE", state["failure"])

    def complete_smoke_attempt(
        self,
        root: Path,
        row: dict[str, object],
        *,
        finalize: bool = True,
    ) -> dict[str, object]:
        entrant_id = str(row["id"])
        state = cloud_sb7.prepare_smoke_attempt(root, entrant_id, row)
        with mock.patch.object(
            cloud_sb7, "snapshot_listening_tcp_ports", return_value=[43210]
        ):
            state = cloud_sb7.persist_listener_isolation(
                root, row, state, smoke=True
            )
        Path(str(state["log"])).write_text(
            "\n".join(map(json.dumps, self.smoke_stream_events(state))) + "\n"
        )
        Path(str(state["nonce_file"])).write_bytes(
            bytes.fromhex(str(state["nonce_hex"]))
        )
        lifecycle_events = []
        request_ids = [f"{entrant_id}-request-1", f"{entrant_id}-request-2"]
        for request_id in request_ids:
            base = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": request_id,
                "provider": row["provider"],
                "model": row["model"],
                "session": f"{entrant_id}-session",
            }
            lifecycle_events.extend(
                [
                    {**base, "state": "queued"},
                    {**base, "state": "admitted"},
                    {**base, "state": "first_item"},
                    {
                        **base,
                        "state": "usage_reported",
                        "usage": {
                            "reported_model": row["model"],
                            "input_tokens": 2,
                            "output_tokens": 3,
                            "total_tokens": 5,
                        },
                    },
                    {
                        **base,
                        "state": "provider_terminal",
                        "usage": {
                            "reported_model": row["model"],
                            "input_tokens": 2,
                            "output_tokens": 3,
                            "total_tokens": 5,
                        },
                    },
                ]
            )
        Path(str(state["provider_lifecycle"])).write_text(
            "\n".join(map(json.dumps, lifecycle_events)) + "\n"
        )
        campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
        ledger_path = Path(str(campaign["budget_ledger"]))
        ledger = cloud_sb7.load_json(ledger_path)
        ledger["settled"].extend(
            {
                "request_id": request_id,
                "provider": row["provider"],
                "model": row["model"],
            }
            for request_id in request_ids
        )
        cloud_sb7.atomic_json(ledger_path, ledger)
        if finalize:
            self.assertTrue(
                cloud_sb7.finalize_smoke_attempt(
                    root, entrant_id, exit_code=0, descendants_clean=True
                )
            )
        return cloud_sb7.read_smoke_state(root, entrant_id)

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
                    "lineage": {
                        "generation": 0,
                        "predecessor_campaign_id": None,
                        "predecessor_contract_sha256": None,
                    },
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

    def public_identity_campaign(
        self, verdict: dict[str, object]
    ) -> dict[str, object]:
        target = json.loads(
            json.dumps(cloud_sb7.QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["target"])
        )
        return {
            "scorer_version": verdict["scorer_version"],
            "publisher": target,
        }

    def make_scored_campaign(self, root: Path) -> tuple[Path, dict[str, object]]:
        entrant_id = "fixture-model"
        row = {
            "id": entrant_id,
            "provider": "fixture",
            "model": entrant_id,
            "accepted_reported_models": [entrant_id],
            "secret_env": "FIXTURE_API_KEY",
            "provider_lane": "fixture",
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
        entrant_manifest = root / "entrant-manifest.json"
        entrant_manifest.write_text(json.dumps({"entrants": [row]}))
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
                "tree": str(root / "entrants" / entrant_id / "tree"),
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
            "expected_checks": 1,
            **json.loads(
                json.dumps(
                    cloud_sb7.QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["target"]
                )
            ),
        }
        campaign = {
            "schema_version": cloud_sb7.CAMPAIGN_SCHEMA,
            "campaign_id": "fixture-campaign",
            "status": "SCORED",
            "lineage": {
                "generation": 0,
                "predecessor_campaign_id": None,
                "predecessor_contract_sha256": None,
            },
            "binary_sha256": "binary",
            "instrument_set_sha256": "instrument",
            "entrant_manifest": str(entrant_manifest),
            "entrant_manifest_sha256": cloud_sb7.sha256_file(entrant_manifest),
            "secret_file": str(secret_file),
            "publisher": publisher,
            "scorer_version": verdict["scorer_version"],
            "calibration": verdict["calibration"],
        }
        cloud_sb7.atomic_json(cloud_sb7.campaign_file(root), campaign)
        cloud_sb7.atomic_json(root / "manager.json", {"status": "IDLE"})
        self.install_live_monitor_lease(root)
        with mock.patch.object(
            cloud_sb7, "snapshot_listening_tcp_ports", return_value=[]
        ):
            cloud_sb7.persist_listener_isolation(
                root,
                row,
                cloud_sb7.read_state(root, entrant_id),
                smoke=False,
            )
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
            ],
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
        subprocess.run(["git", "config", "user.name", "Fixture"], cwd=repo, check=True)
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
                    "smoke_max_turns": cloud_sb7.SMOKE_MAX_TURNS,
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
                "models": {
                    "fixture": [failed_row["model"], carried_row["model"]]
                },
                "roster_evidence": {
                    "fixture": {
                        str(failed_row["model"]): {},
                        str(carried_row["model"]): {},
                    }
                },
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
        for row in (failed_row, carried_row):
            cloud_sb7.update_smoke_state(
                predecessor_root,
                str(row["id"]),
                status="STOPPED",
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

    def make_qualification_fixture(self, root: Path) -> dict[str, object]:
        fixture = self.make_supersession_fixture(root)
        source_root = Path(str(fixture["predecessor"]))
        source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
        source_manifest = cloud_sb7.load_json(Path(str(source["entrant_manifest"])))
        rows = cloud_sb7.entrants(source_manifest)
        for row in rows:
            entrant_id = str(row["id"])
            state = cloud_sb7.read_state(source_root, entrant_id)
            tree = Path(str(state["tree"]))
            for child in tree.iterdir():
                child.unlink()
            Path(str(state["provider_lifecycle"])).write_text("")
            cloud_sb7.update_state(
                source_root,
                entrant_id,
                status="STOPPED",
                provider_episode_attempts=0,
                admitted_requests=0,
                provider_terminal_requests=0,
                score=None,
                verdict=None,
                failure=None,
            )
            cloud_sb7.update_smoke_state(
                source_root,
                entrant_id,
                status="FAILED",
                launch_attempts=0,
                admitted_episodes=0,
                active_attempt=False,
                queued_at="2026-08-23T01:05:35Z",
                failure="instrument qualification failed before admission",
                attempt_evidence_sha256={},
            )
        ledger_path = Path(str(source["budget_ledger"]))
        ledger = cloud_sb7.load_json(ledger_path)
        ledger["spent_upper_bound"] = 0
        ledger["provider_spent_upper_bound"] = {
            provider: 0 for provider in ledger["provider_caps"]
        }
        ledger["outstanding"] = {}
        ledger["settled"] = []
        cloud_sb7.atomic_json(ledger_path, ledger)

        paid_row = rows[0]
        paid_id = str(paid_row["id"])
        cloud_sb7.update_smoke_state(source_root, paid_id, status="PLANNED")
        paid_smoke = self.complete_smoke_attempt(source_root, paid_row)
        cloud_sb7.update_smoke_state(
            source_root,
            paid_id,
            status="FAILED",
            supervisor_pid=None,
            supervisor_pgid=None,
            supervisor_identity=None,
        )
        ledger = cloud_sb7.load_json(ledger_path)
        reserve = cloud_sb7.budget_price(
            cloud_sb7.budget_model_profile(
                cloud_sb7.load_json(Path(str(source["budget_config"]))),
                str(paid_row["provider"]),
                str(paid_row["model"]),
            ),
            int(paid_row["context_limit"]),
            int(paid_row["max_output_tokens"]),
        )
        self.assertIsNotNone(reserve)
        charge = cloud_sb7.budget_price(
            cloud_sb7.budget_model_profile(
                cloud_sb7.load_json(Path(str(source["budget_config"]))),
                str(paid_row["provider"]),
                str(paid_row["model"]),
            ),
            2,
            3,
        )
        self.assertIsNotNone(charge)
        request_ids = [f"{paid_id}-request-1", f"{paid_id}-request-2"]
        ledger["settled"] = [
            {
                "request_id": request_id,
                "provider": paid_row["provider"],
                "model": paid_row["model"],
                "reported_model": paid_row["model"],
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5,
                "charged_upper_bound_usd": charge,
                "reserved_usd": reserve,
                "settled_at_unix_ms": index + 1,
            }
            for index, request_id in enumerate(request_ids)
        ]
        ledger["spent_upper_bound"] = charge * len(request_ids)
        ledger["provider_spent_upper_bound"] = {
            provider: (
                charge * len(request_ids)
                if provider == paid_row["provider"]
                else 0
            )
            for provider in ledger["provider_caps"]
        }
        cloud_sb7.atomic_json(ledger_path, ledger)
        cloud_sb7.manager_state(
            source_root, status="STOPPED", pid=None, pgid=None, identity=None
        )
        cloud_sb7.monitor_state(
            source_root, status="STOPPED", pid=None, pgid=None, identity=None
        )
        cloud_sb7.update_campaign(source_root, status="STOPPED")

        target_manifest = root / "qualification-entrants.json"
        target_value = cloud_sb7.load_json(Path(str(fixture["manifest"])))
        target_manifest.write_text(json.dumps(target_value))
        binary = Path(str(source["binary"]))
        checked_value = dict(fixture["checked"])
        checked_value["binary_sha256"] = cloud_sb7.sha256_file(binary)
        root_cause = root / "qualification-root-cause.txt"
        root_cause.write_text("stream verifier rejected valid structured tool proof\n")
        regression = root / "qualification-regression.txt"
        regression.write_text("structured smoke verifier regression passed\n")
        evidence = root / "qualification-evidence.json"
        cloud_sb7.atomic_json(
            evidence,
            {
                "schema_version": cloud_sb7.QUALIFICATION_RESTART_SCHEMA,
                "classification": "infrastructure_defect",
                "defect_id": "qualification-stream-verifier-001",
                "summary": "The verifier rejected valid provider/tool smoke evidence.",
                "affected_entrants": [row["id"] for row in rows],
                "predecessor_campaign_id": source["campaign_id"],
                "predecessor_binary_sha256": source["binary_sha256"],
                "replacement_binary_sha256": source["binary_sha256"],
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
            "source": source_root,
            "target": root / "qualification-target",
            "binary": binary,
            "manifest": target_manifest,
            "secrets": fixture["secrets"],
            "publisher": fixture["publisher"],
            "evidence": evidence,
            "checked": checked_value,
            "paid_id": paid_id,
            "paid_smoke": paid_smoke,
        }

    def qualification_restart_fixture(
        self, fixture: dict[str, object]
    ) -> dict[str, object]:
        with mock.patch.object(
            cloud_sb7, "preflight", return_value=fixture["checked"]
        ):
            return cloud_sb7.qualification_restart_campaign(
                Path(str(fixture["source"])),
                Path(str(fixture["target"])),
                Path(str(fixture["binary"])),
                Path(str(fixture["manifest"])),
                Path(str(fixture["secrets"])),
                Path(str(fixture["publisher"])),
                Path(str(fixture["evidence"])),
                True,
            )

    def make_orchestrator_recovery_fixture(
        self, root: Path
    ) -> dict[str, object]:
        publisher_repo, _ = self.make_publisher_repo(root)
        manifest_value = cloud_sb7.load_json(cloud_sb7.DEFAULT_ENTRANTS)
        rows = json.loads(json.dumps(cloud_sb7.entrants(manifest_value)))
        for row in rows:
            row["vendor_port"] = self.free_port()
        publisher_manifest = {
            "expectedChecks": 91,
            "entrants": [
                {
                    "key": row["id"],
                    "label": str(row["id"]).replace("-", " ").title(),
                    "model": row["model"],
                    "docId": (
                        "brun-baseline-"
                        + str(row["id"]).replace(".", "-")
                        + "-sb70"
                    ),
                }
                for row in rows
            ],
        }
        (publisher_repo / cloud_sb7.PUBLISHER_MANIFEST).write_text(
            json.dumps(publisher_manifest)
        )
        subprocess.run(["git", "add", "."], cwd=publisher_repo, check=True)
        subprocess.run(
            ["git", "commit", "-qm", "add recovery roster"],
            cwd=publisher_repo,
            check=True,
        )

        manifest_path = root / "recovery-entrants.json"
        manifest_path.write_text(
            json.dumps({**manifest_value, "entrants": rows})
        )
        secret_path = root / "recovery-providers.env"
        secret_path.write_text(
            "\n".join(
                f"{row['secret_env']}=fixture-{index}-secret"
                for index, row in enumerate(rows)
            )
            + "\n"
        )
        secret_path.chmod(0o600)
        binary = root / "goose-recovery-fixture"
        binary.write_text("fixture recovery binary\n")
        binary.chmod(0o700)
        publisher = cloud_sb7.publisher_snapshot(publisher_repo, rows)
        models: dict[str, list[str]] = {}
        roster: dict[str, dict[str, object]] = {}
        for row in rows:
            provider = str(row["provider"])
            model = str(row["model"])
            models.setdefault(provider, []).append(model)
            roster.setdefault(provider, {})[model] = {}
        checked = {
            "checked_at": "now",
            "binary_sha256": cloud_sb7.sha256_file(binary),
            "models": models,
            "roster_evidence": roster,
            "requested_models": [row["model"] for row in rows],
            "ports_free": True,
            "credential_file_mode": "0600",
            "publisher": publisher,
        }

        generation_zero = root / "generation-zero"
        with mock.patch.object(cloud_sb7, "preflight", return_value=checked):
            cloud_sb7.init_campaign(
                generation_zero,
                binary,
                manifest_path,
                secret_path,
                publisher_repo,
                True,
            )
        for row in rows:
            entrant_id = str(row["id"])
            cloud_sb7.update_state(
                generation_zero,
                entrant_id,
                status="STOPPED",
                provider_episode_attempts=0,
                admitted_requests=0,
                provider_terminal_requests=0,
                score=None,
                verdict=None,
                failure=None,
            )
            cloud_sb7.update_smoke_state(
                generation_zero, entrant_id, status="STOPPED"
            )
        affected_id = str(rows[0]["id"])
        cloud_sb7.update_smoke_state(
            generation_zero,
            affected_id,
            status="PRE_ADMISSION_FAILURE",
            launch_attempts=0,
            admitted_episodes=0,
            active_attempt=False,
            queued_at="2026-08-23T03:00:00Z",
            failure="coordinator pre-admission fixture",
        )
        cloud_sb7.manager_state(
            generation_zero,
            status="STOPPED",
            pid=None,
            pgid=None,
            identity=None,
        )
        cloud_sb7.update_campaign(generation_zero, status="STOPPED")

        root_cause = root / "orchestrator-root-cause.txt"
        root_cause.write_text("monitor rejected a valid first full episode\n")
        regression = root / "orchestrator-regression.txt"
        regression.write_text("first-episode lineage and monitor ordering pass\n")
        predecessor = cloud_sb7.load_json(
            cloud_sb7.campaign_file(generation_zero)
        )
        supersession_evidence = root / "supersession-evidence.json"
        cloud_sb7.atomic_json(
            supersession_evidence,
            {
                "schema_version": cloud_sb7.SUPERSESSION_SCHEMA,
                "classification": "infrastructure_defect",
                "defect_id": "fixture-smoke-only-coordinator",
                "summary": "Fixture generation-one coordinator transition.",
                "affected_entrants": [affected_id],
                "predecessor_campaign_id": predecessor["campaign_id"],
                "predecessor_binary_sha256": predecessor["binary_sha256"],
                "replacement_binary_sha256": predecessor["binary_sha256"],
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

        original_freeze = cloud_sb7.freeze_instrument
        coordinator_key = "evals/swarm-bench/bench/cloud_sb7.py"

        def freeze_generation_one(destination: Path) -> dict[str, str]:
            hashes = original_freeze(destination)
            coordinator = destination / coordinator_key
            coordinator.write_text(
                coordinator.read_text() + "\n# generation-one fixture defect\n"
            )
            hashes[coordinator_key] = cloud_sb7.sha256_file(coordinator)
            return hashes

        replacement_hashes = cloud_sb7.instrument_hashes()
        coordinator_suffix = "\n# generation-one fixture defect\n"
        replacement_hashes[coordinator_key] = cloud_sb7.sha256_bytes(
            (
                cloud_sb7.REPO / coordinator_key
            ).read_bytes()
            + coordinator_suffix.encode()
        )
        source_root = root / "generation-one-stopped"
        with (
            mock.patch.object(
                cloud_sb7, "instrument_hashes", return_value=replacement_hashes
            ),
            mock.patch.object(
                cloud_sb7, "freeze_instrument", side_effect=freeze_generation_one
            ),
            mock.patch.object(cloud_sb7, "preflight", return_value=checked),
        ):
            cloud_sb7.supersede_campaign(
                generation_zero,
                source_root,
                binary,
                manifest_path,
                secret_path,
                publisher_repo,
                supersession_evidence,
                True,
            )

        source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
        ledger_path = Path(str(source["budget_ledger"]))
        ledger = cloud_sb7.load_json(ledger_path)
        ledger["outstanding"] = {}
        for index, row in enumerate(rows):
            entrant_id = str(row["id"])
            state = cloud_sb7.read_state(source_root, entrant_id)
            request_id = f"ambiguous-{entrant_id}"
            event = {
                "schema_version": 1,
                "timestamp": f"t-{index}",
                "request_id": request_id,
                "provider": row["provider"],
                "model": row["model"],
                "session": f"session-{index}",
                "state": "queued",
            }
            lifecycle_path = Path(str(state["provider_lifecycle"]))
            lifecycle_path.write_text(json.dumps(event) + "\n")
            (Path(str(state["tree"])) / "partial.txt").write_text(
                f"partial {entrant_id}\n"
            )
            cloud_sb7.update_state(
                source_root,
                entrant_id,
                status="STOPPED",
                provider_episode_attempts=1,
                admitted_requests=0,
                provider_terminal_requests=0,
                score=None,
                verdict=None,
                failure=None,
                exit_code=None,
                finished_at=None,
                raw_tree_sha256=None,
                started_at="2026-08-23T03:06:04Z",
                prompt_sha256=f"{index + 1:064x}",
                command=["goose", "run"],
            )
            profile = cloud_sb7.budget_model_profile(
                cloud_sb7.load_json(Path(str(source["budget_config"]))),
                str(row["provider"]),
                str(row["model"]),
            )
            assert profile is not None
            reserve = cloud_sb7.budget_price(
                profile, int(row["context_limit"]), int(row["max_output_tokens"])
            )
            ledger["outstanding"][request_id] = {
                "request_id": request_id,
                "provider": row["provider"],
                "model": row["model"],
                "reserved_usd": reserve,
                "input_reserve_tokens": row["context_limit"],
                "output_reserve_tokens": row["max_output_tokens"],
                "created_at_unix_ms": index + 1,
            }
        cloud_sb7.atomic_json(ledger_path, ledger)
        cloud_sb7.manager_state(
            source_root,
            status="STOPPED",
            pid=None,
            pgid=None,
            identity=None,
        )
        cloud_sb7.monitor_state(
            source_root,
            status="STOPPED",
            pid=None,
            pgid=None,
            identity=None,
        )
        (source_root / "manager.log").write_bytes(b"")
        (source_root / "monitor.log").write_text(
            cloud_sb7.ORCHESTRATOR_MONITOR_FAILURE
        )
        cloud_sb7.update_campaign(
            source_root, status="STOPPED", failure=None
        )

        target_root = root / "generation-two-recovery"
        stopped_source = cloud_sb7.load_json(
            cloud_sb7.campaign_file(source_root)
        )
        source_evidence = cloud_sb7.orchestrator_source_evidence_snapshot(
            source_root, stopped_source, rows
        )
        incident = cloud_sb7.orchestrator_recovery_incident_identity(
            source_root, stopped_source, source_evidence
        )
        with (
            mock.patch.object(cloud_sb7, "require_smoke_proofs"),
            mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
            mock.patch.object(
                cloud_sb7, "ORCHESTRATOR_RECOVERY_INCIDENT", incident
            ),
        ):
            _, _, source_evidence = cloud_sb7.stopped_orchestrator_recovery_source(
                source_root,
                stopped_source,
                rows,
            )
        recovery_evidence = root / "orchestrator-recovery-evidence.json"
        with (
            mock.patch.object(cloud_sb7, "require_smoke_proofs"),
            mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
            mock.patch.object(
                cloud_sb7, "ORCHESTRATOR_RECOVERY_INCIDENT", incident
            ),
        ):
            evidence = cloud_sb7.orchestrator_recovery_evidence_template(
                source_root,
                target_root,
                root_cause,
                regression,
            )
        self.assertEqual(evidence["entrants"], source_evidence["entrants"])
        cloud_sb7.atomic_json(recovery_evidence, evidence)
        return {
            "source": source_root,
            "target": target_root,
            "evidence": recovery_evidence,
            "rows": rows,
            "incident": incident,
        }

    def orchestrator_recovery_fixture(
        self, fixture: dict[str, object]
    ) -> dict[str, object]:
        with (
            mock.patch.object(cloud_sb7, "require_smoke_proofs"),
            mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
            mock.patch.object(
                cloud_sb7,
                "ORCHESTRATOR_RECOVERY_INCIDENT",
                fixture["incident"],
            ),
        ):
            return cloud_sb7.orchestrator_recovery_campaign(
                Path(str(fixture["source"])),
                Path(str(fixture["target"])),
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
                "fresh strict all-entrant smoke proof",
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

    def test_smoke_stage_supersession_preserves_terminal_reserve_and_full_attempts(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor = Path(str(fixture["predecessor"]))
            failed_id = str(fixture["failed_id"])
            unstarted_id = str(fixture["carried_id"])
            failed_row = cloud_sb7.manifest_row(predecessor, failed_id)

            for entrant_id in (failed_id, unstarted_id):
                state = cloud_sb7.read_state(predecessor, entrant_id)
                Path(str(state["provider_lifecycle"])).unlink(missing_ok=True)
                cloud_sb7.update_state(
                    predecessor,
                    entrant_id,
                    status="STOPPED",
                    provider_episode_attempts=0,
                    admitted_requests=0,
                    provider_terminal_requests=0,
                    score=None,
                    verdict=None,
                    failure=None,
                )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            ledger["settled"] = []
            ledger["outstanding"] = {}
            cloud_sb7.atomic_json(ledger_path, ledger)
            cloud_sb7.update_smoke_state(predecessor, failed_id, status="PLANNED")
            smoke = cloud_sb7.prepare_smoke_attempt(
                predecessor, failed_id, failed_row
            )
            with mock.patch.object(
                cloud_sb7, "snapshot_listening_tcp_ports", return_value=[43210]
            ):
                smoke = cloud_sb7.persist_listener_isolation(
                    predecessor, failed_row, smoke, smoke=True
                )
            request_id = "smoke-terminal-before-settlement"
            usage = {
                "reported_model": failed_row["model"],
                "input_tokens": 10,
                "output_tokens": 10,
                "total_tokens": 20,
            }
            base = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": request_id,
                "provider": failed_row["provider"],
                "model": failed_row["model"],
                "session": "smoke-crash-session",
            }
            events = [
                {**base, "state": "queued"},
                {**base, "state": "admitted"},
                {**base, "state": "first_item"},
                {**base, "state": "usage_reported", "usage": usage},
                {**base, "state": "provider_terminal", "usage": usage},
            ]
            Path(str(smoke["provider_lifecycle"])).write_text(
                "\n".join(map(json.dumps, events)) + "\n"
            )
            cloud_sb7.update_smoke_state(
                predecessor,
                failed_id,
                status="STOPPED",
                supervisor_pid=None,
                supervisor_pgid=None,
                supervisor_identity=None,
            )

            ledger = cloud_sb7.load_json(ledger_path)
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            ledger["settled"] = []
            ledger["outstanding"] = {
                request_id: {
                    "request_id": request_id,
                    "provider": failed_row["provider"],
                    "model": failed_row["model"],
                    "reserved_usd": 0.00012,
                    "input_reserve_tokens": 100,
                    "output_reserve_tokens": 20,
                    "created_at_unix_ms": 2,
                }
            }
            cloud_sb7.atomic_json(ledger_path, ledger)

            successor = self.supersede_fixture(fixture)
            successor_root = Path(str(fixture["successor"]))
            lineage = cloud_sb7.load_json(successor_root / "lineage/lineage.json")
            self.assertEqual(lineage["affected_entrants"], [failed_id])
            self.assertEqual(lineage["unstarted_entrants"], [unstarted_id])
            self.assertEqual(
                lineage["predecessor_terminal_outstanding"][failed_id],
                [request_id],
            )
            self.assertEqual(
                successor["smoke_budget_outstanding_baselines"][failed_id],
                [request_id],
            )
            failed = cloud_sb7.read_state(successor_root, failed_id)
            unstarted = cloud_sb7.read_state(successor_root, unstarted_id)
            self.assertEqual(failed["provider_episode_attempts"], 0)
            self.assertEqual(unstarted["provider_episode_attempts"], 0)
            self.assertEqual(
                unstarted["lineage_role"],
                "unstarted_after_infrastructure_defect",
            )
            for entrant_id in (failed_id, unstarted_id):
                smoke_state = cloud_sb7.read_smoke_state(successor_root, entrant_id)
                self.assertEqual(smoke_state["status"], "PLANNED")
                self.assertEqual(smoke_state["launch_attempts"], 0)
            self.assertIsNone(cloud_sb7.lineage_failure(successor_root))

    def test_supersession_accepts_proven_zero_request_smoke_coordinator_failure(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor = Path(str(fixture["predecessor"]))
            failed_id = str(fixture["failed_id"])
            failed = cloud_sb7.read_state(predecessor, failed_id)
            Path(str(failed["provider_lifecycle"])).unlink(missing_ok=True)
            cloud_sb7.update_state(
                predecessor,
                failed_id,
                status="STOPPED",
                provider_episode_attempts=0,
                admitted_requests=0,
                provider_terminal_requests=0,
                failure="smoke coordinator failed before attempt preparation",
            )
            cloud_sb7.update_smoke_state(
                predecessor,
                failed_id,
                status="FAILED",
                launch_attempts=0,
                admitted_episodes=0,
                active_attempt=False,
                queued_at="2026-08-23T01:05:35Z",
                failure="smoke cannot launch from WAITING_PROVIDER_LANE",
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["settled"] = []
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            cloud_sb7.atomic_json(ledger_path, ledger)

            successor = self.supersede_fixture(fixture)

            successor_root = Path(str(fixture["successor"]))
            lineage = cloud_sb7.load_json(successor_root / "lineage/lineage.json")
            self.assertEqual(lineage["affected_entrants"], [failed_id])
            self.assertEqual(lineage["predecessor_episode_attempts"][failed_id], 0)
            self.assertEqual(
                cloud_sb7.read_state(successor_root, failed_id)[
                    "provider_episode_attempts"
                ],
                0,
            )
            self.assertEqual(successor["smoke_status"], "PLANNED")
            self.assertIsNone(cloud_sb7.lineage_failure(successor_root))

    def test_supersession_rejects_pristine_zero_request_entrant_as_affected(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_supersession_fixture(Path(raw))
            predecessor = Path(str(fixture["predecessor"]))
            failed_id = str(fixture["failed_id"])
            failed = cloud_sb7.read_state(predecessor, failed_id)
            Path(str(failed["provider_lifecycle"])).unlink(missing_ok=True)
            cloud_sb7.update_state(
                predecessor,
                failed_id,
                status="STOPPED",
                provider_episode_attempts=0,
                admitted_requests=0,
                provider_terminal_requests=0,
                failure=None,
            )
            cloud_sb7.update_smoke_state(
                predecessor,
                failed_id,
                status="STOPPED",
                launch_attempts=0,
                admitted_episodes=0,
                active_attempt=False,
                queued_at=None,
                failure=None,
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(predecessor))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["settled"] = []
            ledger["spent_upper_bound"] = 0
            ledger["provider_spent_upper_bound"] = {"fixture": 0}
            cloud_sb7.atomic_json(ledger_path, ledger)

            with self.assertRaisesRegex(SystemExit, "no smoke or full activity"):
                self.supersede_fixture(fixture)

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

    def test_qualification_restart_carries_smoke_spend_and_resets_no_full_outcome(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            source_root = Path(str(fixture["source"]))
            source_campaign_before = cloud_sb7.campaign_file(source_root).read_bytes()
            source_ledger_before = Path(
                str(
                    cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))[
                        "budget_ledger"
                    ]
                )
            ).read_bytes()
            state_before = {
                path: path.read_bytes()
                for path in source_root.glob("entrants/*/state.json")
            }
            smoke_before = {
                path: path.read_bytes()
                for path in source_root.glob("smoke/*/state.json")
            }

            target = self.qualification_restart_fixture(fixture)

            target_root = Path(str(fixture["target"]))
            self.assertEqual(target["lineage"]["generation"], 0)
            self.assertEqual(target["qualification_history"]["restart_count"], 1)
            self.assertEqual(source_campaign_before, cloud_sb7.campaign_file(source_root).read_bytes())
            self.assertEqual(
                source_ledger_before,
                Path(
                    str(
                        cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))[
                            "budget_ledger"
                        ]
                    )
                ).read_bytes(),
            )
            self.assertEqual(
                state_before,
                {path: path.read_bytes() for path in source_root.glob("entrants/*/state.json")},
            )
            self.assertEqual(
                smoke_before,
                {path: path.read_bytes() for path in source_root.glob("smoke/*/state.json")},
            )
            self.assertIsNone(cloud_sb7.lineage_failure(target_root))
            self.assertIn(
                "immutable qualification restart receipt",
                cloud_sb7.lineage_failure(source_root) or "",
            )
            source_ledger = cloud_sb7.load_json(
                Path(
                    str(
                        cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))[
                            "budget_ledger"
                        ]
                    )
                )
            )
            target_ledger = cloud_sb7.load_json(Path(str(target["budget_ledger"])))
            self.assertEqual(target_ledger, source_ledger)
            paid_id = str(fixture["paid_id"])
            self.assertEqual(
                len(target["smoke_budget_settled_baselines"][paid_id]), 2
            )
            qualification_lineage = cloud_sb7.load_json(
                target_root / cloud_sb7.QUALIFICATION_HISTORY_PATH
            )
            for entrant_id in qualification_lineage["entrant_ids"]:
                state = cloud_sb7.read_state(target_root, entrant_id)
                smoke = cloud_sb7.read_smoke_state(target_root, entrant_id)
                self.assertEqual(state["status"], "PLANNED")
                self.assertEqual(state["provider_episode_attempts"], 0)
                self.assertEqual(state["lineage_role"], "qualification_restart")
                self.assertEqual(smoke["status"], "PLANNED")
                self.assertEqual(smoke["launch_attempts"], 0)

    def test_qualification_endpoint_transition_is_exact_and_provider_scoped(
        self,
    ) -> None:
        target = cloud_sb7.load_json(cloud_sb7.DEFAULT_ENTRANTS)
        source = json.loads(json.dumps(target))
        source_zai = next(
            row for row in source["entrants"] if row["provider"] == "zai_api"
        )
        source_zai["endpoint_family"] = "https://api.z.ai/api/paas/v4"
        source_zai.pop("base_url_env", None)
        self.assertIsNone(cloud_sb7.qualification_manifest_failure(source, target))

        for provider, model in (
            ("zai_api", "glm-5.3"),
            ("google", "gemini-3.7-flash"),
            ("custom_deepseek", "deepseek-v4-pro"),
        ):
            with self.subTest(provider=provider):
                candidate = json.loads(json.dumps(target))
                row = next(
                    item
                    for item in candidate["entrants"]
                    if item["provider"] == provider and item["model"] == model
                )
                row["endpoint_family"] = "https://credential-sink.invalid/v1"
                self.assertIn(
                    "unapproved endpoint transition",
                    cloud_sb7.qualification_manifest_failure(source, candidate) or "",
                )

    def test_qualification_publisher_transition_is_one_exact_stable_board_fix(
        self,
    ) -> None:
        stable = {
            "repo": "/Users/mihaiperdum/Projects/LeanZero-website",
            "branch": "master",
            "script": "scripts/seed-baseline-sb7.mjs",
            "manifest": "scripts/data/sb7-cloud-entrants.json",
            "runtime_hashes": {
                "node_modules/@sanity/client": "runtime-client",
                "node_modules/dotenv": "runtime-dotenv",
            },
            "node": {
                "path": "/opt/homebrew/bin/node",
                "sha256": "node-sha256",
                "version": "v26.5.0",
            },
            "env_file": "/Users/mihaiperdum/Projects/LeanZero-website/.env.local",
            "env_file_mode": "0600",
            "env_file_sha256": "env-sha256",
            "sanity_target": {"project_id": "3oa2omis", "dataset": "production"},
            "required_env_present": [
                "SANITY_WRITE_TOKEN",
                "NEXT_PUBLIC_SANITY_PROJECT_ID",
            ],
            "expected_checks": 91,
            "entries": {"glm-5.3": {"doc_id": "brun-baseline-glm-5-3-sb70"}},
            "mode": "live",
            "website_base_url": "https://leanzero.net",
            "revalidate_endpoint": (
                "https://leanzero.net/api/revalidate-benchmarks"
            ),
            "verify_timeout_seconds": 900.0,
            "verify_interval_seconds": 15.0,
            "process_timeout_seconds": 900.0,
        }
        source = {
            **stable,
            **json.loads(
                json.dumps(
                    cloud_sb7.QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["source"]
                )
            ),
            "frozen": {"root": "/source/instrument/publisher"},
        }
        target = {
            **stable,
            **json.loads(
                json.dumps(
                    cloud_sb7.QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["target"]
                )
            ),
            "frozen": {"root": "/target/instrument/publisher"},
        }
        self.assertIsNone(cloud_sb7.qualification_publisher_failure(source, target))

        release_tampers = (
            ("commit", "f" * 40),
            ("instrument_set_sha256", "e" * 64),
        )
        for field, value in release_tampers:
            with self.subTest(field=field):
                candidate = json.loads(json.dumps(target))
                candidate[field] = value
                self.assertIn(
                    "unapproved publisher transition",
                    cloud_sb7.qualification_publisher_failure(source, candidate)
                    or "",
                )
        for tracked_file in (
            "scripts/seed-baseline-sb7.mjs",
            "scripts/lib/sb7-cloud-publisher.mjs",
            "scripts/data/sb7-cloud-entrants.json",
            "package.json",
            "package-lock.json",
        ):
            with self.subTest(tracked_file=tracked_file):
                candidate = json.loads(json.dumps(target))
                candidate["tracked_hashes"][tracked_file] = "d" * 64
                self.assertIn(
                    "unapproved publisher transition",
                    cloud_sb7.qualification_publisher_failure(source, candidate)
                    or "",
                )
        for field, value in (
            ("runtime_hashes", {"node_modules/dotenv": "changed"}),
            ("env_file_sha256", "changed"),
            ("sanity_target", {"project_id": "other", "dataset": "production"}),
            ("expected_checks", 90),
            ("entries", {}),
            ("manifest", "scripts/data/other.json"),
            ("website_base_url", "https://other.invalid"),
            ("process_timeout_seconds", 1.0),
        ):
            with self.subTest(stable_field=field):
                candidate = json.loads(json.dumps(target))
                candidate[field] = value
                self.assertEqual(
                    cloud_sb7.qualification_publisher_failure(source, candidate),
                    f"qualification restart changed publisher field {field}",
                )

    def test_qualification_rejects_local_publisher_tamper_before_preflight(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            source_root = Path(str(fixture["source"]))
            source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
            candidate = json.loads(json.dumps(source["publisher"]))
            candidate.pop("frozen", None)
            candidate["commit"] = "f" * 40
            candidate["instrument_set_sha256"] = "e" * 64
            candidate["tracked_hashes"]["scripts/seed-baseline-sb7.mjs"] = "d" * 64
            with (
                mock.patch.object(
                    cloud_sb7, "publisher_snapshot", return_value=candidate
                ),
                mock.patch.object(cloud_sb7, "preflight") as preflight,
                self.assertRaisesRegex(
                    SystemExit, "unapproved publisher transition"
                ),
            ):
                cloud_sb7.qualification_restart_campaign(
                    source_root,
                    Path(str(fixture["target"])),
                    Path(str(fixture["binary"])),
                    Path(str(fixture["manifest"])),
                    Path(str(fixture["secrets"])),
                    Path(str(fixture["publisher"])),
                    Path(str(fixture["evidence"])),
                    True,
                )
            preflight.assert_not_called()
            self.assertFalse(
                (source_root / cloud_sb7.QUALIFICATION_RESTART_RECEIPT).exists()
            )

    def test_qualification_restart_reuses_one_authenticated_preflight(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            with mock.patch.object(
                cloud_sb7, "preflight", return_value=fixture["checked"]
            ) as checked:
                target = cloud_sb7.qualification_restart_campaign(
                    Path(str(fixture["source"])),
                    Path(str(fixture["target"])),
                    Path(str(fixture["binary"])),
                    Path(str(fixture["manifest"])),
                    Path(str(fixture["secrets"])),
                    Path(str(fixture["publisher"])),
                    Path(str(fixture["evidence"])),
                    True,
                )
            self.assertEqual(checked.call_count, 1)
            self.assertIsNone(
                cloud_sb7.lineage_failure(Path(str(fixture["target"])))
            )
            self.assertEqual(target["qualification_history"]["restart_count"], 1)

    def test_qualification_restart_freezes_publisher_runtime(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            source = cloud_sb7.load_json(
                cloud_sb7.campaign_file(Path(str(fixture["source"])))
            )
            for field, value in (
                ("mode", "dry-run"),
                ("website_base_url", "https://example.invalid"),
                (
                    "revalidate_endpoint",
                    "https://example.invalid/api/revalidate-benchmarks",
                ),
                ("verify_timeout_seconds", 1),
                ("verify_interval_seconds", 1),
                ("process_timeout_seconds", 1),
            ):
                with self.subTest(field=field):
                    target = json.loads(json.dumps(source))
                    target["publisher"][field] = value
                    self.assertEqual(
                        cloud_sb7.qualification_instrument_failure(source, target),
                        f"qualification restart changed publisher field {field}",
                    )

            with mock.patch.object(cloud_sb7, "preflight") as checked:
                with self.assertRaisesRegex(
                    SystemExit, "changed publisher field website_base_url"
                ):
                    cloud_sb7.qualification_restart_campaign(
                        Path(str(fixture["source"])),
                        Path(str(fixture["target"])),
                        Path(str(fixture["binary"])),
                        Path(str(fixture["manifest"])),
                        Path(str(fixture["secrets"])),
                        Path(str(fixture["publisher"])),
                        Path(str(fixture["evidence"])),
                        True,
                        "https://example.invalid",
                    )
                checked.assert_not_called()

    def test_qualification_restart_refuses_any_full_activity_or_hidden_output(self) -> None:
        for defect in ("attempt", "lifecycle", "tree", "score", "outstanding"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_qualification_fixture(Path(raw))
                source_root = Path(str(fixture["source"]))
                source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
                entrant_id = cloud_sb7.load_json(
                    Path(str(source["entrant_manifest"]))
                )["entrants"][0]["id"]
                state = cloud_sb7.read_state(source_root, entrant_id)
                if defect == "attempt":
                    cloud_sb7.update_state(
                        source_root, entrant_id, provider_episode_attempts=1
                    )
                    expected = "after any full benchmark activity"
                elif defect == "lifecycle":
                    Path(str(state["provider_lifecycle"])).write_text("{}\n")
                    expected = "after any full benchmark activity"
                elif defect == "tree":
                    (Path(str(state["tree"])) / "hidden.txt").write_text("outcome\n")
                    expected = "tree is not empty"
                elif defect == "score":
                    (source_root / "scores" / entrant_id).mkdir(parents=True)
                    expected = "score or publication artifacts"
                else:
                    ledger_path = Path(str(source["budget_ledger"]))
                    ledger = cloud_sb7.load_json(ledger_path)
                    ledger["outstanding"]["hidden"] = {
                        "request_id": "hidden",
                        "provider": "fixture",
                        "model": entrant_id,
                        "reserved_usd": 0.00012,
                        "input_reserve_tokens": 100,
                        "output_reserve_tokens": 20,
                        "created_at_unix_ms": 3,
                    }
                    cloud_sb7.atomic_json(ledger_path, ledger)
                    expected = "outstanding budget reservations"
                with self.assertRaisesRegex(SystemExit, expected):
                    self.qualification_restart_fixture(fixture)

    def test_qualification_restart_is_idempotent_and_rejects_forks(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            first = self.qualification_restart_fixture(fixture)
            second = self.qualification_restart_fixture(fixture)
            self.assertEqual(
                first["qualification_history"], second["qualification_history"]
            )
            fork = dict(fixture)
            fork["target"] = Path(raw) / "qualification-fork"
            with self.assertRaisesRegex(SystemExit, "another target"):
                self.qualification_restart_fixture(fork)

    def test_qualification_receipt_blocks_all_internal_source_mutators(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            self.qualification_restart_fixture(fixture)
            source_root = Path(str(fixture["source"]))
            target_root = Path(str(fixture["target"]))
            source_files = [
                cloud_sb7.campaign_file(source_root),
                source_root / "manager.json",
                source_root / "monitor.json",
                *source_root.glob("smoke/*/state.json"),
            ]
            before = {path: path.read_bytes() for path in source_files}
            entrant_ids = [
                state["entrant"] for state in cloud_sb7.status_rows(source_root)
            ]
            for operation in (
                lambda: cloud_sb7.score_all(source_root, entrant_ids),
                lambda: cloud_sb7.monitor_campaign(source_root, poll_seconds=0.001),
                lambda: cloud_sb7.smoke_supervise_claimed(
                    source_root, entrant_ids[0]
                ),
            ):
                with self.assertRaisesRegex(
                    SystemExit, "immutable qualification restart receipt"
                ):
                    operation()
            self.assertEqual(before, {path: path.read_bytes() for path in source_files})
            self.assertIsNone(cloud_sb7.lineage_failure(target_root))

    def test_qualified_campaign_preserves_history_across_one_full_supersession(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            target = self.qualification_restart_fixture(fixture)
            target_root = Path(str(fixture["target"]))
            self.assertEqual(cloud_sb7.stop(target_root), 0)
            manifest_path = Path(str(target["entrant_manifest"]))
            rows = cloud_sb7.entrants(cloud_sb7.load_json(manifest_path))
            carried_row, failed_row = rows
            carried_id = str(carried_row["id"])
            failed_id = str(failed_row["id"])

            carried = cloud_sb7.read_state(target_root, carried_id)
            carried_tree = Path(str(carried["tree"]))
            (carried_tree / "carried.txt").write_text("preserve qualified build\n")
            cloud_sb7.update_state(
                target_root,
                carried_id,
                status="BUILD_COMPLETE",
                provider_episode_attempts=1,
                admitted_requests=1,
                provider_terminal_requests=1,
                raw_tree_sha256=cloud_sb7.hash_tree(carried_tree),
            )
            failed = cloud_sb7.read_state(target_root, failed_id)
            request_id = "qualified-full-failed-request"
            usage = {
                "reported_model": failed_row["model"],
                "input_tokens": 10,
                "output_tokens": 10,
                "total_tokens": 20,
            }
            base = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": request_id,
                "provider": failed_row["provider"],
                "model": failed_row["model"],
                "session": "qualified-full-session",
            }
            events = [
                {**base, "state": "queued"},
                {**base, "state": "admitted"},
                {**base, "state": "first_item"},
                {**base, "state": "usage_reported", "usage": usage},
                {**base, "state": "provider_terminal", "usage": usage},
            ]
            Path(str(failed["provider_lifecycle"])).write_text(
                "\n".join(map(json.dumps, events)) + "\n"
            )
            cloud_sb7.update_state(
                target_root,
                failed_id,
                status="STOPPED",
                provider_episode_attempts=1,
                admitted_requests=1,
                provider_terminal_requests=1,
                failure="audited infrastructure defect",
            )
            ledger_path = Path(str(target["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            config = cloud_sb7.load_json(Path(str(target["budget_config"])))
            profile = cloud_sb7.budget_model_profile(
                config, str(failed_row["provider"]), str(failed_row["model"])
            )
            reserve = cloud_sb7.budget_price(
                profile,
                int(failed_row["context_limit"]),
                int(failed_row["max_output_tokens"]),
            )
            charge = cloud_sb7.budget_price(profile, 10, 10)
            self.assertIsNotNone(reserve)
            self.assertIsNotNone(charge)
            ledger["settled"].append(
                {
                    "request_id": request_id,
                    "provider": failed_row["provider"],
                    "model": failed_row["model"],
                    **usage,
                    "charged_upper_bound_usd": charge,
                    "reserved_usd": reserve,
                    "settled_at_unix_ms": 10,
                }
            )
            ledger["spent_upper_bound"] += charge
            ledger["provider_spent_upper_bound"][failed_row["provider"]] += charge
            cloud_sb7.atomic_json(ledger_path, ledger)

            replacement = Path(raw) / "goose-qualified-replacement"
            replacement.write_text("qualified replacement binary\n")
            replacement.chmod(0o700)
            root_cause = Path(raw) / "qualified-full-root-cause.txt"
            root_cause.write_text("full provider terminal parser defect\n")
            regression = Path(raw) / "qualified-full-regression.txt"
            regression.write_text("full provider terminal regression passed\n")
            evidence = Path(raw) / "qualified-full-evidence.json"
            cloud_sb7.atomic_json(
                evidence,
                {
                    "schema_version": cloud_sb7.SUPERSESSION_SCHEMA,
                    "classification": "infrastructure_defect",
                    "defect_id": "qualified-full-infrastructure-001",
                    "summary": "A full provider infrastructure defect was proven.",
                    "affected_entrants": [failed_id],
                    "predecessor_campaign_id": target["campaign_id"],
                    "predecessor_binary_sha256": target["binary_sha256"],
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
            checked_value = dict(fixture["checked"])
            checked_value["binary_sha256"] = cloud_sb7.sha256_file(replacement)
            successor_root = Path(raw) / "qualified-full-successor"
            with mock.patch.object(
                cloud_sb7, "preflight", return_value=checked_value
            ):
                successor = cloud_sb7.supersede_campaign(
                    target_root,
                    successor_root,
                    replacement,
                    manifest_path,
                    Path(str(fixture["secrets"])),
                    Path(str(fixture["publisher"])),
                    evidence,
                    True,
                )
            self.assertEqual(successor["lineage"]["generation"], 1)
            self.assertEqual(successor["qualification_history"]["restart_count"], 1)
            self.assertIsNone(cloud_sb7.lineage_failure(successor_root))

    def test_same_binary_supersession_is_smoke_only_and_instrument_exact(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rows = [
                {
                    "id": "model",
                    "provider": "google",
                    "model": "gemini-fixture",
                }
            ]
            tree = root / "tree"
            tree.mkdir()
            lifecycle = root / "provider-lifecycle.jsonl"
            lifecycle.write_text("")
            state = {
                "status": "STOPPED",
                "provider_episode_attempts": 0,
                "admitted_requests": 0,
                "provider_terminal_requests": 0,
                "score": None,
                "verdict": None,
                "provider_lifecycle": str(lifecycle),
                "tree": str(tree),
            }
            coordinator = "evals/swarm-bench/bench/cloud_sb7.py"
            stable = "evals/swarm-bench/spec-build-sb7.md"
            predecessor = {
                "instrument_hashes": {coordinator: "old", stable: "same"}
            }
            replacement = {coordinator: "new", stable: "same"}
            self.assertIsNone(
                cloud_sb7.same_binary_supersession_failure(
                    predecessor, rows, {"model": state}, replacement
                )
            )

            no_change = cloud_sb7.same_binary_supersession_failure(
                predecessor,
                rows,
                {"model": state},
                dict(predecessor["instrument_hashes"]),
            )
            self.assertIn("exactly one coordinator", no_change or "")

            extra_change = cloud_sb7.same_binary_supersession_failure(
                predecessor,
                rows,
                {"model": state},
                {coordinator: "new", stable: "changed"},
            )
            self.assertIn("exactly one coordinator", extra_change or "")

            started = {**state, "provider_episode_attempts": 1}
            full_episode = cloud_sb7.same_binary_supersession_failure(
                predecessor, rows, {"model": started}, replacement
            )
            self.assertIn("smoke-only defects", full_episode or "")

            (tree / "artifact.txt").write_text("not pristine\n")
            nonempty = cloud_sb7.same_binary_supersession_failure(
                predecessor, rows, {"model": state}, replacement
            )
            self.assertIn("raw benchmark tree", nonempty or "")

    def test_qualification_restart_crash_boundaries_recover_without_new_spend(self) -> None:
        for boundary in (
            "evidence_bundle_staged",
            "evidence_bundle_committed",
            "source_receipt_committed",
            "staged_initialized",
            "lineage_staged",
            "root_committed",
        ):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_qualification_fixture(Path(raw))

                def crash(stage: str) -> None:
                    if stage == boundary:
                        raise RuntimeError(f"crash at {stage}")

                with mock.patch.object(
                    cloud_sb7, "qualification_fault", side_effect=crash
                ), self.assertRaisesRegex(RuntimeError, boundary):
                    self.qualification_restart_fixture(fixture)
                target = self.qualification_restart_fixture(fixture)
                target_root = Path(str(fixture["target"]))
                self.assertEqual(target["qualification_history"]["restart_count"], 1)
                self.assertIsNone(cloud_sb7.lineage_failure(target_root))

    def test_qualification_restart_recovers_from_mid_evidence_copy_failure(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            original_copy = cloud_sb7.atomic_copy
            failed = False

            def fail_after_first_artifact(
                source: Path, destination: Path, mode: int | None = None
            ) -> None:
                nonlocal failed
                original_copy(source, destination, mode)
                if (
                    not failed
                    and destination.parent.name
                    == cloud_sb7.QUALIFICATION_RESTART_EVIDENCE
                    and destination.name.startswith("artifact-00-")
                ):
                    failed = True
                    raise RuntimeError("simulated mid-copy process loss")

            with mock.patch.object(
                cloud_sb7, "atomic_copy", side_effect=fail_after_first_artifact
            ), self.assertRaisesRegex(RuntimeError, "mid-copy"):
                self.qualification_restart_fixture(fixture)
            source_root = Path(str(fixture["source"]))
            self.assertTrue(
                (source_root / cloud_sb7.QUALIFICATION_RESTART_SEAL).is_file()
            )
            self.assertFalse(
                (source_root / cloud_sb7.QUALIFICATION_RESTART_RECEIPT).exists()
            )
            self.assertFalse(Path(str(fixture["target"])).exists())

            target = self.qualification_restart_fixture(fixture)
            target_root = Path(str(fixture["target"]))
            self.assertEqual(target["qualification_history"]["restart_count"], 1)
            self.assertIsNone(cloud_sb7.lineage_failure(target_root))

    def test_qualification_restart_detects_semantic_and_sealed_artifact_tampering(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_qualification_fixture(Path(raw))
            manifest = cloud_sb7.load_json(Path(str(fixture["manifest"])))
            manifest["entrants"][0]["thinking_effort"] = "high"
            Path(str(fixture["manifest"])).write_text(json.dumps(manifest))
            with self.assertRaisesRegex(SystemExit, "manifest semantics"):
                self.qualification_restart_fixture(fixture)

        for tamper in ("ledger", "source-seal", "target-artifact"):
            with self.subTest(tamper=tamper), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_qualification_fixture(Path(raw))
                target = self.qualification_restart_fixture(fixture)
                target_root = Path(str(fixture["target"]))
                if tamper == "ledger":
                    ledger_path = Path(str(target["budget_ledger"]))
                    ledger = cloud_sb7.load_json(ledger_path)
                    ledger["settled"] = []
                    ledger["spent_upper_bound"] = 0
                    ledger["provider_spent_upper_bound"] = {
                        provider: 0 for provider in ledger["provider_caps"]
                    }
                    cloud_sb7.atomic_json(ledger_path, ledger)
                    expected = "spend decreased"
                elif tamper == "source-seal":
                    seal = Path(str(fixture["source"])) / cloud_sb7.QUALIFICATION_RESTART_SEAL
                    seal.write_text("{}\n")
                    expected = "source seal"
                else:
                    artifact = next(
                        (target_root / "qualification/evidence").glob("artifact-*")
                    )
                    artifact.write_text("changed\n")
                    expected = "artifact changed"
                self.assertIn(expected, cloud_sb7.lineage_failure(target_root) or "")

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
        self.assertEqual(
            rows[0]["endpoint_family"], "https://api.z.ai/api/coding/paas/v4"
        )
        self.assertEqual(rows[0]["base_url_env"], "ZAI_API_BASE_URL")
        policy = cloud_sb7.spend_policy(manifest, rows)
        self.assertEqual(policy["total_cap"], 400.0)
        self.assertEqual(policy["provider_caps"]["google"], 250.0)
        self.assertIs(policy["launch_all_entrants_concurrently"], True)
        self.assertEqual(cloud_sb7.smoke_max_turns(manifest), cloud_sb7.SMOKE_MAX_TURNS)

    def test_provider_endpoint_rejects_credential_exfiltration_shapes(self) -> None:
        self.assertEqual(
            cloud_sb7.normalized_provider_endpoint("https://api.example.test/v1/"),
            "https://api.example.test/v1",
        )
        for malformed in (
            "http://api.example.test/v1",
            "https://user:pass@api.example.test/v1",
            "https://api.example.test/v1?redirect=evil",
            "https://api.example.test/v1#fragment",
        ):
            with self.subTest(malformed=malformed), self.assertRaises(SystemExit):
                cloud_sb7.normalized_provider_endpoint(malformed)

    def test_only_smoke_command_has_safety_turn_limit(self) -> None:
        row = cloud_sb7.entrants(cloud_sb7.load_json(cloud_sb7.DEFAULT_ENTRANTS))[0]
        binary = Path("/frozen/goose")
        build = cloud_sb7.build_goose_command(binary, row, "build prompt")
        smoke = cloud_sb7.smoke_goose_command(
            binary, row, "smoke prompt", cloud_sb7.SMOKE_MAX_TURNS
        )
        self.assertNotIn("--max-turns", build)
        self.assertIn("--quiet", build)
        self.assertIn("--quiet", smoke)
        self.assertEqual(
            smoke[smoke.index("--max-turns") + 1], str(cloud_sb7.SMOKE_MAX_TURNS)
        )
        self.assertEqual(smoke[smoke.index("--provider") + 1], row["provider"])
        self.assertEqual(smoke[smoke.index("--model") + 1], row["model"])

    def test_smoke_environment_uses_shared_budget_and_isolated_paths(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            with mock.patch.object(
                cloud_sb7, "snapshot_listening_tcp_ports", return_value=[43210]
            ):
                state = cloud_sb7.persist_listener_isolation(
                    root, row, state, smoke=True
                )
            with mock.patch.dict(os.environ, {"PATH": "/bin"}, clear=True):
                env = cloud_sb7.child_env(row, state, "active-secret")
            self.assertEqual(env["GOOSE_BENCH_CAMPAIGN"], str(root))
            self.assertEqual(
                env["ZAI_API_BASE_URL"], "https://api.z.ai/api/coding/paas/v4"
            )
            self.assertEqual(
                env["GOOSE_BENCH_BUDGET_LEDGER"], str(root / "budget-ledger.json")
            )
            self.assertEqual(env["GOOSE_TOOL_SANDBOX_ROOT"], state["tree"])

            hostile = dict(row)
            hostile["base_url_env"] = "HOME"
            with self.assertRaisesRegex(SystemExit, "protected env"):
                cloud_sb7.child_env(hostile, state, "active-secret")
            self.assertEqual(env["GOOSE_PATH_ROOT"], state["profile"])
            self.assertEqual(env["GOOSE_BENCH_TOOL_ALLOWLIST"], "developer")
            self.assertEqual(env["GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES"], "true")
            self.assertEqual(
                env["GOOSE_TOOL_SANDBOX_DENY_LOCAL_PORTS"],
                ",".join(map(str, state["sandbox_denied_local_ports"])),
            )

    def test_listener_snapshot_denies_preexisting_and_peer_ports_not_own(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rows = self.make_smoke_campaign(root, entrant_count=2)
            row = rows[0]
            peer_port = int(rows[1]["vendor_port"])
            own_port = int(row["vendor_port"])
            state = cloud_sb7.read_state(root, str(row["id"]))
            with mock.patch.object(
                cloud_sb7,
                "snapshot_listening_tcp_ports",
                return_value=[1234, own_port, 54321],
            ):
                state = cloud_sb7.persist_listener_isolation(
                    root, row, state, smoke=False
                )
            self.assertEqual(
                state["sandbox_denied_local_ports"],
                sorted({1234, 54321, peer_port}),
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            self.assertIsNone(
                cloud_sb7.listener_isolation_failure(
                    campaign, row, state, smoke=False
                )
            )

            snapshot = Path(str(state["sandbox_listener_snapshot"]))
            snapshot.write_text("{}\n")
            self.assertIn(
                "hash changed",
                cloud_sb7.listener_isolation_failure(
                    campaign, row, state, smoke=False
                )
                or "",
            )

    def test_smoke_listener_snapshot_denies_every_manifest_vendor_port(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rows = self.make_smoke_campaign(root, entrant_count=2)
            row = rows[0]
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            with mock.patch.object(
                cloud_sb7, "snapshot_listening_tcp_ports", return_value=[1234]
            ):
                state = cloud_sb7.persist_listener_isolation(
                    root, row, state, smoke=True
                )
            self.assertEqual(
                state["sandbox_denied_local_ports"],
                sorted({1234, *(int(value["vendor_port"]) for value in rows)}),
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            self.assertIsNone(
                cloud_sb7.listener_isolation_failure(
                    campaign, row, state, smoke=True
                )
            )

    def test_lsof_listener_parser_rejects_ambiguous_endpoints(self) -> None:
        self.assertEqual(
            cloud_sb7.parse_lsof_listener_ports(
                "p1\nf2\nn*:1234\nn127.0.0.1:1234\nn[::1]:43210\n"
            ),
            [1234, 43210],
        )
        for malformed in ("nlocalhost:http\n", "nmissing-port\n", "n*:0\n"):
            with self.subTest(malformed=malformed), self.assertRaises(SystemExit):
                cloud_sb7.parse_lsof_listener_ports(malformed)

    def test_smoke_stream_requires_exact_structural_contract(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            log = Path(str(state["log"]))

            def parse(events: list[dict[str, object]]) -> dict[str, object]:
                log.write_text("\n".join(map(json.dumps, events)) + "\n")
                return cloud_sb7.parse_smoke_stream(
                    log,
                    expected_command=str(state["expected_command"]),
                    expected_marker=str(state["final_marker"]),
                    expected_tool_output=str(state["expected_tool_output"]),
                )

            valid = self.smoke_stream_events(state)
            self.assertIs(parse(valid)["valid"], True)

            marker = str(state["final_marker"])
            first = len(marker) // 3
            second = 2 * len(marker) // 3
            thinking_and_chunked_final = [
                {
                    "type": "message",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "thinking",
                                "thinking": (
                                    "I must return " + marker + " after the tool succeeds"
                                ),
                                "signature": "",
                            }
                        ],
                    },
                },
                *json.loads(json.dumps(valid[:2])),
                *[
                    {
                        "type": "message",
                        "message": {
                            "role": "assistant",
                            "content": [{"type": "text", "text": fragment}],
                        },
                    }
                    for fragment in (
                        marker[:first],
                        marker[first:second],
                        marker[second:],
                    )
                ],
                json.loads(json.dumps(valid[-1])),
            ]
            chunked = parse(thinking_and_chunked_final)
            self.assertIs(chunked["valid"], True)
            self.assertIs(chunked["final_text_exact"], True)

            thinking_without_final = json.loads(
                json.dumps(thinking_and_chunked_final)
            )
            del thinking_without_final[3:6]
            reasoning_only = parse(thinking_without_final)
            self.assertIs(reasoning_only["valid"], False)
            self.assertIn(
                "final assistant text after the tool response was not exact",
                reasoning_only["errors"],
            )

            cases: dict[str, list[dict[str, object]]] = {}
            mismatched = json.loads(json.dumps(valid))
            mismatched[1]["message"]["content"][0]["id"] = "wrong-id"
            cases["matching tool IDs"] = mismatched
            failed = json.loads(json.dumps(valid))
            failed[1]["message"]["content"][0]["toolResult"]["status"] = "error"
            cases["successful tool response"] = failed
            unowned_shell = json.loads(json.dumps(valid))
            del unowned_shell[0]["message"]["content"][0]["_meta"]
            cases["extension-qualified shell identity"] = unowned_shell
            false_text_fallback = json.loads(json.dumps(valid))
            false_text_fallback[1]["message"]["content"][0]["toolResult"]["value"][
                "structuredContent"
            ]["stdout"] = "wrong stdout"
            cases["structured stdout authority"] = false_text_fallback
            nonzero_exit = json.loads(json.dumps(valid))
            nonzero_exit[1]["message"]["content"][0]["toolResult"]["value"][
                "structuredContent"
            ]["exit_code"] = 1
            cases["structured zero exit"] = nonzero_exit
            missing_complete = json.loads(json.dumps(valid[:-1]))
            cases["complete event"] = missing_complete
            truncated = json.loads(json.dumps(valid))
            truncated[-1]["outputTokenLimitReached"] = True
            cases["output truncation"] = truncated
            prompt_echo = json.loads(json.dumps(valid))
            prompt_echo.insert(
                0,
                {
                    "type": "message",
                    "message": {
                        "role": "user",
                        "content": [{"type": "text", "text": state["final_marker"]}],
                    },
                },
            )
            cases["prompt echo"] = prompt_echo
            assistant_preface = json.loads(json.dumps(valid))
            assistant_preface[0]["message"]["content"].insert(
                0, {"type": "text", "text": "I will do that"}
            )
            cases["assistant preface"] = assistant_preface
            for label, events in cases.items():
                with self.subTest(label=label):
                    result = parse(events)
                    self.assertIs(result["valid"], False)
                    self.assertTrue(result["errors"])

    def test_process_group_inspector_runs_outside_the_group_it_measures(self) -> None:
        completed = subprocess.CompletedProcess([], 0, stdout="", stderr="")
        with mock.patch.object(
            cloud_sb7.subprocess, "run", return_value=completed
        ) as run:
            self.assertEqual(cloud_sb7.process_group_members(12345), [])
        self.assertIs(run.call_args.kwargs["start_new_session"], True)

    def test_smoke_proof_seals_tool_nonce_lifecycle_and_budget(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = self.complete_smoke_attempt(root, row)
            self.assertEqual(state["status"], "PASS")
            self.assertEqual(state["admitted_episodes"], 1)
            self.assertIsNone(cloud_sb7.smoke_proof_mismatch(root, str(row["id"]), row))
            proof = cloud_sb7.load_json(Path(str(state["proof"])))
            self.assertEqual(
                proof["smoke_contract_sha256"],
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))[
                    "smoke_contract_sha256"
                ],
            )
            self.assertEqual(proof["admission_history"]["episodes_admitted"], 1)
            self.assertEqual(len(proof["evidence"]["terminal_request_ids"]), 2)

    def test_smoke_nonce_wrong_bytes_and_symlink_fail_closed(self) -> None:
        for mode in ("wrong-bytes", "symlink"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                row = self.make_smoke_campaign(root, entrant_count=1)[0]
                state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
                Path(str(state["log"])).write_text(
                    "\n".join(map(json.dumps, self.smoke_stream_events(state))) + "\n"
                )
                nonce = Path(str(state["nonce_file"]))
                if mode == "wrong-bytes":
                    nonce.write_bytes(b"wrong")
                else:
                    outside = root / "outside-nonce"
                    outside.write_bytes(bytes.fromhex(str(state["nonce_hex"])))
                    nonce.symlink_to(outside)
                evidence = cloud_sb7.smoke_attempt_evidence(
                    root,
                    str(row["id"]),
                    exit_code=0,
                    descendants_clean=True,
                )
                self.assertIs(evidence["passed"], False)
                self.assertTrue(
                    any("nonce" in reason for reason in evidence["reasons"])
                )

    def test_smoke_lifecycle_ambiguity_and_outstanding_reserve_block_retry(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            lifecycle = Path(str(state["provider_lifecycle"]))
            lifecycle.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "timestamp": "now",
                        "request_id": "ambiguous-request",
                        "provider": row["provider"],
                        "model": row["model"],
                        "session": "session",
                        "state": "queued",
                    }
                )
                + "\n"
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["outstanding"] = {
                "ambiguous-request": {
                    "provider": row["provider"],
                    "model": row["model"],
                }
            }
            cloud_sb7.atomic_json(ledger_path, ledger)
            self.assertFalse(
                cloud_sb7.finalize_smoke_attempt(
                    root,
                    str(row["id"]),
                    exit_code=None,
                    descendants_clean=True,
                )
            )
            final = cloud_sb7.read_smoke_state(root, str(row["id"]))
            self.assertEqual(final["status"], "FAILED")
            self.assertIn("ambiguous", final["failure"])
            self.assertIn("budget reserves", final["failure"])

    def test_only_proven_pre_admission_failure_can_create_another_attempt(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            Path(str(state["log"])).write_text(
                json.dumps({"type": "error", "error": "local launch failure"}) + "\n"
            )
            self.assertFalse(
                cloud_sb7.finalize_smoke_attempt(
                    root,
                    str(row["id"]),
                    exit_code=2,
                    descendants_clean=True,
                )
            )
            self.assertEqual(
                cloud_sb7.read_smoke_state(root, str(row["id"]))["status"],
                "PRE_ADMISSION_FAILURE",
            )
            second = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            self.assertEqual(second["attempt"], 2)

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            self.complete_smoke_attempt(root, row)
            cloud_sb7.update_smoke_state(
                root, str(row["id"]), status="PRE_ADMISSION_FAILURE"
            )
            with self.assertRaisesRegex(SystemExit, "prior smoke episode"):
                cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)

    def test_tampered_and_predecessor_smoke_proofs_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = self.complete_smoke_attempt(root, row)
            Path(str(state["log"])).write_text("{}\n")
            self.assertIn(
                "changed after PASS",
                cloud_sb7.smoke_proof_mismatch(root, str(row["id"]), row) or "",
            )

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = self.complete_smoke_attempt(root, row)
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            predecessor_contract = campaign["smoke_contract_sha256"]
            campaign["campaign_id"] = "successor"
            campaign["lineage"] = {
                "generation": 1,
                "predecessor_campaign_id": root.name,
                "predecessor_contract_sha256": predecessor_contract,
            }
            campaign = cloud_sb7.bind_smoke_contract(campaign, [row])
            cloud_sb7.atomic_json(cloud_sb7.campaign_file(root), campaign)
            mismatch = cloud_sb7.smoke_proof_mismatch(root, str(row["id"]), row)
            self.assertIn("different campaign contract", mismatch or "")
            self.assertNotEqual(predecessor_contract, campaign["smoke_contract_sha256"])
            self.assertEqual(state["status"], "PASS")

    def test_smoke_launches_all_five_before_wait_and_never_scores_or_publishes(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rows = self.make_smoke_campaign(root)
            events: list[str] = []

            def launch(_root: Path, entrant_id: str) -> mock.Mock:
                events.append(f"launch:{entrant_id}")
                return mock.Mock()

            def wait(
                _root: Path,
                row_ids: list[str],
                supervisors: dict[str, mock.Mock],
            ) -> bool:
                events.append("wait")
                self.assertEqual(set(supervisors), set(row_ids))
                self.assertEqual(len(events), 6)
                for entrant_id in row_ids:
                    cloud_sb7.update_smoke_state(
                        root,
                        entrant_id,
                        status="PASS",
                        proof_sha256=f"proof-{entrant_id}",
                    )
                return True

            with (
                mock.patch.object(
                    cloud_sb7, "recover_smoke_entrant", return_value=False
                ),
                mock.patch.object(
                    cloud_sb7, "launch_smoke_supervisor", side_effect=launch
                ),
                mock.patch.object(cloud_sb7, "wait_for_smokes", side_effect=wait),
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "score_all") as score,
                mock.patch.object(cloud_sb7, "publish_one") as publish,
            ):
                self.assertEqual(cloud_sb7.smoke(root), 0)
            self.assertEqual(events[:-1], [f"launch:{row['id']}" for row in rows])
            self.assertEqual(events[-1], "wait")
            score.assert_not_called()
            publish.assert_not_called()
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            self.assertEqual(campaign["smoke_status"], "PASS")
            self.assertEqual(
                campaign["smoke_raw_tree_sha256_before"],
                campaign["smoke_raw_tree_sha256_after"],
            )

    def test_smoke_attempt_never_mutates_raw_benchmark_tree(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            raw_tree = root / "entrants" / str(row["id"]) / "tree"
            (raw_tree / "sentinel.bin").write_bytes(b"raw benchmark bytes")
            before = cloud_sb7.sha256_tree_exact(raw_tree)
            state = cloud_sb7.prepare_smoke_attempt(root, str(row["id"]), row)
            Path(str(state["tree"]), "smoke-only.txt").write_text("isolated\n")
            self.assertEqual(cloud_sb7.sha256_tree_exact(raw_tree), before)
            self.assertNotEqual(Path(str(state["tree"])), raw_tree)

    def test_dead_smoke_supervisor_recovers_completed_proof_without_new_episode(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            self.complete_smoke_attempt(root, row, finalize=False)
            cloud_sb7.update_smoke_state(
                root,
                str(row["id"]),
                status="RUNNING",
                supervisor_pid=765432,
                supervisor_pgid=765432,
                supervisor_identity="old-process",
                active_attempt=True,
            )
            with (
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
                mock.patch.object(cloud_sb7, "stop_recorded_group", return_value=True),
            ):
                self.assertTrue(cloud_sb7.recover_smoke_entrant(root, str(row["id"])))
            final = cloud_sb7.read_smoke_state(root, str(row["id"]))
            self.assertEqual(final["status"], "PASS")
            self.assertEqual(final["launch_attempts"], 1)
            self.assertEqual(final["admitted_episodes"], 1)

    def test_secret_scan_detects_value_split_across_read_chunks(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            artifact = root / "artifact.log"
            secret = "split-boundary-provider-secret"
            prefix = b"x" * (1024 * 1024 - 7)
            artifact.write_bytes(prefix + secret.encode() + b"tail")
            self.assertEqual(
                cloud_sb7.secret_occurrences([root], [secret]), [str(artifact)]
            )

    def test_build_gate_requires_all_five_current_untampered_smoke_proofs(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            rows = self.make_smoke_campaign(root)
            proof_hashes = {}
            for row in rows:
                state = self.complete_smoke_attempt(root, row)
                proof_hashes[str(row["id"])] = state["proof_sha256"]
            raw_hashes = {
                str(row["id"]): cloud_sb7.sha256_tree_exact(
                    root / "entrants" / str(row["id"]) / "tree"
                )
                for row in rows
            }
            cloud_sb7.update_campaign(
                root,
                smoke_status="PASS",
                smoke_proof_sha256=proof_hashes,
                smoke_raw_tree_sha256_before=raw_hashes,
                smoke_raw_tree_sha256_after=raw_hashes,
            )
            cloud_sb7.require_smoke_proofs(root)
            first = cloud_sb7.read_smoke_state(root, str(rows[0]["id"]))
            Path(str(first["proof"])).write_text("{}\n")
            with self.assertRaisesRegex(SystemExit, "untampered smoke PASS"):
                cloud_sb7.require_smoke_proofs(root)

    def test_successor_contract_carries_terminal_crash_reserve_conservatively(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            predecessor = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            ledger_path = Path(str(predecessor["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["outstanding"] = {
                "terminal-before-settlement": {
                    "provider": row["provider"],
                    "model": row["model"],
                    "reserved_usd": 12.5,
                }
            }
            cloud_sb7.atomic_json(ledger_path, ledger)
            successor = {
                **predecessor,
                "campaign_id": "successor-campaign",
                "lineage": {
                    "generation": 1,
                    "predecessor_campaign_id": predecessor["campaign_id"],
                    "predecessor_contract_sha256": predecessor["smoke_contract_sha256"],
                },
            }
            successor = cloud_sb7.bind_smoke_contract(successor, [row])
            self.assertEqual(
                successor["smoke_budget_outstanding_baselines"][str(row["id"])],
                ["terminal-before-settlement"],
            )
            outstanding, settled, error = cloud_sb7.current_smoke_budget_requests(
                successor, row
            )
            self.assertEqual((outstanding, settled, error), ([], [], None))
            self.assertNotEqual(
                predecessor["smoke_contract_sha256"],
                successor["smoke_contract_sha256"],
            )

    def test_monitor_restarts_only_dead_restart_safe_manager(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            cloud_sb7.manager_state(
                root,
                status="RUNNING",
                pid=999999,
                pgid=999999,
                identity="dead-manager",
            )

            def fake_start(_root: Path) -> int:
                cloud_sb7.manager_state(
                    root,
                    status="STARTING",
                    pid=12345,
                    pgid=12345,
                    identity="new-manager",
                )
                return 0

            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
                mock.patch.object(cloud_sb7, "recover_dead_manager") as recover,
                mock.patch.object(cloud_sb7, "start", side_effect=fake_start) as start,
            ):
                self.assertEqual(cloud_sb7.monitor_tick(root), (False, 0))
            recover.assert_called_once_with(root)
            start.assert_called_once_with(root)
            monitor = cloud_sb7.read_monitor_state(root)
            self.assertEqual(monitor["status"], "RUNNING")
            self.assertEqual(monitor["restarts"], 1)

    def test_monitor_attention_and_published_are_terminal_not_success_proxies(
        self,
    ) -> None:
        for status, expected in (("ATTENTION", 1), ("PUBLISHED", 0)):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                self.make_smoke_campaign(root, entrant_count=1)
                cloud_sb7.update_campaign(
                    root, status=status, failure="fixture failure"
                )
                cloud_sb7.manager_state(root, status=status, failure="fixture failure")
                with (
                    mock.patch.object(
                        cloud_sb7, "stop_runtime_groups_for_attention", return_value=[]
                    ),
                    mock.patch.object(
                        cloud_sb7, "published_campaign_mismatch", return_value=None
                    ),
                ):
                    terminal, exit_code = cloud_sb7.monitor_tick(root)
                self.assertTrue(terminal)
                self.assertEqual(exit_code, expected)
                self.assertEqual(cloud_sb7.read_monitor_state(root)["status"], status)

    def test_monitor_refuses_manager_relaunch_with_ambiguous_admitted_work(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            state = cloud_sb7.read_state(root, str(row["id"]))
            lifecycle = root / "entrants" / str(row["id"]) / "provider-lifecycle.jsonl"
            lifecycle.write_text(
                "\n".join(
                    map(
                        json.dumps,
                        [
                            {
                                "schema_version": 1,
                                "timestamp": "now",
                                "request_id": "admitted-request",
                                "provider": row["provider"],
                                "model": row["model"],
                                "session": "session",
                                "state": lifecycle_state,
                            }
                            for lifecycle_state in ("queued", "admitted")
                        ],
                    )
                )
                + "\n"
            )
            cloud_sb7.update_state(
                root,
                str(row["id"]),
                status="BUILD_RUNNING",
                provider_lifecycle=str(lifecycle),
                supervisor_pid=999999,
                supervisor_identity="dead-supervisor",
            )
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["outstanding"] = {
                "admitted-request": {
                    "provider": row["provider"],
                    "model": row["model"],
                }
            }
            cloud_sb7.atomic_json(ledger_path, ledger)
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
            ):
                mismatch = cloud_sb7.manager_restart_mismatch(root)
            self.assertIn("not pre-admission restart-safe", mismatch or "")
            self.assertIn("admitted", mismatch or "")
            self.assertIn("outstanding", mismatch or "")
            self.assertEqual(state["status"], "PLANNED")

    def test_monitor_start_recovers_stale_pid_without_signaling_reused_process(
        self,
    ) -> None:
        class Launched:
            pid = 24680

        def launch(
            _command: list[str],
            _log_path: Path,
            *,
            on_started: object,
        ) -> Launched:
            on_started(Launched())
            return Launched()

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            cloud_sb7.monitor_state(
                root,
                status="RUNNING",
                pid=13579,
                pgid=13579,
                identity="old-process",
            )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
                mock.patch.object(
                    cloud_sb7, "process_identity", return_value="reused-process"
                ),
                mock.patch.object(cloud_sb7, "stop_group") as stop_group,
                mock.patch.object(
                    cloud_sb7, "launch_detached", side_effect=launch
                ),
            ):
                self.assertEqual(cloud_sb7.monitor_start(root), 0)
            stop_group.assert_not_called()
            monitor = cloud_sb7.read_monitor_state(root)
            self.assertEqual(monitor["status"], "STARTING")
            self.assertEqual(monitor["pid"], Launched.pid)

    def test_monitor_waits_for_ppid_one_before_reporting_running(self) -> None:
        with (
            mock.patch.object(cloud_sb7.os, "getppid", side_effect=[24680, 1]),
            mock.patch.object(cloud_sb7.time, "monotonic", side_effect=[10.0, 10.1]),
            mock.patch.object(cloud_sb7.time, "sleep") as sleep,
        ):
            self.assertEqual(cloud_sb7.wait_for_monitor_detachment(), 1)
        sleep.assert_called_once_with(0.05)

    def test_monitor_refuses_running_state_without_ppid_one(self) -> None:
        with (
            mock.patch.object(cloud_sb7.os, "getppid", return_value=24680),
            mock.patch.object(cloud_sb7.time, "monotonic", side_effect=[10.0, 15.0]),
            mock.patch.object(cloud_sb7.time, "sleep") as sleep,
        ):
            with self.assertRaisesRegex(SystemExit, "still has parent pid 24680"):
                cloud_sb7.wait_for_monitor_detachment()
        sleep.assert_not_called()

    def test_dead_manager_stops_and_recovers_every_interrupted_publication_state(
        self,
    ) -> None:
        for status in cloud_sb7.INTERRUPTED_PUBLICATION_STATES:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                self.make_recovery_campaign(root, "RUNNING")
                cloud_sb7.update_state(
                    root,
                    "model",
                    status=status,
                    publisher_pid=2222,
                    publisher_pgid=2222,
                    publisher_identity="publisher-process",
                )
                cloud_sb7.manager_state(
                    root,
                    status="RUNNING",
                    pid=1111,
                    pgid=1111,
                    identity="dead-manager",
                )
                with (
                    mock.patch.object(cloud_sb7, "process_identity", return_value=None),
                    mock.patch.object(
                        cloud_sb7, "stop_recorded_group", return_value=True
                    ) as stop,
                ):
                    self.assertTrue(cloud_sb7.recover_dead_manager(root))
                self.assertIn(
                    mock.call(2222, 2222, "publisher-process"), stop.call_args_list
                )
                final = cloud_sb7.read_state(root, "model")
                self.assertEqual(final["status"], "PUBLISH_FAILED")
                self.assertIsNone(final["publisher_pid"])

    def test_dead_manager_stops_active_scorer_before_restart(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_recovery_campaign(root, "SCORING")
            cloud_sb7.update_state(
                root,
                "model",
                status="SCORING",
                score_pid=3333,
                score_pgid=3333,
                score_identity="scorer-process",
            )
            cloud_sb7.manager_state(
                root,
                status="SCORING",
                pid=1111,
                pgid=1111,
                identity="dead-manager",
            )
            with (
                mock.patch.object(cloud_sb7, "process_identity", return_value=None),
                mock.patch.object(
                    cloud_sb7, "stop_recorded_group", return_value=True
                ) as stop,
            ):
                self.assertTrue(cloud_sb7.recover_dead_manager(root))
            self.assertIn(mock.call(3333, 3333, "scorer-process"), stop.call_args_list)
            self.assertEqual(
                cloud_sb7.read_state(root, "model")["status"], "SCORE_FAILED"
            )

    def test_split_published_commit_is_recovered_not_reported_as_success(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            cloud_sb7.update_campaign(root, status="SCORING")
            cloud_sb7.manager_state(
                root,
                status="PUBLISHED",
                pid=999999,
                pgid=999999,
                identity="dead-manager",
            )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
                mock.patch.object(
                    cloud_sb7, "manager_restart_mismatch", return_value=None
                ),
                mock.patch.object(cloud_sb7, "recover_dead_manager") as recover,
                mock.patch.object(cloud_sb7, "start", return_value=0) as start,
            ):
                terminal, exit_code = cloud_sb7.monitor_tick(root)
            self.assertFalse(terminal)
            self.assertEqual(exit_code, 0)
            recover.assert_called_once_with(root)
            start.assert_called_once_with(root)

    def test_monitor_stopped_is_intentional_non_success(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            cloud_sb7.update_campaign(root, status="STOPPED")
            self.assertEqual(cloud_sb7.monitor_tick(root), (True, 2))
            self.assertEqual(cloud_sb7.read_monitor_state(root)["status"], "STOPPED")

    def test_offline_progress_replay_has_no_duration_cap_and_requires_recurrence_corroboration(
        self,
    ) -> None:
        healthy = " ".join(
            f"Sentence {index} describes distinct productive work with unique "
            f"value {index}."
            for index in range(100)
        )
        self.assertIs(
            cloud_sb7.monitor_progress_repetition_evidence(healthy)["detected"],
            False,
        )
        noisy_user_text = " ".join(
            ["Repeated tool output must never count as assistant recurrence."] * 100
        )
        noisy_stream = "\n".join(
            [
                json.dumps(
                    {
                        "type": "message",
                        "message": {
                            "role": "user",
                            "content": [
                                {"type": "text", "text": noisy_user_text}
                            ],
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "message",
                        "message": {
                            "role": "assistant",
                            "content": [{"type": "text", "text": healthy}],
                        },
                    }
                ),
            ]
        )
        assistant_only = cloud_sb7.assistant_semantic_stream(noisy_stream)
        self.assertNotIn("Repeated tool output", assistant_only)
        self.assertIs(
            cloud_sb7.monitor_progress_repetition_evidence(assistant_only)[
                "detected"
            ],
            False,
        )
        silence = [
            self.progress_observation(
                healthy,
                recorded_at=(
                    "2026-08-23T00:00:00Z"
                    if index == 0
                    else f"2036-08-{index:02d}T00:00:00Z"
                ),
            )
            for index in range(20)
        ]
        replayed = cloud_sb7.replay_monitor_progress(silence)
        self.assertEqual(replayed[0]["classification"], "PROCESS_BASELINE")
        self.assertTrue(
            all(
                record["classification"] == "PROVIDER_SILENCE_OBSERVED"
                and record["fail_stop"] is False
                for record in replayed[1:]
            )
        )
        self.assertEqual(replayed[-1]["stagnant_observations"], len(replayed) - 1)
        local_silence = cloud_sb7.replay_monitor_progress(
            [
                self.progress_observation(healthy, active_provider=False),
                self.progress_observation(healthy, active_provider=False),
            ]
        )
        self.assertEqual(
            local_silence[-1]["classification"], "LOCAL_SILENCE_OBSERVED"
        )
        self.assertIs(local_silence[-1]["fail_stop"], False)

        loop_sentence = (
            "I am still considering the identical plan and will reconsider it "
            "again now."
        )
        looping = " ".join([loop_sentence] * 80)
        longer_looping = " ".join([loop_sentence] * 160)
        repeated = cloud_sb7.replay_monitor_progress(
            [
                self.progress_observation(looping),
                self.progress_observation(looping),
                self.progress_observation(longer_looping),
            ]
        )
        self.assertEqual(repeated[0]["classification"], "REPETITION_SUSPECTED")
        self.assertIs(repeated[0]["fail_stop"], False)
        self.assertEqual(
            repeated[1]["classification"], "REPETITION_SUSPECTED"
        )
        self.assertIs(repeated[1]["fail_stop"], False)
        self.assertEqual(
            repeated[2]["classification"], "REPETITION_CORROBORATED"
        )
        self.assertIs(repeated[2]["fail_stop"], True)
        self.assertEqual(repeated[2]["corroborated_by_sequence"], 1)

        restarted = cloud_sb7.replay_monitor_progress(
            [
                self.progress_observation(looping),
                self.progress_observation(
                    longer_looping,
                    process_generation="c" * 64,
                ),
            ]
        )
        self.assertEqual(restarted[-1]["classification"], "REPETITION_SUSPECTED")
        self.assertIs(restarted[-1]["fail_stop"], False)
        new_provider_request = cloud_sb7.replay_monitor_progress(
            [
                self.progress_observation(looping),
                self.progress_observation(
                    longer_looping,
                    provider_generation="d" * 64,
                ),
            ]
        )
        self.assertEqual(
            new_provider_request[-1]["classification"], "REPETITION_SUSPECTED"
        )
        self.assertIs(new_provider_request[-1]["fail_stop"], False)
        unstable = self.progress_observation(longer_looping)
        unstable["evidence"]["telemetry"]["stable_read"] = False
        unstable_replay = cloud_sb7.replay_monitor_progress(
            [self.progress_observation(looping), unstable]
        )
        self.assertEqual(
            unstable_replay[-1]["classification"], "EVIDENCE_UNSTABLE"
        )
        self.assertIs(unstable_replay[-1]["fail_stop"], False)

    def test_monitor_progress_real_process_growth_silence_and_durable_orphan_adoption(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            unit = root / "entrants" / entrant_id
            log = unit / "logs/build.log"
            log.parent.mkdir()
            lifecycle = unit / "provider-lifecycle.jsonl"
            telemetry = unit / "tree/.swarm/telemetry.jsonl"
            telemetry.parent.mkdir()
            lifecycle.write_text(
                "\n".join(
                    json.dumps(
                        {
                            "schema_version": 1,
                            "timestamp": f"2026-08-23T00:00:0{index}Z",
                            "request_id": "live-request",
                            "provider": row["provider"],
                            "model": row["model"],
                            "session": "real-process-session",
                            "state": state,
                        }
                    )
                    for index, state in enumerate(("queued", "admitted"))
                )
                + "\n"
            )
            log.write_text(
                json.dumps(
                    {
                        "type": "message",
                        "message": {
                            "role": "assistant",
                            "content": [
                                {
                                    "type": "thinking",
                                    "thinking": "I will inspect the repository once.",
                                }
                            ],
                        },
                    }
                )
                + "\n"
            )
            telemetry.write_text('{"call":1}\n')
            supervisor = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            goose = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                cloud_sb7.update_state(
                    root,
                    entrant_id,
                    status="BUILD_RUNNING",
                    provider_episode_attempts=1,
                    provider_lifecycle=str(lifecycle),
                    build_log=str(log),
                    supervisor_pid=supervisor.pid,
                    supervisor_pgid=supervisor.pid,
                    supervisor_identity=cloud_sb7.process_identity(supervisor.pid),
                    goose_pid=goose.pid,
                    goose_identity=cloud_sb7.process_identity(goose.pid),
                )
                first = cloud_sb7.append_monitor_progress_observation(
                    root,
                    entrant_id,
                    cloud_sb7.monitor_progress_observation(root, entrant_id),
                )
                self.assertEqual(first["classification"], "PROCESS_BASELINE")
                self.assertIs(first["processes"]["supervisor_alive"], True)
                self.assertIs(first["processes"]["goose_alive"], True)

                with log.open("a") as stream:
                    stream.write(
                        json.dumps(
                            {
                                "type": "message",
                                "message": {
                                    "role": "assistant",
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": (
                                                "I created one verified output file."
                                            ),
                                        }
                                    ],
                                },
                            }
                        )
                        + "\n"
                    )
                (unit / "tree/result.txt").write_text("verified output\n")
                with telemetry.open("a") as stream:
                    stream.write('{"call":2}\n')
                second = cloud_sb7.append_monitor_progress_observation(
                    root,
                    entrant_id,
                    cloud_sb7.monitor_progress_observation(root, entrant_id),
                )
                self.assertEqual(second["classification"], "PROGRESSING")
                self.assertEqual(
                    set(second["delta"]["growing_signals"]),
                    {"build_log", "telemetry", "tree"},
                )

                for _ in range(5):
                    silent = cloud_sb7.append_monitor_progress_observation(
                        root,
                        entrant_id,
                        cloud_sb7.monitor_progress_observation(root, entrant_id),
                    )
                    self.assertEqual(
                        silent["classification"], "PROVIDER_SILENCE_OBSERVED"
                    )
                    self.assertIs(silent["fail_stop"], False)

                latest = cloud_sb7.monitor_progress_history(root, entrant_id)[-1]
                orphan_observation = cloud_sb7.monitor_progress_observation(
                    root, entrant_id
                )
                orphan = cloud_sb7.evaluate_monitor_progress(
                    [latest],
                    orphan_observation,
                    int(latest["sequence"]) + 1,
                    str(latest["record_sha256"]),
                )
                cloud_sb7.write_exclusive_json(
                    cloud_sb7.monitor_progress_record_path(
                        root, entrant_id, int(orphan["sequence"])
                    ),
                    orphan,
                )
                adopted = cloud_sb7.append_monitor_progress_observation(
                    root,
                    entrant_id,
                    cloud_sb7.monitor_progress_observation(root, entrant_id),
                )
                self.assertEqual(adopted["sequence"], orphan["sequence"] + 1)
                self.assertEqual(
                    adopted["previous_record_sha256"], orphan["record_sha256"]
                )
                (
                    cloud_sb7.monitor_progress_unit(root, entrant_id)
                    / ".observation-99999999.json.crash-debris"
                ).write_text("unlinked temporary payload\n")
                cloud_sb7.validate_monitor_progress_ledger(root)

                loop_sentence = (
                    "I am still considering the identical plan and will "
                    "reconsider it again now."
                )
                loop_batch = " ".join([loop_sentence] * 80)
                recurrence_records = []
                for _ in range(2):
                    with log.open("a") as stream:
                        stream.write(
                            json.dumps(
                                {
                                    "type": "message",
                                    "message": {
                                        "role": "assistant",
                                        "content": [
                                            {
                                                "type": "thinking",
                                                "thinking": loop_batch,
                                            }
                                        ],
                                    },
                                }
                            )
                            + "\n"
                        )
                    recurrence_records.append(
                        cloud_sb7.append_monitor_progress_observation(
                            root,
                            entrant_id,
                            cloud_sb7.monitor_progress_observation(
                                root, entrant_id
                            ),
                        )
                    )
                self.assertEqual(
                    recurrence_records[0]["classification"],
                    "REPETITION_SUSPECTED",
                )
                self.assertIs(recurrence_records[0]["fail_stop"], False)
                self.assertEqual(
                    recurrence_records[1]["classification"],
                    "REPETITION_CORROBORATED",
                )
                self.assertIs(recurrence_records[1]["fail_stop"], True)
                summaries, progress_failure = cloud_sb7.monitor_progress_tick(root)
                self.assertIs(summaries[0]["fail_stop"], True)
                self.assertIn("monitor progress fail-stop", progress_failure)
            finally:
                if supervisor.poll() is None:
                    cloud_sb7.stop_group(supervisor.pid, grace_seconds=0.1)
                    supervisor.wait(timeout=5)
                if goose.poll() is None:
                    cloud_sb7.stop_group(goose.pid, grace_seconds=0.1)
                    goose.wait(timeout=5)

    def test_monitor_progress_corruption_and_corroborated_failure_fail_closed(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            unit = root / "entrants" / entrant_id
            cloud_sb7.update_state(
                root,
                entrant_id,
                provider_lifecycle=str(unit / "provider-lifecycle.jsonl"),
                build_log=str(unit / "logs/build.log"),
            )
            cloud_sb7.update_state(
                root,
                entrant_id,
                build_log=str(root / "outside.log"),
            )
            with self.assertRaisesRegex(SystemExit, "escaped its entrant unit"):
                cloud_sb7.monitor_progress_observation(root, entrant_id)
            cloud_sb7.update_state(
                root,
                entrant_id,
                build_log=str(unit / "logs/build.log"),
            )
            observation = cloud_sb7.monitor_progress_observation(root, entrant_id)
            record = cloud_sb7.append_monitor_progress_observation(
                root, entrant_id, observation
            )
            record_path = cloud_sb7.monitor_progress_record_path(root, entrant_id, 1)
            changed = cloud_sb7.load_json(record_path)
            changed["reason"] = "tampered"
            cloud_sb7.atomic_json(record_path, changed)
            with self.assertRaisesRegex(SystemExit, "record identity or hash differs"):
                cloud_sb7.validate_monitor_progress_ledger(root)
            self.assertEqual(record["sequence"], 1)
            changed = dict(record)
            changed["classification"] = "PROCESS_BASELINE"
            changed["reason"] = "plausible but false replay classification"
            changed["record_sha256"] = cloud_sb7.monitor_progress_record_sha256(
                changed
            )
            cloud_sb7.atomic_json(record_path, changed)
            with self.assertRaisesRegex(SystemExit, "does not replay"):
                cloud_sb7.validate_monitor_progress_ledger(root)

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            cloud_sb7.manager_state(
                root,
                status="RUNNING",
                pid=os.getpid(),
                pgid=os.getpgrp(),
                identity=cloud_sb7.process_identity(os.getpid()),
            )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(
                    cloud_sb7,
                    "monitor_progress_tick",
                    return_value=([], "corroborated repetition fixture"),
                ),
                mock.patch.object(
                    cloud_sb7, "stop_runtime_groups_for_attention", return_value=[]
                ),
            ):
                self.assertEqual(cloud_sb7.monitor_tick(root), (True, 1))
            self.assertEqual(
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                "ATTENTION",
            )
            self.assertIn(
                "corroborated repetition",
                cloud_sb7.read_monitor_state(root)["failure"],
            )

    def test_stop_owns_live_smoke_supervisor_group(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            cloud_sb7.update_smoke_state(
                root,
                str(row["id"]),
                status="RUNNING",
                supervisor_pid=4444,
                supervisor_pgid=4444,
                supervisor_identity="smoke-supervisor",
            )
            with (
                mock.patch.object(cloud_sb7, "process_alive", return_value=True),
                mock.patch.object(
                    cloud_sb7, "stop_recorded_group", return_value=True
                ) as stop,
                mock.patch.object(cloud_sb7, "port_is_free", return_value=True),
            ):
                self.assertEqual(cloud_sb7.stop(root), 0)
            self.assertIn(
                mock.call(4444, 4444, "smoke-supervisor"), stop.call_args_list
            )
            self.assertEqual(
                cloud_sb7.read_smoke_state(root, str(row["id"]))["status"],
                "STOPPED",
            )

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
            "sandbox_denied_local_ports": [9999, 43210],
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
        self.assertEqual(
            env["GOOSE_TOOL_SANDBOX_DENY_LOCAL_PORTS"], "9999,43210"
        )
        self.assertEqual(env["PATH"], "/usr/bin:/bin:/usr/sbin:/sbin")
        self.assertEqual(env["TMPDIR"], "/tmp/profile/tool-home/tmp")
        del state["sandbox_denied_local_ports"]
        with self.assertRaisesRegex(SystemExit, "listener isolation is required"):
            cloud_sb7.child_env(row, state, "active-secret")

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
            "sandbox_denied_local_ports": [9999, 43210],
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
                self.install_live_monitor_lease(root)

                def launch(*_args: object, **kwargs: object) -> Launched:
                    kwargs["on_started"](Launched())  # type: ignore[index,operator]
                    return Launched()

                with (
                    mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                    mock.patch.object(
                        cloud_sb7, "launch_detached", side_effect=launch
                    ) as launch,
                ):
                    self.assertEqual(cloud_sb7.start(root), 0)
                launch.assert_called_once()
                self.assertEqual(
                    cloud_sb7.load_json(root / "manager.json")["status"], "STARTING"
                )

    def test_publisher_snapshot_pins_commit_inputs_runtime_without_secrets(
        self,
    ) -> None:
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
        campaign = self.public_identity_campaign(verdict)
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
                    "name": "fixture-model on sb-7.0",
                    "url": run_url,
                    "variableMeasured": [{"value": 0.42}],
                }
            )
            + "</script>"
            + "<h1>Fixture Model — 0.4200 on sb-7.0</h1>"
            + "<p>fixture-model · scorer sb-7.0</p>"
        )
        matched, evidence = cloud_sb7.rendered_publication_matches(
            campaign, board, run, "https://example.invalid", entry, verdict
        )
        self.assertTrue(matched, evidence)
        matched, evidence = cloud_sb7.rendered_publication_matches(
            campaign,
            board.replace("0.4200", "0.4100"),
            run,
            "https://example.invalid",
            entry,
            verdict,
        )
        self.assertFalse(matched)
        self.assertFalse(evidence["board_item_exact"])

        for residue in (
            "<p>sb-7.0-rc</p>",
            "<p>Scorer calibration · UNCALIBRATED</p>",
            "<p>rc-grade only</p>",
            '<script type="application/json">{"calibration":"uncalibrated"}</script>',
        ):
            with self.subTest(residue=residue):
                matched, evidence = cloud_sb7.rendered_publication_matches(
                    campaign,
                    board,
                    run + residue,
                    "https://example.invalid",
                    entry,
                    verdict,
                )
                self.assertFalse(matched)
                self.assertFalse(evidence["run_public_identity_exact"])

    def test_remote_receipt_compares_full_checks_and_screenshot_bytes(self) -> None:
        verdict = self.fixture_verdict()
        campaign = self.public_identity_campaign(verdict)
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
            "scorerVersion": "sb-7.0",
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
            receipt = cloud_sb7.remote_publication_receipt(
                campaign, entry, verdict, plan
            )
            self.assertTrue(receipt["matched"], receipt)
            self.assertEqual(
                receipt["expected_public_identity"],
                {"scorer_version": "sb-7.0", "calibration_absent": True},
            )
            self.assertEqual(
                receipt["raw_verdict_identity_sha256"],
                cloud_sb7.raw_publication_identity_sha256(verdict),
            )
            document["checksSummary"][0]["score"] = 0.4
            receipt = cloud_sb7.remote_publication_receipt(
                campaign, entry, verdict, plan
            )
            self.assertFalse(receipt["matched"])
            self.assertIn("document check 0 differs", receipt["reasons"])
            document["checksSummary"][0]["score"] = 0.5
            for stale in (
                {"scorerVersion": "sb-7.0-rc"},
                {"calibration": verdict["calibration"]},
            ):
                with self.subTest(stale=stale):
                    document.update(stale)
                    receipt = cloud_sb7.remote_publication_receipt(
                        campaign, entry, verdict, plan
                    )
                    self.assertFalse(receipt["matched"])
                    if "calibration" in stale:
                        self.assertIn(
                            "document field calibration must be absent",
                            receipt["reasons"],
                        )
                        document.pop("calibration")
                    else:
                        self.assertIn(
                            "document field scorerVersion differs",
                            receipt["reasons"],
                        )
                        document["scorerVersion"] = "sb-7.0"
            document["notes"] = (
                "Scored by sb-7.0-rc. Calibration: UNCALIBRATED; rc-grade only."
            )
            receipt = cloud_sb7.remote_publication_receipt(
                campaign, entry, verdict, plan
            )
            self.assertFalse(receipt["matched"])
            self.assertIn(
                "document notes retain forbidden RC/calibration residue",
                receipt["reasons"],
            )

    def test_rendered_receipt_persists_public_identity_not_raw_rc(self) -> None:
        verdict = self.fixture_verdict()
        campaign = self.public_identity_campaign(verdict)
        campaign["publisher"].update(
            {
                "website_base_url": "https://example.invalid",
                "verify_timeout_seconds": 1,
                "verify_interval_seconds": 0.01,
            }
        )
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
                    "name": "fixture-model on sb-7.0",
                    "url": run_url,
                    "variableMeasured": [{"value": 0.42}],
                }
            )
            + "</script>"
            + "<h1>Fixture Model — 0.4200 on sb-7.0</h1>"
            + "<p>fixture-model · scorer sb-7.0</p>"
        )
        with mock.patch.object(
            cloud_sb7,
            "fetch_rendered_page",
            side_effect=[(200, board, {}), (200, run, {})],
        ):
            receipt = cloud_sb7.verify_rendered_publication(
                campaign, entry, verdict
            )
        self.assertEqual(
            receipt["expected"],
            {
                "doc_id": entry["doc_id"],
                "label": entry["label"],
                "model": entry["model"],
                "score": 0.42,
                "scorer_version": "sb-7.0",
                "calibration_absent": True,
            },
        )
        self.assertNotIn("sb-7.0-rc", json.dumps(receipt["expected"]))
        self.assertNotIn("calibration", receipt["expected"])
        self.assertEqual(
            receipt["raw_verdict_identity_sha256"],
            cloud_sb7.raw_publication_identity_sha256(verdict),
        )

    def test_published_audit_reconstructs_public_identity_and_rejects_stale_rc(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            _, verdict = self.make_scored_campaign(root)
            runs = cloud_sb7.publication_stage(root, "fixture-model")
            state = cloud_sb7.read_state(root, "fixture-model")
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            entry = cloud_sb7.publish_entry(campaign, "fixture-model")
            run_url = (
                "https://example.invalid/agentic-benchmarks/run/"
                "brun-baseline-fixture-model-sb70"
            )
            public = cloud_sb7.public_publication_identity(campaign, verdict)
            rendered = {
                "board_status": 200,
                "run_status": 200,
                "board_item_exact": True,
                "run_visible_exact": True,
                "run_dataset_exact": True,
                "run_public_identity_exact": True,
                "run_url": run_url,
                "expected": cloud_sb7.rendered_publication_expected(
                    campaign, entry, verdict
                ),
                "raw_verdict_identity_sha256": (
                    cloud_sb7.raw_publication_identity_sha256(verdict)
                ),
            }
            cloud_sb7.update_state(
                root,
                "fixture-model",
                status="PUBLISHED",
                publisher_remote_receipt={
                    "matched": True,
                    "expected_public_identity": public,
                    "raw_verdict_identity_sha256": (
                        cloud_sb7.raw_publication_identity_sha256(verdict)
                    ),
                },
                revalidation={
                    "status": 200,
                    "paths": [
                        "/agentic-benchmarks",
                        f"/agentic-benchmarks/run/{entry['doc_id']}",
                    ],
                },
                rendered_verification=rendered,
                published_url=run_url,
                publish_stage=str(runs),
                score_attempts=state["score_attempts"],
            )
            cloud_sb7.update_campaign(root, status="PUBLISHED")
            cloud_sb7.manager_state(root, status="PUBLISHED")
            row = cloud_sb7.load_json(Path(str(campaign["entrant_manifest"])))[
                "entrants"
            ][0]
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "entrants", return_value=[row] * 5),
            ):
                self.assertIsNone(cloud_sb7.published_campaign_mismatch(root))
                stale = json.loads(json.dumps(rendered))
                stale["expected"]["scorer_version"] = "sb-7.0-rc"
                stale["expected"].pop("calibration_absent")
                stale["expected"]["calibration"] = verdict["calibration"]
                cloud_sb7.update_state(
                    root, "fixture-model", rendered_verification=stale
                )
                self.assertIn(
                    "rendered board/run verification is incomplete",
                    cloud_sb7.published_campaign_mismatch(root) or "",
                )

    def test_hermetic_verdict_must_match_frozen_scorer_and_check_contract(self) -> None:
        verdict = self.fixture_verdict()
        campaign = {
            "scorer_version": "sb-7.0-rc",
            "publisher": {"expected_checks": 1},
        }
        self.assertIsNone(cloud_sb7.verdict_failure(verdict, campaign))
        wrong = dict(verdict, scorer_version="sb-7.0")
        self.assertIn(
            "scorer version", cloud_sb7.verdict_failure(wrong, campaign) or ""
        )
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

    def test_ambiguous_publisher_exit_is_accepted_only_with_matching_receipt(
        self,
    ) -> None:
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

    def test_successful_live_process_with_remote_mismatch_is_not_rewritten(
        self,
    ) -> None:
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
                mock.patch.object(cloud_sb7, "require_lineage"),
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

    def test_manage_publishes_completed_builds_before_reporting_failed_builds(
        self,
    ) -> None:
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
                    "lineage": {
                        "generation": 0,
                        "predecessor_campaign_id": None,
                        "predecessor_contract_sha256": None,
                    },
                },
            )
            cloud_sb7.atomic_json(root / "manager.json", {"status": "RUNNING"})
            self.install_live_monitor_lease(root)
            for entrant_id, status in (
                ("complete", "BUILD_COMPLETE"),
                ("failed", "INCOMPLETE"),
            ):
                cloud_sb7.atomic_json(
                    cloud_sb7.state_file(root, entrant_id),
                    {"entrant": entrant_id, "status": status},
                )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(
                    cloud_sb7,
                    "entrants",
                    return_value=[{"id": "complete"}, {"id": "failed"}],
                ),
                mock.patch.object(cloud_sb7, "wait_for_builds", return_value=False),
                mock.patch.object(cloud_sb7, "score_all", return_value=True) as score,
            ):
                self.assertEqual(cloud_sb7.manage(root), 1)
            score.assert_called_once_with(root, ["complete"], finalize_campaign=False)
            self.assertEqual(
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                "ATTENTION",
            )

    def test_orchestrator_recovery_preserves_attempts_ambiguity_and_budget(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            source_root = Path(str(fixture["source"]))
            target_root = Path(str(fixture["target"]))
            source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
            source_ledger = cloud_sb7.load_json(Path(str(source["budget_ledger"])))
            self.assertIsNone(cloud_sb7.lineage_failure(source_root))
            unstarted_id = next(
                str(row["id"])
                for row in fixture["rows"]
                if cloud_sb7.read_state(source_root, str(row["id"]))[
                    "lineage_role"
                ]
                == "unstarted_after_infrastructure_defect"
            )
            unstarted_path = cloud_sb7.state_file(source_root, unstarted_id)
            original_unstarted = unstarted_path.read_bytes()
            cloud_sb7.update_state(
                source_root, unstarted_id, provider_episode_attempts=0
            )
            self.assertIn(
                "attempt counter was reset",
                cloud_sb7.lineage_failure(source_root) or "",
            )
            unstarted_path.write_bytes(original_unstarted)

            target = self.orchestrator_recovery_fixture(fixture)

            self.assertEqual(target["lineage"]["generation"], 2)
            self.assertEqual(target["smoke_status"], "PLANNED")
            self.assertEqual(
                cloud_sb7.load_json(Path(str(target["budget_ledger"]))),
                source_ledger,
            )
            receipt = cloud_sb7.load_json(
                source_root / cloud_sb7.ORCHESTRATOR_RECOVERY_RECEIPT
            )
            self.assertFalse(receipt["provider_terminal_usage_fabricated"])
            self.assertEqual(
                set(receipt["source_ambiguous_request_ids"]),
                {str(row["id"]) for row in fixture["rows"]},
            )
            for row in fixture["rows"]:
                entrant_id = str(row["id"])
                state = cloud_sb7.read_state(target_root, entrant_id)
                self.assertEqual(state["status"], "PLANNED")
                self.assertEqual(state["provider_episode_attempts"], 1)
                self.assertEqual(state["lineage_role"], "orchestrator_recovery_restart")
                self.assertEqual(list(Path(str(state["tree"])).iterdir()), [])
            again = self.orchestrator_recovery_fixture(fixture)
            self.assertEqual(again["campaign_id"], target["campaign_id"])
            fork = {**fixture, "target": Path(raw) / "recovery-fork"}
            with self.assertRaisesRegex(SystemExit, "receipt for another target"):
                self.orchestrator_recovery_fixture(fork)
            self.assertIn(
                "immutable orchestrator recovery receipt",
                cloud_sb7.lineage_failure(source_root) or "",
            )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "port_is_free", return_value=False),
            ):
                self.assertIsNone(cloud_sb7.lineage_failure(target_root))

    def test_orchestrator_recovery_refuses_a_structurally_valid_lookalike(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
                self.assertRaisesRegex(
                    SystemExit, "not the exact sealed 2026-08-23 incident"
                ),
            ):
                cloud_sb7.orchestrator_recovery_campaign(
                    Path(str(fixture["source"])),
                    Path(str(fixture["target"])),
                    Path(str(fixture["evidence"])),
                    True,
                )

    def test_orchestrator_recovery_refuses_outcomes_accounting_drift_and_evidence_tamper(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            source_root = Path(str(fixture["source"]))
            entrant_id = str(fixture["rows"][0]["id"])
            state_path = cloud_sb7.state_file(source_root, entrant_id)
            original_state = state_path.read_bytes()
            cloud_sb7.update_state(source_root, entrant_id, status="BUILD_COMPLETE")
            with self.assertRaisesRegex(SystemExit, "forbidden after a build outcome"):
                self.orchestrator_recovery_fixture(fixture)
            state_path.write_bytes(original_state)

            source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
            ledger_path = Path(str(source["budget_ledger"]))
            original_ledger = ledger_path.read_bytes()
            ledger = cloud_sb7.load_json(ledger_path)
            ledger["outstanding"].pop(f"ambiguous-{entrant_id}")
            cloud_sb7.atomic_json(ledger_path, ledger)
            with self.assertRaisesRegex(SystemExit, "lifecycle and ledger reserve differ"):
                self.orchestrator_recovery_fixture(fixture)
            ledger_path.write_bytes(original_ledger)

            evidence_path = Path(str(fixture["evidence"]))
            original_evidence = evidence_path.read_bytes()
            evidence = cloud_sb7.load_json(evidence_path)
            evidence["monitor_log_sha256"] = "0" * 64
            cloud_sb7.atomic_json(evidence_path, evidence)
            with self.assertRaisesRegex(SystemExit, "does not bind exact monitor_log"):
                self.orchestrator_recovery_fixture(fixture)
            evidence_path.write_bytes(original_evidence)
            self.assertFalse(
                (source_root / cloud_sb7.ORCHESTRATOR_RECOVERY_RECEIPT).exists()
            )

    def test_orchestrator_recovery_crash_boundaries_are_idempotent(self) -> None:
        for boundary in ("source_receipt_committed", "root_committed"):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as raw:
                fixture = self.make_orchestrator_recovery_fixture(Path(raw))

                def fault(stage: str) -> None:
                    if stage == boundary:
                        raise RuntimeError(f"fixture crash at {stage}")

                with (
                    mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                    mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
                    mock.patch.object(
                        cloud_sb7,
                        "ORCHESTRATOR_RECOVERY_INCIDENT",
                        fixture["incident"],
                    ),
                    mock.patch.object(
                        cloud_sb7, "orchestrator_recovery_fault", side_effect=fault
                    ),
                    self.assertRaisesRegex(RuntimeError, boundary),
                ):
                    cloud_sb7.orchestrator_recovery_campaign(
                        Path(str(fixture["source"])),
                        Path(str(fixture["target"])),
                        Path(str(fixture["evidence"])),
                        True,
                    )
                recovered = self.orchestrator_recovery_fixture(fixture)
                self.assertEqual(recovered["lineage"]["generation"], 2)

    def test_orchestrator_recovery_recovers_after_mid_evidence_copy_failure(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            source_root = Path(str(fixture["source"]))
            original_copy = cloud_sb7.atomic_copy
            failed = False

            def fail_mid_copy(
                source: Path, destination: Path, mode: int | None = None
            ) -> None:
                nonlocal failed
                if not failed and destination.name.startswith("artifact-00-"):
                    failed = True
                    raise OSError("fixture mid-copy failure")
                original_copy(source, destination, mode)

            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "require_clean_source_worktree"),
                mock.patch.object(
                    cloud_sb7,
                    "ORCHESTRATOR_RECOVERY_INCIDENT",
                    fixture["incident"],
                ),
                mock.patch.object(cloud_sb7, "atomic_copy", side_effect=fail_mid_copy),
                self.assertRaisesRegex(OSError, "mid-copy"),
            ):
                cloud_sb7.orchestrator_recovery_campaign(
                    source_root,
                    Path(str(fixture["target"])),
                    Path(str(fixture["evidence"])),
                    True,
                )
            self.assertFalse(
                (source_root / cloud_sb7.ORCHESTRATOR_RECOVERY_RECEIPT).exists()
            )
            recovered = self.orchestrator_recovery_fixture(fixture)
            self.assertEqual(recovered["lineage"]["generation"], 2)

    def test_manager_launch_requires_live_detached_monitor_identity(self) -> None:
        class Launched:
            pid = 97531

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "launch_detached") as launch,
                self.assertRaisesRegex(SystemExit, "ready detached monitor"),
            ):
                cloud_sb7.start(root)
            launch.assert_not_called()

            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
            cloud_sb7.monitor_state(
                root,
                status="RUNNING",
                pid=24680,
                pgid=24680,
                identity="monitor-identity",
                parent_pid=1,
                session_id=24680,
                detached_session=True,
                smoke_contract_sha256=campaign["smoke_contract_sha256"],
                lease_id="fixture-monitor-lease",
            )
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=True),
                mock.patch.object(
                    cloud_sb7, "launch_detached", return_value=Launched()
                ) as launch,
            ):
                self.assertEqual(cloud_sb7.start(root), 0)
            launch.assert_called_once()

    def test_monitor_launch_checks_lineage_before_detaching(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            with (
                mock.patch.object(
                    cloud_sb7,
                    "require_lineage",
                    side_effect=SystemExit("fixture lineage rejection"),
                ),
                mock.patch.object(cloud_sb7, "require_smoke_proofs") as smoke,
                mock.patch.object(cloud_sb7, "launch_detached") as launch,
                self.assertRaisesRegex(SystemExit, "fixture lineage rejection"),
            ):
                cloud_sb7.monitor_start(root)
            smoke.assert_not_called()
            launch.assert_not_called()

    def test_generation_two_excludes_only_exact_carried_reservations(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            target_root = Path(str(fixture["target"]))
            self.orchestrator_recovery_fixture(fixture)
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(target_root))
            row = fixture["rows"][0]
            entrant_id = str(row["id"])

            outstanding, error = (
                cloud_sb7.current_full_episode_outstanding_reservations(
                    target_root, campaign, row
                )
            )
            self.assertEqual((outstanding, error), ([], None))

            lineage = cloud_sb7.load_json(
                target_root / cloud_sb7.ORCHESTRATOR_RECOVERY_PATH
            )
            carried_id = lineage["source_ambiguous_request_ids"][entrant_id][0]
            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            current_id = f"episode-2-{entrant_id}"
            reservation = dict(ledger["outstanding"][carried_id])
            reservation["request_id"] = current_id
            reservation["created_at_unix_ms"] = 999999
            ledger["outstanding"][current_id] = reservation
            cloud_sb7.atomic_json(ledger_path, ledger)

            outstanding, error = (
                cloud_sb7.current_full_episode_outstanding_reservations(
                    target_root, campaign, row
                )
            )
            self.assertEqual((outstanding, error), ([current_id], None))

            ledger["outstanding"].pop(carried_id)
            cloud_sb7.atomic_json(ledger_path, ledger)
            outstanding, error = (
                cloud_sb7.current_full_episode_outstanding_reservations(
                    target_root, campaign, row
                )
            )
            self.assertEqual(outstanding, [])
            self.assertIn("reservation", error or "")

    def test_generation_two_budget_history_rejects_rollback_of_a_new_head(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            target_root = Path(str(fixture["target"]))
            self.orchestrator_recovery_fixture(fixture)
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(target_root))
            ledger_path = Path(str(campaign["budget_ledger"]))
            original = cloud_sb7.load_json(ledger_path)
            row = fixture["rows"][0]
            carried = next(
                value
                for value in original["outstanding"].values()
                if value["provider"] == row["provider"]
                and value["model"] == row["model"]
            )
            current = json.loads(json.dumps(original))
            request_id = "generation-two-new-reservation"
            current["outstanding"][request_id] = {
                **carried,
                "request_id": request_id,
                "created_at_unix_ms": 999999,
            }
            cloud_sb7.atomic_json(ledger_path, current)

            head = cloud_sb7.anchor_budget_ledger(target_root)

            self.assertEqual(head["sequence"], 1)
            self.assertIsNone(cloud_sb7.lineage_failure(target_root))
            cloud_sb7.atomic_json(ledger_path, original)
            self.assertIn(
                "rolled back behind its durable head",
                cloud_sb7.lineage_failure(target_root) or "",
            )

    def test_generation_two_accounting_requires_exact_terminal_settlement(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            target_root = Path(str(fixture["target"]))
            self.orchestrator_recovery_fixture(fixture)
            campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(target_root))
            row = fixture["rows"][0]
            entrant_id = str(row["id"])
            state = cloud_sb7.read_state(target_root, entrant_id)
            lifecycle_path = Path(str(state["provider_lifecycle"]))
            request_id = "generation-two-terminal-request"
            usage = {
                "reported_model": row["model"],
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5,
            }
            base = {
                "schema_version": 1,
                "timestamp": "now",
                "request_id": request_id,
                "provider": row["provider"],
                "model": row["model"],
                "session": "generation-two-session",
            }
            lifecycle_path.write_text(
                "\n".join(
                    map(
                        json.dumps,
                        [
                            {**base, "state": "queued"},
                            {**base, "state": "admitted"},
                            {**base, "state": "usage_reported", "usage": usage},
                            {**base, "state": "provider_terminal", "usage": usage},
                        ],
                    )
                )
                + "\n"
            )
            self.assertIn(
                "terminal lifecycle and settlements differ exactly",
                cloud_sb7.generation_two_entrant_accounting_failure(
                    target_root, campaign, row
                )
                or "",
            )

            ledger_path = Path(str(campaign["budget_ledger"]))
            ledger = cloud_sb7.load_json(ledger_path)
            config = cloud_sb7.load_json(Path(str(campaign["budget_config"])))
            profile = cloud_sb7.budget_model_profile(
                config, str(row["provider"]), str(row["model"])
            )
            assert profile is not None
            reserve = cloud_sb7.budget_price(
                profile, int(row["context_limit"]), int(row["max_output_tokens"])
            )
            charge = cloud_sb7.budget_price(profile, 2, 3)
            assert reserve is not None and charge is not None
            ledger["settled"].append(
                {
                    "request_id": request_id,
                    "provider": row["provider"],
                    "model": row["model"],
                    "reported_model": row["model"],
                    "input_tokens": 2,
                    "output_tokens": 3,
                    "total_tokens": 5,
                    "charged_upper_bound_usd": charge,
                    "reserved_usd": reserve,
                    "settled_at_unix_ms": 999999,
                }
            )
            ledger["spent_upper_bound"] += charge
            ledger["provider_spent_upper_bound"][str(row["provider"])] += charge
            cloud_sb7.atomic_json(ledger_path, ledger)
            self.assertIsNone(
                cloud_sb7.generation_two_entrant_accounting_failure(
                    target_root, campaign, row
                )
            )

    def test_provider_spawn_failure_reclaims_only_unstarted_episode(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "entrants/model/tree/.swarm").mkdir(parents=True)
            telemetry = root / "entrants/model/tree/.swarm/telemetry.jsonl"
            telemetry.write_text("")
            cloud_sb7.atomic_json(
                cloud_sb7.state_file(root, "model"),
                {
                    "entrant": "model",
                    "status": "BUILD_RUNNING",
                    "provider_episode_attempts": 2,
                    "goose_pid": None,
                    "started_at": "now",
                    "prompt_sha256": "f" * 64,
                    "command": ["goose", "run"],
                },
            )

            state = cloud_sb7.rollback_provider_episode_before_process(
                root,
                "model",
                1,
                telemetry,
                "provider process was not created",
            )

            self.assertEqual(state["status"], "PRE_ADMISSION_FAILURE")
            self.assertEqual(state["provider_episode_attempts"], 1)
            self.assertIsNone(state["started_at"])
            self.assertIsNone(state["prompt_sha256"])
            self.assertIsNone(state["command"])
            self.assertFalse(telemetry.exists())

    def test_monitor_death_stops_real_supervisor_group(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            lifecycle = root / "entrants" / entrant_id / "provider-lifecycle.jsonl"
            lifecycle.write_text("")
            monitor = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            worker = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(120)"],
                start_new_session=True,
            )
            try:
                campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
                cloud_sb7.update_campaign(root, status="RUNNING")
                lease_id = "real-monitor-lease"
                cloud_sb7.monitor_state(
                    root,
                    status="RUNNING",
                    pid=monitor.pid,
                    pgid=monitor.pid,
                    identity=cloud_sb7.process_identity(monitor.pid),
                    parent_pid=1,
                    session_id=monitor.pid,
                    detached_session=True,
                    smoke_contract_sha256=campaign["smoke_contract_sha256"],
                    lease_id=lease_id,
                )
                cloud_sb7.manager_state(
                    root, status="RUNNING", monitor_lease_id=lease_id
                )
                cloud_sb7.update_state(
                    root,
                    entrant_id,
                    status="WAITING_PROVIDER_LANE",
                    provider=row["provider"],
                    model=row["model"],
                    provider_lifecycle=str(lifecycle),
                    supervisor_pid=worker.pid,
                    supervisor_pgid=worker.pid,
                    supervisor_identity=cloud_sb7.process_identity(worker.pid),
                )
                os.kill(monitor.pid, signal.SIGKILL)
                monitor.wait(timeout=5)

                self.assertFalse(
                    cloud_sb7.wait_for_builds(
                        root, [entrant_id], poll_seconds=0.01
                    )
                )
                worker.wait(timeout=5)
                self.assertEqual(
                    cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                    "ATTENTION",
                )
                self.assertIn(
                    cloud_sb7.read_state(root, entrant_id)["status"],
                    {"PRE_ADMISSION_FAILURE", "INCOMPLETE"},
                )
            finally:
                if monitor.poll() is None:
                    cloud_sb7.stop_group(monitor.pid, grace_seconds=0.1)
                if worker.poll() is None:
                    cloud_sb7.stop_group(worker.pid, grace_seconds=0.1)

    def test_stale_monitor_lease_blocks_live_publication_before_process(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            monitor = cloud_sb7.read_monitor_state(root)
            monitor["heartbeat_monotonic"] = (
                time.monotonic() - cloud_sb7.MONITOR_LEASE_TIMEOUT_SECONDS - 1
            )
            cloud_sb7.atomic_json(root / "monitor.json", monitor)
            with mock.patch.object(cloud_sb7, "run_publisher") as publisher:
                self.assertFalse(cloud_sb7.publish_one(root, "fixture-model"))
            publisher.assert_not_called()

    def test_stale_monitor_lease_blocks_scorer_before_process(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_scored_campaign(root)
            cloud_sb7.update_state(root, "fixture-model", status="BUILD_COMPLETE")
            monitor = cloud_sb7.read_monitor_state(root)
            monitor["heartbeat_monotonic"] = (
                time.monotonic() - cloud_sb7.MONITOR_LEASE_TIMEOUT_SECONDS - 1
            )
            cloud_sb7.atomic_json(root / "monitor.json", monitor)
            with mock.patch.object(cloud_sb7, "launch_after_receipt") as launch:
                self.assertFalse(cloud_sb7.score_one(root, "fixture-model"))
            launch.assert_not_called()

    def test_attention_campaign_refuses_direct_supervisor_before_process(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            cloud_sb7.update_campaign(root, status="ATTENTION")
            with (
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7.subprocess, "Popen") as launch,
            ):
                self.assertEqual(
                    cloud_sb7.supervise_claimed(root, str(row["id"])), 2
                )
            launch.assert_not_called()

    def test_smoke_manager_hands_success_to_detached_monitor(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            observed = []

            def start_monitor(start_root: Path) -> int:
                observed.append(
                    cloud_sb7.read_smoke_manager_state(start_root)["status"]
                )
                return 0

            with (
                mock.patch.object(cloud_sb7, "smoke", return_value=0),
                mock.patch.object(
                    cloud_sb7, "monitor_start", side_effect=start_monitor
                ) as monitor,
                mock.patch.object(
                    cloud_sb7, "process_identity", return_value="smoke-manager"
                ),
            ):
                self.assertEqual(cloud_sb7.smoke_manage(root), 0)
            monitor.assert_called_once_with(root)
            self.assertEqual(observed, ["RUNNING"])
            self.assertEqual(
                cloud_sb7.read_smoke_manager_state(root)["status"], "HANDED_OFF"
            )

    def test_durable_smoke_relaunches_a_dead_manager_and_adopts_handoff(
        self,
    ) -> None:
        class Launched:
            def __init__(self, pid: int) -> None:
                self.pid = pid

        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.make_smoke_campaign(root, entrant_count=1)
            launches: list[int] = []

            def launch(*_args: object, **kwargs: object) -> Launched:
                proc = Launched(24680 + len(launches))
                launches.append(proc.pid)
                kwargs["on_started"](proc)
                if len(launches) == 2:
                    cloud_sb7.update_campaign(root, smoke_status="PASS")
                    cloud_sb7.smoke_manager_state(root, status="HANDED_OFF")
                return proc

            with (
                mock.patch.object(cloud_sb7, "require_lineage"),
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "process_alive", return_value=False),
                mock.patch.object(
                    cloud_sb7, "stop_recorded_group", return_value=True
                ),
                mock.patch.object(
                    cloud_sb7, "process_identity", return_value="smoke-manager"
                ),
                mock.patch.object(
                    cloud_sb7, "launch_after_receipt", side_effect=launch
                ),
            ):
                self.assertEqual(cloud_sb7.durable_smoke(root, poll_seconds=0), 0)
            self.assertEqual(launches, [24680, 24681])

    def test_attention_resume_restarts_monitor_before_manager(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            row = self.make_smoke_campaign(root, entrant_count=1)[0]
            entrant_id = str(row["id"])
            lifecycle = root / "entrants" / entrant_id / "provider-lifecycle.jsonl"
            lifecycle.write_text("")
            cloud_sb7.update_state(
                root,
                entrant_id,
                status="PRE_ADMISSION_FAILURE",
                provider=row["provider"],
                model=row["model"],
                provider_lifecycle=str(lifecycle),
                supervisor_pgid=None,
            )
            cloud_sb7.update_campaign(root, status="ATTENTION", failure="fixture")
            cloud_sb7.manager_state(
                root,
                status="ATTENTION",
                pid=None,
                pgid=None,
                identity=None,
            )
            cloud_sb7.monitor_state(
                root,
                status="ATTENTION",
                pid=None,
                pgid=None,
                identity=None,
            )
            with (
                mock.patch.object(cloud_sb7, "require_lineage"),
                mock.patch.object(cloud_sb7, "require_smoke_proofs"),
                mock.patch.object(cloud_sb7, "recover_dead_manager"),
                mock.patch.object(cloud_sb7, "recover_interrupted_publication"),
                mock.patch.object(cloud_sb7, "monitor_start", return_value=0) as start,
            ):
                self.assertEqual(cloud_sb7.resume_campaign(root), 0)
            start.assert_called_once_with(root)
            self.assertEqual(
                cloud_sb7.read_state(root, entrant_id)["status"], "PLANNED"
            )
            self.assertEqual(
                cloud_sb7.load_json(cloud_sb7.campaign_file(root))["status"],
                "INITIALIZED",
            )
            self.assertEqual(
                cloud_sb7.load_json(root / "manager.json")["status"], "IDLE"
            )

    def test_gated_child_never_execs_before_parent_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            marker = root / "executed.txt"
            child_pid = root / "child.pid"
            bench = Path(cloud_sb7.__file__).resolve().parent
            command = (
                "import subprocess,sys,time; from pathlib import Path; "
                f"sys.path.insert(0, {str(bench)!r}); import cloud_sb7; "
                "def_started=lambda proc: "
                f"(Path({str(child_pid)!r}).write_text(str(proc.pid)), time.sleep(120)); "
                "cloud_sb7.launch_after_receipt([sys.executable, '-c', "
                f"\"from pathlib import Path; Path({str(marker)!r}).write_text('ran')\"], "
                f"cwd=Path({str(root)!r}), env=None, stdin=subprocess.DEVNULL, "
                "stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, "
                f"gate_dir=Path({str(root / 'gates')!r}), on_started=def_started)"
            )
            parent = subprocess.Popen(
                [sys.executable, "-c", command], start_new_session=True
            )
            child = 0
            try:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and not child_pid.is_file():
                    time.sleep(0.02)
                self.assertTrue(child_pid.is_file())
                child = int(child_pid.read_text())
                os.kill(parent.pid, signal.SIGKILL)
                parent.wait(timeout=5)
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and cloud_sb7.process_alive(child):
                    time.sleep(0.02)
                self.assertFalse(cloud_sb7.process_alive(child))
                self.assertFalse(marker.exists())
            finally:
                if parent.poll() is None:
                    cloud_sb7.stop_group(parent.pid, grace_seconds=0.1)
                if child and cloud_sb7.process_alive(child):
                    cloud_sb7.stop_group(child, grace_seconds=0.1)

    def test_orchestrator_evidence_requires_clean_tree_and_distinct_artifacts(
        self,
    ) -> None:
        with mock.patch.object(
            cloud_sb7,
            "git_value",
            return_value=" M evals/swarm-bench/bench/cloud_sb7.py",
        ), self.assertRaisesRegex(SystemExit, "must be clean"):
            cloud_sb7.require_clean_source_worktree()

        with tempfile.TemporaryDirectory() as raw:
            fixture = self.make_orchestrator_recovery_fixture(Path(raw))
            evidence_path = Path(str(fixture["evidence"]))
            original = cloud_sb7.load_json(evidence_path)
            same_file = json.loads(json.dumps(original))
            same_file["artifacts"][1]["path"] = same_file["artifacts"][0]["path"]
            same_file["artifacts"][1]["sha256"] = same_file["artifacts"][0][
                "sha256"
            ]
            cloud_sb7.atomic_json(evidence_path, same_file)
            with self.assertRaisesRegex(SystemExit, "distinct files"):
                self.orchestrator_recovery_fixture(fixture)

            duplicate_role = json.loads(json.dumps(original))
            duplicate_role["artifacts"][1]["role"] = "root_cause"
            cloud_sb7.atomic_json(evidence_path, duplicate_role)
            with self.assertRaisesRegex(SystemExit, "role is duplicated"):
                self.orchestrator_recovery_fixture(fixture)


if __name__ == "__main__":
    unittest.main()
