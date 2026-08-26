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
START_MARKER = "# BEGIN TERMINAL_CLOSURE_RUN_BINDING"
END_MARKER = "# END TERMINAL_CLOSURE_RUN_BINDING"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
RUN_ID_RE = re.compile(r"^swarm-\d{8}-\d{9}$")
GENERATION_RE = re.compile(r"^v21-r(?:[5-9]|[1-9]\d+)$")
TARGET_DOCUMENT_ID = "brun-fleet-qwen38-brainwaves-sb70"
PROTECTED_DOCUMENT_IDS = frozenset(
    {"brun-fleet-qwen38-sb70", "brun-fleet-qwen-sb70"}
)
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
APPROVED_CONTROLLER_SOURCE_SHA256 = (
    "a3c8bde115bed931b67c6aacf22f23dba26a4c5d5056cc043628edf019de0b20"
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
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise SuccessorBindingError("successor instrument manifest schema changed")

    publication_id = launch.get("publication_document_id")
    policy = manifest.get("sb7_policy")
    if (
        publication_id != TARGET_DOCUMENT_ID
        or not isinstance(policy, dict)
        or policy.get("publication_document_id") != TARGET_DOCUMENT_ID
        or policy.get("protected_document_ids") != sorted(PROTECTED_DOCUMENT_IDS)
        or policy.get("publish_from_run_build_auto_score") is not False
        or policy.get("website_surface") != "stable-sb7"
        or policy.get("spec_and_scorer_unchanged_from_v6") is not True
    ):
        raise SuccessorBindingError("successor publication/protected-document policy changed")

    candidate = launch.get("candidate")
    binary = launch.get("binary")
    started = launch.get("run_started_identity")
    if (
        not isinstance(candidate, dict)
        or not COMMIT_RE.fullmatch(str(candidate.get("commit", "")))
        or not COMMIT_RE.fullmatch(str(candidate.get("tree", "")))
        or not isinstance(binary, dict)
        or not SHA256_RE.fullmatch(str(binary.get("sha256", "")))
        or not isinstance(started, dict)
        or not RUN_ID_RE.fullmatch(str(started.get("run_id", "")))
    ):
        raise SuccessorBindingError("successor launch source/run identity is malformed")
    if (
        manifest.get("candidate_commit") != candidate["commit"]
        or manifest.get("candidate_tree") != candidate["tree"]
        or launch.get("instrument_manifest_sha256") != sha256_bytes(manifest_payload)
    ):
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
        or sha256_bytes(read_stable_bytes(binary_path, read_only=True))
        != binary["sha256"]
    ):
        raise SuccessorBindingError("successor binary path/hash/mode differs")
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
        "instrument_recorded_binary": manifest.get("binary"),
        "fleet_seal_path": str(fleet_seal),
        "candidate_commit": candidate["commit"],
        "candidate_tree": candidate["tree"],
        "binary_path": str(binary_path.resolve()),
        "binary_sha256": binary["sha256"],
        "run_id": started["run_id"],
        "processes": normalized_process_receipts(launch),
        "target_document_id": TARGET_DOCUMENT_ID,
        "protected_document_ids": sorted(PROTECTED_DOCUMENT_IDS),
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
        != sorted(PROTECTED_DOCUMENT_IDS)
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
        f"V19_PROTECTED_DOCUMENT_IDS = frozenset({sorted(PROTECTED_DOCUMENT_IDS)!r})",
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
    usage_policy_source = require_regular(
        pathlib.Path(base_config["usage_policy"]["path"]), read_only=True
    )
    publisher_payload = read_stable_bytes(publisher_source, read_only=True)
    usage_policy_payload = read_stable_bytes(usage_policy_source, read_only=True)
    if sha256_bytes(publisher_payload) != base_config["publisher"]["sha256"]:
        raise SuccessorBindingError("base guarded publisher hash changed")
    if sha256_bytes(usage_policy_payload) != base_config["usage_policy"]["sha256"]:
        raise SuccessorBindingError("base usage policy hash changed")

    rendered = render_controller(controller_source_payload, evidence, base_config)
    controller_sha256 = sha256_bytes(rendered)
    template = successor_template(
        base_config,
        evidence,
        controller_sha256,
        publisher_path,
        usage_policy_path,
    )
    template_payload = json.dumps(template, indent=2, sort_keys=True).encode() + b"\n"
    receipt_payload = canonical_json(evidence) + b"\n"

    template_path = state_dir / "template.json"
    outputs = {
        pathlib.Path("successor-binding.json"): (receipt_payload, 0o400),
        pathlib.Path("bootstrap/terminal_closure.py"): (rendered, 0o500),
        pathlib.Path("bootstrap/seed-fleet-brainwaves-sb70.mjs"): (
            publisher_payload,
            0o500,
        ),
        pathlib.Path("bootstrap/usage_impairment.py"): (
            usage_policy_payload,
            0o400,
        ),
        pathlib.Path("template.json"): (template_payload, 0o400),
    }

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
            immutable_sources = (
                (publisher_source, publisher_payload, True),
                (usage_policy_source, usage_policy_payload, True),
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
            or read_stable_bytes(path, read_only=True) != payload
            or stat.S_IMODE(path.stat().st_mode) != mode
        ):
            raise SuccessorBindingError(
                f"append-only successor output changed: {relative.as_posix()}"
            )
    module = load_generated_module(controller_path)
    module.validate_config(module.load_config(template_path), allow_unarmed=True)
    return {
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
        "protected_document_ids": sorted(PROTECTED_DOCUMENT_IDS),
        "base_config_unchanged": True,
        "predecessor_config_sha256": protected_before,
        "source_controller_sha256": evidence["source_controller_sha256"],
    }


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
    )
    if args.bind:
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
        if completed.returncode != 0:
            raise SuccessorBindingError("generated successor binder failed closed")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
