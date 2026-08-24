#!/usr/bin/env python3
"""Detached, restart-safe terminal closure for the immutable v17 SB7 run.

The live run is read-only. This controller authenticates its already-running processes from the
launch receipt, waits for natural terminal evidence, seals the complete raw tree, scores only a
disposable clone with the frozen CLI scorer, then invokes the dedicated exact-ID publisher.
"""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import math
import os
import pathlib
import re
import shutil
import signal
import socket
import stat
import subprocess
import sys
import threading
import time
from typing import Any, Iterable, Mapping, Sequence


SCHEMA_VERSION = 1
SEED_RE = re.compile(r"^[0-9a-f]{16}$")
SENSITIVE_NAME_RE = re.compile(r"(?:token|secret|authorization|api[_-]?key|password)", re.I)
SENSITIVE_TEXT_PATTERNS = (
    re.compile(r"(authorization\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+", re.I),
    re.compile(r"(x-reval-key\s*[:=]\s*)[^\s,;]+", re.I),
    re.compile(r"((?:api[_-]?key|token|secret|password)\s*[:=]\s*)[^\s,;]+", re.I),
    re.compile(r"bearer\s+[A-Za-z0-9._~+/=-]+", re.I),
)
TERMINAL_PHASES = {"complete", "failed", "stopped"}
SB7_TIERS = frozenset({"A", "B", "C", "D", "J", "V", "P", "T", "X", "R", "E"})


class ClosureError(RuntimeError):
    pass


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def ensure_secure_dir(path: pathlib.Path) -> None:
    if path.is_symlink() or (path.exists() and not path.is_dir()):
        raise ClosureError(f"secure directory target is not a real directory: {path}")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.chmod(0o700)


def atomic_write(path: pathlib.Path, payload: bytes, mode: int = 0o600) -> None:
    ensure_secure_dir(path.parent)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{os.urandom(6).hex()}.tmp")
    descriptor = os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, mode)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        path.chmod(mode)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary.exists():
            temporary.unlink()


def atomic_json(path: pathlib.Path, value: Any, mode: int = 0o600) -> None:
    atomic_write(path, json.dumps(value, indent=2, sort_keys=True).encode() + b"\n", mode)


def read_json(path: pathlib.Path) -> Any:
    def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise ClosureError(f"{path} repeats JSON key {key!r}")
            value[key] = item
        return value

    with path.open(encoding="utf-8") as handle:
        return json.load(handle, object_pairs_hook=unique_object)


def redact_text(value: Any, secrets: Iterable[str] = ()) -> str:
    rendered = str(value)
    for secret in secrets:
        if isinstance(secret, str) and len(secret) >= 6:
            rendered = rendered.replace(secret, "[REDACTED]")
    for pattern in SENSITIVE_TEXT_PATTERNS:
        if pattern.pattern.startswith("bearer"):
            rendered = pattern.sub("Bearer [REDACTED]", rendered)
        else:
            rendered = pattern.sub(r"\1[REDACTED]", rendered)
    return rendered


def safe_environment(extra: dict[str, str] | None = None) -> dict[str, str]:
    allowed = {
        name: value
        for name, value in os.environ.items()
        if name in {"HOME", "PATH", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "SHELL"}
        and not SENSITIVE_NAME_RE.search(name)
    }
    allowed.update({"PYTHONDONTWRITEBYTECODE": "1", "PYTHONUNBUFFERED": "1"})
    if extra:
        if any(SENSITIVE_NAME_RE.search(name) for name in extra):
            raise ClosureError("refusing to place a secret-named value in a child environment")
        allowed.update(extra)
    return allowed


def full_process_identity(pid: int) -> bytes | None:
    completed = subprocess.run(
        [
            "ps",
            "-p",
            str(pid),
            "-o",
            "pid=",
            "-o",
            "ppid=",
            "-o",
            "pgid=",
            "-o",
            "lstart=",
            "-o",
            "command=",
        ],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    identity = completed.stdout.strip()
    return identity if completed.returncode == 0 and identity else None


def full_process_identity_sha256(pid: int) -> str | None:
    identity = full_process_identity(pid)
    return sha256_bytes(identity) if identity is not None else None


def reparented_process_identity_matches(identity: bytes, expected_sha256: str) -> bool:
    match = re.fullmatch(rb"(\d+)(\s+)(\d+)(\s+)(\d+)(.*)", identity, re.S)
    if match is None:
        return False
    pid = int(match.group(1))
    current_parent = int(match.group(3))
    process_group = int(match.group(5))
    if current_parent != 1 or process_group != pid:
        return False
    parent_field_width = len(match.group(2)) + len(match.group(3))
    for original_parent in range(2, 100_000):
        parent = str(original_parent).encode()
        if len(parent) > parent_field_width:
            break
        candidate = (
            match.group(1)
            + parent.rjust(parent_field_width, b" ")
            + match.group(4)
            + match.group(5)
            + match.group(6)
        )
        if sha256_bytes(candidate) == expected_sha256:
            return True
    return False


def safe_process_receipt(pid: int) -> dict[str, Any] | None:
    completed = subprocess.run(
        ["ps", "-p", str(pid), "-o", "pid=", "-o", "lstart=", "-o", "comm="],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    identity = completed.stdout.strip()
    if completed.returncode != 0 or not identity:
        return None
    return {"pid": pid, "identity_sha256": sha256_bytes(identity)}


def validate_authenticated_process(role: str, receipt: dict[str, Any]) -> bool:
    pid = receipt.get("pid")
    expected = receipt.get("identity_sha256")
    if not isinstance(pid, int) or not isinstance(expected, str):
        raise ClosureError(f"{role} launch receipt is malformed")
    identity = full_process_identity(pid)
    if identity is None:
        return False
    observed = sha256_bytes(identity)
    if observed != expected:
        if not reparented_process_identity_matches(identity, expected):
            raise ClosureError(
                f"{role} pid {pid} no longer matches its authenticated launch identity"
            )
    return True


def read_jsonl(path: pathlib.Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("rb") as handle:
        payload = handle.read()
    if payload and not payload.endswith(b"\n"):
        raise ClosureError(f"{path} has an incomplete terminal JSONL line")
    for index, raw in enumerate(payload.splitlines(), start=1):
        try:
            row = json.loads(raw)
        except json.JSONDecodeError as error:
            raise ClosureError(f"{path}:{index} is malformed: {error}") from error
        if not isinstance(row, dict):
            raise ClosureError(f"{path}:{index} is not an object")
        rows.append(row)
    return rows


def path_is_within(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def tree_manifest(root: pathlib.Path) -> dict[str, Any]:
    root = root.resolve()
    if not root.is_dir():
        raise ClosureError(f"tree does not exist: {root}")
    entries: list[dict[str, Any]] = []
    total_bytes = 0
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        metadata = path.lstat()
        mode = stat.S_IMODE(metadata.st_mode)
        if stat.S_ISDIR(metadata.st_mode):
            entries.append({"path": relative, "type": "directory", "mode": mode})
            continue
        if stat.S_ISREG(metadata.st_mode):
            digest = sha256_file(path)
            after = path.lstat()
            before_identity = (
                metadata.st_dev,
                metadata.st_ino,
                metadata.st_mode,
                metadata.st_size,
                metadata.st_mtime_ns,
            )
            after_identity = (
                after.st_dev,
                after.st_ino,
                after.st_mode,
                after.st_size,
                after.st_mtime_ns,
            )
            if before_identity != after_identity:
                raise ClosureError(f"tree file changed while it was hashed: {relative}")
            entries.append(
                {
                    "path": relative,
                    "type": "file",
                    "mode": mode,
                    "size": metadata.st_size,
                    "sha256": digest,
                }
            )
            total_bytes += metadata.st_size
            continue
        if stat.S_ISLNK(metadata.st_mode):
            target = path.resolve()
            if not path_is_within(target, root):
                raise ClosureError(f"tree contains an escaping symlink: {relative}")
            entries.append(
                {
                    "path": relative,
                    "type": "symlink",
                    "mode": mode,
                    "target_sha256": sha256_bytes(os.readlink(path).encode()),
                }
            )
            continue
        raise ClosureError(f"tree contains an unsupported special file: {relative}")
    digest = sha256_bytes(canonical_json(entries))
    return {
        "schema_version": SCHEMA_VERSION,
        "root": str(root),
        "captured_at": utc_now(),
        "entries": entries,
        "entry_count": len(entries),
        "total_bytes": total_bytes,
        "tree_sha256": digest,
    }


def manifests_equal(left: dict[str, Any], right: dict[str, Any]) -> bool:
    return left.get("tree_sha256") == right.get("tree_sha256") and left.get("entries") == right.get("entries")


def finite_number(value: Any, minimum: float | None = None, maximum: float | None = None) -> bool:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    number = float(value)
    return bool(
        math.isfinite(number)
        and (minimum is None or number >= minimum)
        and (maximum is None or number <= maximum)
    )


def validate_sb7_score_payload(
    score: Mapping[str, Any], expected: Mapping[str, Any], fixture_seed: str
) -> None:
    if score.get("fixture_seed") != fixture_seed or not SEED_RE.fullmatch(fixture_seed):
        raise ClosureError("score used a different or malformed fixture_seed")
    if score.get("scorer_version") != expected["raw_scorer_version"]:
        raise ClosureError("score has the wrong raw scorer version")
    if not re.search(r"uncalibrated|rc-grade", str(score.get("calibration", "")), re.I):
        raise ClosureError("score lacks raw RC calibration disclosure")
    if not finite_number(score.get("score"), 0, 1) or not isinstance(score.get("excellent"), bool):
        raise ClosureError("score summary is malformed")
    if (
        not finite_number(score.get("inner"), 0, 1)
        or not isinstance(score.get("excellence_gate"), bool)
        or not isinstance(score.get("solid"), bool)
    ):
        raise ClosureError("score composition summary is malformed")
    tiers = score.get("tiers")
    if not isinstance(tiers, dict) or set(tiers) != SB7_TIERS:
        raise ClosureError("score tier registry differs from frozen SB7")
    for tier_name in SB7_TIERS:
        tier = tiers[tier_name]
        mean = tier.get("mean") if isinstance(tier, dict) else tier
        if not finite_number(mean, 0, 1):
            raise ClosureError(f"score tier {tier_name} mean is malformed")
    checks = score.get("checks")
    if not isinstance(checks, list) or len(checks) != expected["check_count"]:
        raise ClosureError("score check count differs")
    names: set[str] = set()
    for index, row in enumerate(checks):
        if not isinstance(row, dict):
            raise ClosureError(f"score check {index} is not an object")
        name = row.get("check")
        if not isinstance(name, str) or not name or name in names:
            raise ClosureError(f"score check {index} has an invalid identity")
        if row.get("tier") not in SB7_TIERS or not finite_number(row.get("score"), 0, 1):
            raise ClosureError(f"score check {name} is malformed")
        if not isinstance(row.get("detail"), str):
            raise ClosureError(f"score check {name} lacks detail evidence")
        names.add(name)
    excellence = score.get("excellence")
    if not isinstance(excellence, dict):
        raise ClosureError("score excellence evidence is malformed")
    if not finite_number(excellence.get("fraction"), 0, 1) or not finite_number(
        excellence.get("e_mean"), 0, 1
    ):
        raise ClosureError("score excellence means are malformed")
    conditions = excellence.get("conditions")
    if not isinstance(conditions, list) or not conditions:
        raise ClosureError("score excellence conditions are missing")
    condition_names: set[str] = set()
    for condition in conditions:
        if not isinstance(condition, dict):
            raise ClosureError("score excellence condition is malformed")
        name = condition.get("name")
        value = condition.get("value")
        if (
            not isinstance(name, str)
            or not name
            or name in condition_names
            or not isinstance(condition.get("ok"), bool)
            or (value is not None and not finite_number(value))
        ):
            raise ClosureError("score excellence condition is malformed")
        condition_names.add(name)
    critical = score.get("critical")
    if not isinstance(critical, dict):
        raise ClosureError("score critical evidence is malformed")
    for field in ("floor", "multiplier", "pre_severity_score"):
        if not finite_number(critical.get(field), 0, 1):
            raise ClosureError(f"score critical.{field} is malformed")
    critical_rows = critical.get("rows")
    if not isinstance(critical_rows, list):
        raise ClosureError("score critical rows are missing")
    for row in critical_rows:
        if (
            not isinstance(row, dict)
            or not isinstance(row.get("check"), str)
            or not finite_number(row.get("score"), 0, 1)
            or not finite_number(row.get("factor"), 0, 1)
            or not isinstance(row.get("why"), str)
        ):
            raise ClosureError("score critical row is malformed")
    telemetry = score.get("telemetry")
    if not isinstance(telemetry, dict) or not isinstance(telemetry.get("nodes"), dict):
        raise ClosureError("score lacks exact engine telemetry")
    if set(telemetry["nodes"]) != set(expected["telemetry_nodes"]):
        raise ClosureError("score telemetry node identity differs")
    numeric_fields = ("calls", "prompt_tokens", "completion_tokens", "prefill_tok_s", "decode_tok_s")
    for field in numeric_fields:
        if field not in telemetry or not finite_number(telemetry[field], 0):
            raise ClosureError(f"score telemetry.{field} is malformed")
    if not finite_number(telemetry.get("calls"), 1):
        raise ClosureError("score telemetry contains no completed model calls")
    for node_name, node in telemetry["nodes"].items():
        if not isinstance(node, dict):
            raise ClosureError(f"score telemetry node {node_name} is malformed")
        for field in numeric_fields:
            if field not in node or not finite_number(node[field], 0):
                raise ClosureError(f"score telemetry node {node_name}.{field} is malformed")
        if not finite_number(node.get("calls"), 1):
            raise ClosureError(f"score telemetry node {node_name} contains no completed calls")
    for field in ("calls", "prompt_tokens", "completion_tokens"):
        if sum(node[field] for node in telemetry["nodes"].values()) != telemetry[field]:
            raise ClosureError(f"score telemetry.{field} does not reconcile its node totals")


def load_config(path: pathlib.Path) -> dict[str, Any]:
    config = read_json(path.resolve())
    if config.get("schema_version") != SCHEMA_VERSION:
        raise ClosureError("closure config schema_version must be 1")
    return config


def validate_config(config: dict[str, Any]) -> None:
    required_sections = {"expected", "publication", "publisher", "runtime"}
    if not required_sections.issubset(config):
        missing = sorted(required_sections - set(config))
        raise ClosureError(f"closure config lacks required sections: {missing}")
    configured_live_root = pathlib.Path(config["live_root"])
    configured_state_dir = pathlib.Path(config["state_dir"])
    configured_run_dir = pathlib.Path(config["run_dir"])
    if configured_live_root.is_symlink() or configured_state_dir.is_symlink() or configured_run_dir.is_symlink():
        raise ClosureError("closure root paths must not be symbolic links")
    live_root = configured_live_root.resolve()
    state_dir = configured_state_dir.resolve()
    run_dir = configured_run_dir.resolve()
    if path_is_within(state_dir, live_root):
        raise ClosureError("closure state must be outside the immutable live v17 root")
    if not path_is_within(run_dir, live_root):
        raise ClosureError("configured run_dir is not inside the authenticated live root")
    score_lock = pathlib.Path(config["score_lock_path"]).resolve()
    if path_is_within(score_lock, live_root):
        raise ClosureError("the serial scorer lock must be outside the immutable live root")
    publication = config["publication"]
    target = publication["target_document_id"]
    protected = publication["protected_document_ids"]
    if target != "brun-fleet-qwen38-brainwaves-sb70":
        raise ClosureError("publication target is not the dedicated Brainwaves document")
    if target in protected or set(protected) != {
        "brun-fleet-qwen38-sb70",
        "brun-fleet-qwen-sb70",
    }:
        raise ClosureError("protected benchmark document set changed")
    if config["expected"]["vendor_port"] != 18970:
        raise ClosureError("v17 advertised/scoring port must remain 18970")
    if config["expected"].get("entrant") != "swarm-3node-qwen38-brainwaves":
        raise ClosureError("v17 entrant identity changed")
    if config["expected"].get("raw_scorer_version") != "sb-7.0-rc":
        raise ClosureError("v17 raw scorer convention changed")
    if config["expected"].get("check_count") != 91:
        raise ClosureError("v17 frozen scorer check count changed")
    if sorted(config["expected"].get("telemetry_nodes") or []) != [
        "gabee",
        "mihai",
        "workhorse",
    ]:
        raise ClosureError("v17 telemetry node contract changed")
    for field in ("controller_sha256",):
        if not re.fullmatch(r"[0-9a-f]{64}", str(config.get(field, ""))):
            raise ClosureError(f"{field} must be a frozen SHA-256")
    for field in ("sha256", "node_sha256", "package_lock_sha256", "package_json_sha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(config["publisher"].get(field, ""))):
            raise ClosureError(f"publisher.{field} must be a frozen SHA-256")
    if int(config.get("max_score_attempts", 0)) < 1:
        raise ClosureError("max_score_attempts must be positive")
    if int(config.get("max_publish_attempts", 0)) < 1:
        raise ClosureError("max_publish_attempts must be positive")
    if float(config.get("score_timeout_seconds", 0)) <= 0:
        raise ClosureError("score_timeout_seconds must be positive")


class DurableEvents:
    def __init__(self, path: pathlib.Path, secrets: Iterable[str] = ()) -> None:
        ensure_secure_dir(path.parent)
        descriptor = os.open(path, os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
        os.fchmod(descriptor, 0o600)
        self.handle = os.fdopen(descriptor, "a", encoding="utf-8", buffering=1)
        self.path = path
        self.secrets = tuple(secrets)

    def emit(self, event: str, **fields: Any) -> None:
        row = {"at": utc_now(), "event": event, **fields}
        rendered = json.dumps(row, sort_keys=True)
        rendered = redact_text(rendered, self.secrets)
        self.handle.write(rendered + "\n")
        self.handle.flush()
        os.fsync(self.handle.fileno())


class TerminalClosure:
    def __init__(self, config_path: pathlib.Path) -> None:
        self.config_path = config_path.resolve()
        self.config = load_config(self.config_path)
        validate_config(self.config)
        self.live_root = pathlib.Path(self.config["live_root"]).resolve()
        self.run_dir = pathlib.Path(self.config["run_dir"]).resolve()
        self.state_dir = pathlib.Path(self.config["state_dir"]).resolve()
        ensure_secure_dir(self.state_dir)
        self.events = DurableEvents(self.state_dir / "events.jsonl")
        self.state_path = self.state_dir / "state.json"
        self.stop_path = self.state_dir / "STOP"

    def checkpoint(self, phase: str, **fields: Any) -> None:
        previous = read_json(self.state_path) if self.state_path.is_file() else {}
        state = {
            **previous,
            "schema_version": SCHEMA_VERSION,
            "phase": phase,
            "updated_at": utc_now(),
            **fields,
        }
        atomic_json(self.state_path, state)
        self.events.emit("phase", phase=phase)

    def check_stop(self) -> None:
        if self.stop_path.exists():
            self.checkpoint("stopped")
            raise SystemExit(75)

    def record_process_exit_observation(self, role: str, pid: int) -> dict[str, Any]:
        path = self.state_dir / f"{role}-exit-observation.json"
        if path.is_file():
            receipt = read_json(path)
            if receipt.get("role") != role or receipt.get("pid") != pid:
                raise ClosureError(f"{role} exit observation identity changed")
            return receipt
        receipt = {
            "schema_version": SCHEMA_VERSION,
            "role": role,
            "pid": pid,
            "observed_exited_at": utc_now(),
            "exit_code_observable": False,
        }
        atomic_json(path, receipt)
        self.events.emit(f"{role}_exit_observed", pid=pid, exit_code_observable=False)
        return receipt

    def validate_frozen_inputs(self) -> tuple[dict[str, Any], dict[str, Any]]:
        expected = self.config["expected"]
        launch_path = self.live_root / "launch.json"
        manifest_path = self.live_root / "instrument-manifest.json"
        if sha256_file(launch_path) != expected["launch_sha256"]:
            raise ClosureError("v17 launch receipt changed")
        if sha256_file(manifest_path) != expected["instrument_manifest_sha256"]:
            raise ClosureError("v17 instrument manifest changed")
        launch = read_json(launch_path)
        manifest = read_json(manifest_path)
        if launch.get("publication_document_id") != self.config["publication"]["target_document_id"]:
            raise ClosureError("launch publication identity changed")
        policy = manifest.get("sb7_policy") or {}
        if policy.get("publication_document_id") != self.config["publication"]["target_document_id"]:
            raise ClosureError("instrument publication identity changed")
        if set(policy.get("protected_document_ids") or []) != set(
            self.config["publication"]["protected_document_ids"]
        ):
            raise ClosureError("instrument protected-document set changed")
        if (
            policy.get("publish_from_run_build_auto_score") is not False
            or policy.get("website_surface") != "stable-sb7"
            or policy.get("entrant") != expected["entrant"]
            or policy.get("spec_and_scorer_unchanged_from_v6") is not True
        ):
            raise ClosureError("instrument SB7 closure policy changed")
        if launch.get("candidate", {}).get("commit") != expected["candidate_commit"]:
            raise ClosureError("candidate commit changed")
        if manifest.get("candidate_commit") != expected["candidate_commit"]:
            raise ClosureError("instrument candidate commit changed")
        if launch.get("entrant") != expected["entrant"]:
            raise ClosureError("launch entrant changed")
        if launch.get("vendor_port") != expected["vendor_port"]:
            raise ClosureError("advertised vendor port changed")
        if launch.get("run_started_identity", {}).get("run_id") != expected["run_id"]:
            raise ClosureError("run identity changed")
        if sorted(launch.get("run_started_identity", {}).get("pool_models") or []) != sorted(
            expected["models"]
        ):
            raise ClosureError("launch model pool changed")
        binary = pathlib.Path(launch["binary"]["path"])
        if (
            binary.is_symlink()
            or not binary.is_file()
            or sha256_file(binary) != expected["binary_sha256"]
            or launch["binary"].get("sha256") != expected["binary_sha256"]
            or binary.stat().st_mode & 0o222
        ):
            raise ClosureError("frozen binary hash/mode changed")
        expected_files = expected["instrument_files"]
        if manifest.get("files") != expected_files:
            raise ClosureError("instrument file inventory differs from the committed closure config")
        for relative, digest in expected_files.items():
            path = self.live_root / "instrument" / relative
            if path.is_symlink() or not path.is_file() or sha256_file(path) != digest:
                raise ClosureError(f"frozen instrument changed: {relative}")
            if path.stat().st_mode & 0o222:
                raise ClosureError(f"frozen instrument became writable: {relative}")
        publisher = pathlib.Path(self.config["publisher"]["path"])
        if publisher.is_symlink() or sha256_file(publisher) != self.config["publisher"]["sha256"]:
            raise ClosureError("guarded publisher hash changed")
        if sha256_file(pathlib.Path(__file__).resolve()) != self.config["controller_sha256"]:
            raise ClosureError("terminal closure controller hash changed")
        node = pathlib.Path(self.config["publisher"]["node"])
        if node.is_symlink() or sha256_file(node) != self.config["publisher"]["node_sha256"]:
            raise ClosureError("publisher/render Node runtime hash changed")
        package_lock = pathlib.Path(self.config["publisher"]["package_lock"])
        if sha256_file(package_lock) != self.config["publisher"]["package_lock_sha256"]:
            raise ClosureError("publisher dependency lock changed")
        package_json = pathlib.Path(self.config["publisher"]["package_json"])
        if sha256_file(package_json) != self.config["publisher"]["package_json_sha256"]:
            raise ClosureError("publisher package manifest changed")
        lsof = pathlib.Path(self.config["runtime"]["lsof"])
        if not lsof.is_file() or not os.access(lsof, os.X_OK):
            raise ClosureError("lsof runtime is unavailable")
        return launch, manifest

    def wait_for_terminal(self, launch: dict[str, Any]) -> dict[str, Any]:
        poll_seconds = float(self.config.get("poll_seconds", 10))
        last_report = 0.0
        previously_alive = {"harness": True, "goose": True, "monitor": True}
        while True:
            self.check_stop()
            harness_alive = validate_authenticated_process("harness", launch["harness"])
            goose_alive = validate_authenticated_process("goose", launch["goose"])
            monitor_alive = validate_authenticated_process("monitor", launch["monitor"])
            current_alive = {
                "harness": harness_alive,
                "goose": goose_alive,
                "monitor": monitor_alive,
            }
            for role, alive in current_alive.items():
                if previously_alive[role] and not alive:
                    self.record_process_exit_observation(role, launch[role]["pid"])
            previously_alive = current_alive
            if goose_alive and not harness_alive:
                raise ClosureError("authenticated harness exited while Goose remains alive")
            if not monitor_alive:
                self.monitor_terminal_row()
            if not harness_alive and not goose_alive and not monitor_alive:
                return self.terminal_evidence(launch)
            now = time.monotonic()
            if now - last_report >= 60:
                self.events.emit(
                    "waiting_for_terminal",
                    harness_alive=harness_alive,
                    goose_alive=goose_alive,
                    monitor_alive=monitor_alive,
                )
                last_report = now
            time.sleep(poll_seconds)

    def terminal_evidence(self, launch: dict[str, Any]) -> dict[str, Any]:
        run_rows = read_jsonl(self.run_dir / "run.jsonl")
        started = [row for row in run_rows if row.get("event") == "run_started"]
        finished = [row for row in run_rows if row.get("event") == "run_finished"]
        if len(started) != 1 or len(finished) != 1:
            raise ClosureError("run log lacks exactly one run_started and one run_finished")
        if started[0].get("run_id") != self.config["expected"]["run_id"]:
            raise ClosureError("run_started id differs from the launch receipt")
        monitor_terminal_row = self.monitor_terminal_row()
        auto_verdict_path = self.run_dir / "verdict.json"
        aggregate_path = self.live_root / f"{self.config['expected']['entrant']}.json"
        if not auto_verdict_path.is_file() or not aggregate_path.is_file():
            raise ClosureError("harness exited without both raw auto-verdict artifacts")
        auto_verdict = read_json(auto_verdict_path)
        aggregate = read_json(aggregate_path)
        if not isinstance(aggregate, list) or len(aggregate) != 1:
            raise ClosureError("harness aggregate must contain exactly rep 0")
        if canonical_json(aggregate[0]) != canonical_json(auto_verdict):
            raise ClosureError("raw auto-verdict and harness aggregate differ")
        seed = auto_verdict.get("fixture_seed")
        if not isinstance(seed, str) or not SEED_RE.fullmatch(seed):
            raise ClosureError("raw auto-verdict lacks an exact 16-hex fixture_seed")
        validate_sb7_score_payload(auto_verdict, self.config["expected"], seed)
        if (
            auto_verdict.get("entrant") != self.config["expected"]["entrant"]
            or auto_verdict.get("rep") != 0
            or auto_verdict.get("vendor_port") != self.config["expected"]["vendor_port"]
        ):
            raise ClosureError("raw auto-verdict run identity differs")
        agent = auto_verdict.get("agent") or {}
        if (
            agent.get("exit") != 0
            or agent.get("timed_out") is not False
            or not finite_number(agent.get("secs"), 0)
        ):
            raise ClosureError("Goose did not reach a natural successful process terminal")
        observed_pool = sorted(auto_verdict.get("actual_pool") or [])
        expected_pool = sorted(self.config["expected"]["models"])
        if observed_pool != expected_pool or auto_verdict.get("actual_nodes") != 3:
            raise ClosureError("raw auto-verdict fleet differs from the authenticated launch")
        harness_exit = self.record_process_exit_observation("harness", launch["harness"]["pid"])
        goose_exit = self.record_process_exit_observation("goose", launch["goose"]["pid"])
        monitor_exit = self.record_process_exit_observation("monitor", launch["monitor"]["pid"])
        evidence = {
            "schema_version": SCHEMA_VERSION,
            "captured_at": utc_now(),
            "harness_exit": {
                "pid": launch["harness"]["pid"],
                "observed_exited": True,
                "exit_code_observable": False,
                "observed_exited_at": harness_exit["observed_exited_at"],
                "success_inferred_from_authenticated_terminal_artifacts": True,
            },
            "goose_exit": {
                "pid": launch["goose"]["pid"],
                "exit_code": agent["exit"],
                "timed_out": agent["timed_out"],
                "observed_exited_at": goose_exit["observed_exited_at"],
            },
            "monitor_exit": {
                "pid": launch["monitor"]["pid"],
                "observed_exited_at": monitor_exit["observed_exited_at"],
                "exit_code_observable": False,
            },
            "monitor_terminal_sha256": sha256_bytes(canonical_json(monitor_terminal_row)),
            "run_started_sha256": sha256_bytes(canonical_json(started[0])),
            "run_finished_sha256": sha256_bytes(canonical_json(finished[0])),
            "run_started_at": started[0].get("ts") or started[0].get("at") or started[0].get("started_at"),
            "run_finished_at": finished[0].get("ts") or finished[0].get("at") or finished[0].get("finished_at"),
            "engine_events": len(run_rows),
            "fixture_seed": seed,
            "auto_verdict_sha256": sha256_file(auto_verdict_path),
            "aggregate_sha256": sha256_file(aggregate_path),
            "harness_log_sha256": sha256_file(self.live_root / "harness.log"),
        }
        if not evidence["run_started_at"] or not evidence["run_finished_at"]:
            raise ClosureError("terminal events lack timestamps")
        try:
            started_at = dt.datetime.fromisoformat(str(evidence["run_started_at"]).replace("Z", "+00:00"))
            finished_at = dt.datetime.fromisoformat(str(evidence["run_finished_at"]).replace("Z", "+00:00"))
        except ValueError as error:
            raise ClosureError("terminal event timestamps are not ISO datetimes") from error
        if started_at.utcoffset() is None or finished_at.utcoffset() is None:
            raise ClosureError("terminal event timestamps must include a UTC offset")
        if finished_at < started_at:
            raise ClosureError("run_finished predates run_started")
        evidence_path = self.state_dir / "terminal-evidence.json"
        if evidence_path.is_file():
            existing = read_json(evidence_path)
            comparable_existing = {key: value for key, value in existing.items() if key != "captured_at"}
            comparable_new = {key: value for key, value in evidence.items() if key != "captured_at"}
            if comparable_existing != comparable_new:
                raise ClosureError("terminal evidence differs from its durable receipt")
            return existing
        atomic_json(evidence_path, evidence)
        return evidence

    def monitor_terminal_row(self) -> dict[str, Any]:
        monitor_rows = read_jsonl(self.run_dir / ".swarm-monitor" / "watch.jsonl")
        if any(row.get("event") in {"incident_detected", "incident_captured"} for row in monitor_rows):
            raise ClosureError("v17 monitor recorded an incident")
        terminal = [
            row
            for row in monitor_rows
            if row.get("event") == "monitor_completed" and row.get("outcome") == "run_finished"
        ]
        if len(terminal) != 1:
            raise ClosureError("monitor lacks one durable run_finished completion")
        return terminal[0]

    def assert_live_processes_exited(self, launch: dict[str, Any]) -> None:
        for role in ("harness", "goose", "monitor"):
            if validate_authenticated_process(role, launch[role]):
                raise ClosureError(f"authenticated {role} is still live at the raw-tree seal boundary")

    def assert_no_open_tree_files(self) -> None:
        lsof = self.config["runtime"]["lsof"]
        completed = subprocess.run(
            [lsof, "+D", str(self.run_dir)],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if completed.returncode == 0:
            raise ClosureError("a process still has the raw build tree open")
        if completed.returncode not in {0, 1} or completed.stderr.strip():
            raise ClosureError("lsof could not positively establish a closed raw build tree")

    def seal_raw_tree(self) -> dict[str, Any]:
        seal_path = self.state_dir / "raw-tree-seal.json"
        if seal_path.is_file():
            seal = read_json(seal_path)
            current = tree_manifest(self.run_dir)
            if not manifests_equal(seal, current):
                raise ClosureError("raw build tree changed after it was sealed")
            return seal
        self.assert_no_open_tree_files()
        first = tree_manifest(self.run_dir)
        time.sleep(float(self.config.get("seal_settle_seconds", 2)))
        second = tree_manifest(self.run_dir)
        if not manifests_equal(first, second):
            raise ClosureError("raw build tree did not remain stable across the sealing interval")
        atomic_json(seal_path, second)
        return second

    def clone_for_attempt(self, attempt_dir: pathlib.Path, raw_seal: dict[str, Any]) -> pathlib.Path:
        if not path_is_within(attempt_dir, self.state_dir / "scoring"):
            raise ClosureError("scoring attempt escaped the private closure state")
        clone = attempt_dir / "tree"
        ensure_secure_dir(attempt_dir)
        if clone.exists():
            if clone.is_symlink() or not clone.is_dir():
                raise ClosureError("disposable scoring clone is not a real directory")
            existing = tree_manifest(clone)
            if manifests_equal(raw_seal, existing):
                return clone
            shutil.rmtree(clone)
        shutil.copytree(self.run_dir, clone, symlinks=True)
        if clone.is_symlink() or not clone.is_dir():
            raise ClosureError("disposable scoring clone creation was redirected")
        copied = tree_manifest(clone)
        if not manifests_equal(raw_seal, copied):
            raise ClosureError("disposable scoring clone differs from the raw seal")
        atomic_json(attempt_dir / "clone-seal.json", copied)
        return clone

    def successful_score_attempt(
        self, attempt: pathlib.Path
    ) -> tuple[pathlib.Path, dict[str, Any]] | None:
        result_path = attempt / "worker-result.json"
        score_path = attempt / "raw-score.json"
        seal_path = attempt / "score-tree-seal.json"
        clone = attempt / "tree"
        if any(path.is_symlink() for path in (result_path, score_path, seal_path)):
            raise ClosureError("successful scoring evidence contains a symbolic link")
        if clone.is_symlink() or not clone.is_dir():
            raise ClosureError("successful scoring clone is not a real directory")
        if not result_path.is_file() or not score_path.is_file() or not seal_path.is_file():
            return None
        result = read_json(result_path)
        if (
            result.get("exit_code") != 0
            or result.get("scorer_exit_code") != 0
            or result.get("descendants_clean") is not True
            or result.get("fixture_seed") is None
            or result.get("port") != self.config["expected"]["vendor_port"]
            or result.get("scorer_sha256")
            != self.config["expected"]["instrument_files"][
                "evals/swarm-bench/bench/score_sb7.py"
            ]
            or result.get("raw_tree_sha256")
            != read_json(self.state_dir / "raw-tree-seal.json").get("tree_sha256")
            or sha256_file(score_path) != result.get("score_sha256")
            or sha256_file(seal_path) != result.get("score_tree_seal_sha256")
        ):
            return None
        seal = read_json(seal_path)
        if seal.get("tree_sha256") != result.get("score_tree_sha256"):
            return None
        if not manifests_equal(seal, tree_manifest(clone)):
            raise ClosureError("successful scoring clone changed after its evidence seal")
        return score_path, result

    def successful_score(self) -> tuple[pathlib.Path, dict[str, Any]] | None:
        score_root = self.state_dir / "scoring"
        if not score_root.is_dir():
            return None
        for attempt in sorted(score_root.glob("attempt-*")):
            success = self.successful_score_attempt(attempt)
            if success is not None:
                return success
        return None

    def start_score_attempt(
        self,
        attempt: int,
        terminal: dict[str, Any],
        raw_seal: dict[str, Any],
    ) -> tuple[pathlib.Path, dict[str, Any]]:
        attempt_dir = self.state_dir / "scoring" / f"attempt-{attempt}"
        clone = self.clone_for_attempt(attempt_dir, raw_seal)
        job = {
            "schema_version": SCHEMA_VERSION,
            "attempt": attempt,
            "clone": str(clone),
            "raw_tree": str(self.run_dir),
            "raw_tree_sha256": raw_seal["tree_sha256"],
            "seed": terminal["fixture_seed"],
            "scorer": str(
                self.live_root
                / "instrument"
                / "evals/swarm-bench/bench/score_sb7.py"
            ),
            "scorer_sha256": self.config["expected"]["instrument_files"][
                "evals/swarm-bench/bench/score_sb7.py"
            ],
            "port": self.config["expected"]["vendor_port"],
            "score_output": str(attempt_dir / "raw-score.json"),
            "score_log": str(attempt_dir / "score.log"),
            "result": str(attempt_dir / "worker-result.json"),
            "lock": self.config["score_lock_path"],
            "stop": str(self.stop_path),
            "timeout_seconds": self.config["score_timeout_seconds"],
            "instrument_root": str(self.live_root / "instrument"),
            "instrument_files": self.config["expected"]["instrument_files"],
            "render_node": self.config["publisher"]["node"],
            "render_node_sha256": self.config["publisher"]["node_sha256"],
            "score_contract": {
                "raw_scorer_version": self.config["expected"]["raw_scorer_version"],
                "check_count": self.config["expected"]["check_count"],
                "telemetry_nodes": self.config["expected"]["telemetry_nodes"],
            },
            "vendor_source": str(
                self.live_root
                / "instrument"
                / "evals/swarm-bench/bench/vendor_service_v3.py"
            ),
        }
        atomic_json(attempt_dir / "job.json", job)
        command = [
            sys.executable,
            "-B",
            "-u",
            str(pathlib.Path(__file__).resolve()),
            "score-worker",
            "--job",
            str(attempt_dir / "job.json"),
        ]
        descriptor = os.open(attempt_dir / "worker.log", os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
        with os.fdopen(descriptor, "ab", buffering=0) as worker_log:
            worker = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=worker_log,
                stderr=subprocess.STDOUT,
                env=safe_environment(),
                start_new_session=True,
            )
        receipt = safe_process_receipt(worker.pid)
        if receipt is None:
            raise ClosureError("score worker did not remain alive long enough to authenticate")
        atomic_json(attempt_dir / "worker.pid.json", receipt)
        self.events.emit("score_worker_started", attempt=attempt, pid=worker.pid)
        return attempt_dir, receipt

    def wait_score_worker(self, attempt_dir: pathlib.Path, receipt: dict[str, Any]) -> dict[str, Any]:
        result_path = attempt_dir / "worker-result.json"
        while True:
            self.check_stop()
            if result_path.is_file():
                return read_json(result_path)
            current = safe_process_receipt(receipt["pid"])
            if current is None or current["identity_sha256"] != receipt["identity_sha256"]:
                time.sleep(0.2)
                if result_path.is_file():
                    return read_json(result_path)
                scorer_state_path = attempt_dir / "scorer-state.json"
                scorer_pid_path = attempt_dir / "scorer.pid.json"
                scorer_state = read_json(scorer_state_path) if scorer_state_path.is_file() else {}
                scorer_receipt = read_json(scorer_pid_path) if scorer_pid_path.is_file() else None
                scorer_alive = False
                if scorer_receipt:
                    observed = safe_process_receipt(scorer_receipt["pid"])
                    scorer_alive = bool(
                        observed and observed["identity_sha256"] == scorer_receipt["identity_sha256"]
                    )
                pgid = scorer_state.get("process_group_id")
                group_alive = isinstance(pgid, int) and process_group_exists(pgid)
                if scorer_alive:
                    if pgid != scorer_receipt["pid"]:
                        raise ClosureError("orphaned scorer process-group identity changed")
                    self.events.emit("orphaned_scorer_rejected", attempt=attempt_dir.name)
                    terminate_process_group(pgid)
                elif group_alive:
                    self.events.emit(
                        "unowned_process_group_not_signalled",
                        attempt=attempt_dir.name,
                        process_group_id=pgid,
                    )
                recovered = {
                    "schema_version": SCHEMA_VERSION,
                    "attempt": int(attempt_dir.name.removeprefix("attempt-")),
                    "completed_at": utc_now(),
                    "exit_code": 125,
                    "failure": (
                        "score worker exited without a durable result; attempt rejected"
                        if not group_alive or scorer_alive
                        else "score worker exited and an unauthenticated process group remains; "
                        "attempt rejected without signalling it"
                    ),
                    "score_sha256": None,
                }
                atomic_json(result_path, recovered)
                return recovered
            time.sleep(float(self.config.get("poll_seconds", 10)))

    def authoritative_score(
        self,
        launch: dict[str, Any],
        terminal: dict[str, Any],
        raw_seal: dict[str, Any],
    ) -> tuple[pathlib.Path, dict[str, Any]]:
        successful = self.successful_score()
        if successful is None:
            max_attempts = int(self.config.get("max_score_attempts", 2))
            for attempt in range(1, max_attempts + 1):
                attempt_dir = self.state_dir / "scoring" / f"attempt-{attempt}"
                result_path = attempt_dir / "worker-result.json"
                if result_path.is_file():
                    result = read_json(result_path)
                    validated = self.successful_score_attempt(attempt_dir)
                    if validated is not None:
                        successful = validated
                        break
                    continue
                pid_path = attempt_dir / "worker.pid.json"
                if pid_path.is_file():
                    receipt = read_json(pid_path)
                    result = self.wait_score_worker(attempt_dir, receipt)
                else:
                    attempt_dir, receipt = self.start_score_attempt(attempt, terminal, raw_seal)
                    result = self.wait_score_worker(attempt_dir, receipt)
                if result.get("exit_code") == 0:
                    validated = self.successful_score_attempt(attempt_dir)
                    if validated is not None:
                        successful = validated
                        break
            if successful is None:
                raise ClosureError("authoritative scorer exhausted its bounded attempts")
        score_path, worker_result = successful
        score = read_json(score_path)
        self.validate_score(score, terminal)
        parent_owned = {"entrant", "rep", "agent", "actual_pool", "actual_nodes", "vendor_port", "closure"}
        overlap = sorted(parent_owned & set(score))
        if overlap:
            raise ClosureError(f"raw scorer attempted to supply parent-owned fields: {overlap}")
        if worker_result.get("fixture_seed") != terminal["fixture_seed"]:
            raise ClosureError("score worker receipt used a different fixture_seed")
        auto = read_json(self.run_dir / "verdict.json")
        safe_agent = {
            "exit": auto["agent"]["exit"],
            "timed_out": auto["agent"]["timed_out"],
            "secs": auto["agent"]["secs"],
        }
        score.update(
            {
                "entrant": auto["entrant"],
                "rep": auto["rep"],
                "agent": safe_agent,
                "actual_pool": auto["actual_pool"],
                "actual_nodes": auto["actual_nodes"],
                "vendor_port": auto["vendor_port"],
                "closure": {
                    "raw_tree_sha256": raw_seal["tree_sha256"],
                    "scorer_sha256": worker_result["scorer_sha256"],
                    "score_tree_sha256": worker_result["score_tree_sha256"],
                    "auto_verdict_sha256": terminal["auto_verdict_sha256"],
                },
            }
        )
        authoritative_path = self.state_dir / "authoritative-verdict.json"
        atomic_json(authoritative_path, score)
        provenance = {
            "schema_version": SCHEMA_VERSION,
            "fixture_seed": terminal["fixture_seed"],
            "raw_tree_sha256": raw_seal["tree_sha256"],
            "scorer_sha256": worker_result["scorer_sha256"],
            "score_tree_sha256": worker_result["score_tree_sha256"],
            "candidate_commit": launch["candidate"]["commit"],
            "engine_events": terminal["engine_events"],
            "run_started_at": terminal["run_started_at"],
            "run_finished_at": terminal["run_finished_at"],
            "authoritative_verdict_sha256": sha256_file(authoritative_path),
        }
        provenance_path = self.state_dir / "scoring-provenance.json"
        atomic_json(provenance_path, provenance)
        current_raw = tree_manifest(self.run_dir)
        if not manifests_equal(raw_seal, current_raw):
            raise ClosureError("raw tree changed during authoritative scoring")
        return authoritative_path, provenance

    def validate_score(self, score: dict[str, Any], terminal: dict[str, Any]) -> None:
        validate_sb7_score_payload(score, self.config["expected"], terminal["fixture_seed"])
        raw_score = read_json(self.run_dir / "verdict.json")
        raw_registry = [
            (row.get("check"), row.get("tier")) for row in raw_score.get("checks", [])
        ]
        authoritative_registry = [
            (row.get("check"), row.get("tier")) for row in score.get("checks", [])
        ]
        if authoritative_registry != raw_registry:
            raise ClosureError("authoritative scorer check registry differs from the raw auto-verdict")

    def publish_and_verify(
        self,
        authoritative_path: pathlib.Path,
        provenance: dict[str, Any],
    ) -> dict[str, Any]:
        receipt_path = self.state_dir / "publication-receipt.json"
        if receipt_path.is_file():
            return self.validate_publication_receipt(read_json(receipt_path))
        publisher = pathlib.Path(self.config["publisher"]["path"])
        score_success = self.successful_score()
        if score_success is None:
            raise ClosureError("publication requested without an authoritative score")
        score_path = score_success[0]
        clone = score_path.parent / "tree"
        score_tree_seal = read_json(score_path.parent / "score-tree-seal.json")
        if not manifests_equal(score_tree_seal, tree_manifest(clone)):
            raise ClosureError("scoring clone changed before publication")
        provenance_path = self.state_dir / "scoring-provenance.json"
        command = [
            self.config["publisher"]["node"],
            str(publisher),
            "--tree",
            str(clone),
            "--verdict",
            str(authoritative_path),
            "--provenance",
            str(provenance_path),
            "--receipt",
            str(receipt_path),
            "--state",
            str(self.state_dir / "publisher-state.json"),
            "--site-root",
            self.config["publisher"]["site_root"],
            "--env-file",
            self.config["publisher"]["env_file"],
            "--base-url",
            self.config["publisher"]["base_url"],
            "--package-lock",
            self.config["publisher"]["package_lock"],
            "--package-lock-sha256",
            self.config["publisher"]["package_lock_sha256"],
            "--package-json",
            self.config["publisher"]["package_json"],
            "--package-json-sha256",
            self.config["publisher"]["package_json_sha256"],
            "--live",
        ]
        log_path = self.state_dir / "publisher.log"
        pid_path = self.state_dir / "publisher.pid.json"
        max_attempts = int(self.config["max_publish_attempts"])
        existing_process = read_json(pid_path) if pid_path.is_file() else None
        first_attempt = 1
        if existing_process and isinstance(existing_process.get("attempt"), int):
            observed = safe_process_receipt(existing_process["pid"])
            active = bool(
                observed
                and observed["identity_sha256"] == existing_process.get("identity_sha256")
            )
            first_attempt = existing_process["attempt"] if active else existing_process["attempt"] + 1
        for attempt in range(first_attempt, max_attempts + 1):
            if receipt_path.is_file():
                break
            process_receipt = read_json(pid_path) if pid_path.is_file() else None
            current = None
            if process_receipt and process_receipt.get("attempt") == attempt:
                current = safe_process_receipt(process_receipt["pid"])
                if current and current["identity_sha256"] != process_receipt["identity_sha256"]:
                    raise ClosureError("publisher pid identity changed")
            if current is None:
                self.validate_frozen_inputs()
                descriptor = os.open(log_path, os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
                os.fchmod(descriptor, 0o600)
                with os.fdopen(descriptor, "ab", buffering=0) as log:
                    publisher_process = subprocess.Popen(
                        command,
                        stdin=subprocess.DEVNULL,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        env=safe_environment(),
                        start_new_session=True,
                    )
                process_receipt = safe_process_receipt(publisher_process.pid)
                if process_receipt is None:
                    time.sleep(0.2)
                    if receipt_path.is_file():
                        break
                    self.events.emit("publisher_attempt_exited", attempt=attempt)
                    continue
                started_epoch = time.time()
                process_receipt.update(
                    {
                        "attempt": attempt,
                        "process_group_id": publisher_process.pid,
                        "started_epoch": started_epoch,
                        "deadline_epoch": started_epoch + float(self.config["publish_timeout_seconds"]),
                    }
                )
                atomic_json(pid_path, process_receipt)
                self.events.emit("publisher_started", attempt=attempt, pid=publisher_process.pid)
            while not receipt_path.is_file():
                if self.stop_path.exists():
                    terminate_process_group(process_receipt.get("process_group_id"))
                    self.check_stop()
                current = safe_process_receipt(process_receipt["pid"])
                if current is None:
                    break
                if current["identity_sha256"] != process_receipt["identity_sha256"]:
                    raise ClosureError("publisher pid identity changed")
                if time.time() >= float(process_receipt["deadline_epoch"]):
                    terminate_process_group(process_receipt.get("process_group_id"))
                    self.events.emit("publisher_attempt_timed_out", attempt=attempt)
                    break
                time.sleep(float(self.config.get("poll_seconds", 10)))
        if not receipt_path.is_file():
            raise ClosureError("guarded publisher exhausted its bounded restart-safe attempts")
        if not manifests_equal(score_tree_seal, tree_manifest(clone)):
            raise ClosureError("scoring clone changed during publication")
        receipt = read_json(receipt_path)
        return self.validate_publication_receipt(receipt)

    def validate_publication_receipt(self, receipt: dict[str, Any]) -> dict[str, Any]:
        if receipt.get("target_document_id") != self.config["publication"]["target_document_id"]:
            raise ClosureError("publisher wrote the wrong target receipt")
        if set(receipt.get("protected_document_ids") or []) != set(
            self.config["publication"]["protected_document_ids"]
        ):
            raise ClosureError("publisher protected-document receipt differs")
        if receipt.get("protected_before_sha256") != receipt.get("protected_after_sha256"):
            raise ClosureError("protected document receipts changed")
        authoritative_path = self.state_dir / "authoritative-verdict.json"
        if receipt.get("authoritative_verdict_sha256") != sha256_file(authoritative_path):
            raise ClosureError("publisher receipt does not identify the authoritative verdict")
        if receipt.get("create_only") is not True:
            raise ClosureError("publisher receipt lacks its create-only guarantee")
        verification = receipt.get("verification") or {}
        if not (
            verification.get("zero_rc") is True
            and verification.get("board_json_ld") == "ItemList"
            and verification.get("run_json_ld") == "Dataset"
            and verification.get("asset_checks")
            and verification.get("telemetry_sha256")
            and verification.get("sanity_exact") is True
            and verification.get("screenshots_exact") is True
            and verification.get("no_cache_requests") is True
        ):
            raise ClosureError("publication receipt lacks full rendered verification")
        return receipt

    def run(self) -> int:
        execution_lock = os.open(
            self.state_dir / "supervisor-execution.lock", os.O_CREAT | os.O_RDWR, 0o600
        )
        os.fchmod(execution_lock, 0o600)
        try:
            fcntl.flock(execution_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            os.close(execution_lock)
            raise ClosureError("another authenticated closure supervisor is already running") from error
        try:
            return self._run_locked()
        finally:
            fcntl.flock(execution_lock, fcntl.LOCK_UN)
            os.close(execution_lock)

    def _run_locked(self) -> int:
        self.checkpoint("validating")
        launch, _manifest = self.validate_frozen_inputs()
        terminal_path = self.state_dir / "terminal-evidence.json"
        if terminal_path.is_file():
            self.assert_live_processes_exited(launch)
            terminal = self.terminal_evidence(launch)
        else:
            self.checkpoint("waiting_for_terminal")
            terminal = self.wait_for_terminal(launch)
        self.assert_live_processes_exited(launch)
        self.validate_frozen_inputs()
        self.checkpoint("terminal_authenticated")
        raw_seal = self.seal_raw_tree()
        self.checkpoint("raw_tree_sealed", raw_tree_sha256=raw_seal["tree_sha256"])
        self.validate_frozen_inputs()
        authoritative_path, provenance = self.authoritative_score(
            launch, terminal, raw_seal
        )
        self.validate_frozen_inputs()
        self.checkpoint(
            "scored",
            authoritative_verdict_sha256=sha256_file(authoritative_path),
        )
        receipt = self.publish_and_verify(authoritative_path, provenance)
        self.validate_frozen_inputs()
        self.terminal_evidence(launch)
        if not manifests_equal(raw_seal, tree_manifest(self.run_dir)):
            raise ClosureError("raw build tree changed during publication")
        final = {
            "schema_version": SCHEMA_VERSION,
            "completed_at": utc_now(),
            "target_document_id": receipt["target_document_id"],
            "raw_tree_sha256": raw_seal["tree_sha256"],
            "authoritative_verdict_sha256": sha256_file(authoritative_path),
            "publication_receipt_sha256": sha256_file(
                self.state_dir / "publication-receipt.json"
            ),
            "protected_documents_unchanged": True,
            "rendered_verified": True,
            "fixture_seed": terminal["fixture_seed"],
            "scorer_sha256": provenance["scorer_sha256"],
            "score_tree_sha256": provenance["score_tree_sha256"],
        }
        atomic_json(self.state_dir / "result.json", final)
        self.checkpoint("complete", result=final)
        self.events.emit("closure_complete", **final)
        return 0


def local_vendor_secret(path: pathlib.Path) -> str | None:
    text = path.read_text(encoding="utf-8")
    match = re.search(r"^API_KEY\s*=\s*['\"]([^'\"]+)['\"]", text, re.M)
    return match.group(1) if match else None


def port_is_available(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            listener.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


def process_group_exists(process_group_id: int) -> bool:
    try:
        os.killpg(process_group_id, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def terminate_process_group(process_group_id: Any, grace_seconds: float = 10) -> None:
    if not isinstance(process_group_id, int) or process_group_id <= 1:
        return
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process_group_id, signal.SIGTERM)
    deadline = time.monotonic() + grace_seconds
    while process_group_exists(process_group_id) and time.monotonic() < deadline:
        time.sleep(0.1)
    if process_group_exists(process_group_id):
        with contextlib.suppress(ProcessLookupError):
            os.killpg(process_group_id, signal.SIGKILL)


def verify_instrument_inventory(job: Mapping[str, Any]) -> None:
    root = pathlib.Path(str(job["instrument_root"]))
    if root.is_symlink() or not root.is_dir():
        raise ClosureError("score instrument root is not a real directory")
    files = job.get("instrument_files")
    if not isinstance(files, dict) or not files:
        raise ClosureError("score job has no frozen instrument inventory")
    for relative, digest in files.items():
        path = root / relative
        if path.is_symlink() or not path.is_file() or sha256_file(path) != digest:
            raise ClosureError(f"score worker frozen instrument changed: {relative}")
        if path.stat().st_mode & 0o222:
            raise ClosureError(f"score worker frozen instrument became writable: {relative}")


def drain_redacted_output(
    stream: Any,
    output: Any,
    secrets: Sequence[str],
    outcome: dict[str, Any],
) -> None:
    try:
        for line in iter(stream.readline, b""):
            output.write(redact_text(line.decode("utf-8", "replace"), secrets).encode())
            output.flush()
    except BaseException as error:
        outcome["error"] = f"{type(error).__name__}: {redact_text(error, secrets)}"
    finally:
        with contextlib.suppress(Exception):
            stream.close()


def acquire_score_lock(descriptor: int, stop_path: pathlib.Path) -> bool:
    while True:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except BlockingIOError:
            if stop_path.exists():
                return False
            time.sleep(1)


def score_worker_impl(job: Mapping[str, Any]) -> int:
    result_path = pathlib.Path(str(job["result"]))
    if result_path.is_symlink():
        raise ClosureError("score worker result is symbolic")
    if result_path.is_file():
        exit_code = read_json(result_path).get("exit_code")
        return int(exit_code) if isinstance(exit_code, int) else 1
    scorer = pathlib.Path(str(job["scorer"]))
    clone = pathlib.Path(str(job["clone"]))
    stop_path = pathlib.Path(str(job["stop"]))
    if clone.is_symlink() or not clone.is_dir():
        raise ClosureError("score worker clone is not a real directory")
    if clone.parent.resolve() != result_path.parent.resolve():
        raise ClosureError("score worker clone escaped its private attempt")
    for output_name in ("score_output", "score_log"):
        output = pathlib.Path(str(job[output_name]))
        if output.parent.resolve() != result_path.parent.resolve() or output.is_symlink():
            raise ClosureError(f"score worker {output_name} escaped its private attempt")
    if sha256_file(scorer) != job["scorer_sha256"]:
        raise ClosureError("score worker scorer hash changed")
    verify_instrument_inventory(job)
    render_node = pathlib.Path(str(job["render_node"]))
    if render_node.is_symlink() or sha256_file(render_node) != job["render_node_sha256"]:
        raise ClosureError("score worker render Node runtime changed")
    if tree_manifest(clone)["tree_sha256"] != job["raw_tree_sha256"]:
        raise ClosureError("score worker clone differs from the raw seal")
    seed = str(job["seed"])
    if not SEED_RE.fullmatch(seed):
        raise ClosureError("score worker seed is invalid")
    lock_path = pathlib.Path(str(job["lock"]))
    if lock_path.is_symlink():
        raise ClosureError("serial scorer lock is symbolic")
    ensure_secure_dir(lock_path.parent)
    lock_descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o600)
    os.fchmod(lock_descriptor, 0o600)
    if not acquire_score_lock(lock_descriptor, stop_path):
        result = {
            "schema_version": SCHEMA_VERSION,
            "attempt": job["attempt"],
            "completed_at": utc_now(),
            "exit_code": 75,
            "failure": "closure stop requested before the scorer lock was acquired",
            "score_sha256": None,
        }
        atomic_json(result_path, result)
        os.close(lock_descriptor)
        return 75
    try:
        port = int(job["port"])
        if port != 18970 or not port_is_available(port):
            raise ClosureError(f"scoring port {port} is not isolated")
        attempt_dir = result_path.parent
        runtime_root = attempt_dir / "runtime"
        runtime_home = runtime_root / "home"
        runtime_tmp = runtime_root / "tmp"
        ensure_secure_dir(runtime_home)
        ensure_secure_dir(runtime_tmp)
        score_environment = safe_environment(
            {
                "HOME": str(runtime_home),
                "TMPDIR": str(runtime_tmp),
                "GOOSE_SWARM_RENDER_NODE": str(render_node),
            }
        )
        command = [
            sys.executable,
            "-B",
            "-u",
            str(scorer),
            "--tree",
            str(clone),
            "--port",
            str(port),
            "--seed",
            seed,
            "--json-out",
            str(job["score_output"]),
        ]
        secret = local_vendor_secret(pathlib.Path(str(job["vendor_source"])))
        secrets = [secret] if secret else []
        log_path = pathlib.Path(str(job["score_log"]))
        log_descriptor = os.open(log_path, os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
        os.fchmod(log_descriptor, 0o600)
        with os.fdopen(log_descriptor, "ab", buffering=0) as score_log:
            scorer_process = subprocess.Popen(
                command,
                cwd=scorer.parent,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                env=score_environment,
                start_new_session=True,
            )
            started_epoch = time.time()
            deadline_epoch = started_epoch + float(job["timeout_seconds"])
            scorer_receipt = safe_process_receipt(scorer_process.pid)
            scorer_state = {
                "schema_version": SCHEMA_VERSION,
                "pid": scorer_process.pid,
                "process_group_id": scorer_process.pid,
                "started_at": utc_now(),
                "started_epoch": started_epoch,
                "deadline_epoch": deadline_epoch,
            }
            atomic_json(attempt_dir / "scorer-state.json", scorer_state)
            if scorer_receipt is not None:
                atomic_json(attempt_dir / "scorer.pid.json", scorer_receipt)
            reader_outcome: dict[str, Any] = {}
            if scorer_process.stdout is None:
                raise ClosureError("score worker did not receive the scorer output channel")
            reader = threading.Thread(
                target=drain_redacted_output,
                args=(scorer_process.stdout, score_log, secrets, reader_outcome),
                daemon=True,
            )
            reader.start()
            termination_reason: str | None = None
            while scorer_process.poll() is None:
                if stop_path.exists():
                    termination_reason = "closure stop requested during authoritative scoring"
                    break
                if time.time() >= deadline_epoch:
                    termination_reason = "authoritative scorer exceeded its frozen timeout"
                    break
                time.sleep(0.5)
            if termination_reason:
                terminate_process_group(scorer_process.pid)
            scorer_exit = scorer_process.wait()
            reader.join(timeout=5)
            had_survivors = process_group_exists(scorer_process.pid)
            if had_survivors:
                terminate_process_group(scorer_process.pid)
            if reader.is_alive():
                raise ClosureError("score output channel did not reach EOF")
            if reader_outcome.get("error"):
                raise ClosureError(f"score output capture failed: {reader_outcome['error']}")
            score_log.flush()
            os.fsync(score_log.fileno())
        verify_instrument_inventory(job)
        if sha256_file(render_node) != job["render_node_sha256"]:
            raise ClosureError("render Node runtime changed during authoritative scoring")
        accepted_exit = scorer_exit
        failure = termination_reason
        if termination_reason:
            accepted_exit = 75 if stop_path.exists() else 124
        if had_survivors:
            accepted_exit = 70
            failure = "authoritative scorer left descendant processes; attempt rejected"
        score_output = pathlib.Path(str(job["score_output"]))
        score_tree_seal_path = result_path.parent / "score-tree-seal.json"
        score_tree_sha256 = None
        if accepted_exit == 0 and score_output.is_file():
            score_payload = read_json(score_output)
            if not isinstance(score_payload, dict):
                raise ClosureError("authoritative scorer output is not an object")
            validate_sb7_score_payload(score_payload, job["score_contract"], seed)
            forbidden = {"entrant", "rep", "agent", "actual_pool", "actual_nodes", "vendor_port", "closure"}
            if forbidden & set(score_payload):
                raise ClosureError("authoritative scorer supplied parent-owned publication fields")
            score_tree_seal = tree_manifest(clone)
            atomic_json(score_tree_seal_path, score_tree_seal)
            score_tree_sha256 = score_tree_seal["tree_sha256"]
        result = {
            "schema_version": SCHEMA_VERSION,
            "attempt": job["attempt"],
            "completed_at": utc_now(),
            "exit_code": accepted_exit,
            "scorer_exit_code": scorer_exit,
            "failure": failure,
            "scorer_sha256": job["scorer_sha256"],
            "raw_tree_sha256": job["raw_tree_sha256"],
            "fixture_seed": seed,
            "port": port,
            "descendants_clean": not had_survivors,
            "score_sha256": sha256_file(score_output)
            if accepted_exit == 0 and score_output.is_file()
            else None,
            "score_tree_sha256": score_tree_sha256,
            "score_tree_seal_sha256": sha256_file(score_tree_seal_path)
            if score_tree_seal_path.is_file()
            else None,
            "log_sha256": sha256_file(log_path),
        }
        atomic_json(result_path, result)
        return accepted_exit
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)


def score_worker(job_path: pathlib.Path) -> int:
    os.umask(0o077)
    job: Mapping[str, Any] | None = None
    try:
        if job_path.is_symlink() or not job_path.is_file():
            raise ClosureError("score worker job is not a regular file")
        job = read_json(job_path.resolve())
        return score_worker_impl(job)
    except BaseException as error:
        if job is not None and isinstance(job.get("result"), str):
            result_path = pathlib.Path(str(job["result"]))
            atomic_json(
                result_path,
                {
                    "schema_version": SCHEMA_VERSION,
                    "attempt": job.get("attempt"),
                    "completed_at": utc_now(),
                    "exit_code": 70,
                    "failure": redact_text(error),
                    "score_sha256": None,
                },
            )
        return 70


def snapshot_instrument(source_config_path: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    source_config = load_config(source_config_path)
    validate_config(source_config)
    state_dir = pathlib.Path(source_config["state_dir"]).resolve()
    ensure_secure_dir(state_dir)
    instrument = state_dir / "closure-instrument"
    if instrument.is_symlink() or (instrument.exists() and not instrument.is_dir()):
        raise ClosureError("closure instrument target is not a real directory")
    script_source = pathlib.Path(__file__).resolve()
    publisher_source = pathlib.Path(source_config["publisher"]["path"]).resolve()
    script_target = instrument / "terminal_closure.py"
    publisher_target = instrument / "seed-fleet-brainwaves-sb70.mjs"
    config_target = instrument / "config.json"
    if not instrument.exists():
        if sha256_file(script_source) != source_config["controller_sha256"]:
            raise ClosureError("controller hash changed before closure snapshot")
        if sha256_file(publisher_source) != source_config["publisher"]["sha256"]:
            raise ClosureError("publisher hash changed before closure snapshot")
        temporary = state_dir / (
            f".closure-instrument.{os.getpid()}.{os.urandom(6).hex()}.tmp"
        )
        ensure_secure_dir(temporary)
        temporary_script = temporary / script_target.name
        temporary_publisher = temporary / publisher_target.name
        temporary_config = temporary / config_target.name
        atomic_write(temporary_script, script_source.read_bytes(), 0o500)
        atomic_write(temporary_publisher, publisher_source.read_bytes(), 0o500)
        frozen_config = json.loads(json.dumps(source_config))
        frozen_config["publisher"]["path"] = str(publisher_target.resolve())
        atomic_json(temporary_config, frozen_config, 0o400)
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "created_at": utc_now(),
            "files": {
                script_target.name: sha256_file(temporary_script),
                publisher_target.name: sha256_file(temporary_publisher),
                config_target.name: sha256_file(temporary_config),
            },
        }
        atomic_json(temporary / "manifest.json", manifest, 0o400)
        os.replace(temporary, instrument)
        directory = os.open(state_dir, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    else:
        manifest_path = instrument / "manifest.json"
        if not manifest_path.is_file():
            raise ClosureError("closure instrument snapshot is incomplete")
        manifest = read_json(manifest_path)
        if set(manifest.get("files") or {}) != {
            script_target.name,
            publisher_target.name,
            config_target.name,
        }:
            raise ClosureError("closure instrument inventory changed")
        for name, digest in manifest["files"].items():
            path = instrument / name
            if path.is_symlink() or not path.is_file() or sha256_file(path) != digest:
                raise ClosureError(f"closure instrument changed: {name}")
    return script_target, config_target


def spawn_supervisor(config_path: pathlib.Path, resume: bool) -> int:
    source_config = load_config(config_path)
    validate_config(source_config)
    state_dir = pathlib.Path(source_config["state_dir"]).resolve()
    ensure_secure_dir(state_dir)
    lock_path = state_dir / "supervisor.lock"
    lock_descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o600)
    os.fchmod(lock_descriptor, 0o600)
    fcntl.flock(lock_descriptor, fcntl.LOCK_EX)
    try:
        script, frozen_config = snapshot_instrument(config_path)
        stop_path = state_dir / "STOP"
        if resume and stop_path.exists():
            stop_path.unlink()
        pid_path = state_dir / "supervisor.pid.json"
        if pid_path.is_file():
            receipt = read_json(pid_path)
            current = safe_process_receipt(receipt["pid"])
            if current and current["identity_sha256"] == receipt["identity_sha256"]:
                print(f"closure already running pid={receipt['pid']}")
                return 0
        descriptor = os.open(state_dir / "closure.log", os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600)
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "ab", buffering=0) as log:
            process = subprocess.Popen(
                [sys.executable, "-B", "-u", str(script), "run", "--config", str(frozen_config)],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                env=safe_environment(),
                start_new_session=True,
            )
        receipt = safe_process_receipt(process.pid)
        if receipt is None:
            raise ClosureError("closure supervisor exited during detached launch")
        atomic_json(pid_path, receipt)
        print(f"closure started pid={process.pid} state={state_dir}")
        return 0
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)


def status(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    state_dir = pathlib.Path(config["state_dir"])
    state = read_json(state_dir / "state.json") if (state_dir / "state.json").is_file() else {}
    receipt = read_json(state_dir / "supervisor.pid.json") if (state_dir / "supervisor.pid.json").is_file() else None
    running = False
    if receipt:
        current = safe_process_receipt(receipt["pid"])
        running = bool(current and current["identity_sha256"] == receipt["identity_sha256"])
    print(
        json.dumps(
            {
                "running": running,
                "pid": receipt.get("pid") if receipt else None,
                "phase": state.get("phase", "not-started"),
                "updated_at": state.get("updated_at"),
                "state_dir": str(state_dir),
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0 if running or state.get("phase") in TERMINAL_PHASES else 1


def watch(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    state_dir = pathlib.Path(config["state_dir"])
    events_path = state_dir / "events.jsonl"
    offset = 0
    while True:
        if events_path.is_file():
            with events_path.open(encoding="utf-8") as handle:
                handle.seek(offset)
                for line in handle:
                    print(line, end="", flush=True)
                offset = handle.tell()
        state = read_json(state_dir / "state.json") if (state_dir / "state.json").is_file() else {}
        if state.get("phase") in TERMINAL_PHASES:
            return 0 if state.get("phase") == "complete" else 1
        time.sleep(2)


def results(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    result_path = pathlib.Path(config["state_dir"]) / "result.json"
    if not result_path.is_file():
        print("closure result is not available")
        return 1
    print(json.dumps(read_json(result_path), indent=2, sort_keys=True))
    return 0


def stop(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    stop_path = pathlib.Path(config["state_dir"]) / "STOP"
    atomic_write(stop_path, (utc_now() + "\n").encode())
    print("closure stop requested; the live v17 run will not be signalled")
    return 0


def preflight(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    validate_config(config)
    audit = object.__new__(TerminalClosure)
    audit.config = config
    audit.live_root = pathlib.Path(config["live_root"]).resolve()
    audit.run_dir = pathlib.Path(config["run_dir"]).resolve()
    launch, manifest = TerminalClosure.validate_frozen_inputs(audit)
    processes = {
        role: validate_authenticated_process(role, launch[role])
        for role in ("harness", "goose", "monitor")
    }
    print(
        json.dumps(
            {
                "schema_version": SCHEMA_VERSION,
                "ready": True,
                "live_root_read_only": True,
                "run_id": config["expected"]["run_id"],
                "target_document_id": config["publication"]["target_document_id"],
                "protected_document_ids": config["publication"]["protected_document_ids"],
                "vendor_port": config["expected"]["vendor_port"],
                "instrument_files": len(manifest["files"]),
                "authenticated_processes_alive": processes,
                "state_dir": config["state_dir"],
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)
    for name in ("preflight", "start", "resume", "status", "watch", "results", "stop", "run"):
        sub = commands.add_parser(name)
        sub.add_argument("--config", type=pathlib.Path, required=True)
    worker = commands.add_parser("score-worker")
    worker.add_argument("--job", type=pathlib.Path, required=True)
    return root


def main(argv: Sequence[str] | None = None) -> int:
    os.umask(0o077)
    args = parser().parse_args(argv)
    if args.command == "preflight":
        return preflight(args.config)
    if args.command == "start":
        return spawn_supervisor(args.config, resume=False)
    if args.command == "resume":
        return spawn_supervisor(args.config, resume=True)
    if args.command == "status":
        return status(args.config)
    if args.command == "watch":
        return watch(args.config)
    if args.command == "results":
        return results(args.config)
    if args.command == "stop":
        return stop(args.config)
    if args.command == "score-worker":
        return score_worker(args.job)
    closure = TerminalClosure(args.config)
    try:
        return closure.run()
    except SystemExit:
        raise
    except BaseException as error:
        message = redact_text(error)
        atomic_json(
            closure.state_dir / "failure.json",
            {
                "schema_version": SCHEMA_VERSION,
                "failed_at": utc_now(),
                "error_type": type(error).__name__,
                "message": message,
            },
        )
        closure.checkpoint("failed", failure=message)
        closure.events.emit("closure_failed", error_type=type(error).__name__, message=message)
        print(f"closure failed: {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
