#!/usr/bin/env python3
"""Transactional runtime for a schema-2 swarm lever campaign.

The engine owns the control catalogue. This module owns only campaign state: an
immutable queue, one-control arm receipts, guarded global-config activation,
crash recovery, event verification, and idempotent measurement.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import fcntl
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple

if __package__:
    from . import campaign_controls as controls
else:
    import campaign_controls as controls


CAMPAIGN_SCHEMA = 2
QUEUE_SCHEMA = 2
ACTIVATION_SCHEMA = 2
ARM_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")


class CampaignRuntimeError(ValueError):
    pass


def _canonical_json(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def _sha256_value(value: Any) -> str:
    return hashlib.sha256(_canonical_json(value)).hexdigest()


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _read_json(path: Path, label: str) -> Dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise CampaignRuntimeError(f"cannot read {label}: {path}") from exc
    if not isinstance(value, dict):
        raise CampaignRuntimeError(f"{label} must be an object: {path}")
    return value


def _atomic_bytes(path: Path, value: bytes, mode: Optional[int] = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        staged = Path(handle.name)
        handle.write(value)
        handle.flush()
        os.fsync(handle.fileno())
    if mode is not None:
        staged.chmod(mode)
    os.replace(staged, path)


def _atomic_json(path: Path, value: Mapping[str, Any]) -> None:
    _atomic_bytes(path, json.dumps(value, indent=2, sort_keys=True).encode() + b"\n")


def _write_same_or_create(path: Path, value: bytes, label: str) -> None:
    if path.exists():
        if path.read_bytes() != value:
            raise CampaignRuntimeError(f"{label} already exists with different evidence: {path}")
        return
    _atomic_bytes(path, value)


@contextmanager
def _state_lock(state: Path):
    state.mkdir(parents=True, exist_ok=True)
    lock_path = state / ".campaign-state.lock"
    with lock_path.open("a+b") as handle:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def _queue_path(state: Path) -> Path:
    return state / "QUEUE.schema2.jsonl"


def _plan_path(state: Path) -> Path:
    return state / "CAMPAIGN.schema2.json"


def _lock_path(state: Path) -> Path:
    return state / "CAMPAIGN.controls.lock.json"


def _active_path(state: Path) -> Path:
    return state / "CAMPAIGN.active.json"


def _arm_dir(state: Path, arm: str) -> Path:
    return state / "campaign-arms" / arm


def _profile_bytes(profile: Mapping[str, Any]) -> bytes:
    return json.dumps(profile, indent=2, sort_keys=True).encode() + b"\n"


def _normalise_variants(
    catalog: controls.ControlCatalog, payload: Mapping[str, Any]
) -> Dict[str, Any]:
    variants: Dict[str, Any] = {}
    for spelling, value in payload.items():
        row = catalog.causal_config(spelling)
        canonical = str(row["canonical"])
        if canonical in variants:
            raise CampaignRuntimeError(
                f"variant assigned twice after alias resolution: {canonical}"
            )
        variants[canonical] = value
    return variants


def _full_reference_profile(
    catalog: controls.ControlCatalog, reference_profile: Optional[Path]
) -> Dict[str, Any]:
    behavior = {
        canonical: row
        for canonical, row in catalog.config.items()
        if row["campaign_role"] == "behavior"
    }
    if reference_profile is None:
        profile = {canonical: row["default"] for canonical, row in behavior.items()}
    else:
        profile = controls.parse_behavior_profile(
            catalog, controls.read_profile(reference_profile)
        )
    missing = sorted(set(behavior) - set(profile))
    extra = sorted(set(profile) - set(behavior))
    if missing or extra:
        raise CampaignRuntimeError(
            "reference profile must explicitly name every persisted behavior control; "
            f"missing={missing}, extra={extra}"
        )
    return {canonical: profile[canonical] for canonical in sorted(profile)}


def _candidate_values(
    catalog: controls.ControlCatalog,
    reference: Mapping[str, Any],
    variants: Mapping[str, Any],
) -> Dict[str, Any]:
    candidates: Dict[str, Any] = {}
    for canonical, reference_value in reference.items():
        row = catalog.config[canonical]
        if canonical in variants:
            candidate = variants[canonical]
        elif row["value_type"] == "boolean":
            candidate = not reference_value
        else:
            raise CampaignRuntimeError(
                f"{canonical} is a {row['value_type']} behavior control and needs an explicit variant"
            )
        if candidate == reference_value:
            raise CampaignRuntimeError(
                f"variant for {canonical} equals the reference and would create a no-op arm"
            )
        candidates[canonical] = candidate
    unused = sorted(set(variants) - set(reference))
    if unused:
        raise CampaignRuntimeError(f"variants do not belong to behavior controls: {unused}")
    return candidates


def _inventory(
    catalog: controls.ControlCatalog, queued: Iterable[str]
) -> Dict[str, Any]:
    queued_set = set(queued)
    rows = []
    for canonical, row in sorted(catalog.config.items()):
        rows.append(
            {
                "canonical": canonical,
                "source": "config",
                "campaign_role": row["campaign_role"],
                "disposition": row["disposition"],
                "value_type": row["value_type"],
                "default": row["default"],
                "campaign_status": (
                    "queued_one_control_arm"
                    if canonical in queued_set
                    else f"classified_not_queued:{row['campaign_role']}"
                ),
            }
        )
    for canonical, row in sorted(catalog.environment_only.items()):
        rows.append(
            {
                "canonical": canonical,
                "source": "environment",
                "campaign_role": row["campaign_role"],
                "disposition": row["disposition"],
                "campaign_status": "unreachable_from_persisted_config",
            }
        )
    registry = catalog.export["control_registry"]
    return {
        "schema_version": 1,
        "registry_sha256": catalog.registry_sha256,
        "controls": rows,
        "aliases": registry["aliases"],
        "environment_readers": registry["environment_readers"],
    }


def _prepare_arm(
    catalog: controls.ControlCatalog,
    lock_receipt: Mapping[str, Any],
    state: Path,
    arm: str,
    reference: Mapping[str, Any],
    candidate: Mapping[str, Any],
    runtime_baseline: Path,
    replicate: bool,
) -> Dict[str, Any]:
    arm_dir = _arm_dir(state, arm)
    reference_path = state / "campaign-profiles" / "reference.json"
    candidate_path = arm_dir / "candidate.json"
    _write_same_or_create(reference_path, _profile_bytes(reference), "reference profile")
    _write_same_or_create(candidate_path, _profile_bytes(candidate), "candidate profile")
    receipt = controls.prepare_arm_receipt(
        catalog,
        lock_receipt,
        arm,
        controls.read_profile(reference_path),
        controls.read_profile(candidate_path),
        runtime_baseline,
        arm_dir / "staged-config.yaml",
        replicate,
    )
    receipt_path = arm_dir / "arm.receipt.json"
    controls.create_or_resume_arm_receipt(receipt_path, receipt)
    return {
        "schema_version": QUEUE_SCHEMA,
        "arm": arm,
        "receipt": str(receipt_path.resolve()),
        "receipt_sha256": controls.sha256_file(receipt_path),
        "staged_config": receipt["staged_config"],
        "staged_config_sha256": receipt["staged_config_sha256"],
        "replicate": replicate,
        "delta": receipt["delta"],
    }


def generate_campaign(
    state: Path,
    engine: Path,
    runtime_baseline: Path,
    spec: Path,
    port: int,
    variants_path: Optional[Path] = None,
    reference_profile: Optional[Path] = None,
    reference_replicates: int = 3,
    expected_build_sha: Optional[str] = None,
) -> Dict[str, Any]:
    state = state.expanduser().resolve()
    runtime_baseline = runtime_baseline.expanduser().resolve()
    spec = spec.expanduser().resolve()
    if reference_replicates < 1:
        raise CampaignRuntimeError("reference_replicates must be positive")
    if not runtime_baseline.is_file() or not spec.is_file():
        raise CampaignRuntimeError("runtime baseline and spec must both exist")
    with _state_lock(state):
        lock_receipt = controls.create_or_resume_lock(
            engine, _lock_path(state), expected_build_sha
        )
        catalog, lock_receipt = controls.verify_lock(engine, _lock_path(state))
        reference = _full_reference_profile(catalog, reference_profile)
        raw_variants = _read_json(variants_path, "campaign variants") if variants_path else {}
        variants = _normalise_variants(catalog, raw_variants)
        candidates = _candidate_values(catalog, reference, variants)

        rows = []
        reference_arm = "reference-1"
        for index in range(1, reference_replicates + 1):
            arm = f"reference-{index}"
            row = _prepare_arm(
                catalog,
                lock_receipt,
                state,
                arm,
                reference,
                reference,
                runtime_baseline,
                True,
            )
            row.update(
                {
                    "port": port,
                    "spec": str(spec),
                    "spec_sha256": controls.sha256_file(spec),
                    "reference_arm": None if index == 1 else reference_arm,
                }
            )
            rows.append(row)
        for canonical, candidate_value in sorted(candidates.items()):
            candidate = dict(reference)
            candidate[canonical] = candidate_value
            arm = f"arm-{canonical}"
            row = _prepare_arm(
                catalog,
                lock_receipt,
                state,
                arm,
                reference,
                candidate,
                runtime_baseline,
                False,
            )
            row.update(
                {
                    "port": port,
                    "spec": str(spec),
                    "spec_sha256": controls.sha256_file(spec),
                    "reference_arm": reference_arm,
                }
            )
            rows.append(row)

        queue_bytes = b"".join(_canonical_json(row) + b"\n" for row in rows)
        queue_sha = _sha256_bytes(queue_bytes)
        controls_source = Path(controls.__file__).resolve()
        plan = {
            "schema_version": CAMPAIGN_SCHEMA,
            "engine_path": str(engine.expanduser().resolve()),
            "engine_binary_sha256": lock_receipt["engine_binary_sha256"],
            "engine": catalog.export["engine"],
            "registry_sha256": catalog.registry_sha256,
            "control_environment_sha256": catalog.export[
                "control_environment_sha256"
            ],
            "controls_consumer": str(controls_source),
            "controls_consumer_sha256": controls.sha256_file(controls_source),
            "runtime_baseline": str(runtime_baseline),
            "runtime_baseline_sha256": controls.sha256_file(runtime_baseline),
            "spec": str(spec),
            "spec_sha256": controls.sha256_file(spec),
            "port": port,
            "reference_arm": reference_arm,
            "reference_replicates": reference_replicates,
            "queue": str(_queue_path(state)),
            "queue_sha256": queue_sha,
            "arm_count": len(rows),
        }
        plan_bytes = json.dumps(plan, indent=2, sort_keys=True).encode() + b"\n"
        _write_same_or_create(_plan_path(state), plan_bytes, "campaign plan")
        _write_same_or_create(
            state / "CAMPAIGN.inventory.json",
            json.dumps(
                _inventory(catalog, candidates), indent=2, sort_keys=True
            ).encode()
            + b"\n",
            "campaign inventory",
        )
        _write_same_or_create(_queue_path(state), queue_bytes, "campaign queue")
        return plan


def _load_queue(state: Path, plan: Mapping[str, Any]) -> List[Dict[str, Any]]:
    queue = Path(str(plan.get("queue", "")))
    if queue != _queue_path(state):
        raise CampaignRuntimeError("campaign queue path is not the state queue")
    if not queue.is_file() or controls.sha256_file(queue) != plan.get("queue_sha256"):
        raise CampaignRuntimeError("campaign queue is missing or changed after generation")
    rows = []
    for number, line in enumerate(queue.read_text().splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            raise CampaignRuntimeError(f"invalid campaign queue row {number}") from exc
        if not isinstance(row, dict) or row.get("schema_version") != QUEUE_SCHEMA:
            raise CampaignRuntimeError(f"unsupported campaign queue row {number}")
        arm = row.get("arm")
        if not isinstance(arm, str) or not ARM_NAME.fullmatch(arm):
            raise CampaignRuntimeError(f"invalid campaign arm in row {number}")
        rows.append(row)
    if len(rows) != plan.get("arm_count") or len({r["arm"] for r in rows}) != len(rows):
        raise CampaignRuntimeError("campaign queue arm count or identity is invalid")
    return rows


def load_campaign(
    state: Path,
) -> Tuple[Dict[str, Any], List[Dict[str, Any]], controls.ControlCatalog, Dict[str, Any]]:
    state = state.expanduser().resolve()
    plan = _read_json(_plan_path(state), "campaign plan")
    if plan.get("schema_version") != CAMPAIGN_SCHEMA:
        raise CampaignRuntimeError("unsupported campaign plan schema")
    consumer = Path(str(plan.get("controls_consumer", "")))
    if consumer.resolve() != Path(controls.__file__).resolve():
        raise CampaignRuntimeError("campaign controls consumer path changed")
    if not consumer.is_file() or controls.sha256_file(consumer) != plan.get(
        "controls_consumer_sha256"
    ):
        raise CampaignRuntimeError("campaign controls consumer changed after generation")
    engine = Path(str(plan.get("engine_path", "")))
    catalog, lock_receipt = controls.verify_lock(engine, _lock_path(state))
    for key, actual in (
        ("engine_binary_sha256", lock_receipt["engine_binary_sha256"]),
        ("engine", catalog.export["engine"]),
        ("registry_sha256", catalog.registry_sha256),
        (
            "control_environment_sha256",
            catalog.export["control_environment_sha256"],
        ),
    ):
        if plan.get(key) != actual:
            raise CampaignRuntimeError(f"campaign plan {key} differs from the sealed engine")
    baseline = Path(str(plan.get("runtime_baseline", "")))
    spec = Path(str(plan.get("spec", "")))
    if not baseline.is_file() or controls.sha256_file(baseline) != plan.get(
        "runtime_baseline_sha256"
    ):
        raise CampaignRuntimeError("runtime baseline changed after campaign generation")
    if not spec.is_file() or controls.sha256_file(spec) != plan.get("spec_sha256"):
        raise CampaignRuntimeError("campaign spec changed after generation")
    return plan, _load_queue(state, plan), catalog, lock_receipt


def _row_for_arm(rows: Sequence[Mapping[str, Any]], arm: str) -> Dict[str, Any]:
    matches = [dict(row) for row in rows if row.get("arm") == arm]
    if len(matches) != 1:
        raise CampaignRuntimeError(f"arm is not uniquely present in the campaign queue: {arm}")
    return matches[0]


def _receipt_for_row(
    row: Mapping[str, Any],
    catalog: controls.ControlCatalog,
    lock_receipt: Mapping[str, Any],
) -> Dict[str, Any]:
    receipt_path = Path(str(row.get("receipt", "")))
    if not receipt_path.is_file() or controls.sha256_file(receipt_path) != row.get(
        "receipt_sha256"
    ):
        raise CampaignRuntimeError(f"arm receipt is missing or changed: {row.get('arm')}")
    receipt = _read_json(receipt_path, "arm receipt")
    controls.validate_arm_receipt(catalog, lock_receipt, receipt)
    if receipt.get("arm") != row.get("arm"):
        raise CampaignRuntimeError("queue arm and receipt arm differ")
    return receipt


def _verification_path(state: Path, arm: str) -> Path:
    return _arm_dir(state, arm) / "verification.json"


def _validate_verification(
    state: Path,
    row: Mapping[str, Any],
    catalog: controls.ControlCatalog,
    lock_receipt: Mapping[str, Any],
) -> Dict[str, Any]:
    receipt = _receipt_for_row(row, catalog, lock_receipt)
    path = _verification_path(state, str(row["arm"]))
    verification = _read_json(path, "arm verification")
    if verification.get("schema_version") != 1:
        raise CampaignRuntimeError("unsupported arm verification schema")
    if verification.get("arm") != row["arm"]:
        raise CampaignRuntimeError("verification belongs to another arm")
    if verification.get("arm_receipt_sha256") != controls.sha256_value(receipt):
        raise CampaignRuntimeError("verification arm receipt digest is invalid")
    if verification.get("engine") != catalog.export["engine"]:
        raise CampaignRuntimeError("verification engine identity is invalid")
    if verification.get("registry_sha256") != catalog.registry_sha256:
        raise CampaignRuntimeError("verification registry digest is invalid")
    if verification.get("control_environment_sha256") != catalog.export.get(
        "control_environment_sha256"
    ):
        raise CampaignRuntimeError("verification environment digest is invalid")
    executed = verification.get("executed_controls")
    if not isinstance(executed, dict) or verification.get(
        "executed_controls_sha256"
    ) != controls.sha256_value(executed):
        raise CampaignRuntimeError("verification executed-control digest is invalid")
    event_log = Path(str(verification.get("event_log", "")))
    if not event_log.is_file() or controls.sha256_file(event_log) != verification.get(
        "event_log_sha256"
    ):
        raise CampaignRuntimeError("verification event log changed")
    if verification.get("executed_delta_from_reference") != row.get("delta"):
        raise CampaignRuntimeError("verification did not execute the queued delta")
    return verification


def next_arm(state: Path) -> Optional[Dict[str, Any]]:
    state = state.expanduser().resolve()
    with _state_lock(state):
        plan, rows, catalog, lock_receipt = load_campaign(state)
        active_path = _active_path(state)
        if active_path.exists():
            active = _read_json(active_path, "active campaign arm")
            row = _row_for_arm(rows, str(active.get("arm", "")))
            row["resume"] = True
            row["engine_path"] = plan["engine_path"]
            return row
        for row in rows:
            verification = _verification_path(state, str(row["arm"]))
            if verification.exists():
                _validate_verification(state, row, catalog, lock_receipt)
                continue
            result = dict(row)
            result["resume"] = False
            result["engine_path"] = plan["engine_path"]
            return result
        return None


def _latest_event_log(swarm_dir: Path) -> Optional[Path]:
    logs = sorted(swarm_dir.glob("run-swarm-*.jsonl"))
    return logs[-1] if logs else None


def _has_run_finished(path: Optional[Path]) -> bool:
    if path is None:
        return False
    try:
        with path.open(encoding="utf-8", errors="replace") as handle:
            for line in handle:
                try:
                    if json.loads(line).get("event") == "run_finished":
                        return True
                except (json.JSONDecodeError, AttributeError):
                    continue
    except OSError:
        return False
    return False


def _fresh_live_runs(
    build_root: Path,
    now: Optional[dt.datetime] = None,
    heartbeat_max_age: float = 180.0,
) -> List[Tuple[str, float]]:
    now = now or dt.datetime.now(dt.timezone.utc)
    live = []
    for swarm_dir in sorted(build_root.glob("*/.swarm")):
        heartbeat = swarm_dir / "heartbeat"
        if not heartbeat.is_file() or _has_run_finished(_latest_event_log(swarm_dir)):
            continue
        try:
            timestamp = dt.datetime.fromisoformat(
                heartbeat.read_text().strip().replace("Z", "+00:00")
            )
            if timestamp.tzinfo is None:
                raise ValueError("heartbeat has no timezone")
        except (OSError, ValueError) as exc:
            raise CampaignRuntimeError(
                f"cannot establish heartbeat age for {swarm_dir.parent}"
            ) from exc
        age = (now - timestamp).total_seconds()
        if age < -5:
            raise CampaignRuntimeError(f"heartbeat is in the future for {swarm_dir.parent}")
        if age < heartbeat_max_age:
            live.append((swarm_dir.parent.name, age))
    return live


def _refuse_live_mutation(build_root: Path) -> None:
    live = _fresh_live_runs(build_root)
    if live:
        detail = ", ".join(f"{name} ({age:.0f}s)" for name, age in live)
        raise CampaignRuntimeError(
            f"refusing global config mutation while a run heartbeat is live: {detail}"
        )


def _activation_matches(
    activation: Mapping[str, Any], row: Mapping[str, Any], global_config: Path
) -> None:
    if activation.get("schema_version") != ACTIVATION_SCHEMA:
        raise CampaignRuntimeError("unsupported active-arm schema")
    if activation.get("arm") != row.get("arm"):
        raise CampaignRuntimeError(
            f"another campaign arm owns global config: {activation.get('arm')}"
        )
    if activation.get("receipt_sha256") != row.get("receipt_sha256"):
        raise CampaignRuntimeError("active arm receipt differs from the queue")
    if activation.get("staged_config_sha256") != row.get("staged_config_sha256"):
        raise CampaignRuntimeError("active arm staged config differs from the queue")
    if activation.get("global_config") != str(global_config):
        raise CampaignRuntimeError("active arm names a different global config")


def activate_arm(
    state: Path,
    arm: str,
    global_config: Path,
    build_root: Path,
    recovery: bool = False,
    pre_commit_hook: Optional[Callable[[], None]] = None,
    failure_point: Optional[str] = None,
) -> Dict[str, Any]:
    state = state.expanduser().resolve()
    global_config = global_config.expanduser().resolve()
    build_root = build_root.expanduser().resolve()
    if not global_config.is_file() or global_config.is_symlink():
        raise CampaignRuntimeError("global config must be an existing regular non-symlink file")
    _refuse_live_mutation(build_root)
    if pre_commit_hook:
        pre_commit_hook()
    with _state_lock(state):
        plan, rows, catalog, lock_receipt = load_campaign(state)
        row = _row_for_arm(rows, arm)
        receipt = _receipt_for_row(row, catalog, lock_receipt)
        _refuse_live_mutation(build_root)
        staged = Path(str(row["staged_config"]))
        staged_bytes = staged.read_bytes()
        if _sha256_bytes(staged_bytes) != row["staged_config_sha256"]:
            raise CampaignRuntimeError("staged config changed before activation")
        active_path = _active_path(state)
        run_dir = build_root / arm
        snapshot = run_dir / ".arm-config.yaml"
        receipt_snapshot = run_dir / ".campaign-arm.json"
        if active_path.exists():
            activation = _read_json(active_path, "active campaign arm")
            _activation_matches(activation, row, global_config)
            if not recovery:
                raise CampaignRuntimeError(
                    "arm already owns the activation lease; explicit crash recovery is required"
                )
            if _has_run_finished(_latest_event_log(run_dir / ".swarm")):
                raise CampaignRuntimeError(
                    "finished arm is awaiting verification and cannot be relaunched"
                )
            current_sha = controls.sha256_file(global_config)
            if current_sha != row["staged_config_sha256"]:
                if not recovery:
                    raise CampaignRuntimeError(
                        "global config diverged from the active arm; only explicit crash recovery may restore it"
                    )
                restorations = list(activation.get("restorations") or [])
                restorations.append({"observed_sha256": current_sha})
                activation["restorations"] = restorations
                _atomic_bytes(global_config, staged_bytes, global_config.stat().st_mode)
            if not snapshot.exists() or controls.sha256_file(snapshot) != row[
                "staged_config_sha256"
            ]:
                _atomic_bytes(snapshot, staged_bytes)
            _write_same_or_create(
                receipt_snapshot,
                json.dumps(receipt, indent=2, sort_keys=True).encode() + b"\n",
                "run arm receipt",
            )
            activation["state"] = "activated"
            _atomic_json(active_path, activation)
            return activation

        pre_bytes = global_config.read_bytes()
        pre_sha = _sha256_bytes(pre_bytes)
        backup = state / "campaign-config-backups" / f"{pre_sha}.yaml"
        _write_same_or_create(backup, pre_bytes, "pre-campaign config backup")
        activation = {
            "schema_version": ACTIVATION_SCHEMA,
            "state": "preparing",
            "arm": arm,
            "receipt": row["receipt"],
            "receipt_sha256": row["receipt_sha256"],
            "staged_config": row["staged_config"],
            "staged_config_sha256": row["staged_config_sha256"],
            "global_config": str(global_config),
            "pre_activation_sha256": pre_sha,
            "pre_activation_backup": str(backup),
            "queue_sha256": plan["queue_sha256"],
            "restorations": [],
        }
        _atomic_json(active_path, activation)
        if failure_point == "after_intent":
            raise RuntimeError("injected crash after activation intent")
        _atomic_bytes(global_config, staged_bytes, global_config.stat().st_mode)
        if failure_point == "after_global_config":
            raise RuntimeError("injected crash after global config mutation")
        _atomic_bytes(snapshot, staged_bytes)
        _write_same_or_create(
            receipt_snapshot,
            json.dumps(receipt, indent=2, sort_keys=True).encode() + b"\n",
            "run arm receipt",
        )
        activation["state"] = "activated"
        _atomic_json(active_path, activation)
        return activation


def _event_counts(path: Path) -> Counter:
    counts: Counter = Counter()
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict):
                counts[event.get("event")] += 1
    return counts


def verify_arm(state: Path, arm: str, event_log: Path) -> Dict[str, Any]:
    state = state.expanduser().resolve()
    event_log = event_log.expanduser().resolve()
    with _state_lock(state):
        _plan, rows, catalog, lock_receipt = load_campaign(state)
        row = _row_for_arm(rows, arm)
        arm_receipt = _receipt_for_row(row, catalog, lock_receipt)
        counts = _event_counts(event_log)
        if counts["run_finished"] != 1:
            raise CampaignRuntimeError(
                f"event log must contain exactly one run_finished; found {counts['run_finished']}"
            )
        reference = None
        if row.get("reference_arm"):
            reference_row = _row_for_arm(rows, str(row["reference_arm"]))
            reference = _validate_verification(
                state, reference_row, catalog, lock_receipt
            )
        verification = controls.prepare_verification_receipt(
            catalog,
            lock_receipt,
            arm_receipt,
            controls.read_levers_event(event_log),
            event_log,
            reference,
        )
        path = _verification_path(state, arm)
        controls.create_or_resume_arm_receipt(path, verification)
        return _validate_verification(state, row, catalog, lock_receipt)


def release_arm(
    state: Path, arm: str, build_root: Path, verification: Mapping[str, Any]
) -> None:
    state = state.expanduser().resolve()
    build_root = build_root.expanduser().resolve()
    _refuse_live_mutation(build_root)
    with _state_lock(state):
        _plan, rows, catalog, lock_receipt = load_campaign(state)
        row = _row_for_arm(rows, arm)
        expected = _validate_verification(state, row, catalog, lock_receipt)
        if expected != verification:
            raise CampaignRuntimeError("release verification changed during measurement")
        active_path = _active_path(state)
        history = state / "campaign-activations" / f"{arm}.released.json"
        if not active_path.exists():
            released = _read_json(history, "released activation")
            if released.get("verification_sha256") != controls.sha256_value(
                verification
            ):
                raise CampaignRuntimeError("released activation has different evidence")
            return
        activation = _read_json(active_path, "active campaign arm")
        _activation_matches(
            activation, row, Path(str(activation.get("global_config", "")))
        )
        activation["state"] = "released"
        activation["verification_sha256"] = controls.sha256_value(verification)
        _write_same_or_create(
            history,
            json.dumps(activation, indent=2, sort_keys=True).encode() + b"\n",
            "released activation",
        )
        active_path.unlink()


def _measure_events(event_log: Path) -> Dict[str, Any]:
    events: Counter = Counter()
    verdicts: Counter = Counter()
    elapsed_by_task: Dict[str, int] = {}
    values: Dict[str, Any] = {
        "conf": None,
        "floor": None,
        "tasks": 0,
        "ready@t0": 0,
        "done": 0,
        "failed": 0,
        "bonus": 0,
        "claimed_passed": None,
        "claimed_verified": None,
        "demoted": None,
        "research_tools": None,
        "grounded": None,
        "invented": None,
        "persona": None,
        "notes_used": 0,
        "total_min": None,
        "plan_min": None,
        "exec_min": None,
        "research_min": None,
        "splits": 0,
    }
    with event_log.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            kind = event.get("event")
            events[kind] += 1
            if kind == "plan_loaded":
                values["conf"] = event.get("plan_confidence")
                values["floor"] = event.get("ask_floor")
                tasks = event.get("tasks") or []
                values["tasks"] = len(tasks)
                values["ready@t0"] = sum(
                    1 for task in tasks if not (task.get("deps") or task.get("depends_on"))
                )
            elif kind == "complete_result":
                values["claimed_passed"] = event.get("passed")
                values["claimed_verified"] = event.get("verified")
            elif kind == "complete_result_revised":
                values["demoted"] = event.get("reason")
                if event.get("verified") is not None:
                    values["claimed_verified"] = event.get("verified")
            elif kind == "judge_verdict":
                sub = event.get("sub") or {}
                verdicts[event.get("verdict") or sub.get("verdict") or "?"] += 1
                if (event.get("action") or sub.get("action")) == "split":
                    values["splits"] += 1
            elif kind == "task_completed" and event.get("elapsed_ms"):
                elapsed_by_task[str(event.get("task_id"))] = event["elapsed_ms"]
            elif kind == "confidence_retarget" and event.get("action") == "re_research":
                values["grounded"] = event.get("grounded", values["grounded"])
                for resolution in event.get("resolutions") or []:
                    if values["invented"] is None:
                        values["invented"] = 0
                    if not resolution.get("grounded"):
                        values["invented"] += 1
            elif kind == "research_tools":
                values["research_tools"] = len(event.get("available") or [])
            elif kind == "persona_learned":
                values["persona"] = "written" if event.get("written") else "no"
            elif kind == "persona_loaded":
                values["persona"] = f"reused({event.get('runs')})"
            elif kind == "user_note_consumed":
                values["notes_used"] += 1
            elif kind == "run_finished":
                report = event.get("report") or {}
                values["done"] = len(report.get("done") or [])
                values["failed"] = len(report.get("failed") or [])
                values["bonus"] = len(report.get("bonus") or [])
                phases = event.get("phases") or {}
                values["total_min"] = phases.get("total_min")
                values["plan_min"] = phases.get("planning_min")
                values["exec_min"] = phases.get("execute_min")
                values["research_min"] = phases.get("research_min")
    sink_min = round(elapsed_by_task.get("integrate-verify", 0) / 60000, 1)
    values.update(
        {
            "finished": events["run_finished"],
            "asked": events["low_confidence_ask"],
            "judge_corrective": sum(
                count
                for verdict, count in verdicts.items()
                if verdict not in ("ok", "observed")
            ),
            "sink_min": sink_min,
            "sink_pct_of_exec": (
                round(100 * sink_min / values["exec_min"])
                if values["exec_min"]
                else 0
            ),
        }
    )
    return values


def _append_ledger_idempotently(ledger: Path, row: Mapping[str, Any]) -> bool:
    existing_rows: List[Dict[str, str]] = []
    existing_fields: List[str] = []
    if ledger.exists() and ledger.stat().st_size:
        with ledger.open(newline="") as handle:
            reader = csv.DictReader(handle, delimiter="\t")
            existing_fields = list(reader.fieldnames or [])
            existing_rows = [dict(item) for item in reader]
    evidence = str(row["event_log_sha256"])
    for existing in existing_rows:
        if existing.get("event_log_sha256") == evidence:
            if existing.get("arm") != str(row["arm"]):
                raise CampaignRuntimeError("one event log is already attributed to another arm")
            return False
        if existing.get("arm") == str(row["arm"]) and existing.get(
            "control_verification_sha256"
        ):
            raise CampaignRuntimeError("arm already has different verified evidence in the ledger")
    fields = existing_fields + [key for key in row if key not in existing_fields]
    if not fields:
        fields = list(row)
    existing_rows.append({key: "" if value is None else str(value) for key, value in row.items()})
    with tempfile.NamedTemporaryFile(
        "w", newline="", dir=ledger.parent, delete=False
    ) as handle:
        staged = Path(handle.name)
        writer = csv.DictWriter(handle, fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(existing_rows)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(staged, ledger)
    return True


def measure_arm(
    state: Path,
    arm: str,
    build_dir: Path,
    ledger: Path,
    event_log: Optional[Path] = None,
) -> Tuple[Dict[str, Any], bool]:
    state = state.expanduser().resolve()
    build_dir = build_dir.expanduser().resolve()
    build_root = build_dir.parent
    if event_log is None:
        event_log = _latest_event_log(build_dir / ".swarm")
    if event_log is None:
        raise CampaignRuntimeError(f"no run log under {build_dir}/.swarm")
    event_log = event_log.resolve()
    verification = verify_arm(state, arm, event_log)
    plan, rows, catalog, lock_receipt = load_campaign(state)
    row_spec = _row_for_arm(rows, arm)
    _validate_verification(state, row_spec, catalog, lock_receipt)
    metrics = _measure_events(event_log)
    executed = verification["executed_controls"]
    behavior_on = sorted(
        canonical
        for canonical, value in executed.items()
        if value is True
        and canonical in catalog.config
        and catalog.config[canonical]["campaign_role"] == "behavior"
    )
    row: Dict[str, Any] = {
        "arm": arm,
        "build": build_dir.name,
        **metrics,
        "levers_on": ",".join(behavior_on),
        "lever_count": len(behavior_on),
        "declared_delta": ",".join(row_spec["delta"]),
        "executed_delta": ",".join(verification["executed_delta_from_reference"]),
        "engine_build_sha": plan["engine"]["build_sha"],
        "registry_sha256": plan["registry_sha256"],
        "event_log_sha256": verification["event_log_sha256"],
        "control_verification_sha256": controls.sha256_value(verification),
        "comparable": True,
        "spec_pass": "",
        "spec_total": "",
    }
    ledger = ledger.expanduser().resolve()
    ledger.parent.mkdir(parents=True, exist_ok=True)
    with _state_lock(state):
        appended = _append_ledger_idempotently(ledger, row)
    release_arm(state, arm, build_root, verification)
    return row, appended


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    generate = subparsers.add_parser("generate")
    generate.add_argument("--state", type=Path, required=True)
    generate.add_argument("--engine", type=Path, required=True)
    generate.add_argument("--runtime-baseline", type=Path, required=True)
    generate.add_argument("--spec", type=Path, required=True)
    generate.add_argument("--port", type=int, required=True)
    generate.add_argument("--variants", type=Path)
    generate.add_argument("--reference-profile", type=Path)
    generate.add_argument("--reference-replicates", type=int, default=3)
    generate.add_argument("--expected-build-sha")

    next_parser = subparsers.add_parser("next-arm")
    next_parser.add_argument("--state", type=Path, required=True)

    activate = subparsers.add_parser("activate")
    activate.add_argument("--state", type=Path, required=True)
    activate.add_argument("--arm", required=True)
    activate.add_argument("--global-config", type=Path, required=True)
    activate.add_argument("--build-root", type=Path, required=True)
    activate.add_argument("--recovery", action="store_true")

    verify = subparsers.add_parser("verify")
    verify.add_argument("--state", type=Path, required=True)
    verify.add_argument("--arm", required=True)
    verify.add_argument("--event-log", type=Path, required=True)

    measure = subparsers.add_parser("measure")
    measure.add_argument("--state", type=Path, required=True)
    measure.add_argument("--arm", required=True)
    measure.add_argument("--build-dir", type=Path, required=True)
    measure.add_argument("--ledger", type=Path, required=True)
    measure.add_argument("--event-log", type=Path)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _build_parser().parse_args(argv)
    try:
        if args.command == "generate":
            result = generate_campaign(
                args.state,
                args.engine,
                args.runtime_baseline,
                args.spec,
                args.port,
                args.variants,
                args.reference_profile,
                args.reference_replicates,
                args.expected_build_sha,
            )
        elif args.command == "next-arm":
            result = next_arm(args.state)
            if result is None:
                return 3
        elif args.command == "activate":
            result = activate_arm(
                args.state,
                args.arm,
                args.global_config,
                args.build_root,
                args.recovery,
            )
        elif args.command == "verify":
            result = verify_arm(args.state, args.arm, args.event_log)
        else:
            result, appended = measure_arm(
                args.state,
                args.arm,
                args.build_dir,
                args.ledger,
                args.event_log,
            )
            result = {"appended": appended, "row": result}
        print(json.dumps(result, sort_keys=True))
        return 0
    except (CampaignRuntimeError, controls.ControlManifestError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
