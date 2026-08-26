#!/usr/bin/env python3
"""Create an append-only terminal-closure source/config pair for one live V21 successor."""

from __future__ import annotations

import argparse
import ctypes
import errno
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
from typing import Any, Mapping, Sequence


SCHEMA_VERSION = 1
SUCCESSOR_INSTRUMENT_MANIFEST_SCHEMA_VERSION = 2
START_MARKER = "# BEGIN TERMINAL_CLOSURE_RUN_BINDING"
END_MARKER = "# END TERMINAL_CLOSURE_RUN_BINDING"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
RUN_ID_RE = re.compile(r"^swarm-\d{8}-\d{9}$")
GENERATION_RE = re.compile(r"^v21-r(?:[5-9]|[1-9]\d+)$")
TARGET_DOCUMENT_ID = "brun-fleet-qwen38-brainwaves-sb70"
PROTECTED_DOCUMENT_ID_ORDER = (
    "brun-fleet-qwen38-sb70",
    "brun-fleet-qwen-sb70",
)
PROTECTED_DOCUMENT_IDS = frozenset(PROTECTED_DOCUMENT_ID_ORDER)
EXACT_MODEL_ALIASES = frozenset(
    {
        "gabee-qwen3.8-27b-brainwaves-mxfp8-mlx",
        "mihai-qwen3.8-27b-brainwaves-mxfp8-mlx",
        "workhorse-qwen3.8-27b-brainwaves-mxfp8-mlx",
    }
)
EXACT_CONTEXT_BY_ROLE = {"local": 262_144, "workhorse": 262_144, "mac": 135_936}
ROLE_PREFIXES = {"local": "mihai-", "workhorse": "workhorse-", "mac": "gabee-"}
EXPECTED_ARTIFACT_PATH_SHA256 = (
    "a08b6e855ac5fc1045921c633a902189d73be4584ee685cd4a49834d6850b136"
)
EXPECTED_QUANTIZATION_SHA256 = (
    "267f67a60ca5f10733c610ff4be23d9bf6107a66de92ba1f37757c1d8aaec767"
)
EXPECTED_PLANNER_MODEL = "workhorse-qwen3.8-27b-brainwaves-mxfp8-mlx"
APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION = {
    "v21-r5": "0bfa5e8d13708919d69c1cdd26f52baa77b2f47da0ef7a41818477bec3fe3e04",
}
APPROVED_TERMINAL_PREDECESSOR_CONFIG_SHA256_BY_GENERATION = {
    "v21-r5": "5941e31cf886ed3ddaab649fe18961c7e68fbb2af38a495a300fa58355735891",
}
APPROVED_CONTROLLER_SOURCE_SHA256 = (
    "c7608f873a925f54693058af139bd8ae99395f9902c21cee4e868e6853662ec6"
)
APPROVED_USAGE_POLICY_SOURCE_SHA256 = (
    "8363461152fa30c6b48c97e142e06d8eca1fc61b1a9c639b5b7abd27a2cb9d2c"
)
APPROVED_PUBLISHER_SOURCE_SHA256 = (
    "aacd87a9e727ef20f23aa629ca3b08eb4e4888c7c179e7689f4bc1e20d7efb00"
)
APPROVED_PUBLISHER_COMMIT = "1561e4602bc9be5a256d5cf9acf345c0d5b940cd"
SCORE_ADOPTION_SOURCE_FILES = (
    "config.json",
    "failure.json",
    "raw-tree-seal.json",
    "terminal-evidence.json",
    "usage-contract.json",
    "successor-binding.json",
    "closure-instrument/terminal_closure.py",
    "scoring/attempt-1/job.json",
    "scoring/attempt-1/worker-result.json",
    "scoring/attempt-1/raw-score.json",
    "scoring/attempt-1/clone-seal.json",
    "scoring/attempt-1/descendants.json",
    "scoring/attempt-1/spawn-journal.txt",
    "scoring/attempt-1/score.log",
    "scoring/attempt-1/scorer-state.json",
    "scoring/attempt-1/scorer.pid.json",
    "scoring/attempt-1/worker.pid.json",
    "scoring/attempt-1/runtime/playwright-node",
)
PUBLISHER_ADOPTION_SOURCE_FILES = (
    "config.json",
    "state.json",
    "publisher-state.json",
    "publisher.pid.json",
    "supervisor.pid.json",
    "authoritative-verdict.json",
    "scoring-provenance.json",
    "closure-instrument/seed-fleet-brainwaves-sb70.mjs",
)


class SuccessorBindingError(RuntimeError):
    pass


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def stable_file_sha256(
    path: pathlib.Path,
    *,
    read_only: bool = False,
    maximum_bytes: int = 512 * 1024 * 1024,
) -> tuple[str, int]:
    require_regular(path, read_only=read_only)
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise SuccessorBindingError(f"binding input is not regular: {path}")
        if read_only and stat.S_IMODE(before.st_mode) & 0o222:
            raise SuccessorBindingError(f"binding input is writable: {path}")
        if before.st_size > maximum_bytes:
            raise SuccessorBindingError(f"binding input exceeds its size bound: {path}")
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(descriptor, 1024 * 1024)
            if not block:
                break
            digest.update(block)
            size += len(block)
            if size > maximum_bytes:
                raise SuccessorBindingError(
                    f"binding input exceeds its size bound: {path}"
                )
        after_descriptor = os.fstat(descriptor)
        after_path = path.lstat()
    finally:
        os.close(descriptor)
    identity = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
    )
    if identity != (
        after_descriptor.st_dev,
        after_descriptor.st_ino,
        after_descriptor.st_mode,
        after_descriptor.st_size,
        after_descriptor.st_mtime_ns,
    ) or identity != (
        after_path.st_dev,
        after_path.st_ino,
        after_path.st_mode,
        after_path.st_size,
        after_path.st_mtime_ns,
    ):
        raise SuccessorBindingError(f"binding input changed while read: {path}")
    return digest.hexdigest(), size


def decode_json(payload: bytes, path: pathlib.Path) -> dict[str, Any]:
    def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise SuccessorBindingError(f"{path} repeats JSON key {key!r}")
            value[key] = item
        return value

    try:
        value = json.loads(payload, object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SuccessorBindingError(f"could not read exact JSON evidence: {path}") from error
    if not isinstance(value, dict):
        raise SuccessorBindingError(f"JSON evidence is not an object: {path}")
    return value


def read_stable_bytes(
    path: pathlib.Path,
    *,
    read_only: bool = False,
    maximum_bytes: int = 64 * 1024 * 1024,
) -> bytes:
    require_regular(path, read_only=read_only)
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise SuccessorBindingError(f"binding input is not regular: {path}")
        if read_only and stat.S_IMODE(before.st_mode) & 0o222:
            raise SuccessorBindingError(f"binding input is writable: {path}")
        if before.st_size > maximum_bytes:
            raise SuccessorBindingError(f"binding input exceeds its size bound: {path}")
        chunks: list[bytes] = []
        size = 0
        while True:
            block = os.read(descriptor, min(1024 * 1024, maximum_bytes + 1 - size))
            if not block:
                break
            chunks.append(block)
            size += len(block)
            if size > maximum_bytes:
                raise SuccessorBindingError(f"binding input exceeds its size bound: {path}")
        after_descriptor = os.fstat(descriptor)
        after_path = path.lstat()
    finally:
        os.close(descriptor)
    identity = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
    )
    if identity != (
        after_descriptor.st_dev,
        after_descriptor.st_ino,
        after_descriptor.st_mode,
        after_descriptor.st_size,
        after_descriptor.st_mtime_ns,
    ) or identity != (
        after_path.st_dev,
        after_path.st_ino,
        after_path.st_mode,
        after_path.st_size,
        after_path.st_mtime_ns,
    ):
        raise SuccessorBindingError(f"binding input changed while read: {path}")
    return b"".join(chunks)


def read_json(path: pathlib.Path, *, read_only: bool = False) -> dict[str, Any]:
    return decode_json(read_stable_bytes(path, read_only=read_only), path)


def require_regular(path: pathlib.Path, *, read_only: bool = False) -> pathlib.Path:
    if path.is_symlink() or not path.is_file():
        raise SuccessorBindingError(f"binding input is missing or linked: {path}")
    if read_only and stat.S_IMODE(path.stat().st_mode) & 0o222:
        raise SuccessorBindingError(f"binding input is writable: {path}")
    return path.resolve()


def path_is_within(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def stable_tree_content_sha256(root: pathlib.Path) -> str:
    if root.is_symlink() or not root.is_dir():
        raise SuccessorBindingError("score adoption clone is not a real directory")
    root = root.resolve()
    entries: list[dict[str, Any]] = []
    for path in sorted(
        root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()
    ):
        relative = path.relative_to(root).as_posix()
        metadata = path.lstat()
        mode = stat.S_IMODE(metadata.st_mode)
        if stat.S_ISDIR(metadata.st_mode):
            entries.append({"path": relative, "type": "directory", "mode": mode})
        elif stat.S_ISREG(metadata.st_mode):
            digest, size = stable_file_sha256(path)
            entries.append(
                {
                    "path": relative,
                    "type": "file",
                    "mode": mode,
                    "size": size,
                    "sha256": digest,
                }
            )
        elif stat.S_ISLNK(metadata.st_mode):
            target = path.resolve()
            if not path_is_within(target, root):
                raise SuccessorBindingError(
                    f"score adoption clone contains an escaping symlink: {relative}"
                )
            entries.append(
                {
                    "path": relative,
                    "type": "symlink",
                    "mode": mode,
                    "target_sha256": sha256_bytes(os.readlink(path).encode()),
                }
            )
        else:
            raise SuccessorBindingError(
                f"score adoption clone contains a special file: {relative}"
            )
    return sha256_bytes(canonical_json(entries))


def create_once(path: pathlib.Path, payload: bytes, mode: int) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.parent.is_symlink():
        raise SuccessorBindingError(f"append-only parent is linked: {path.parent}")
    try:
        descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, mode)
    except FileExistsError:
        if path.is_symlink() or not path.is_file() or path.read_bytes() != payload:
            raise SuccessorBindingError(
                f"append-only successor already exists with different bytes: {path}"
            )
        if stat.S_IMODE(path.stat().st_mode) != mode:
            raise SuccessorBindingError(f"append-only successor mode changed: {path}")
        return False
    with os.fdopen(descriptor, "wb") as handle:
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())
    path.chmod(mode)
    return True


def rename_directory_create_only(source: pathlib.Path, target: pathlib.Path) -> None:
    if sys.platform != "darwin":
        raise SuccessorBindingError(
            "append-only successor commit requires macOS RENAME_EXCL"
        )
    library = ctypes.CDLL(None, use_errno=True)
    rename_exclusive = library.renamex_np
    rename_exclusive.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    rename_exclusive.restype = ctypes.c_int
    if rename_exclusive(os.fsencode(source), os.fsencode(target), 0x00000004) != 0:
        error = ctypes.get_errno()
        if error in (errno.EEXIST, errno.ENOTEMPTY):
            raise SuccessorBindingError(
                "append-only successor destination already exists"
            )
        raise SuccessorBindingError(
            f"append-only successor exclusive commit failed with errno {error}"
        )


def normalized_process_receipts(launch: Mapping[str, Any]) -> dict[str, dict[str, Any]]:
    receipts: dict[str, dict[str, Any]] = {}
    for role in ("harness", "goose", "monitor"):
        receipt = launch.get(role)
        if not isinstance(receipt, dict):
            raise SuccessorBindingError(f"launch lacks exact {role} process evidence")
        pid = receipt.get("pid")
        identity = receipt.get("identity_sha256")
        if (
            not isinstance(pid, int)
            or isinstance(pid, bool)
            or pid <= 1
            or not SHA256_RE.fullmatch(str(identity))
        ):
            raise SuccessorBindingError(f"launch {role} process evidence is malformed")
        receipts[role] = {"pid": pid, "identity_sha256": identity}
    return receipts


def validated_manifest_files(value: Any, *, identity: str) -> dict[str, str]:
    if not isinstance(value, dict) or not value:
        raise SuccessorBindingError(f"{identity} lacks its exact file inventory")
    for relative, digest in value.items():
        if not isinstance(relative, str) or not relative or "\\" in relative:
            raise SuccessorBindingError(f"{identity} contains an invalid path/hash")
        relative_path = pathlib.PurePosixPath(relative)
        if (
            relative_path.is_absolute()
            or relative_path.as_posix() != relative
            or relative_path == pathlib.PurePosixPath(".")
            or ".." in relative_path.parts
            or not SHA256_RE.fullmatch(str(digest))
        ):
            raise SuccessorBindingError(f"{identity} contains an invalid path/hash")
    return value


def validate_successor_instrument_manifest(
    manifest: Mapping[str, Any],
    launch: Mapping[str, Any],
    *,
    binary_size: int,
) -> None:
    expected_top_level = {
        "schema_version",
        "prepared_at",
        "candidate_branch",
        "candidate_remote_ref",
        "candidate_commit",
        "candidate_tree",
        "candidate_clean",
        "binary",
        "instrument_provenance",
        "files",
        "wrapper_sha256",
        "runtime_policy",
        "sb7_policy",
        "publisher_closure",
        "privacy",
    }
    if (
        manifest.get("schema_version")
        != SUCCESSOR_INSTRUMENT_MANIFEST_SCHEMA_VERSION
        or set(manifest) != expected_top_level
    ):
        raise SuccessorBindingError(
            "successor instrument manifest schema-2 contract changed"
        )

    candidate = launch["candidate"]
    launch_binary = launch["binary"]
    branch = manifest.get("candidate_branch")
    remote_ref = manifest.get("candidate_remote_ref")
    if (
        not isinstance(manifest.get("prepared_at"), str)
        or not manifest["prepared_at"]
        or not isinstance(branch, str)
        or not branch
        or not isinstance(remote_ref, str)
        or remote_ref != candidate.get("remote_ref")
        or not remote_ref.endswith(f"/{branch}")
        or candidate.get("remote_commit") != candidate["commit"]
        or manifest.get("candidate_commit") != candidate["commit"]
        or manifest.get("candidate_tree") != candidate["tree"]
        or manifest.get("candidate_clean") is not True
    ):
        raise SuccessorBindingError(
            "successor instrument manifest candidate identity differs"
        )

    recorded_binary = manifest.get("binary")
    if (
        not isinstance(recorded_binary, dict)
        or set(recorded_binary)
        != {"path", "sha256", "size_bytes", "source_commit", "source_tree"}
        or recorded_binary.get("path") != launch_binary["path"]
        or recorded_binary.get("sha256") != launch_binary["sha256"]
        or not isinstance(recorded_binary.get("size_bytes"), int)
        or isinstance(recorded_binary.get("size_bytes"), bool)
        or recorded_binary.get("size_bytes") != binary_size
        or recorded_binary.get("source_commit") != candidate["commit"]
        or recorded_binary.get("source_tree") != candidate["tree"]
    ):
        raise SuccessorBindingError(
            "successor instrument manifest binary identity differs"
        )

    files = validated_manifest_files(
        manifest.get("files"), identity="successor instrument manifest"
    )
    provenance = manifest.get("instrument_provenance")
    if not isinstance(provenance, dict) or set(provenance) != {
        "candidate_archive",
        "inherited_overlay",
        "total_file_count",
        "tracked_only_archive",
        "python_cache_debris_forbidden",
        "symlinks_forbidden",
    }:
        raise SuccessorBindingError(
            "successor instrument manifest provenance contract changed"
        )
    archive = provenance.get("candidate_archive")
    overlay = provenance.get("inherited_overlay")
    if (
        not isinstance(archive, dict)
        or set(archive)
        != {"commit", "tree", "scope", "file_count", "inventory_sha256"}
        or archive.get("commit") != candidate["commit"]
        or archive.get("tree") != candidate["tree"]
        or archive.get("scope")
        != ["evals/swarm-bench", "scripts/monitor_swarm_run.py"]
        or not isinstance(archive.get("file_count"), int)
        or isinstance(archive.get("file_count"), bool)
        or archive["file_count"] <= 0
        or not SHA256_RE.fullmatch(str(archive.get("inventory_sha256", "")))
        or not isinstance(overlay, dict)
        or set(overlay) != {"source_run", "source_manifest_sha256", "files"}
        or not pathlib.Path(str(overlay.get("source_run", ""))).is_absolute()
        or not SHA256_RE.fullmatch(str(overlay.get("source_manifest_sha256", "")))
    ):
        raise SuccessorBindingError(
            "successor instrument manifest provenance identity differs"
        )
    overlay_files = validated_manifest_files(
        overlay.get("files"), identity="successor inherited overlay"
    )
    archive_files = set(files) - set(overlay_files)
    if (
        any(files.get(relative) != digest for relative, digest in overlay_files.items())
        or archive.get("file_count") != len(archive_files)
        or provenance.get("total_file_count") != len(files)
        or provenance.get("total_file_count")
        != archive.get("file_count") + len(overlay_files)
        or provenance.get("tracked_only_archive") is not True
        or provenance.get("python_cache_debris_forbidden") is not True
        or provenance.get("symlinks_forbidden") is not True
        or any(
            relative != "scripts/monitor_swarm_run.py"
            and not relative.startswith("evals/swarm-bench/")
            for relative in archive_files
        )
    ):
        raise SuccessorBindingError(
            "successor instrument manifest provenance inventory differs"
        )

    runtime_policy = manifest.get("runtime_policy")
    if not isinstance(runtime_policy, dict) or set(runtime_policy) != {
        "child_environment",
        "deferred_live_fleet_seal",
        "exact_context_length_by_role",
        "lm_studio_cli_path",
        "lm_studio_cli_sha256",
        "minimum_context_length",
        "monitor_policy",
    } or (
        runtime_policy.get("child_environment") != "fixed-explicit-allowlist"
        or runtime_policy.get("deferred_live_fleet_seal") is not True
        or runtime_policy.get("exact_context_length_by_role")
        != EXACT_CONTEXT_BY_ROLE
        or not pathlib.Path(
            str(runtime_policy.get("lm_studio_cli_path", ""))
        ).is_absolute()
        or not SHA256_RE.fullmatch(
            str(runtime_policy.get("lm_studio_cli_sha256", ""))
        )
        or runtime_policy.get("minimum_context_length")
        != min(EXACT_CONTEXT_BY_ROLE.values())
        or runtime_policy.get("monitor_policy") != "observation-only"
    ):
        raise SuccessorBindingError(
            "successor instrument manifest runtime policy changed"
        )

    entrant = launch.get("entrant")
    if manifest.get("sb7_policy") != {
        "spec_and_scorer_unchanged_from_v6": True,
        "website_surface": "stable-sb7",
        "publish_from_run_build_auto_score": False,
        "entrant": entrant,
        "publication_document_id": TARGET_DOCUMENT_ID,
        "protected_document_ids": list(PROTECTED_DOCUMENT_ID_ORDER),
    }:
        raise SuccessorBindingError(
            "successor publication/protected-document policy changed"
        )
    if manifest.get("publisher_closure") != {
        "binding": "deferred-until-authenticated-run-started-and-fixture-seed",
        "protected_publication_untouched_during_freeze": True,
        "publish_from_run_build_auto_score": False,
        "publisher_present_in_main_instrument": False,
    }:
        raise SuccessorBindingError(
            "successor instrument manifest publisher closure changed"
        )
    if manifest.get("privacy") != {
        "environment_values_persisted": False,
        "raw_argv_persisted": False,
        "secret_fields_persisted": False,
    }:
        raise SuccessorBindingError(
            "successor instrument manifest privacy contract changed"
        )
    wrapper_sha256 = manifest.get("wrapper_sha256")
    if (
        not SHA256_RE.fullmatch(str(wrapper_sha256))
        or wrapper_sha256 != launch.get("wrapper_sha256")
    ):
        raise SuccessorBindingError(
            "successor instrument manifest wrapper identity differs"
        )


def find_launch_controller(live_root: pathlib.Path, expected_sha256: str) -> pathlib.Path:
    matches = [
        path.resolve()
        for path in live_root.glob("launch_local_*.py")
        if not path.is_symlink()
        and path.is_file()
        and sha256_bytes(read_stable_bytes(path, read_only=True)) == expected_sha256
    ]
    if len(matches) != 1:
        raise SuccessorBindingError(
            "successor launch root must contain exactly one hash-matching launch_local_*.py"
        )
    return matches[0]


def successor_evidence(
    generation: str,
    live_root: pathlib.Path,
    state_dir: pathlib.Path,
) -> dict[str, Any]:
    if not GENERATION_RE.fullmatch(generation):
        raise SuccessorBindingError("successor generation must be v21-r5 or later")
    if live_root.is_symlink() or not live_root.is_dir():
        raise SuccessorBindingError("successor live root is missing or linked")
    live_root = live_root.resolve()
    state_dir = state_dir.resolve()
    if live_root.name != f"local-sb7-engine-{generation}":
        raise SuccessorBindingError("successor live root does not match its generation")
    if state_dir == live_root or path_is_within(state_dir, live_root):
        raise SuccessorBindingError("successor closure state must be outside the live run")

    launch_path = require_regular(live_root / "launch.json")
    launch_payload = read_stable_bytes(launch_path)
    launch = decode_json(launch_payload, launch_path)
    manifest_path = require_regular(live_root / "instrument-manifest.json", read_only=True)
    manifest_payload = read_stable_bytes(manifest_path, read_only=True)
    manifest = decode_json(manifest_payload, manifest_path)
    if launch.get("schema_version") != SCHEMA_VERSION:
        raise SuccessorBindingError("successor launch schema changed")
    candidate = launch.get("candidate")
    binary = launch.get("binary")
    started = launch.get("run_started_identity")
    if (
        not isinstance(candidate, dict)
        or not COMMIT_RE.fullmatch(str(candidate.get("commit", "")))
        or not COMMIT_RE.fullmatch(str(candidate.get("tree", "")))
        or not isinstance(binary, dict)
        or not SHA256_RE.fullmatch(str(binary.get("sha256", "")))
        or launch.get("publication_document_id") != TARGET_DOCUMENT_ID
        or not isinstance(started, dict)
        or not RUN_ID_RE.fullmatch(str(started.get("run_id", "")))
    ):
        raise SuccessorBindingError("successor launch source/run identity is malformed")
    if launch.get("instrument_manifest_sha256") != sha256_bytes(manifest_payload):
        raise SuccessorBindingError("successor launch/manifest source identity differs")

    run_dir = pathlib.Path(str(started.get("working_dir", "")))
    if (
        not run_dir.is_absolute()
        or not path_is_within(run_dir, live_root)
        or run_dir.parent.resolve() != live_root
    ):
        raise SuccessorBindingError("successor run directory escaped its live root")
    binary_path = pathlib.Path(str(binary.get("path", "")))
    if (
        not binary_path.is_absolute()
        or not path_is_within(binary_path, live_root)
        or require_regular(binary_path, read_only=True) != binary_path.resolve()
    ):
        raise SuccessorBindingError("successor binary path/hash/mode differs")
    binary_sha256, binary_size = stable_file_sha256(binary_path, read_only=True)
    if binary_sha256 != binary["sha256"]:
        raise SuccessorBindingError("successor binary path/hash/mode differs")
    validate_successor_instrument_manifest(
        manifest,
        launch,
        binary_size=binary_size,
    )
    launcher_sha256 = launch.get("launch_controller_sha256")
    if not SHA256_RE.fullmatch(str(launcher_sha256)):
        raise SuccessorBindingError("successor launch controller hash is malformed")
    launcher = find_launch_controller(live_root, str(launcher_sha256))
    if launcher.name != f"launch_local_{generation.replace('-', '_')}.py":
        raise SuccessorBindingError("successor launcher name does not match its generation")
    fleet_seal = require_regular(live_root / "fleet-seal.json")

    return {
        "schema_version": SCHEMA_VERSION,
        "generation": generation,
        "live_root": str(live_root),
        "run_dir": str(run_dir.resolve()),
        "state_dir": str(state_dir),
        "launch_path": str(launch_path),
        "launch_sha256": sha256_bytes(launch_payload),
        "launch_controller_path": str(launcher),
        "launch_controller_sha256": launcher_sha256,
        "instrument_manifest_path": str(manifest_path),
        "instrument_manifest_sha256": sha256_bytes(manifest_payload),
        "instrument_manifest_schema_version": manifest["schema_version"],
        "instrument_recorded_binary": manifest.get("binary"),
        "fleet_seal_path": str(fleet_seal),
        "candidate_commit": candidate["commit"],
        "candidate_tree": candidate["tree"],
        "binary_path": str(binary_path.resolve()),
        "binary_sha256": binary["sha256"],
        "run_id": started["run_id"],
        "processes": normalized_process_receipts(launch),
        "target_document_id": TARGET_DOCUMENT_ID,
        "protected_document_ids": list(PROTECTED_DOCUMENT_ID_ORDER),
    }


def binding_block(evidence: Mapping[str, Any], base_config: Mapping[str, Any]) -> str:
    publisher = base_config.get("publisher")
    usage_policy = base_config.get("usage_policy")
    publication = base_config.get("publication")
    if (
        not isinstance(publisher, dict)
        or not isinstance(usage_policy, dict)
        or not isinstance(publication, dict)
        or publication.get("target_document_id") != TARGET_DOCUMENT_ID
        or publication.get("protected_document_ids")
        != list(PROTECTED_DOCUMENT_ID_ORDER)
    ):
        raise SuccessorBindingError("base closure publication contract is not protected")
    required_hashes = (
        publisher.get("sha256"),
        usage_policy.get("sha256"),
        evidence.get("instrument_manifest_sha256"),
    )
    if any(not SHA256_RE.fullmatch(str(value)) for value in required_hashes):
        raise SuccessorBindingError("base closure frozen source hash is malformed")
    recorded_binary = evidence.get("instrument_recorded_binary")
    if not isinstance(recorded_binary, dict):
        raise SuccessorBindingError("instrument manifest lacks its recorded binary")

    root = pathlib.Path(str(evidence["live_root"]))
    state = pathlib.Path(str(evidence["state_dir"]))
    assignments = [
        f"V19_GENERATION = {evidence['generation']!r}",
        f"V19_LIVE_ROOT = pathlib.Path({str(root)!r})",
        f"V19_RUN_DIR = pathlib.Path({str(evidence['run_dir'])!r})",
        f"V19_STATE_DIR = pathlib.Path({str(state)!r})",
        "V19_BOUND_CONFIG = V19_STATE_DIR / 'config.json'",
        f"V19_SCORE_LOCK = pathlib.Path({str(root) + '-score.lock'!r})",
        f"V19_LAUNCHER = pathlib.Path({str(evidence['launch_controller_path'])!r})",
        f"V19_LAUNCHER_SHA256 = {evidence['launch_controller_sha256']!r}",
        f"V19_FLEET_SEAL = pathlib.Path({str(evidence['fleet_seal_path'])!r})",
        f"V19_TARGET_DOCUMENT_ID = {TARGET_DOCUMENT_ID!r}",
        "V19_PROTECTED_DOCUMENT_ID_ORDER = "
        f"{PROTECTED_DOCUMENT_ID_ORDER!r}",
        "V19_PROTECTED_DOCUMENT_IDS = "
        "frozenset(V19_PROTECTED_DOCUMENT_ID_ORDER)",
        f"V19_EXACT_MODEL_ALIASES = frozenset({sorted(EXACT_MODEL_ALIASES)!r})",
        f"V19_EXACT_CONTEXT_BY_ROLE = {EXACT_CONTEXT_BY_ROLE!r}",
        f"V19_EXPECTED_ARTIFACT_PATH_SHA256 = {EXPECTED_ARTIFACT_PATH_SHA256!r}",
        f"V19_EXPECTED_QUANTIZATION_SHA256 = {EXPECTED_QUANTIZATION_SHA256!r}",
        f"V19_EXPECTED_PLANNER_MODEL = {EXPECTED_PLANNER_MODEL!r}",
        f"V19_ROLE_PREFIXES = {ROLE_PREFIXES!r}",
        f"V19_CANDIDATE_COMMIT = {evidence['candidate_commit']!r}",
        f"V19_CANDIDATE_TREE = {evidence['candidate_tree']!r}",
        f"V19_BINARY = pathlib.Path({str(evidence['binary_path'])!r})",
        f"V19_BINARY_SHA256 = {evidence['binary_sha256']!r}",
        f"V19_INSTRUMENT_MANIFEST_SHA256 = {evidence['instrument_manifest_sha256']!r}",
        "V19_INSTRUMENT_MANIFEST_SCHEMA_VERSION = "
        f"{evidence['instrument_manifest_schema_version']!r}",
        f"V19_INSTRUMENT_RECORDED_BINARY = {recorded_binary!r}",
        f"V19_PUBLISHER_COMMIT = {publisher.get('git_commit')!r}",
        f"V19_PUBLISHER_SHA256 = {publisher.get('sha256')!r}",
        f"V19_USAGE_POLICY_SHA256 = {usage_policy.get('sha256')!r}",
        f"V19_PUBLISHER_MARKER = {publication.get('provenance_marker')!r}",
        f"V19_PUBLISHER_ROOT = pathlib.Path({publisher.get('site_root')!r})",
        "V19_PUBLISHER_PATH = V19_STATE_DIR / 'bootstrap' / 'seed-fleet-brainwaves-sb70.mjs'",
        "V19_USAGE_POLICY_PATH = V19_STATE_DIR / 'bootstrap' / 'usage_impairment.py'",
        f"V19_BOUND_LAUNCH_SHA256 = {evidence['launch_sha256']!r}",
        f"V19_BOUND_RUN_ID = {evidence['run_id']!r}",
        f"V19_BOUND_PROCESSES = {dict(evidence['processes'])!r}",
    ]
    return START_MARKER + "\n" + "\n".join(assignments) + "\n" + END_MARKER


def render_controller(
    source: bytes, evidence: Mapping[str, Any], base_config: Mapping[str, Any]
) -> bytes:
    text = source.decode("utf-8")
    start = text.find(START_MARKER)
    end = text.find(END_MARKER)
    if start < 0 or end < 0 or end <= start:
        raise SuccessorBindingError("terminal closer lacks its unique run-binding markers")
    if text.find(START_MARKER, start + 1) >= 0 or text.find(END_MARKER, end + 1) >= 0:
        raise SuccessorBindingError("terminal closer repeats its run-binding markers")
    end += len(END_MARKER)
    return (text[:start] + binding_block(evidence, base_config) + text[end:]).encode()


def successor_template(
    base_config: Mapping[str, Any],
    evidence: Mapping[str, Any],
    controller_sha256: str,
    publisher_path: pathlib.Path,
    usage_policy_path: pathlib.Path,
) -> dict[str, Any]:
    template = json.loads(json.dumps(base_config))
    template.update(
        {
            "armed": False,
            "binding": None,
            "closure_generation": evidence["generation"],
            "live_root": evidence["live_root"],
            "run_dir": evidence["run_dir"],
            "state_dir": evidence["state_dir"],
            "score_lock_path": str(pathlib.Path(str(evidence["live_root"]) + "-score.lock")),
            "bound_config_path": str(pathlib.Path(str(evidence["state_dir"])) / "config.json"),
            "controller_sha256": controller_sha256,
        }
    )
    template.pop("binding_successor", None)
    template["expected"].update(
        {
            "candidate_commit": evidence["candidate_commit"],
            "candidate_tree": evidence["candidate_tree"],
            "binary_path": evidence["binary_path"],
            "binary_sha256": evidence["binary_sha256"],
            "launch_controller_path": evidence["launch_controller_path"],
            "launch_controller_sha256": evidence["launch_controller_sha256"],
            "instrument_manifest_sha256": evidence["instrument_manifest_sha256"],
            "run_id": None,
            "fixture_seed": None,
            "models": None,
            "instrument_files": None,
            "launch_sha256": None,
            "run_started_sha256": None,
            "trace_header_sha256": None,
            "fleet_seal_sha256": None,
            "fleet_binding_sha256": None,
        }
    )
    template["publisher"]["path"] = str(publisher_path)
    template["usage_policy"]["path"] = str(usage_policy_path)
    template["binding_successor"] = {
        "schema_version": SCHEMA_VERSION,
        "generation": evidence["generation"],
        "launch_sha256": evidence["launch_sha256"],
        "run_id": evidence["run_id"],
        "processes": evidence["processes"],
        "target_document_id": evidence["target_document_id"],
        "protected_document_ids": evidence["protected_document_ids"],
    }
    return template


def process_exists(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def post_terminal_armed_config(
    *,
    generation: str,
    evidence: Mapping[str, Any],
    template: Mapping[str, Any],
    template_payload: bytes,
    predecessor_config_path: pathlib.Path,
) -> dict[str, Any]:
    predecessor_config_path = require_regular(
        predecessor_config_path, read_only=True
    )
    predecessor_payload = read_stable_bytes(
        predecessor_config_path, read_only=True
    )
    approved = APPROVED_TERMINAL_PREDECESSOR_CONFIG_SHA256_BY_GENERATION.get(
        generation
    )
    if approved is None or sha256_bytes(predecessor_payload) != approved:
        raise SuccessorBindingError(
            "terminal predecessor closure config is not approved"
        )
    predecessor = decode_json(predecessor_payload, predecessor_config_path)
    predecessor_state = pathlib.Path(str(predecessor.get("state_dir", ""))).resolve()
    if predecessor_config_path != predecessor_state / "config.json":
        raise SuccessorBindingError("terminal predecessor config escaped its state")
    if (
        predecessor.get("armed") is not True
        or predecessor.get("closure_generation") != generation
        or predecessor.get("live_root") != evidence["live_root"]
        or predecessor.get("run_dir") != evidence["run_dir"]
        or predecessor.get("publication") != template.get("publication")
        or predecessor.get("binding_successor")
        != template.get("binding_successor")
    ):
        raise SuccessorBindingError("terminal predecessor run/publication identity differs")
    expected = predecessor.get("expected")
    binding = predecessor.get("binding")
    if (
        not isinstance(expected, dict)
        or expected.get("run_id") != evidence["run_id"]
        or not isinstance(binding, dict)
        or binding.get("generation") != generation
        or binding.get("launch_sha256") != evidence["launch_sha256"]
    ):
        raise SuccessorBindingError("terminal predecessor binding identity differs")
    state = read_json(predecessor_state / "state.json")
    failure = read_json(predecessor_state / "failure.json")
    supervisor = read_json(predecessor_state / "supervisor.pid.json")
    if (
        state.get("phase") != "failed"
        or failure.get("error_type") != "ClosureError"
        or failure.get("message")
        != "score contains degraded product-probe evidence in probe_unavailable"
    ):
        raise SuccessorBindingError(
            "terminal predecessor did not fail at the approved raw-probe boundary"
        )
    supervisor_pid = supervisor.get("pid")
    if (
        not isinstance(supervisor_pid, int)
        or isinstance(supervisor_pid, bool)
        or process_exists(supervisor_pid)
    ):
        raise SuccessorBindingError("terminal predecessor supervisor is still live")
    for receipt in evidence["processes"].values():
        if process_exists(int(receipt["pid"])):
            raise SuccessorBindingError(
                "terminal predecessor launch process is still live"
            )

    run_dir = pathlib.Path(str(evidence["run_dir"]))
    auto_path = require_regular(run_dir / "verdict.json")
    aggregate_path = require_regular(
        pathlib.Path(str(evidence["live_root"]))
        / "swarm-3node-qwen38-brainwaves.json"
    )
    auto = read_json(auto_path)
    aggregate = json.loads(read_stable_bytes(aggregate_path))
    if (
        not isinstance(aggregate, list)
        or len(aggregate) != 1
        or canonical_json(aggregate[0]) != canonical_json(auto)
        or auto.get("fixture_seed") != expected.get("fixture_seed")
        or auto.get("entrant") != "swarm-3node-qwen38-brainwaves"
        or auto.get("rep") != 0
        or auto.get("vendor_port") != 18970
        or not isinstance(auto.get("probe_unavailable"), list)
        or not auto["probe_unavailable"]
    ):
        raise SuccessorBindingError(
            "terminal predecessor raw verdict does not prove the recoverable boundary"
        )

    armed = json.loads(json.dumps(template))
    dynamic_fields = (
        "run_id",
        "fixture_seed",
        "models",
        "instrument_files",
        "launch_sha256",
        "run_started_sha256",
        "trace_header_sha256",
        "fleet_seal_sha256",
        "fleet_binding_sha256",
    )
    armed["armed"] = True
    armed["expected"].update({field: expected[field] for field in dynamic_fields})
    armed_binding = json.loads(json.dumps(binding))
    armed_binding["template_sha256"] = sha256_bytes(template_payload)
    armed["binding"] = armed_binding
    return armed


def score_adoption_contract(
    attempt_path: pathlib.Path,
    evidence: Mapping[str, Any],
    publication: Mapping[str, Any],
) -> dict[str, Any]:
    attempt = attempt_path.resolve()
    if attempt.name != "attempt-1" or attempt.parent.name != "scoring":
        raise SuccessorBindingError("score adoption source is not exact attempt-1")
    state = attempt.parent.parent
    if state == pathlib.Path(str(evidence["state_dir"])).resolve():
        raise SuccessorBindingError("score adoption source cannot be its successor")
    files: dict[str, str] = {}
    for relative in SCORE_ADOPTION_SOURCE_FILES:
        path = require_regular(state / relative)
        files[relative] = stable_file_sha256(path)[0]
    source_config = read_json(state / "config.json")
    source_failure = read_json(state / "failure.json")
    source_worker = read_json(attempt / "worker-result.json")
    source_job = read_json(attempt / "job.json")
    raw_seal = read_json(state / "raw-tree-seal.json")
    clone_seal = read_json(attempt / "clone-seal.json")
    if (
        source_config.get("armed") is not True
        or source_config.get("closure_generation") != evidence["generation"]
        or source_config.get("live_root") != evidence["live_root"]
        or source_config.get("run_dir") != evidence["run_dir"]
        or source_config.get("publication") != publication
        or source_config.get("expected", {}).get("run_id") != evidence["run_id"]
        or source_config.get("controller_sha256")
        != files["closure-instrument/terminal_closure.py"]
    ):
        raise SuccessorBindingError("score adoption source closure identity differs")
    if (
        source_failure.get("error_type") != "ClosureError"
        or source_failure.get("message")
        != "attempt-1 did not prove descendant cleanup; refusing retry"
        or source_worker.get("schema_version") != SCHEMA_VERSION
        or source_worker.get("attempt") != 1
        or source_worker.get("exit_code") != 70
        or source_worker.get("failure")
        != "score contains degraded product-probe evidence in probe_unavailable"
        or source_worker.get("score_sha256") is not None
        or set(source_worker)
        != {
            "schema_version",
            "attempt",
            "completed_at",
            "exit_code",
            "failure",
            "score_sha256",
        }
        or not isinstance(source_worker.get("completed_at"), str)
    ):
        raise SuccessorBindingError("score adoption source failure boundary differs")
    score_output = attempt / "raw-score.json"
    if (
        source_job.get("schema_version") != SCHEMA_VERSION
        or source_job.get("attempt") != 1
        or source_job.get("clone") != str(attempt / "tree")
        or source_job.get("score_output") != str(score_output)
        or source_job.get("score_log") != str(attempt / "score.log")
        or source_job.get("result") != str(attempt / "worker-result.json")
        or source_job.get("raw_tree") != evidence["run_dir"]
        or source_job.get("raw_tree_sha256") != raw_seal.get("tree_sha256")
        or source_job.get("seed")
        != source_config.get("expected", {}).get("fixture_seed")
        or source_job.get("port") != 18970
        or clone_seal.get("tree_sha256") != raw_seal.get("tree_sha256")
        or raw_seal.get("root") != evidence["run_dir"]
        or not SHA256_RE.fullmatch(str(raw_seal.get("tree_sha256", "")))
    ):
        raise SuccessorBindingError("score adoption job or initial seal differs")
    first_tree_sha256 = stable_tree_content_sha256(attempt / "tree")
    second_tree_sha256 = stable_tree_content_sha256(attempt / "tree")
    if first_tree_sha256 != second_tree_sha256:
        raise SuccessorBindingError("score adoption source tree is not quiescent")
    return {
        "schema_version": SCHEMA_VERSION,
        "source_state": str(state),
        "source_attempt": str(attempt),
        "source_files": files,
        "source_clone_tree_sha256": first_tree_sha256,
        "source_initial_clone_tree_sha256": clone_seal["tree_sha256"],
        "source_raw_tree_sha256": raw_seal["tree_sha256"],
        "expected_failure": (
            "score contains degraded product-probe evidence in probe_unavailable"
        ),
    }


def publisher_adoption_contract(
    publisher_state_path: pathlib.Path,
    evidence: Mapping[str, Any],
    publication: Mapping[str, Any],
) -> dict[str, Any]:
    publisher_state_path = require_regular(publisher_state_path)
    source_state = publisher_state_path.parent.resolve()
    if publisher_state_path != source_state / "publisher-state.json":
        raise SuccessorBindingError("publisher adoption source escaped its state")
    if source_state == pathlib.Path(str(evidence["state_dir"])).resolve():
        raise SuccessorBindingError("publisher adoption source cannot be its successor")
    if (source_state / "publication-receipt.json").exists():
        raise SuccessorBindingError("publisher adoption source is already complete")

    source_state_row = read_json(source_state / "state.json")
    source_publisher_pid = read_json(source_state / "publisher.pid.json")
    source_supervisor_pid = read_json(source_state / "supervisor.pid.json")
    for label, receipt in (
        ("publisher", source_publisher_pid),
        ("supervisor", source_supervisor_pid),
    ):
        pid = receipt.get("pid")
        if (
            not isinstance(pid, int)
            or isinstance(pid, bool)
            or process_exists(pid)
        ):
            raise SuccessorBindingError(
                f"publisher adoption source {label} is still live"
            )
    if source_state_row.get("phase") not in {"stopped", "failed"}:
        raise SuccessorBindingError("publisher adoption source is not terminal")

    files: dict[str, str] = {}
    for relative in PUBLISHER_ADOPTION_SOURCE_FILES:
        path = require_regular(source_state / relative)
        files[relative] = stable_file_sha256(path)[0]
    source_config = read_json(source_state / "config.json")
    publisher_state = read_json(publisher_state_path)
    authoritative = read_json(source_state / "authoritative-verdict.json")
    provenance = read_json(source_state / "scoring-provenance.json")
    if (
        source_config.get("armed") is not True
        or source_config.get("closure_generation") != evidence["generation"]
        or source_config.get("live_root") != evidence["live_root"]
        or source_config.get("run_dir") != evidence["run_dir"]
        or source_config.get("publication") != publication
        or source_config.get("expected", {}).get("run_id") != evidence["run_id"]
        or source_config.get("publisher", {}).get("sha256")
        != files["closure-instrument/seed-fleet-brainwaves-sb70.mjs"]
    ):
        raise SuccessorBindingError("publisher adoption source identity differs")
    authoritative_sha256 = files["authoritative-verdict.json"]
    if (
        provenance.get("authoritative_verdict_sha256") != authoritative_sha256
        or authoritative.get("entrant") != "swarm-3node-qwen38-brainwaves"
        or authoritative.get("fixture_seed")
        != source_config.get("expected", {}).get("fixture_seed")
    ):
        raise SuccessorBindingError(
            "publisher adoption authoritative evidence differs"
        )
    assets = publisher_state.get("assets")
    if (
        publisher_state.get("schema_version") != SCHEMA_VERSION
        or publisher_state.get("initialized") is not True
        or publisher_state.get("target_document_id") != TARGET_DOCUMENT_ID
        or publisher_state.get("authoritative_verdict_sha256")
        != authoritative_sha256
        or publisher_state.get("document_written") is not True
        or publisher_state.get("planned_document_sha256")
        != publisher_state.get("document_sha256")
        or not SHA256_RE.fullmatch(
            str(publisher_state.get("protected_before_sha256", ""))
        )
        or not SHA256_RE.fullmatch(
            str(publisher_state.get("document_sha256", ""))
        )
        or not isinstance(assets, list)
        or not assets
    ):
        raise SuccessorBindingError("publisher adoption state is incomplete")
    seen_shots: set[str] = set()
    for asset in assets:
        shot_key = asset.get("shot_key") if isinstance(asset, dict) else None
        if (
            not isinstance(asset, dict)
            or not SHA256_RE.fullmatch(str(shot_key or ""))
            or shot_key in seen_shots
            or not SHA256_RE.fullmatch(str(asset.get("sha256", "")))
            or not SHA256_RE.fullmatch(str(asset.get("pixels_sha256", "")))
            or not isinstance(asset.get("asset_id"), str)
            or not asset["asset_id"].startswith("image-")
            or not isinstance(asset.get("width"), int)
            or asset["width"] < 2
            or not isinstance(asset.get("height"), int)
            or asset["height"] < 2
            or not isinstance(asset.get("filename"), str)
            or not isinstance(asset.get("caption"), str)
        ):
            raise SuccessorBindingError("publisher adoption asset is malformed")
        seen_shots.add(shot_key)
    return {
        "schema_version": SCHEMA_VERSION,
        "source_state": str(source_state),
        "source_files": files,
        "publisher_state_sha256": files["publisher-state.json"],
        "authoritative_verdict_sha256": authoritative_sha256,
        "target_document_id": TARGET_DOCUMENT_ID,
        "protected_before_sha256": publisher_state["protected_before_sha256"],
        "document_sha256": publisher_state["document_sha256"],
        "asset_ids": [asset["asset_id"] for asset in assets],
        "document_written": True,
        "create_only_resume": True,
    }


def publisher_successor_identity(source: pathlib.Path) -> tuple[pathlib.Path, str]:
    source = require_regular(source)
    completed = subprocess.run(
        ["git", "-C", str(source.parent), "rev-parse", "--show-toplevel", "HEAD"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    lines = completed.stdout.decode("utf-8", "strict").splitlines()
    if completed.returncode != 0 or len(lines) != 2:
        raise SuccessorBindingError("publisher successor Git identity is unavailable")
    site_root = pathlib.Path(lines[0]).resolve()
    commit = lines[1]
    if (
        not path_is_within(source, site_root)
        or commit != APPROVED_PUBLISHER_COMMIT
        or not COMMIT_RE.fullmatch(commit)
        or sha256_file(source) != APPROVED_PUBLISHER_SOURCE_SHA256
    ):
        raise SuccessorBindingError("publisher successor source is not approved")
    return site_root, commit


def load_generated_module(path: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location("terminal_closure_successor_bound", path)
    if spec is None or spec.loader is None:
        raise SuccessorBindingError("generated terminal closer could not be imported")
    module = importlib.util.module_from_spec(spec)
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
    return module


def generate_successor(
    *,
    generation: str,
    live_root: pathlib.Path,
    state_dir: pathlib.Path,
    base_config_path: pathlib.Path,
    controller_source: pathlib.Path,
    terminal_predecessor_config_path: pathlib.Path | None = None,
    score_adoption_attempt_path: pathlib.Path | None = None,
    publisher_successor_source_path: pathlib.Path | None = None,
    publisher_adoption_state_path: pathlib.Path | None = None,
) -> dict[str, Any]:
    base_config_path = require_regular(base_config_path)
    controller_source = require_regular(controller_source)
    base_payload = read_stable_bytes(base_config_path)
    approved_predecessor = APPROVED_PREDECESSOR_CONFIG_SHA256_BY_GENERATION.get(
        generation
    )
    if (
        approved_predecessor is None
        or sha256_bytes(base_payload) != approved_predecessor
    ):
        raise SuccessorBindingError(
            "predecessor closure config is not approved for this successor generation"
        )
    base_config = decode_json(base_payload, base_config_path)
    protected_before = sha256_bytes(base_payload)
    evidence = successor_evidence(generation, live_root, state_dir)
    evidence["predecessor_config_sha256"] = protected_before
    controller_source_payload = read_stable_bytes(controller_source)
    if sha256_bytes(controller_source_payload) != APPROVED_CONTROLLER_SOURCE_SHA256:
        raise SuccessorBindingError("terminal closer source is not the approved controller")
    evidence["source_controller_sha256"] = sha256_bytes(controller_source_payload)
    state_dir = pathlib.Path(str(evidence["state_dir"]))
    bootstrap = state_dir / "bootstrap"
    controller_path = bootstrap / "terminal_closure.py"
    publisher_path = bootstrap / "seed-fleet-brainwaves-sb70.mjs"
    usage_policy_path = bootstrap / "usage_impairment.py"
    publisher_source = require_regular(
        pathlib.Path(base_config["publisher"]["path"]), read_only=True
    )
    base_usage_policy_source = require_regular(
        pathlib.Path(base_config["usage_policy"]["path"]), read_only=True
    )
    publisher_payload = read_stable_bytes(publisher_source, read_only=True)
    base_usage_policy_payload = read_stable_bytes(
        base_usage_policy_source, read_only=True
    )
    if sha256_bytes(publisher_payload) != base_config["publisher"]["sha256"]:
        raise SuccessorBindingError("base guarded publisher hash changed")
    if (
        sha256_bytes(base_usage_policy_payload)
        != base_config["usage_policy"]["sha256"]
    ):
        raise SuccessorBindingError("base usage policy hash changed")

    usage_policy_source = require_regular(
        controller_source.with_name("usage_impairment.py")
    )
    usage_policy_payload = read_stable_bytes(usage_policy_source)
    usage_policy_sha256 = sha256_bytes(usage_policy_payload)
    if usage_policy_sha256 != APPROVED_USAGE_POLICY_SOURCE_SHA256:
        raise SuccessorBindingError("successor usage policy is not approved")
    successor_base_config = json.loads(json.dumps(base_config))
    successor_base_config["usage_policy"]["sha256"] = usage_policy_sha256
    frozen_publisher_payload = publisher_payload
    publisher_successor_source = None
    if publisher_successor_source_path is not None:
        publisher_successor_source = require_regular(
            publisher_successor_source_path
        )
        publisher_site_root, publisher_commit = publisher_successor_identity(
            publisher_successor_source
        )
        frozen_publisher_payload = read_stable_bytes(publisher_successor_source)
        successor_base_config["publisher"].update(
            {
                "sha256": sha256_bytes(frozen_publisher_payload),
                "git_commit": publisher_commit,
                "site_root": str(publisher_site_root),
            }
        )
    adoption_contract = None
    if score_adoption_attempt_path is not None:
        adoption_contract = score_adoption_contract(
            score_adoption_attempt_path,
            evidence,
            successor_base_config["publication"],
        )
        successor_base_config["score_adoption"] = adoption_contract
    else:
        successor_base_config.pop("score_adoption", None)
    publisher_adoption = None
    publisher_adoption_payload = None
    if publisher_adoption_state_path is not None:
        publisher_adoption = publisher_adoption_contract(
            publisher_adoption_state_path,
            evidence,
            successor_base_config["publication"],
        )
        publisher_adoption_payload = read_stable_bytes(
            require_regular(publisher_adoption_state_path)
        )

    rendered = render_controller(
        controller_source_payload, evidence, successor_base_config
    )
    controller_sha256 = sha256_bytes(rendered)
    template = successor_template(
        successor_base_config,
        evidence,
        controller_sha256,
        publisher_path,
        usage_policy_path,
    )
    template_payload = json.dumps(template, indent=2, sort_keys=True).encode() + b"\n"
    armed_payload = None
    if terminal_predecessor_config_path is not None:
        armed = post_terminal_armed_config(
            generation=generation,
            evidence=evidence,
            template=template,
            template_payload=template_payload,
            predecessor_config_path=terminal_predecessor_config_path,
        )
        armed_payload = json.dumps(armed, indent=2, sort_keys=True).encode() + b"\n"
    receipt_payload = canonical_json(evidence) + b"\n"

    template_path = state_dir / "template.json"
    outputs = {
        pathlib.Path("successor-binding.json"): (receipt_payload, 0o400),
        pathlib.Path("bootstrap/terminal_closure.py"): (rendered, 0o500),
        pathlib.Path("bootstrap/seed-fleet-brainwaves-sb70.mjs"): (
            frozen_publisher_payload,
            0o500,
        ),
        pathlib.Path("bootstrap/usage_impairment.py"): (
            usage_policy_payload,
            0o400,
        ),
        pathlib.Path("template.json"): (template_payload, 0o400),
    }
    if armed_payload is not None:
        outputs[pathlib.Path("config.json")] = (armed_payload, 0o400)
    if publisher_adoption is not None and publisher_adoption_payload is not None:
        outputs[pathlib.Path("publisher-state.json")] = (
            publisher_adoption_payload,
            0o600,
        )
        outputs[pathlib.Path("publisher-adoption-receipt.json")] = (
            canonical_json(publisher_adoption) + b"\n",
            0o400,
        )

    if not state_dir.exists():
        state_dir.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        temporary = state_dir.parent / (
            f".{state_dir.name}.{os.getpid()}.{os.urandom(6).hex()}.tmp"
        )
        temporary.mkdir(mode=0o700)
        try:
            for relative, (payload, mode) in outputs.items():
                create_once(temporary / relative, payload, mode)
            staged_controller = temporary / "bootstrap/terminal_closure.py"
            staged_template = temporary / "template.json"
            module = load_generated_module(staged_controller)
            module.validate_config(module.load_config(staged_template), allow_unarmed=True)
            staged_config = temporary / "config.json"
            if armed_payload is not None:
                module.validate_config(module.load_config(staged_config))
            refreshed_evidence = successor_evidence(generation, live_root, state_dir)
            comparable_evidence = {
                key: value
                for key, value in evidence.items()
                if key not in {"predecessor_config_sha256", "source_controller_sha256"}
            }
            if canonical_json(refreshed_evidence) != canonical_json(comparable_evidence):
                raise SuccessorBindingError(
                    "successor launch evidence changed before append-only commit"
                )
            if score_adoption_attempt_path is not None:
                refreshed_adoption = score_adoption_contract(
                    score_adoption_attempt_path,
                    evidence,
                    successor_base_config["publication"],
                )
                if canonical_json(refreshed_adoption) != canonical_json(
                    adoption_contract
                ):
                    raise SuccessorBindingError(
                        "score adoption evidence changed before append-only commit"
                    )
            if publisher_adoption_state_path is not None:
                refreshed_publisher_adoption = publisher_adoption_contract(
                    publisher_adoption_state_path,
                    evidence,
                    successor_base_config["publication"],
                )
                if canonical_json(refreshed_publisher_adoption) != canonical_json(
                    publisher_adoption
                ) or read_stable_bytes(
                    require_regular(publisher_adoption_state_path)
                ) != publisher_adoption_payload:
                    raise SuccessorBindingError(
                        "publisher adoption evidence changed before append-only commit"
                    )
            immutable_sources = (
                (publisher_source, publisher_payload, True),
                (base_usage_policy_source, base_usage_policy_payload, True),
                (usage_policy_source, usage_policy_payload, False),
            )
            if publisher_successor_source is not None:
                immutable_sources = (
                    *immutable_sources,
                    (
                        publisher_successor_source,
                        frozen_publisher_payload,
                        False,
                    ),
                )
            if any(
                read_stable_bytes(path, read_only=read_only) != payload
                for path, payload, read_only in (
                    (base_config_path, base_payload, False),
                    (controller_source, controller_source_payload, False),
                    *immutable_sources,
                )
            ):
                raise SuccessorBindingError(
                    "successor binding source changed before append-only commit"
                )
            rename_directory_create_only(temporary, state_dir)
            parent_descriptor = os.open(state_dir.parent, os.O_RDONLY)
            try:
                os.fsync(parent_descriptor)
            finally:
                os.close(parent_descriptor)
        finally:
            if temporary.exists():
                shutil.rmtree(temporary)
    elif state_dir.is_symlink() or not state_dir.is_dir():
        raise SuccessorBindingError("append-only successor state is not a real directory")

    for relative, (payload, mode) in outputs.items():
        path = state_dir / relative
        if (
            path.is_symlink()
            or not path.is_file()
            or read_stable_bytes(path, read_only=mode & 0o222 == 0) != payload
            or stat.S_IMODE(path.stat().st_mode) != mode
        ):
            raise SuccessorBindingError(
                f"append-only successor output changed: {relative.as_posix()}"
            )
    module = load_generated_module(controller_path)
    module.validate_config(module.load_config(template_path), allow_unarmed=True)
    config_path = state_dir / "config.json"
    if armed_payload is not None:
        module.validate_config(module.load_config(config_path))
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "generation": generation,
        "controller": str(controller_path),
        "controller_sha256": controller_sha256,
        "template": str(template_path),
        "template_sha256": sha256_file(template_path),
        "binding_receipt": str(state_dir / "successor-binding.json"),
        "binding_receipt_sha256": sha256_file(state_dir / "successor-binding.json"),
        "run_id": evidence["run_id"],
        "target_document_id": TARGET_DOCUMENT_ID,
        "protected_document_ids": list(PROTECTED_DOCUMENT_ID_ORDER),
        "base_config_unchanged": True,
        "predecessor_config_sha256": protected_before,
        "source_controller_sha256": evidence["source_controller_sha256"],
        "source_usage_policy_sha256": usage_policy_sha256,
    }
    if armed_payload is not None:
        receipt["config"] = str(config_path)
        receipt["config_sha256"] = sha256_file(config_path)
    if adoption_contract is not None:
        receipt["score_adoption_sha256"] = sha256_bytes(
            canonical_json(adoption_contract)
        )
    if publisher_adoption is not None:
        receipt["publisher_adoption_sha256"] = sha256_bytes(
            canonical_json(publisher_adoption)
        )
    if publisher_successor_source is not None:
        receipt["source_publisher_sha256"] = sha256_bytes(
            frozen_publisher_payload
        )
        receipt["source_publisher_commit"] = APPROVED_PUBLISHER_COMMIT
    return receipt


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    root.add_argument("--generation", required=True)
    root.add_argument("--live-root", type=pathlib.Path, required=True)
    root.add_argument("--state-dir", type=pathlib.Path, required=True)
    root.add_argument("--base-config", type=pathlib.Path, required=True)
    root.add_argument(
        "--controller-source",
        type=pathlib.Path,
        default=pathlib.Path(__file__).with_name("terminal_closure.py"),
    )
    root.add_argument("--terminal-predecessor-config", type=pathlib.Path)
    root.add_argument("--score-adoption-attempt", type=pathlib.Path)
    root.add_argument("--publisher-successor-source", type=pathlib.Path)
    root.add_argument("--publisher-adoption-state", type=pathlib.Path)
    root.add_argument("--bind", action="store_true")
    return root


def main(argv: Sequence[str] | None = None) -> int:
    os.umask(0o077)
    args = parser().parse_args(argv)
    receipt = generate_successor(
        generation=args.generation,
        live_root=args.live_root,
        state_dir=args.state_dir,
        base_config_path=args.base_config,
        controller_source=args.controller_source,
        terminal_predecessor_config_path=args.terminal_predecessor_config,
        score_adoption_attempt_path=args.score_adoption_attempt,
        publisher_successor_source_path=args.publisher_successor_source,
        publisher_adoption_state_path=args.publisher_adoption_state,
    )
    if args.bind:
        if receipt.get("config"):
            completed = None
        else:
            completed = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    receipt["controller"],
                    "bind-v21",
                    "--template",
                    receipt["template"],
                ],
                check=False,
            )
        if completed is not None and completed.returncode != 0:
            raise SuccessorBindingError("generated successor binder failed closed")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
