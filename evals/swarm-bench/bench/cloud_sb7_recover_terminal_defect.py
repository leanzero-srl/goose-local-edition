#!/usr/bin/env python3
"""Recover one fully accounted cloud entrant from a coordinator defect."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any, Mapping

import cloud_sb7

EXACT_FAILURE = "background tool descendants survived the build process"


def require_sealed_failure(expected_failure: str) -> None:
    if expected_failure != EXACT_FAILURE:
        raise SystemExit(
            "recovery failure does not match the sealed descendant-cleanup incident"
        )


def terminal_defect_entrants(
    states: Mapping[str, Mapping[str, Any]], expected_failure: str
) -> set[str]:
    return {
        entrant_id
        for entrant_id, state in states.items()
        if state.get("status") == "INCOMPLETE"
        and state.get("failure") == expected_failure
    }


def recovery_partition_failure(
    states: Mapping[str, Mapping[str, Any]],
    expected_affected_entrants: set[str],
    expected_failure: str,
) -> tuple[set[str], str | None]:
    affected_entrants = terminal_defect_entrants(states, expected_failure)
    if not expected_affected_entrants:
        return affected_entrants, "expected affected entrant set is empty"
    missing_expected = expected_affected_entrants - affected_entrants
    if missing_expected:
        return affected_entrants, (
            "expected incident entrants do not carry the sealed failure: "
            + ", ".join(sorted(missing_expected))
        )
    for entrant_id in sorted(affected_entrants):
        affected = states.get(entrant_id)
        if not isinstance(affected, Mapping):
            return affected_entrants, f"affected entrant state is missing: {entrant_id}"
        admitted = int(affected.get("admitted_requests", -1))
        terminal = int(affected.get("provider_terminal_requests", -1))
        if admitted <= 0 or admitted != terminal:
            return affected_entrants, (
                "affected entrant provider lifecycle is not fully terminal: "
                + entrant_id
            )
        if affected.get("budget_outstanding_request_ids"):
            return affected_entrants, (
                "affected entrant retains provider budget reservations: "
                + entrant_id
            )
    unsuccessful = sorted(
        entrant_id
        for entrant_id, state in states.items()
        if entrant_id not in affected_entrants
        and state.get("status") not in cloud_sb7.BUILD_SUCCESS_STATES
    )
    if unsuccessful:
        return affected_entrants, (
            "unaffected entrants have no carryable build outcome: "
            + ", ".join(unsuccessful)
        )
    return affected_entrants, None


def readiness_failure(
    campaign: Mapping[str, Any],
    manager: Mapping[str, Any],
    states: Mapping[str, Mapping[str, Any]],
    expected_affected_entrants: set[str],
    expected_failure: str,
    *,
    manager_alive: bool,
) -> tuple[str, str, set[str]]:
    affected_entrants = terminal_defect_entrants(states, expected_failure)
    discovered = ",".join(sorted(affected_entrants)) or "none"
    if campaign.get("status") != "ATTENTION":
        return (
            "WAIT",
            f"campaign={campaign.get('status')}; terminal_defect_entrants={discovered}",
            affected_entrants,
        )
    if manager.get("status") != "ATTENTION":
        return (
            "WAIT",
            f"manager={manager.get('status')}; terminal_defect_entrants={discovered}",
            affected_entrants,
        )
    if manager_alive:
        return (
            "WAIT",
            "manager is finishing its terminal transition; "
            f"terminal_defect_entrants={discovered}",
            affected_entrants,
        )
    affected_entrants, partition_failure = recovery_partition_failure(
        states, expected_affected_entrants, expected_failure
    )
    if partition_failure:
        return "REFUSE", partition_failure, affected_entrants
    return (
        "READY",
        f"{len(affected_entrants)} terminal entrant(s) can be superseded: {discovered}",
        affected_entrants,
    )


def log(message: str) -> None:
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    print(f"{stamp} {message}", flush=True)


def load_states(root: Path) -> dict[str, dict[str, Any]]:
    campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(root))
    manifest = cloud_sb7.load_json(Path(str(campaign["entrant_manifest"])))
    return {
        str(row["id"]): cloud_sb7.read_state(root, str(row["id"]))
        for row in cloud_sb7.entrants(manifest)
    }


def wait_until_ready(
    source_root: Path,
    expected_affected_entrants: set[str],
    expected_failure: str,
    poll_seconds: float,
) -> set[str]:
    prior = None
    while True:
        campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
        manager = cloud_sb7.load_json(source_root / "manager.json")
        states = load_states(source_root)
        manager_alive = cloud_sb7.process_alive(
            manager.get("pid"), manager.get("identity")
        )
        disposition, reason, affected_entrants = readiness_failure(
            campaign,
            manager,
            states,
            expected_affected_entrants,
            expected_failure,
            manager_alive=manager_alive,
        )
        current = (disposition, reason)
        if current != prior:
            log(f"{disposition}: {reason}")
            prior = current
        if disposition == "READY":
            return affected_entrants
        if disposition == "REFUSE":
            raise SystemExit(reason)
        time.sleep(poll_seconds)


def write_defect_evidence(
    source_root: Path,
    target_root: Path,
    affected_entrants: set[str],
    root_cause: Path,
    regression_test: Path,
) -> Path:
    campaign = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
    binary = Path(str(campaign["binary"]))
    evidence_path = source_root.parent / f".{target_root.name}-defect-evidence.json"
    artifacts = [
        {
            "role": "root_cause",
            "path": str(root_cause.resolve()),
            "sha256": cloud_sb7.sha256_file(root_cause),
        },
        {
            "role": "regression_test",
            "path": str(regression_test.resolve()),
            "sha256": cloud_sb7.sha256_file(regression_test),
        },
    ]
    payload = {
        "schema_version": cloud_sb7.SUPERSESSION_SCHEMA,
        "classification": "infrastructure_defect",
        "defect_id": "cloud-descendant-cleanup-20260823",
        "summary": (
            "The coordinator rejected a full provider episode after it had "
            "successfully terminated every model-authored background process."
        ),
        "affected_entrants": sorted(affected_entrants),
        "predecessor_campaign_id": campaign["campaign_id"],
        "predecessor_binary_sha256": campaign["binary_sha256"],
        "replacement_binary_sha256": cloud_sb7.sha256_file(binary),
        "fix_source_commit": cloud_sb7.git_value("rev-parse", "HEAD"),
        "artifacts": artifacts,
    }
    cloud_sb7.atomic_json(evidence_path, payload)
    return evidence_path


def recover(args: argparse.Namespace) -> None:
    source_root = args.from_root.resolve()
    target_root = args.root.resolve()
    expected_affected_entrants = set(args.affected_entrant)
    require_sealed_failure(args.expected_failure)
    root_cause = args.root_cause.resolve()
    regression_test = args.regression_test.resolve()
    if source_root == target_root or source_root.parent != target_root.parent:
        raise SystemExit("source and target must be distinct sibling campaign roots")
    affected_entrants = wait_until_ready(
        source_root,
        expected_affected_entrants,
        args.expected_failure,
        args.poll_seconds,
    )
    log("stopping the sealed predecessor after all unaffected publications completed")
    cloud_sb7.stop(source_root)
    stopped_affected, partition_failure = recovery_partition_failure(
        load_states(source_root), expected_affected_entrants, args.expected_failure
    )
    if partition_failure:
        raise SystemExit(
            "terminal recovery partition changed while stopping predecessor: "
            + partition_failure
        )
    if stopped_affected != affected_entrants:
        raise SystemExit(
            "terminal recovery affected set changed while stopping predecessor: "
            f"ready={','.join(sorted(affected_entrants))} "
            f"stopped={','.join(sorted(stopped_affected))}"
        )
    source = cloud_sb7.load_json(cloud_sb7.campaign_file(source_root))
    evidence = write_defect_evidence(
        source_root,
        target_root,
        affected_entrants,
        root_cause,
        regression_test,
    )
    publisher = source["publisher"]
    log("creating one-hop successor with successful entrants carried immutably")
    cloud_sb7.supersede_campaign(
        source_root,
        target_root,
        Path(str(source["binary"])),
        Path(str(source["entrant_manifest"])),
        Path(str(source["secret_file"])),
        Path(str(publisher["repo"])),
        evidence,
        publisher.get("mode") == "live",
        str(publisher["website_base_url"]),
        float(publisher["verify_timeout_seconds"]),
        float(publisher["verify_interval_seconds"]),
        float(publisher["process_timeout_seconds"]),
    )
    log("running the successor's fresh strict five-provider smoke gate")
    if cloud_sb7.durable_smoke(target_root) != 0:
        raise SystemExit("successor smoke gate failed")
    log(
        "starting the manager on the monitor handed off by durable smoke; "
        "only affected full arms are planned"
    )
    cloud_sb7.start(target_root)
    for entrant_id in sorted(affected_entrants):
        state = cloud_sb7.read_state(target_root, entrant_id)
        if state.get("status") not in {
            "PLANNED",
            "WAITING_PROVIDER_LANE",
            "BUILD_RUNNING",
        }:
            raise SystemExit(
                f"affected successor did not launch: {entrant_id}={state.get('status')}"
            )
    carried = {
        entrant_id: candidate.get("status")
        for entrant_id, candidate in load_states(target_root).items()
        if entrant_id not in affected_entrants
    }
    if any(status not in cloud_sb7.BUILD_SUCCESS_STATES for status in carried.values()):
        raise SystemExit("successor did not preserve all unaffected build outcomes")
    log(
        f"RECOVERY_STARTED target={target_root} "
        f"affected={','.join(sorted(affected_entrants))}"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--from-root", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument(
        "--affected-entrant",
        action="append",
        required=True,
        help=(
            "known incident entrant; every additional terminal entrant with the exact "
            "sealed failure is discovered and included automatically"
        ),
    )
    parser.add_argument("--expected-failure", required=True)
    parser.add_argument("--root-cause", type=Path, required=True)
    parser.add_argument("--regression-test", type=Path, required=True)
    parser.add_argument("--poll-seconds", type=float, default=20.0)
    args = parser.parse_args()
    if args.poll_seconds <= 0:
        raise SystemExit("poll interval must be positive")
    recover(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
