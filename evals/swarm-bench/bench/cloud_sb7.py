#!/usr/bin/env python3
"""Persistent SB7 cloud build, hermetic-score and publication coordinator.

The build and score state machines are deliberately separate.  A build owns one
immutable binary, fixture seed, vendor port, profile root, process group and raw
tree.  Scoring always operates on a disposable clone and never mutates the raw
tree.  Provider lanes sharing one credential serialize unless their manifest
explicitly records independently proven concurrency.  A successful hermetic
score is staged once, published under a stable document id, revalidated, and
accepted only after both the Sanity receipt and rendered public pages match.
"""

from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import html.parser
import json
import math
import os
import re
import selectors
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, Mapping

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
DEFAULT_ENTRANTS = HERE / "cloud-sb7-entrants.json"
DEFAULT_SECRET_FILE = (
    Path.home() / ".agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env"
)
DEFAULT_ROOT = Path.home() / "goose-builds/cloud-sb7-20260823"
TERMINAL_BUILD_STATES = {
    "BUILD_COMPLETE",
    "INCOMPLETE",
    "PRE_ADMISSION_FAILURE",
    "STOPPED",
}
RETRYABLE_BUILD_STATES = {"PLANNED", "PRE_ADMISSION_FAILURE"}
RESTARTABLE_CAMPAIGN_STATES = {
    "INITIALIZED",
    "RUNNING",
    "BUILD_COMPLETE",
    "SCORING",
    "ATTENTION",
}
POST_BUILD_STATES = {
    "SCORING",
    "SCORE_FAILED",
    "SCORED",
    "PUBLISH_VALIDATING",
    "PUBLISH_VALIDATED",
    "PUBLISHING",
    "PUBLISHED_UNVERIFIED",
    "REVALIDATING",
    "REVALIDATED",
    "VERIFYING_RENDERED",
    "PUBLISH_FAILED",
    "PUBLISHED",
}
BUILD_SUCCESS_STATES = {"BUILD_COMPLETE"} | POST_BUILD_STATES
CAMPAIGN_SCHEMA = 2
SUPERSESSION_SCHEMA = 1
SUPERSESSION_RECEIPT = "supersession-receipt.json"
QUALIFICATION_RESTART_SCHEMA = 1
QUALIFICATION_RESTART_RECEIPT = "qualification-restart-receipt.json"
QUALIFICATION_RESTART_SEAL = "qualification-restart-seal.json"
QUALIFICATION_RESTART_EVIDENCE = "qualification-restart-evidence"
QUALIFICATION_HISTORY_PATH = "qualification/qualification-restart.json"
SUPERSESSION_ALLOWED_INSTRUMENT_CHANGES = {
    "evals/swarm-bench/bench/cloud_sb7.py",
}
QUALIFICATION_ALLOWED_INSTRUMENT_CHANGES = {
    "evals/swarm-bench/bench/cloud_sb7.py",
    "evals/swarm-bench/bench/cloud-sb7-entrants.json",
}
QUALIFICATION_ALLOWED_ENDPOINT_TRANSITIONS = {
    ("zai_api", "glm-5.3"): {
        "source": {
            "endpoint_family": "https://api.z.ai/api/paas/v4",
            "base_url_env": None,
        },
        "target": {
            "endpoint_family": "https://api.z.ai/api/coding/paas/v4",
            "base_url_env": "ZAI_API_BASE_URL",
        },
    }
}
QUALIFICATION_PUBLISHER_RUNTIME_FIELDS = (
    "mode",
    "website_base_url",
    "revalidate_endpoint",
    "verify_timeout_seconds",
    "verify_interval_seconds",
    "process_timeout_seconds",
)
QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION = {
    "source": {
        "commit": "817b5367bd8a176c45aff1bdc1c0fb2bea32ea4a",
        "instrument_set_sha256": (
            "b6ab4f36cd217d491ff1e928059bc74ef67a6361b6be9f7c88df06c705862384"
        ),
        "tracked_hashes": {
            "scripts/seed-baseline-sb7.mjs": (
                "8ea74cca6f15938245aca72e7abf612c2f203398eb4c1c38639f1ad5a26ff65c"
            ),
            "scripts/lib/sb7-cloud-publisher.mjs": (
                "7f24c787a6d747b137a383f0f5a03112fb32951e522ec1f29965d612847b58a3"
            ),
            "scripts/data/sb7-cloud-entrants.json": (
                "b9768381e373f24cfee7225120923d8b5838fe25725d8f7c7b898beadca70cfa"
            ),
            "package.json": (
                "3129a0f4cac40d2687053ceb7651f24a260af02926cdc9bd4d6d0befe533bb9e"
            ),
            "package-lock.json": (
                "3f1463f43e8a36232fa64796c5a979d8a1a36784c930f6aead92e6d3d212748d"
            ),
        },
    },
    "target": {
        "commit": "694927b0b610c93f0c34dee01004c6def367e670",
        "instrument_set_sha256": (
            "5bb8138f206aea054076c6100b0f6aa94d82f31e154ddd46babc214a8ddc4de7"
        ),
        "tracked_hashes": {
            "scripts/seed-baseline-sb7.mjs": (
                "c2a03dea1ba42b64f71350e8c1fc144fe25a8edfedfe51ccb8f6e12453e529de"
            ),
            "scripts/lib/sb7-cloud-publisher.mjs": (
                "c479e4718aa733cf7848ebbbf1ebff2195894fea2a833852632bf729cfc39177"
            ),
            "scripts/data/sb7-cloud-entrants.json": (
                "b9768381e373f24cfee7225120923d8b5838fe25725d8f7c7b898beadca70cfa"
            ),
            "package.json": (
                "3129a0f4cac40d2687053ceb7651f24a260af02926cdc9bd4d6d0befe533bb9e"
            ),
            "package-lock.json": (
                "3f1463f43e8a36232fa64796c5a979d8a1a36784c930f6aead92e6d3d212748d"
            ),
        },
    },
    "changed_tracked_files": {
        "scripts/seed-baseline-sb7.mjs",
        "scripts/lib/sb7-cloud-publisher.mjs",
    },
    "raw_scorer_version": "sb-7.0-rc",
    "public_scorer_version": "sb-7.0",
}
REQUIRED_BINARY_MARKERS = (
    "GOOSE_PROVIDER_LIFECYCLE_FILE",
    "GOOSE_PROVIDER_LIFECYCLE_STRICT",
    "GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES",
    "GOOSE_BENCH_BUDGET_CONFIG",
    "GOOSE_BENCH_BUDGET_CONFIG_SHA256",
    "GOOSE_BENCH_BUDGET_LEDGER",
    "GOOSE_BENCH_EXPECTED_PROVIDER",
    "GOOSE_BENCH_SECRET_ENV_NAME",
    "GOOSE_BENCH_TOOL_ALLOWLIST",
    "GOOSE_TOOL_SANDBOX_ROOT",
    "GOOSE_TOOL_SANDBOX_DENY_LOCAL_PORTS",
)
PUBLISHER_SCRIPT = Path("scripts/seed-baseline-sb7.mjs")
PUBLISHER_MANIFEST = Path("scripts/data/sb7-cloud-entrants.json")
PUBLISHER_FILES = (
    PUBLISHER_SCRIPT,
    Path("scripts/lib/sb7-cloud-publisher.mjs"),
    PUBLISHER_MANIFEST,
    Path("package.json"),
    Path("package-lock.json"),
)
PUBLISHER_RUNTIME_PACKAGES = ("@sanity/client", "dotenv")
PUBLISHER_REQUIRED_ENV = ("SANITY_WRITE_TOKEN", "NEXT_PUBLIC_SANITY_PROJECT_ID")
DEFAULT_WEBSITE_BASE_URL = "https://leanzero.net"
DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS = 900.0
DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS = 15.0
DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS = 900.0
INTERRUPTED_PUBLICATION_STATES = {
    "PUBLISH_VALIDATING",
    "PUBLISHING",
    "REVALIDATING",
    "VERIFYING_RENDERED",
}
SMOKE_MAX_TURNS = 3
SMOKE_PROOF_SCHEMA = 1
SMOKE_NONCE_NAME = "contract-smoke-nonce.bin"
SMOKE_TERMINAL_STATES = {"PASS", "FAILED", "PRE_ADMISSION_FAILURE", "STOPPED"}
SMOKE_RETRYABLE_STATES = {"PLANNED", "PRE_ADMISSION_FAILURE"}
SMOKE_PREPARABLE_STATES = SMOKE_RETRYABLE_STATES | {"WAITING_PROVIDER_LANE"}
MONITOR_TERMINAL_STATES = {"PUBLISHED", "ATTENTION", "STOPPED"}
MONITOR_DETACH_TIMEOUT_SECONDS = 5.0


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha1_file(path: Path) -> str:
    h = hashlib.sha1()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_tree_exact(root: Path) -> str:
    if not root.is_dir() or root.is_symlink():
        raise SystemExit(f"runtime package is missing or linked: {root}")
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise SystemExit(f"runtime package contains a symbolic link: {path}")
        if not path.is_file():
            continue
        relative = str(path.relative_to(root)).encode()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def artifact_tree_sha256(
    root: Path, *, excluded_relative_paths: Iterable[str] = ()
) -> str:
    if not root.is_dir() or root.is_symlink():
        raise SystemExit(f"artifact tree is missing or linked: {root}")
    excluded = set(excluded_relative_paths)
    digest = hashlib.sha256()

    def walk_failed(error: OSError) -> None:
        raise SystemExit(f"artifact tree cannot be read: {error}")

    for directory, names, files in os.walk(
        root, followlinks=False, onerror=walk_failed
    ):
        names.sort()
        files.sort()
        base = Path(directory)
        for name in [*names, *files]:
            path = base / name
            relative_text = str(path.relative_to(root))
            if relative_text in excluded:
                continue
            relative = relative_text.encode()
            digest.update(len(relative).to_bytes(8, "big"))
            digest.update(relative)
            if path.is_symlink():
                digest.update(b"L")
                target = os.readlink(path).encode()
                digest.update(len(target).to_bytes(8, "big"))
                digest.update(target)
            elif path.is_dir():
                digest.update(b"D")
            elif path.is_file():
                digest.update(b"F")
                with path.open("rb") as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
            else:
                raise SystemExit(f"artifact tree contains a special file: {path}")
    return digest.hexdigest()


def optional_artifact_tree_sha256(root: Path) -> str | None:
    return artifact_tree_sha256(root) if root.is_dir() else None


def fsync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def atomic_copy(source: Path, destination: Path, mode: int | None = None) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, raw = tempfile.mkstemp(prefix=f".{destination.name}.", dir=destination.parent)
    try:
        with source.open("rb") as incoming, os.fdopen(descriptor, "wb") as outgoing:
            shutil.copyfileobj(incoming, outgoing)
            outgoing.flush()
            os.fsync(outgoing.fileno())
        os.chmod(raw, mode if mode is not None else source.stat().st_mode & 0o777)
        os.replace(raw, destination)
        fsync_directory(destination.parent)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(raw)


def write_exclusive_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        try:
            os.link(temporary, path)
        except FileExistsError:
            if path.read_bytes() != payload:
                raise SystemExit(
                    f"immutable receipt already exists with different content: {path}"
                ) from None
        fsync_directory(path.parent)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary)


def atomic_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, raw = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(raw, path)
        fsync_directory(path.parent)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(raw)


def load_json(path: Path) -> Dict[str, Any]:
    def unique_object(pairs: list[tuple[str, Any]]) -> Dict[str, Any]:
        value: Dict[str, Any] = {}
        for key, nested in pairs:
            if key in value:
                raise ValueError(f"duplicate object key: {key}")
            value[key] = nested
        return value

    try:
        with path.open() as stream:
            value = json.load(stream, object_pairs_hook=unique_object)
    except (json.JSONDecodeError, ValueError) as error:
        raise SystemExit(f"invalid JSON in {path}: {error}") from None
    if not isinstance(value, dict):
        raise SystemExit(f"expected an object in {path}")
    return value


def parse_secret_file(path: Path) -> Dict[str, str]:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"secret file is missing: {path}")
    mode = path.stat().st_mode & 0o777
    if mode & 0o077:
        raise SystemExit(f"secret file must be mode 0600, found {mode:04o}: {path}")
    values: Dict[str, str] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if line.startswith("export "):
            line = line[7:].strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip().strip("'\"")
    return values


def parse_env_file(path: Path) -> Dict[str, str]:
    if not path.is_file():
        raise SystemExit(f"environment file is missing: {path}")
    return parse_env_text(path.read_text())


def parse_env_text(raw_text: str) -> Dict[str, str]:
    values: Dict[str, str] = {}
    for raw in raw_text.splitlines():
        line = raw.strip()
        if line.startswith("export "):
            line = line[7:].strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip().strip("'\"")
    return values


def normalized_website_base_url(value: str) -> str:
    try:
        parsed = urllib.parse.urlsplit(value)
        parsed.port
    except ValueError:
        raise SystemExit("website base URL is malformed") from None
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.path not in {"", "/"}
    ):
        raise SystemExit(
            "website base URL must be an https origin without credentials, path, "
            "query, or fragment"
        )
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, "", "", ""))


def normalized_provider_endpoint(value: Any) -> str:
    if not isinstance(value, str):
        raise SystemExit("provider endpoint must be a string")
    try:
        parsed = urllib.parse.urlsplit(value)
        parsed.port
    except ValueError:
        raise SystemExit("provider endpoint is malformed") from None
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or (parsed.path and not parsed.path.startswith("/"))
    ):
        raise SystemExit(
            "provider endpoint must be an https base URL without credentials, "
            "query, or fragment"
        )
    return urllib.parse.urlunsplit(
        (parsed.scheme, parsed.netloc, parsed.path.rstrip("/"), "", "")
    )


def entrants(manifest: Mapping[str, Any]) -> list[Dict[str, Any]]:
    rows = manifest.get("entrants")
    if not isinstance(rows, list) or not rows:
        raise SystemExit("entrant manifest has no entrants")
    out: list[Dict[str, Any]] = []
    seen_ids: set[str] = set()
    seen_ports: set[int] = set()
    required = {
        "id",
        "provider",
        "model",
        "secret_env",
        "provider_lane",
        "endpoint_family",
        "thinking_effort",
        "context_limit",
        "max_output_tokens",
        "accepted_reported_models",
        "vendor_port",
        "pricing",
    }
    for raw in rows:
        if not isinstance(raw, dict):
            raise SystemExit("entrant manifest rows must be objects")
        missing = required - raw.keys()
        if missing:
            raise SystemExit(f"entrant is missing {sorted(missing)}: {raw.get('id')}")
        entrant_id = str(raw["id"])
        port = int(raw["vendor_port"])
        if entrant_id in seen_ids:
            raise SystemExit(f"duplicate entrant id: {entrant_id}")
        if port in seen_ports:
            raise SystemExit(f"duplicate vendor port: {port}")
        if port < 8899:
            raise SystemExit(
                f"SB7 vendor ports must be >= 8899: {entrant_id} -> {port}"
            )
        base_url_env = raw.get("base_url_env")
        if base_url_env is not None and (
            not isinstance(base_url_env, str)
            or not re.fullmatch(r"[A-Z][A-Z0-9_]*", base_url_env)
            or base_url_env == raw["secret_env"]
        ):
            raise SystemExit(f"invalid non-secret base URL environment: {entrant_id}")
        endpoint_family = (
            normalized_provider_endpoint(raw["endpoint_family"])
            if base_url_env is not None
            else str(raw["endpoint_family"])
        )
        seen_ids.add(entrant_id)
        seen_ports.add(port)
        row = dict(raw)
        row["endpoint_family"] = endpoint_family
        out.append(row)
    return out


def spend_policy(
    manifest: Mapping[str, Any], rows: Iterable[Mapping[str, Any]]
) -> Dict[str, Any]:
    rows = list(rows)
    policy = manifest.get("spend_policy")
    if not isinstance(policy, dict):
        raise SystemExit("entrant manifest has no spend_policy")
    if policy.get("terminal_safe_retry_limit") != 0:
        raise SystemExit("cloud benchmark transport retry policy must be exactly zero")
    if policy.get("max_full_episodes_per_model") != 2:
        raise SystemExit("cloud benchmark allows one initial episode plus one restart")
    if policy.get("launch_all_entrants_concurrently") is not True:
        raise SystemExit("cloud benchmark must launch every entrant concurrently")
    lanes = [str(row["provider_lane"]) for row in rows]
    if len(set(lanes)) != len(lanes):
        raise SystemExit("concurrent cloud entrants require distinct provider lanes")
    total_cap = float(policy.get("total_cap", 0))
    provider_caps = policy.get("provider_caps")
    if total_cap <= 0 or not isinstance(provider_caps, dict):
        raise SystemExit("spend_policy requires a positive total_cap and provider_caps")
    if sum(float(value) for value in provider_caps.values()) > total_cap:
        raise SystemExit("provider spend caps exceed the total campaign cap")
    for row in rows:
        provider = str(row["provider"])
        if float(provider_caps.get(provider, 0)) <= 0:
            raise SystemExit(f"spend_policy has no positive cap for {provider}")
        pricing = row.get("pricing")
        if not isinstance(pricing, dict):
            raise SystemExit(f"entrant has no pricing record: {row['id']}")
        for key in ("input_per_million", "output_per_million", "source", "verified_at"):
            if key not in pricing:
                raise SystemExit(f"pricing for {row['id']} is missing {key}")
        input_rate = float(
            pricing.get(
                "input_over_threshold_per_million", pricing["input_per_million"]
            )
        )
        output_rate = float(
            pricing.get(
                "output_over_threshold_per_million", pricing["output_per_million"]
            )
        )
        if input_rate < 0 or output_rate < 0:
            raise SystemExit(f"pricing rates must be non-negative: {row['id']}")
        reported_models = row["accepted_reported_models"]
        if (
            not isinstance(reported_models, list)
            or not reported_models
            or any(not isinstance(model, str) or not model for model in reported_models)
            or len(set(reported_models)) != len(reported_models)
        ):
            raise SystemExit(
                f"accepted reported models must be an explicit non-empty unique list: {row['id']}"
            )
        worst_single = (
            int(row["context_limit"]) * input_rate
            + int(row["max_output_tokens"]) * output_rate
        ) / 1_000_000
        if worst_single > float(provider_caps[provider]):
            raise SystemExit(
                f"one worst-case {row['id']} request (${worst_single:.2f}) exceeds "
                f"the {provider} cap"
            )
    return dict(policy)


def smoke_max_turns(manifest: Mapping[str, Any]) -> int:
    value = manifest.get("smoke_max_turns")
    if isinstance(value, bool) or not isinstance(value, int):
        raise SystemExit("entrant manifest smoke_max_turns must be an integer")
    if value != SMOKE_MAX_TURNS:
        raise SystemExit(
            f"cloud contract smoke max turns must be exactly {SMOKE_MAX_TURNS}"
        )
    return value


def validated_campaign_lineage(campaign: Mapping[str, Any]) -> Dict[str, Any]:
    lineage = campaign.get("lineage")
    if not isinstance(lineage, dict):
        raise SystemExit("campaign has no explicit smoke lineage")
    generation = lineage.get("generation")
    predecessor_id = lineage.get("predecessor_campaign_id")
    predecessor_contract = lineage.get("predecessor_contract_sha256")
    if (
        isinstance(generation, bool)
        or not isinstance(generation, int)
        or generation < 0
    ):
        raise SystemExit("campaign smoke lineage generation is invalid")
    if generation == 0:
        if predecessor_id is not None or predecessor_contract is not None:
            raise SystemExit("root campaign smoke lineage cannot name a predecessor")
    elif (
        not isinstance(predecessor_id, str)
        or not predecessor_id
        or not isinstance(predecessor_contract, str)
        or re.fullmatch(r"[0-9a-f]{64}", predecessor_contract) is None
    ):
        raise SystemExit("successor campaign smoke lineage is incomplete")
    return {
        "generation": generation,
        "predecessor_campaign_id": predecessor_id,
        "predecessor_contract_sha256": predecessor_contract,
    }


def validated_qualification_history(
    campaign: Mapping[str, Any],
) -> Dict[str, Any] | None:
    history = campaign.get("qualification_history")
    if history is None:
        return None
    expected = {
        "restart_count",
        "transition_id",
        "subject_root",
        "source_campaign_id",
        "source_contract_sha256",
        "path",
        "sha256",
    }
    if not isinstance(history, dict) or set(history) != expected:
        raise SystemExit("campaign qualification history pointer is malformed")
    if history.get("restart_count") != 1:
        raise SystemExit("campaign qualification restart count is not exactly one")
    if history.get("path") != QUALIFICATION_HISTORY_PATH:
        raise SystemExit("campaign qualification history path is not frozen")
    for key in (
        "transition_id",
        "subject_root",
        "source_campaign_id",
        "source_contract_sha256",
        "sha256",
    ):
        value = history.get(key)
        if not isinstance(value, str) or not value:
            raise SystemExit(f"campaign qualification history has no {key}")
    for key in ("source_contract_sha256", "sha256"):
        if re.fullmatch(r"[0-9a-f]{64}", str(history[key])) is None:
            raise SystemExit(f"campaign qualification history has invalid {key}")
    return dict(history)


def smoke_contract_identity(campaign: Mapping[str, Any]) -> str:
    lineage = validated_campaign_lineage(campaign)
    qualification_history = validated_qualification_history(campaign)
    normalized: Dict[str, Dict[str, list[str]]] = {}
    for field in (
        "smoke_budget_settled_baselines",
        "smoke_budget_outstanding_baselines",
    ):
        baselines = campaign.get(field)
        if not isinstance(baselines, dict) or not baselines:
            raise SystemExit(f"campaign has no {field}")
        normalized_baselines: Dict[str, list[str]] = {}
        for entrant_id, request_ids in sorted(baselines.items()):
            if (
                not isinstance(entrant_id, str)
                or not entrant_id
                or not isinstance(request_ids, list)
                or any(not isinstance(value, str) or not value for value in request_ids)
                or request_ids != sorted(set(request_ids))
            ):
                raise SystemExit(f"campaign {field} is malformed")
            normalized_baselines[entrant_id] = request_ids
        normalized[field] = normalized_baselines
    if set(normalized["smoke_budget_settled_baselines"]) != set(
        normalized["smoke_budget_outstanding_baselines"]
    ):
        raise SystemExit("campaign smoke budget baseline entrant sets differ")
    payload = {
        "schema_version": CAMPAIGN_SCHEMA,
        "campaign_id": campaign.get("campaign_id"),
        "lineage": lineage,
        "binary_sha256": campaign.get("binary_sha256"),
        "instrument_set_sha256": campaign.get("instrument_set_sha256"),
        "entrant_manifest_sha256": campaign.get("entrant_manifest_sha256"),
        "budget_config_sha256": campaign.get("budget_config_sha256"),
        "smoke_max_turns": campaign.get("smoke_max_turns"),
        **normalized,
    }
    if qualification_history is not None:
        payload["qualification_history"] = qualification_history
    for field in (
        "campaign_id",
        "binary_sha256",
        "instrument_set_sha256",
        "entrant_manifest_sha256",
        "budget_config_sha256",
    ):
        if not isinstance(payload[field], str) or not payload[field]:
            raise SystemExit(f"campaign smoke contract has no {field}")
    if payload["smoke_max_turns"] != SMOKE_MAX_TURNS:
        raise SystemExit("campaign smoke contract has the wrong max-turn limit")
    return sha256_bytes(
        json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    )


def binary_missing_markers(binary: Path) -> list[str]:
    needles = {marker: marker.encode() for marker in REQUIRED_BINARY_MARKERS}
    found: set[str] = set()
    tail = b""
    with binary.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            data = tail + chunk
            for marker, needle in needles.items():
                if marker not in found and needle in data:
                    found.add(marker)
            if len(found) == len(needles):
                break
            tail = data[-max(map(len, needles.values())) :]
    return sorted(set(needles) - found)


def campaign_file(root: Path) -> Path:
    return root / "campaign.json"


def state_file(root: Path, entrant_id: str) -> Path:
    return root / "entrants" / entrant_id / "state.json"


def smoke_state_file(root: Path, entrant_id: str) -> Path:
    return root / "smoke" / entrant_id / "state.json"


def read_state(root: Path, entrant_id: str) -> Dict[str, Any]:
    return load_json(state_file(root, entrant_id))


def update_state(root: Path, entrant_id: str, **changes: Any) -> Dict[str, Any]:
    path = state_file(root, entrant_id)
    lock_path = path.with_suffix(".lock")
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            state = load_json(path)
            state.update(changes)
            state["updated_at"] = utc_now()
            atomic_json(path, state)
            return state
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def read_smoke_state(root: Path, entrant_id: str) -> Dict[str, Any]:
    return load_json(smoke_state_file(root, entrant_id))


def update_smoke_state(root: Path, entrant_id: str, **changes: Any) -> Dict[str, Any]:
    path = smoke_state_file(root, entrant_id)
    lock_path = path.with_suffix(".lock")
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            state = load_json(path)
            state.update(changes)
            state["updated_at"] = utc_now()
            atomic_json(path, state)
            return state
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def manager_state(root: Path, **changes: Any) -> Dict[str, Any]:
    path = root / "manager.json"
    lock_path = root / "manager.lock"
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            state = (
                load_json(path)
                if path.is_file()
                else {"schema_version": CAMPAIGN_SCHEMA}
            )
            state.update(changes)
            state["updated_at"] = utc_now()
            atomic_json(path, state)
            return state
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def read_monitor_state(root: Path) -> Dict[str, Any]:
    path = root / "monitor.json"
    return load_json(path) if path.is_file() else {"schema_version": CAMPAIGN_SCHEMA}


def monitor_state(root: Path, **changes: Any) -> Dict[str, Any]:
    path = root / "monitor.json"
    lock_path = root / "monitor.lock"
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            state = read_monitor_state(root)
            state.update(changes)
            state["heartbeat_at"] = utc_now()
            atomic_json(path, state)
            return state
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def update_campaign(root: Path, **changes: Any) -> Dict[str, Any]:
    path = campaign_file(root)
    lock_path = root / "campaign.lock"
    with lock_path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            campaign = load_json(path)
            campaign.update(changes)
            campaign["updated_at"] = utc_now()
            atomic_json(path, campaign)
            return campaign
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def port_is_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            sock.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


def git_value(*args: str) -> str:
    proc = subprocess.run(
        ["git", *args], cwd=REPO, text=True, capture_output=True, check=False
    )
    return proc.stdout.strip() if proc.returncode == 0 else "unknown"


def git_value_at(repo: Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", *args], cwd=repo, text=True, capture_output=True, check=False
    )
    if proc.returncode != 0:
        detail = proc.stderr.strip().splitlines()
        suffix = f": {detail[-1]}" if detail else ""
        raise SystemExit(f"git {' '.join(args)} failed in {repo}{suffix}")
    return proc.stdout.strip()


def publisher_entries(
    manifest: Mapping[str, Any], rows: Iterable[Mapping[str, Any]]
) -> Dict[str, Dict[str, str]]:
    raw_entries = manifest.get("entrants")
    if not isinstance(raw_entries, list):
        raise SystemExit("publisher manifest has no entrants array")
    by_key: Dict[str, Mapping[str, Any]] = {}
    for raw in raw_entries:
        if not isinstance(raw, dict) or not isinstance(raw.get("key"), str):
            raise SystemExit("publisher manifest contains a malformed entrant")
        key = str(raw["key"])
        if key in by_key:
            raise SystemExit(f"publisher manifest repeats entrant: {key}")
        by_key[key] = raw

    resolved: Dict[str, Dict[str, str]] = {}
    for row in rows:
        entrant_id = str(row["id"])
        entry = by_key.get(entrant_id)
        if entry is None:
            raise SystemExit(
                f"publisher manifest is missing cloud entrant: {entrant_id}"
            )
        model = str(entry.get("model", ""))
        if model != str(row["model"]):
            raise SystemExit(
                f"publisher model mismatch for {entrant_id}: {model or '<missing>'}"
            )
        label = str(entry.get("label", "")).strip()
        doc_id = str(entry.get("docId", ""))
        stable_stem = re.sub(r"[^a-z0-9]+", "-", entrant_id).strip("-")
        expected_doc_id = f"brun-baseline-{stable_stem}-sb70"
        if not label:
            raise SystemExit(f"publisher label is missing for {entrant_id}")
        if doc_id != expected_doc_id:
            raise SystemExit(
                f"publisher doc id for {entrant_id} must be {expected_doc_id}"
            )
        resolved[entrant_id] = {
            "key": entrant_id,
            "label": label,
            "model": model,
            "doc_id": doc_id,
        }
    return resolved


def resolve_runtime_package(repo: Path, start: Path, name: str) -> Path | None:
    cursor = start
    while True:
        candidate = cursor / "node_modules" / name
        if (candidate / "package.json").is_file():
            if candidate.is_symlink():
                raise SystemExit(f"publisher runtime package is linked: {candidate}")
            try:
                candidate.resolve().relative_to(repo.resolve())
            except ValueError:
                raise SystemExit(
                    f"publisher runtime package escapes the website repo: {candidate}"
                ) from None
            return candidate
        if cursor == repo:
            return None
        parent = cursor.parent
        if parent == cursor:
            return None
        cursor = parent


def publisher_runtime_hashes(repo: Path) -> Dict[str, str]:
    pending = [(name, repo, True) for name in PUBLISHER_RUNTIME_PACKAGES]
    packages: Dict[str, str] = {}
    visited: set[Path] = set()
    while pending:
        name, start, required = pending.pop(0)
        package = resolve_runtime_package(repo, start, name)
        if package is None:
            if required:
                raise SystemExit(
                    f"publisher runtime dependency cannot be resolved: {name}"
                )
            continue
        resolved = package.resolve()
        if resolved in visited:
            continue
        visited.add(resolved)
        manifest = load_json(package / "package.json")
        relative = str(package.relative_to(repo))
        packages[relative] = sha256_tree_exact(package)

        dependencies = manifest.get("dependencies")
        if dependencies is not None and not isinstance(dependencies, dict):
            raise SystemExit(
                f"publisher package dependencies are malformed: {relative}"
            )
        for dependency in sorted((dependencies or {}).keys()):
            pending.append((str(dependency), package, True))

        optional = manifest.get("optionalDependencies")
        if optional is not None and not isinstance(optional, dict):
            raise SystemExit(
                f"publisher package optionalDependencies are malformed: {relative}"
            )
        for dependency in sorted((optional or {}).keys()):
            pending.append((str(dependency), package, False))

        peers = manifest.get("peerDependencies")
        peer_meta = manifest.get("peerDependenciesMeta")
        if peers is not None and not isinstance(peers, dict):
            raise SystemExit(
                f"publisher package peerDependencies are malformed: {relative}"
            )
        if peer_meta is not None and not isinstance(peer_meta, dict):
            raise SystemExit(
                f"publisher package peerDependenciesMeta are malformed: {relative}"
            )
        for dependency in sorted((peers or {}).keys()):
            metadata = (peer_meta or {}).get(dependency, {})
            is_optional = (
                isinstance(metadata, dict) and metadata.get("optional") is True
            )
            pending.append((str(dependency), package, not is_optional))
    return dict(sorted(packages.items()))


def read_publisher_env(repo: Path) -> tuple[Dict[str, Any], Dict[str, str]]:
    env_file = repo / ".env.local"
    if env_file.is_symlink():
        raise SystemExit(
            f"publisher environment file cannot be a symbolic link: {env_file}"
        )
    if not env_file.is_file():
        raise SystemExit(f"environment file is missing: {env_file}")
    mode = env_file.stat().st_mode & 0o777
    if mode & 0o077:
        raise SystemExit(
            f"publisher environment file must be mode 0600, found {mode:04o}: "
            f"{env_file}"
        )
    try:
        raw = env_file.read_bytes()
        values = parse_env_text(raw.decode())
    except UnicodeDecodeError:
        raise SystemExit(
            f"publisher environment file is not UTF-8: {env_file}"
        ) from None
    missing_env = [name for name in PUBLISHER_REQUIRED_ENV if not values.get(name)]
    if missing_env:
        raise SystemExit(
            f"publisher .env.local is missing variables: {', '.join(missing_env)}"
        )
    project_id = values["NEXT_PUBLIC_SANITY_PROJECT_ID"]
    dataset = values.get("NEXT_PUBLIC_SANITY_DATASET", "production")
    if not re.fullmatch(r"[a-z0-9-]+", project_id):
        raise SystemExit("publisher Sanity project id is malformed")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", dataset):
        raise SystemExit("publisher Sanity dataset is malformed")
    identity = {
        "env_file": str(env_file),
        "env_file_mode": f"{mode:04o}",
        "env_file_sha256": sha256_bytes(raw),
        "sanity_target": {
            "project_id": project_id,
            "dataset": dataset,
        },
    }
    return identity, values


def publisher_env_identity(repo: Path) -> Dict[str, Any]:
    identity, _ = read_publisher_env(repo)
    return identity


def publisher_snapshot(
    publisher_repo: Path, rows: Iterable[Mapping[str, Any]]
) -> Dict[str, Any]:
    repo = publisher_repo.resolve()
    if not repo.is_dir():
        raise SystemExit(f"publisher repo is missing: {repo}")
    top = Path(git_value_at(repo, "rev-parse", "--show-toplevel")).resolve()
    if top != repo:
        raise SystemExit(f"publisher repo must be its git root: {repo} (found {top})")
    dirty = git_value_at(repo, "status", "--porcelain", "--untracked-files=all")
    if dirty:
        raise SystemExit("publisher website worktree must be clean before it is pinned")

    tracked_hashes: Dict[str, str] = {}
    for relative in PUBLISHER_FILES:
        path = repo / relative
        if not path.is_file():
            raise SystemExit(f"publisher input is missing: {path}")
        git_value_at(repo, "ls-files", "--error-unmatch", str(relative))
        tracked_hashes[str(relative)] = sha256_file(path)

    runtime_hashes = publisher_runtime_hashes(repo)

    node_from_path = shutil.which("node")
    if not node_from_path:
        raise SystemExit("node is not available for the website publisher")
    node_path = Path(node_from_path).resolve()
    node_version = subprocess.run(
        [str(node_path), "--version"],
        text=True,
        capture_output=True,
        check=False,
    )
    if node_version.returncode != 0 or not node_version.stdout.strip():
        raise SystemExit(f"cannot execute pinned publisher node runtime: {node_path}")

    env_identity = publisher_env_identity(repo)

    manifest = load_json(repo / PUBLISHER_MANIFEST)
    entries = publisher_entries(manifest, rows)
    expected_checks = manifest.get("expectedChecks")
    if not isinstance(expected_checks, int) or expected_checks <= 0:
        raise SystemExit("publisher manifest expectedChecks must be a positive integer")
    all_hashes = {
        **tracked_hashes,
        **runtime_hashes,
        ".env.local": str(env_identity["env_file_sha256"]),
    }
    return {
        "repo": str(repo),
        "commit": git_value_at(repo, "rev-parse", "HEAD"),
        "branch": git_value_at(repo, "branch", "--show-current"),
        "script": str(PUBLISHER_SCRIPT),
        "manifest": str(PUBLISHER_MANIFEST),
        "tracked_hashes": tracked_hashes,
        "runtime_hashes": runtime_hashes,
        "instrument_set_sha256": sha256_bytes(
            json.dumps(all_hashes, sort_keys=True).encode()
        ),
        "node": {
            "path": str(node_path),
            "sha256": sha256_file(node_path),
            "version": node_version.stdout.strip(),
        },
        **env_identity,
        "required_env_present": list(PUBLISHER_REQUIRED_ENV),
        "expected_checks": expected_checks,
        "entries": entries,
    }


def publisher_mismatch(campaign: Mapping[str, Any]) -> str | None:
    expected = campaign.get("publisher")
    if not isinstance(expected, dict):
        return "campaign has no pinned publisher"
    try:
        manifest = load_json(Path(str(campaign["entrant_manifest"])))
        current = publisher_snapshot(Path(str(expected["repo"])), entrants(manifest))
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return f"pinned publisher cannot be verified: {error}"
    compared = (
        "repo",
        "commit",
        "script",
        "manifest",
        "tracked_hashes",
        "runtime_hashes",
        "instrument_set_sha256",
        "node",
        "env_file",
        "env_file_mode",
        "env_file_sha256",
        "sanity_target",
        "expected_checks",
        "entries",
    )
    changed = [key for key in compared if current.get(key) != expected.get(key)]
    if changed:
        return f"publisher changed after freeze: {', '.join(changed)}"
    return None


def freeze_publisher_runtime(
    destination: Path, publisher: Mapping[str, Any]
) -> Dict[str, Any]:
    repo = Path(str(publisher["repo"]))
    destination.mkdir(parents=True, exist_ok=False)
    tracked = publisher.get("tracked_hashes")
    runtime = publisher.get("runtime_hashes")
    if not isinstance(tracked, dict) or not isinstance(runtime, dict):
        raise SystemExit("publisher snapshot has no executable input hashes")
    for relative in tracked:
        source = repo / str(relative)
        target = destination / str(relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    for relative in runtime:
        source = repo / str(relative)
        target = destination / str(relative)
        shutil.copytree(source, target, dirs_exist_ok=True)

    copied_tracked = {
        str(relative): sha256_file(destination / str(relative)) for relative in tracked
    }
    copied_runtime = {
        str(relative): sha256_tree_exact(destination / str(relative))
        for relative in runtime
    }
    if copied_tracked != tracked or copied_runtime != runtime:
        raise SystemExit("publisher executable inputs changed while they were frozen")
    return {
        "root": str(destination),
        "tracked_hashes": copied_tracked,
        "runtime_hashes": copied_runtime,
        "instrument_set_sha256": sha256_bytes(
            json.dumps({**copied_tracked, **copied_runtime}, sort_keys=True).encode()
        ),
    }


def frozen_publisher_mismatch(campaign: Mapping[str, Any]) -> str | None:
    publisher = campaign.get("publisher")
    frozen = publisher.get("frozen") if isinstance(publisher, dict) else None
    if not isinstance(frozen, dict):
        return "campaign has no frozen publisher runtime"
    root = Path(str(frozen.get("root", "")))
    tracked = frozen.get("tracked_hashes")
    runtime = frozen.get("runtime_hashes")
    if not isinstance(tracked, dict) or not isinstance(runtime, dict):
        return "campaign frozen publisher hashes are malformed"
    try:
        current_tracked = {
            str(relative): sha256_file(root / str(relative)) for relative in tracked
        }
        current_runtime = {
            str(relative): sha256_tree_exact(root / str(relative))
            for relative in runtime
        }
    except (OSError, SystemExit) as error:
        return f"frozen publisher runtime cannot be verified: {error}"
    if current_tracked != tracked or current_runtime != runtime:
        return "frozen publisher runtime changed after freeze"
    return None


def instrument_files() -> list[Path]:
    paths = [
        REPO / "evals/swarm-bench/spec-build-sb7.md",
        HERE / "sb7-thresholds.json",
        HERE / "score_sb7.py",
        HERE / "vendor_service_v3.py",
        HERE / "vendor_docs_v3.md",
        HERE / "fixtures_v3.py",
        HERE / "schedule_sb7.py",
        HERE / "product_probe_v3.mjs",
        HERE / "score_build.py",
        HERE / "vendor_service.py",
        HERE / "fixtures.py",
        HERE / "perf_probe.py",
        HERE / "product_probe.mjs",
        HERE / "cloud_sb7.py",
        HERE / "cloud-sb7-entrants.json",
    ]
    paths.extend(sorted((HERE / "probes").glob("*.py")))
    return paths


def instrument_hashes() -> Dict[str, str]:
    result: Dict[str, str] = {}
    for path in instrument_files():
        if not path.is_file():
            raise SystemExit(f"instrument input is missing: {path}")
        result[str(path.relative_to(REPO))] = sha256_file(path)
    return result


def freeze_instrument(
    destination_root: Path,
    source_repo: Path = REPO,
    paths: Iterable[Path] | None = None,
) -> Dict[str, str]:
    destination_root.mkdir(parents=True, exist_ok=False)
    frozen: Dict[str, str] = {}
    selected = list(paths) if paths is not None else instrument_files()
    for source in selected:
        try:
            relative = source.relative_to(source_repo)
        except ValueError:
            raise SystemExit(
                f"instrument input is outside its source repo: {source}"
            ) from None
        if not source.is_file():
            raise SystemExit(f"instrument input is missing: {source}")
        destination = destination_root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        source_hash = sha256_file(source)
        if sha256_file(destination) != source_hash:
            raise SystemExit(f"instrument copy hash mismatch: {relative}")
        frozen[str(relative)] = source_hash
    return frozen


def campaign_instrument_path(campaign: Mapping[str, Any], relative: str) -> Path:
    root = campaign.get("instrument_root")
    if not root:
        raise SystemExit("campaign has no frozen instrument root")
    path = Path(str(root)) / relative
    try:
        path.resolve().relative_to(Path(str(root)).resolve())
    except ValueError:
        raise SystemExit(
            f"instrument path escapes its frozen root: {relative}"
        ) from None
    return path


def instrument_mismatch(campaign: Mapping[str, Any]) -> str | None:
    manifest_path = Path(str(campaign.get("entrant_manifest", "")))
    manifest_hash = campaign.get("entrant_manifest_sha256")
    if (
        not manifest_path.is_file()
        or not isinstance(manifest_hash, str)
        or sha256_file(manifest_path) != manifest_hash
    ):
        return "frozen entrant manifest changed after freeze"
    expected = campaign.get("instrument_hashes")
    if not isinstance(expected, dict) or not expected:
        return "campaign has no frozen instrument hashes"
    current: Dict[str, str] = {}
    missing: list[str] = []
    for relative in expected:
        path = campaign_instrument_path(campaign, str(relative))
        if not path.is_file():
            missing.append(str(relative))
            continue
        current[str(relative)] = sha256_file(path)
    if current == expected:
        return None
    changed = sorted(
        key
        for key in set(expected) | set(current)
        if expected.get(key) != current.get(key)
    )
    changed = sorted(set(changed) | set(missing))
    return f"instrument changed after freeze: {', '.join(changed)}"


def fetch_json(url: str, headers: Mapping[str, str]) -> Dict[str, Any]:
    request = urllib.request.Request(url, headers=dict(headers))
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            value = json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(
            f"authenticated roster failed: HTTP {error.code} from {url}"
        ) from None
    except Exception as error:
        raise SystemExit(
            f"authenticated roster failed: {type(error).__name__} from {url}"
        ) from None
    if not isinstance(value, dict):
        raise SystemExit(f"authenticated roster returned a non-object: {url}")
    return value


def authenticated_rosters(
    secret_values: Mapping[str, str], rows: Iterable[Mapping[str, Any]]
) -> Dict[str, Any]:
    required = ("ZHIPU_API_KEY", "GOOGLE_API_KEY", "DEEPSEEK_API_KEY")
    missing = [name for name in required if not secret_values.get(name)]
    if missing:
        raise SystemExit(f"secret file is missing variables: {', '.join(missing)}")

    rows = list(rows)
    zai_endpoints = {
        str(row["endpoint_family"]).rstrip("/")
        for row in rows
        if row["provider"] == "zai_api"
    }
    if len(zai_endpoints) != 1:
        raise SystemExit("Z.AI entrants must share one explicit endpoint family")
    zai_endpoint = next(iter(zai_endpoints))
    zai = fetch_json(
        f"{zai_endpoint}/models",
        {"Authorization": f"Bearer {secret_values['ZHIPU_API_KEY']}"},
    )
    google = fetch_json(
        "https://generativelanguage.googleapis.com/v1beta/models",
        {"x-goog-api-key": secret_values["GOOGLE_API_KEY"]},
    )
    deepseek = fetch_json(
        "https://api.deepseek.com/models",
        {"Authorization": f"Bearer {secret_values['DEEPSEEK_API_KEY']}"},
    )
    zai_rows = {
        str(row.get("id", "")): dict(row)
        for row in zai.get("data", [])
        if isinstance(row, dict) and row.get("id")
    }
    google_rows = {
        str(row.get("name", "")).split("/")[-1]: dict(row)
        for row in google.get("models", [])
        if isinstance(row, dict) and row.get("name")
    }
    deepseek_rows = {
        str(row.get("id", "")): dict(row)
        for row in deepseek.get("data", [])
        if isinstance(row, dict) and row.get("id")
    }
    reported_models: Dict[str, Dict[str, list[str]]] = {
        "zai_api": {model: [model] for model in zai_rows},
        "google": {},
        "custom_deepseek": {model: [model] for model in deepseek_rows},
    }
    for model, metadata in google_rows.items():
        aliases = {model}
        version = metadata.get("version")
        if isinstance(version, str) and version:
            aliases.add(f"gemini-{version}")
        reported_models["google"][model] = sorted(aliases)
    return {
        "models": {
            "zai_api": set(zai_rows),
            "google": set(google_rows),
            "custom_deepseek": set(deepseek_rows),
        },
        "accepted_reported_models": reported_models,
        "evidence": {
            "zai_api": zai_rows,
            "google": google_rows,
            "custom_deepseek": deepseek_rows,
        },
    }


def validate_rosters(
    rows: Iterable[Mapping[str, Any]], rosters: Mapping[str, Any]
) -> None:
    models = rosters.get("models")
    reported_models = rosters.get("accepted_reported_models")
    evidence = rosters.get("evidence")
    if (
        not isinstance(models, dict)
        or not isinstance(reported_models, dict)
        or not isinstance(evidence, dict)
    ):
        raise SystemExit("authenticated roster snapshot is malformed")
    for row in rows:
        provider = str(row["provider"])
        model = str(row["model"])
        provider_models = models.get(provider)
        if not isinstance(provider_models, set) or model not in provider_models:
            raise SystemExit(
                f"exact model is not in the authenticated {provider} roster: {model}"
            )
        provider_aliases = reported_models.get(provider)
        expected_aliases = (
            provider_aliases.get(model) if isinstance(provider_aliases, dict) else None
        )
        if row["accepted_reported_models"] != expected_aliases:
            raise SystemExit(
                f"accepted reported models do not match authenticated roster metadata: {model}"
            )
        if provider == "google":
            provider_evidence = evidence.get(provider)
            metadata = (
                provider_evidence.get(model)
                if isinstance(provider_evidence, dict)
                else None
            )
            if not isinstance(metadata, dict):
                raise SystemExit(
                    f"authenticated Google roster has no metadata for exact model: {model}"
                )
            for roster_key, manifest_key in (
                ("inputTokenLimit", "context_limit"),
                ("outputTokenLimit", "max_output_tokens"),
            ):
                authenticated_limit = metadata.get(roster_key)
                if isinstance(authenticated_limit, bool) or not isinstance(
                    authenticated_limit, int
                ):
                    raise SystemExit(
                        f"authenticated Google roster has no integer {roster_key}: {model}"
                    )
                if authenticated_limit != int(row[manifest_key]):
                    raise SystemExit(
                        f"manifest {manifest_key} does not match authenticated Google "
                        f"{roster_key} for {model}: {row[manifest_key]} != "
                        f"{authenticated_limit}"
                    )


def preflight(
    binary: Path,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
) -> Dict[str, Any]:
    if sys.platform != "darwin" or not os.access("/usr/bin/sandbox-exec", os.X_OK):
        raise SystemExit("cloud benchmark requires the verified macOS tool sandbox")
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    spend_policy(manifest, rows)
    smoke_max_turns(manifest)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit(f"goose binary is missing or not executable: {binary}")
    missing_markers = binary_missing_markers(binary)
    if missing_markers:
        raise SystemExit(
            "goose binary lacks required cloud safety capabilities: "
            + ", ".join(missing_markers)
        )
    dirty = git_value("status", "--porcelain", "--untracked-files=all")
    if dirty:
        raise SystemExit(
            "cloud benchmark source worktree must be clean before it is frozen"
        )
    secret_values = parse_secret_file(secret_path)
    rosters = authenticated_rosters(secret_values, rows)
    validate_rosters(rows, rosters)
    busy = [
        str(row["vendor_port"])
        for row in rows
        if not port_is_free(int(row["vendor_port"]))
    ]
    if busy:
        raise SystemExit(f"vendor ports are already occupied: {', '.join(busy)}")
    selected_evidence = {
        provider: {
            str(row["model"]): rosters["evidence"][provider][str(row["model"])]
            for row in rows
            if row["provider"] == provider
        }
        for provider in {str(row["provider"]) for row in rows}
    }
    publisher = publisher_snapshot(publisher_repo, rows)
    return {
        "checked_at": utc_now(),
        "binary_sha256": sha256_file(binary),
        "models": {key: sorted(value) for key, value in rosters["models"].items()},
        "roster_evidence": selected_evidence,
        "requested_models": [str(row["model"]) for row in rows],
        "ports_free": True,
        "credential_file_mode": f"{secret_path.stat().st_mode & 0o777:04o}",
        "publisher": publisher,
    }


def validated_preflight_snapshot(
    value: Mapping[str, Any],
    binary: Path,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
) -> Dict[str, Any]:
    expected_keys = {
        "checked_at",
        "binary_sha256",
        "models",
        "roster_evidence",
        "requested_models",
        "ports_free",
        "credential_file_mode",
        "publisher",
    }
    if not isinstance(value, Mapping) or set(value) != expected_keys:
        raise SystemExit("verified preflight snapshot has an invalid schema")
    checked = dict(value)
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    requested_models = [str(row["model"]) for row in rows]
    if checked.get("binary_sha256") != sha256_file(binary):
        raise SystemExit("verified preflight snapshot binary changed")
    if checked.get("requested_models") != requested_models:
        raise SystemExit("verified preflight snapshot model roster changed")
    if checked.get("ports_free") is not True:
        raise SystemExit("verified preflight snapshot did not prove free ports")
    current_mode = f"{secret_path.stat().st_mode & 0o777:04o}"
    if checked.get("credential_file_mode") != current_mode:
        raise SystemExit("verified preflight credential mode changed")
    models = checked.get("models")
    evidence = checked.get("roster_evidence")
    if not isinstance(models, dict) or not isinstance(evidence, dict):
        raise SystemExit("verified preflight roster evidence is malformed")
    for row in rows:
        provider = str(row["provider"])
        model = str(row["model"])
        provider_models = models.get(provider)
        provider_evidence = evidence.get(provider)
        if (
            not isinstance(provider_models, list)
            or model not in provider_models
            or not isinstance(provider_evidence, dict)
            or model not in provider_evidence
        ):
            raise SystemExit(
                f"verified preflight snapshot does not prove {provider}/{model}"
            )
    busy = [
        str(row["vendor_port"])
        for row in rows
        if not port_is_free(int(row["vendor_port"]))
    ]
    if busy:
        raise SystemExit(
            "vendor ports changed after authenticated preflight: " + ", ".join(busy)
        )
    if checked.get("publisher") != publisher_snapshot(publisher_repo, rows):
        raise SystemExit("publisher changed after authenticated preflight")
    checked_at = checked.get("checked_at")
    if not isinstance(checked_at, str) or not checked_at:
        raise SystemExit("verified preflight snapshot has no timestamp")
    return checked


def init_campaign(
    root: Path,
    binary: Path,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
    publish_live: bool,
    website_base_url: str = DEFAULT_WEBSITE_BASE_URL,
    publish_verify_timeout_seconds: float = DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    publish_verify_interval_seconds: float = DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    publish_process_timeout_seconds: float = DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
    verified_preflight: Mapping[str, Any] | None = None,
) -> Dict[str, Any]:
    if campaign_file(root).exists():
        existing = load_json(campaign_file(root))
        if existing.get("status") in {
            "INITIALIZED",
            "RUNNING",
            "BUILD_COMPLETE",
            "SCORING",
        }:
            return existing
        raise SystemExit(
            f"campaign already exists with status {existing.get('status')}: {root}"
        )

    if not publish_live:
        raise SystemExit(
            "cloud campaign init requires explicit --publish-live; dry-run-only campaigns "
            "cannot satisfy the publication contract"
        )
    website_base_url = normalized_website_base_url(website_base_url)
    if (
        publish_verify_timeout_seconds <= 0
        or publish_verify_interval_seconds <= 0
        or publish_process_timeout_seconds <= 0
    ):
        raise SystemExit(
            "publisher process and rendered-verification timing must be positive"
        )

    checked = validated_preflight_snapshot(
        (
            verified_preflight
            if verified_preflight is not None
            else preflight(binary, manifest_path, secret_path, publisher_repo)
        ),
        binary,
        manifest_path,
        secret_path,
        publisher_repo,
    )
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    policy = spend_policy(manifest, rows)
    smoke_turn_limit = smoke_max_turns(manifest)
    root.mkdir(parents=True, exist_ok=False)
    (root / "instrument").mkdir()
    instrument_root = root / "instrument/source"
    hashes = freeze_instrument(instrument_root)
    (root / "entrants").mkdir()
    (root / "smoke").mkdir()
    (root / "locks").mkdir()
    (root / "scores").mkdir()
    (root / "publish").mkdir()
    frozen_binary = root / "instrument/goose"
    shutil.copy2(binary, frozen_binary)
    frozen_binary.chmod(frozen_binary.stat().st_mode | 0o100)

    manifest_copy = root / "instrument/cloud-sb7-entrants.json"
    shutil.copy2(manifest_path, manifest_copy)
    budget_config_path = root / "instrument/budget-config.json"
    budget_config = {
        "schema_version": 1,
        "currency": policy.get("currency", "USD"),
        "total_cap": float(policy["total_cap"]),
        "provider_caps": policy["provider_caps"],
        "models": {
            f"{row['provider']}/{row['model']}": {
                "provider": row["provider"],
                "model": row["model"],
                "accepted_reported_models": row["accepted_reported_models"],
                "context_limit": row["context_limit"],
                "max_output_tokens": row["max_output_tokens"],
                "pricing": row["pricing"],
            }
            for row in rows
        },
    }
    atomic_json(budget_config_path, budget_config)
    budget_ledger_path = root / "budget-ledger.json"
    atomic_json(
        budget_ledger_path,
        {
            "schema_version": 1,
            "currency": policy.get("currency", "USD"),
            "total_cap": float(policy["total_cap"]),
            "provider_caps": policy["provider_caps"],
            "spent_upper_bound": 0.0,
            "provider_spent_upper_bound": {
                provider: 0.0 for provider in policy["provider_caps"]
            },
            "outstanding": {},
            "settled": [],
            "updated_at": utc_now(),
        },
    )
    prompt_source = (
        instrument_root / "evals/swarm-bench/spec-build-sb7.md"
    ).read_bytes()
    publisher = dict(checked["publisher"])
    publisher["frozen"] = freeze_publisher_runtime(
        root / "instrument/publisher", publisher
    )
    publisher.update(
        {
            "mode": "live",
            "website_base_url": website_base_url.rstrip("/"),
            "revalidate_endpoint": (
                f"{website_base_url.rstrip('/')}/api/revalidate-benchmarks"
            ),
            "verify_timeout_seconds": publish_verify_timeout_seconds,
            "verify_interval_seconds": publish_verify_interval_seconds,
            "process_timeout_seconds": publish_process_timeout_seconds,
        }
    )
    campaign = {
        "schema_version": CAMPAIGN_SCHEMA,
        "campaign_id": root.name,
        "created_at": utc_now(),
        "status": "INITIALIZED",
        "source_repo": str(REPO),
        "source_commit": git_value("rev-parse", "HEAD"),
        "source_branch": git_value("branch", "--show-current"),
        "binary": str(frozen_binary),
        "binary_sha256": sha256_file(frozen_binary),
        "entrant_manifest": str(manifest_copy),
        "entrant_manifest_sha256": sha256_file(manifest_copy),
        "budget_config": str(budget_config_path),
        "budget_config_sha256": sha256_file(budget_config_path),
        "budget_ledger": str(budget_ledger_path),
        "instrument_root": str(instrument_root),
        "coordinator": str(instrument_root / "evals/swarm-bench/bench/cloud_sb7.py"),
        "scorer": str(instrument_root / "evals/swarm-bench/bench/score_sb7.py"),
        "instrument_hashes": hashes,
        "instrument_set_sha256": sha256_bytes(
            json.dumps(hashes, sort_keys=True).encode()
        ),
        "prompt_source_sha256": sha256_bytes(prompt_source),
        "secret_file": str(secret_path),
        "preflight": {
            key: value
            for key, value in checked.items()
            if key not in {"models", "publisher"}
        },
        "publisher": publisher,
        "requested_models": checked["requested_models"],
        "scorer_version": manifest.get("suite"),
        "calibration": manifest.get("calibration"),
        "smoke_max_turns": smoke_turn_limit,
        "smoke_status": "PLANNED",
        "lineage": {
            "generation": 0,
            "predecessor_campaign_id": None,
            "predecessor_contract_sha256": None,
        },
    }
    campaign = bind_smoke_contract(campaign, rows)
    atomic_json(campaign_file(root), campaign)

    for row in rows:
        entrant_id = str(row["id"])
        unit = root / "entrants" / entrant_id
        (unit / "tree").mkdir(parents=True)
        (unit / "profile").mkdir()
        (unit / "logs").mkdir()
        seed = secrets.token_hex(8)
        state = {
            "schema_version": CAMPAIGN_SCHEMA,
            "entrant": entrant_id,
            "provider": row["provider"],
            "model": row["model"],
            "provider_lane": row["provider_lane"],
            "status": "PLANNED",
            "provider_episode_attempts": 0,
            "fixture_seed": seed,
            "vendor_port": int(row["vendor_port"]),
            "tree": str(unit / "tree"),
            "profile": str(unit / "profile"),
            "build_log": str(unit / "logs/build.log"),
            "vendor_trace": str(unit / "vendor-trace-build.jsonl"),
            "provider_lifecycle": str(unit / "provider-lifecycle.jsonl"),
            "budget_config_sha256": campaign["budget_config_sha256"],
            "thinking_effort": row["thinking_effort"],
            "context_limit": int(row["context_limit"]),
            "max_output_tokens": int(row["max_output_tokens"]),
            "endpoint_family": row["endpoint_family"],
            "created_at": utc_now(),
            "updated_at": utc_now(),
            "admitted_requests": 0,
            "provider_terminal_requests": 0,
            "publish_doc_id": publisher["entries"][entrant_id]["doc_id"],
            "publish_label": publisher["entries"][entrant_id]["label"],
        }
        atomic_json(state_file(root, entrant_id), state)
        smoke_unit = root / "smoke" / entrant_id
        (smoke_unit / "attempts").mkdir(parents=True)
        atomic_json(
            smoke_state_file(root, entrant_id),
            {
                "schema_version": CAMPAIGN_SCHEMA,
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
                "context_limit": int(row["context_limit"]),
                "max_output_tokens": int(row["max_output_tokens"]),
                "endpoint_family": row["endpoint_family"],
                "created_at": utc_now(),
                "updated_at": utc_now(),
            },
        )
    manager_state(root, status="IDLE", pid=None, pgid=None)
    monitor_state(root, status="IDLE", pid=None, pgid=None, restarts=0)
    return campaign


def manifest_row(root: Path, entrant_id: str) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    for row in entrants(manifest):
        if row["id"] == entrant_id:
            return row
    raise SystemExit(f"unknown entrant in campaign: {entrant_id}")


def build_prompt(port: int, campaign: Mapping[str, Any]) -> str:
    from vendor_service_v3 import API_KEY, DOCS_PATH  # noqa: PLC0415

    spec = campaign_instrument_path(
        campaign, "evals/swarm-bench/spec-build-sb7.md"
    ).read_text()
    return (
        spec.replace("{DOCS_URL}", f"http://127.0.0.1:{port}{DOCS_PATH}")
        .replace("{BASE_URL}", f"http://127.0.0.1:{port}")
        .replace("{API_KEY}", API_KEY)
    )


def build_goose_command(binary: Path, row: Mapping[str, Any], prompt: str) -> list[str]:
    return [
        str(binary),
        "run",
        "--quiet",
        "--provider",
        str(row["provider"]),
        "--model",
        str(row["model"]),
        "--output-format",
        "stream-json",
        "-t",
        prompt,
    ]


def smoke_shell_command(nonce: bytes) -> tuple[str, str]:
    nonce_hex = nonce.hex()
    verified = f"NONCE_VERIFIED:{sha256_bytes(nonce)}"
    command = (
        "/usr/bin/python3 -c 'from pathlib import Path; import stat; "
        f'p=Path("{SMOKE_NONCE_NAME}"); expected=bytes.fromhex("{nonce_hex}"); '
        "assert not p.exists() and not p.is_symlink(); p.write_bytes(expected); "
        "actual=p.read_bytes(); assert actual == expected; "
        "assert stat.S_ISREG(p.lstat().st_mode) and not p.is_symlink(); "
        f'print("{verified}")'
        "'"
    )
    return command, verified


def smoke_prompt(command: str, marker: str) -> str:
    return (
        "This is a strict provider/tool contract smoke test. Do not explain or plan. "
        "Make exactly one developer__shell tool request, with no other tools, and pass "
        "exactly the command below as its command argument. The command writes a random "
        "nonce to a regular file, reads and verifies the same bytes within that one tool "
        "call, and emits its own verification proof.\n\n"
        f"{command}\n\n"
        "Only after the successful tool response, return exactly this assistant text "
        "with no quotes, markdown, prefix, suffix, or extra whitespace:\n"
        f"{marker}"
    )


def smoke_goose_command(
    binary: Path,
    row: Mapping[str, Any],
    prompt: str,
    max_turns: int,
) -> list[str]:
    return [
        str(binary),
        "run",
        "--quiet",
        "--provider",
        str(row["provider"]),
        "--model",
        str(row["model"]),
        "--output-format",
        "stream-json",
        "--max-turns",
        str(max_turns),
        "-t",
        prompt,
    ]


def prepare_smoke_attempt(
    root: Path, entrant_id: str, row: Mapping[str, Any]
) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    state = read_smoke_state(root, entrant_id)
    if state.get("status") not in SMOKE_PREPARABLE_STATES:
        raise SystemExit(f"{entrant_id} smoke cannot launch from {state.get('status')}")
    ambiguity = smoke_attempt_history_failure(root, entrant_id, row)
    if ambiguity:
        raise SystemExit(f"{entrant_id} smoke cannot be retried: {ambiguity}")
    contract = smoke_contract_identity(campaign)
    if (
        campaign.get("smoke_contract_sha256") != contract
        or state.get("smoke_contract_sha256") != contract
    ):
        raise SystemExit(f"{entrant_id} smoke contract identity changed before launch")
    attempt = int(state.get("launch_attempts", 0)) + 1
    attempt_root = root / "smoke" / entrant_id / "attempts" / f"attempt-{attempt}"
    attempt_root.mkdir(parents=True, exist_ok=False)
    tree = attempt_root / "tree"
    profile = attempt_root / "profile"
    tree.mkdir()
    profile.mkdir()
    (attempt_root / "logs").mkdir()
    nonce = secrets.token_bytes(32)
    command, verified = smoke_shell_command(nonce)
    marker = f"SB7_CONTRACT_SMOKE_PASS_{secrets.token_hex(16)}"
    prompt = smoke_prompt(command, marker)
    prompt_path = attempt_root / "prompt.txt"
    prompt_path.write_text(prompt)
    return update_smoke_state(
        root,
        entrant_id,
        status="PREPARING",
        active_attempt=True,
        launch_attempts=attempt,
        attempt=attempt,
        attempt_root=str(attempt_root),
        tree=str(tree),
        profile=str(profile),
        log=str(attempt_root / "logs/smoke.log"),
        provider_lifecycle=str(attempt_root / "provider-lifecycle.jsonl"),
        prompt=str(prompt_path),
        prompt_sha256=sha256_file(prompt_path),
        expected_command=command,
        expected_command_sha256=sha256_bytes(command.encode()),
        expected_tool_output=verified,
        final_marker=marker,
        nonce_hex=nonce.hex(),
        nonce_file=str(tree / SMOKE_NONCE_NAME),
        campaign_root=str(root),
        budget_config=str(campaign["budget_config"]),
        budget_ledger=str(campaign["budget_ledger"]),
        budget_config_sha256=campaign["budget_config_sha256"],
        smoke_max_turns=int(campaign["smoke_max_turns"]),
        admitted_requests=0,
        provider_terminal_requests=0,
        failure=None,
        supervisor_pid=os.getpid(),
        supervisor_pgid=os.getpgrp(),
        supervisor_identity=process_identity(os.getpid()),
    )


def _walk_strings(
    value: Any, path: tuple[Any, ...] = ()
) -> Iterator[tuple[tuple[Any, ...], str]]:
    if isinstance(value, str):
        yield path, value
    elif isinstance(value, dict):
        for key, nested in value.items():
            yield from _walk_strings(nested, (*path, key))
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            yield from _walk_strings(nested, (*path, index))


def _has_output_limit_metadata(value: Any) -> bool:
    if isinstance(value, dict):
        for key, nested in value.items():
            if (
                key in {"outputTokenLimitReached", "output_token_limit_reached"}
                and nested is True
            ):
                return True
            if _has_output_limit_metadata(nested):
                return True
    elif isinstance(value, list):
        return any(_has_output_limit_metadata(item) for item in value)
    return False


def _is_developer_shell_request(
    item: Mapping[str, Any], value: Mapping[str, Any]
) -> bool:
    name = value.get("name")
    if name == "developer__shell":
        return True
    metadata = item.get("_meta")
    return (
        name == "shell"
        and isinstance(metadata, dict)
        and metadata.get("goose_extension") == "developer"
    )


def _structured_shell_stdout_is_exact(
    value: Mapping[str, Any], expected_output: str
) -> bool:
    structured = value.get("structuredContent")
    if not isinstance(structured, dict):
        return False
    exit_code = structured.get("exit_code")
    return (
        type(exit_code) is int
        and exit_code == 0
        and structured.get("stdout") == expected_output
    )


def parse_smoke_stream(
    path: Path,
    *,
    expected_command: str,
    expected_marker: str,
    expected_tool_output: str,
) -> Dict[str, Any]:
    errors: list[str] = []
    events: list[Dict[str, Any]] = []
    if not path.is_file() or path.is_symlink():
        return {
            "valid": False,
            "errors": ["stream log is missing or symbolic"],
            "events": 0,
            "complete_events": 0,
            "tool_requests": 0,
            "tool_responses": 0,
        }
    try:
        text = path.read_bytes().decode("utf-8")
    except UnicodeDecodeError:
        return {
            "valid": False,
            "errors": ["stream log is not UTF-8"],
            "events": 0,
            "complete_events": 0,
            "tool_requests": 0,
            "tool_responses": 0,
        }
    for line_number, raw in enumerate(text.splitlines(), start=1):
        if not raw.strip():
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            errors.append(f"line {line_number}: malformed stream JSON")
            continue
        if not isinstance(event, dict):
            errors.append(f"line {line_number}: stream event is not an object")
            continue
        events.append(event)

    requests: list[Dict[str, Any]] = []
    responses: list[Dict[str, Any]] = []
    assistant_text: list[tuple[int, str]] = []
    complete_positions: list[int] = []
    for position, event in enumerate(events):
        event_type = event.get("type")
        if _has_output_limit_metadata(event):
            errors.append(f"event {position}: output token limit was reached")
        if event_type == "error":
            errors.append(f"event {position}: error event")
        if event_type == "complete":
            complete_positions.append(position)
            continue
        if event_type != "message":
            if any(expected_marker in value for _, value in _walk_strings(event)):
                errors.append(
                    f"event {position}: final marker appeared outside assistant text"
                )
            continue
        message = event.get("message")
        if not isinstance(message, dict):
            errors.append(f"event {position}: message payload is malformed")
            continue
        role = message.get("role")
        content = message.get("content")
        if not isinstance(content, list):
            errors.append(f"event {position}: message content is malformed")
            continue
        allowed_marker_paths: set[tuple[Any, ...]] = set()
        for index, item in enumerate(content):
            if not isinstance(item, dict):
                errors.append(f"event {position}: message content item is malformed")
                continue
            item_type = item.get("type")
            if item_type == "toolRequest":
                requests.append({"position": position, "role": role, "item": item})
            elif item_type == "toolResponse":
                responses.append({"position": position, "role": role, "item": item})
            elif item_type == "text" and isinstance(item.get("text"), str):
                value = str(item["text"])
                if role == "assistant":
                    assistant_text.append((position, value))
                    allowed_marker_paths.add(("message", "content", index, "text"))
            elif (
                item_type == "thinking"
                and role == "assistant"
                and isinstance(item.get("thinking"), str)
            ):
                allowed_marker_paths.add(
                    ("message", "content", index, "thinking")
                )
        for string_path, value in _walk_strings(event):
            if expected_marker in value and string_path not in allowed_marker_paths:
                errors.append(
                    f"event {position}: final marker appeared outside assistant text"
                )

    if len(requests) != 1:
        errors.append(f"expected one tool request, found {len(requests)}")
    request_id: str | None = None
    request_position = -1
    if requests:
        request = requests[0]
        item = request["item"]
        request_position = int(request["position"])
        request_id = item.get("id") if isinstance(item.get("id"), str) else None
        tool_call = item.get("toolCall")
        value = tool_call.get("value") if isinstance(tool_call, dict) else None
        arguments = value.get("arguments") if isinstance(value, dict) else None
        if (
            request_id is None
            or request.get("role") != "assistant"
            or not isinstance(tool_call, dict)
            or tool_call.get("status") != "success"
            or not isinstance(value, dict)
            or not _is_developer_shell_request(item, value)
            or not isinstance(arguments, dict)
            or arguments.get("command") != expected_command
        ):
            errors.append("developer__shell request did not match the frozen command")

    paired = [
        response
        for response in responses
        if request_id is not None and response["item"].get("id") == request_id
    ]
    if len(responses) != 1 or len(paired) != 1:
        errors.append(
            "expected one tool response paired to the developer__shell request by ID"
        )
    response_position = -1
    if paired:
        response = paired[0]
        response_position = int(response["position"])
        result = response["item"].get("toolResult")
        value = result.get("value") if isinstance(result, dict) else None
        if response_position <= request_position:
            errors.append("tool response occurred before its request")
        if (
            not isinstance(result, dict)
            or response.get("role") != "user"
            or result.get("status") != "success"
            or not isinstance(value, dict)
            or value.get("isError") is True
            or not _structured_shell_stdout_is_exact(value, expected_tool_output)
        ):
            errors.append(
                "developer__shell response was failed, erroneous, or unproven"
            )

    final_text = "".join(
        value for position, value in assistant_text if position > response_position
    )
    if response_position < 0 or final_text != expected_marker:
        errors.append("final assistant text after the tool response was not exact")
    if any(
        expected_marker in value and position <= response_position
        for position, value in assistant_text
    ):
        errors.append("final marker appeared before the paired tool response")
    if any(
        value for position, value in assistant_text if position <= response_position
    ):
        errors.append("assistant emitted text before the paired tool response")
    if len(complete_positions) != 1:
        errors.append(f"expected one complete event, found {len(complete_positions)}")
    elif complete_positions[0] != len(events) - 1:
        errors.append("complete event was not the final stream event")
    elif response_position < 0 or complete_positions[0] <= response_position:
        errors.append("complete event occurred before the paired tool response")

    return {
        "valid": not errors,
        "errors": errors,
        "events": len(events),
        "complete_events": len(complete_positions),
        "tool_requests": len(requests),
        "tool_responses": len(responses),
        "request_id": request_id,
        "paired_response": len(paired) == 1,
        "final_text_exact": final_text == expected_marker,
        "output_token_limit_reached": any(
            _has_output_limit_metadata(event) for event in events
        ),
    }


SAFE_ENV_NAMES = {
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
    "COLORTERM",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "NO_COLOR",
}


def canonical_listener_ports(values: Iterable[Any]) -> list[int]:
    ports = list(values)
    if any(
        isinstance(port, bool)
        or not isinstance(port, int)
        or port <= 0
        or port > 65535
        for port in ports
    ):
        raise SystemExit("listener snapshot contains an invalid TCP port")
    if ports != sorted(set(ports)):
        raise SystemExit("listener snapshot ports are not unique and ascending")
    return ports


def parse_lsof_listener_ports(output: str) -> list[int]:
    ports: set[int] = set()
    for line in output.splitlines():
        if not line.startswith("n"):
            continue
        endpoint = line[1:]
        _, separator, raw_port = endpoint.rpartition(":")
        if not separator or not raw_port.isascii() or not raw_port.isdigit():
            raise SystemExit(f"lsof returned an unparseable TCP listener: {endpoint}")
        port = int(raw_port)
        if port <= 0 or port > 65535:
            raise SystemExit(f"lsof returned an invalid TCP listener: {endpoint}")
        ports.add(port)
    return sorted(ports)


def snapshot_listening_tcp_ports() -> list[int]:
    try:
        result = subprocess.run(
            [
                "/usr/sbin/lsof",
                "-nP",
                "-iTCP",
                "-sTCP:LISTEN",
                "-F",
                "pn",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
            env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"cannot snapshot pre-existing TCP listeners: {error}") from None
    if result.returncode not in {0, 1} or (result.returncode == 1 and result.stderr):
        raise SystemExit(
            f"lsof TCP listener snapshot failed with exit {result.returncode}"
        )
    return parse_lsof_listener_ports(result.stdout)


def persist_listener_isolation(
    root: Path,
    row: Mapping[str, Any],
    state: Mapping[str, Any],
    *,
    smoke: bool,
) -> Dict[str, Any]:
    entrant_id = str(row.get("id", row.get("entrant", "")))
    if not entrant_id:
        raise SystemExit("sandbox listener snapshot has no entrant identity")
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    manifest_ports = sorted(int(value["vendor_port"]) for value in entrants(manifest))
    canonical_listener_ports(manifest_ports)
    preexisting_ports = snapshot_listening_tcp_ports()
    own_vendor_port = None if smoke else int(row["vendor_port"])
    denied_ports = sorted(
        (set(preexisting_ports) | set(manifest_ports))
        - ({own_vendor_port} if own_vendor_port is not None else set())
    )
    canonical_listener_ports(denied_ports)
    snapshot = {
        "schema_version": 1,
        "captured_at": utc_now(),
        "entrant": entrant_id,
        "phase": "smoke" if smoke else "full",
        "entrant_manifest_sha256": campaign["entrant_manifest_sha256"],
        "preexisting_listener_ports": preexisting_ports,
        "manifest_vendor_ports": manifest_ports,
        "own_vendor_port": own_vendor_port,
        "denied_local_ports": denied_ports,
    }
    base = Path(str(state["attempt_root"])) if smoke else Path(str(state["tree"])).parent
    snapshot_path = base / "sandbox-listeners.json"
    atomic_json(snapshot_path, snapshot)
    changes = {
        "sandbox_listener_snapshot": str(snapshot_path),
        "sandbox_listener_snapshot_sha256": sha256_file(snapshot_path),
        "sandbox_preexisting_listener_ports": preexisting_ports,
        "sandbox_manifest_vendor_ports": manifest_ports,
        "sandbox_denied_local_ports": denied_ports,
        "sandbox_denied_local_ports_sha256": sha256_bytes(
            ",".join(map(str, denied_ports)).encode()
        ),
    }
    if smoke:
        return update_smoke_state(root, entrant_id, **changes)
    return update_state(root, entrant_id, **changes)


def listener_isolation_failure(
    campaign: Mapping[str, Any],
    row: Mapping[str, Any],
    state: Mapping[str, Any],
    *,
    smoke: bool,
) -> str | None:
    try:
        entrant_id = str(row.get("id", row.get("entrant", "")))
        if not entrant_id:
            return "sandbox listener snapshot has no entrant identity"
        snapshot_path = Path(str(state["sandbox_listener_snapshot"]))
        expected_path = (
            Path(str(state["attempt_root"])) / "sandbox-listeners.json"
            if smoke
            else Path(str(state["tree"])).parent / "sandbox-listeners.json"
        )
        if (
            snapshot_path.resolve() != expected_path.resolve()
            or snapshot_path.is_symlink()
            or not snapshot_path.is_file()
        ):
            return "sandbox listener snapshot is missing, linked, or misplaced"
        if sha256_file(snapshot_path) != state.get("sandbox_listener_snapshot_sha256"):
            return "sandbox listener snapshot hash changed"
        snapshot = load_json(snapshot_path)
        expected_keys = {
            "schema_version",
            "captured_at",
            "entrant",
            "phase",
            "entrant_manifest_sha256",
            "preexisting_listener_ports",
            "manifest_vendor_ports",
            "own_vendor_port",
            "denied_local_ports",
        }
        if set(snapshot) != expected_keys:
            return "sandbox listener snapshot schema is malformed"
        preexisting = canonical_listener_ports(snapshot["preexisting_listener_ports"])
        manifest_ports = canonical_listener_ports(snapshot["manifest_vendor_ports"])
        denied = canonical_listener_ports(snapshot["denied_local_ports"])
        current_manifest = load_json(Path(str(campaign["entrant_manifest"])))
        expected_manifest_ports = sorted(
            int(value["vendor_port"]) for value in entrants(current_manifest)
        )
        own_vendor_port = None if smoke else int(row["vendor_port"])
        expected_denied = sorted(
            (set(preexisting) | set(expected_manifest_ports))
            - ({own_vendor_port} if own_vendor_port is not None else set())
        )
        if (
            snapshot["schema_version"] != 1
            or not isinstance(snapshot["captured_at"], str)
            or not snapshot["captured_at"]
            or snapshot["entrant"] != entrant_id
            or snapshot["phase"] != ("smoke" if smoke else "full")
            or snapshot["entrant_manifest_sha256"]
            != campaign["entrant_manifest_sha256"]
            or manifest_ports != expected_manifest_ports
            or snapshot["own_vendor_port"] != own_vendor_port
            or denied != expected_denied
            or state.get("sandbox_preexisting_listener_ports") != preexisting
            or state.get("sandbox_manifest_vendor_ports") != manifest_ports
            or state.get("sandbox_denied_local_ports") != denied
            or state.get("sandbox_denied_local_ports_sha256")
            != sha256_bytes(",".join(map(str, denied)).encode())
        ):
            return "sandbox listener isolation differs from its frozen campaign"
    except (OSError, KeyError, TypeError, json.JSONDecodeError, SystemExit) as error:
        return f"sandbox listener isolation cannot be verified: {error}"
    return None


def child_env(
    row: Mapping[str, Any], state: Mapping[str, Any], secret_value: str
) -> Dict[str, str]:
    env = {key: value for key, value in os.environ.items() if key in SAFE_ENV_NAMES}
    profile = Path(str(state["profile"]))
    tool_home = profile / "tool-home"
    tool_home.mkdir(parents=True, exist_ok=True)
    (tool_home / "tmp").mkdir(exist_ok=True)
    tree = Path(str(state["tree"]))
    campaign_root = Path(str(state.get("campaign_root", tree.parents[2])))
    budget_config = Path(
        str(state.get("budget_config", campaign_root / "instrument/budget-config.json"))
    )
    budget_ledger = Path(
        str(state.get("budget_ledger", campaign_root / "budget-ledger.json"))
    )
    if "sandbox_denied_local_ports" not in state:
        raise SystemExit("sandbox listener isolation is required before agent launch")
    denied_local_ports = canonical_listener_ports(state["sandbox_denied_local_ports"])
    env.update(
        {
            str(row["secret_env"]): secret_value,
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(profile),
            "TMPDIR": str(tool_home / "tmp"),
            "GOOSE_PATH_ROOT": str(state["profile"]),
            "GOOSE_PROVIDER": str(row["provider"]),
            "GOOSE_MODEL": str(row["model"]),
            "GOOSE_MODE": "auto",
            "GOOSE_DISABLE_SESSION_NAMING": "true",
            "GOOSE_THINKING_EFFORT": str(row["thinking_effort"]),
            "GOOSE_CONTEXT_LIMIT": str(row["context_limit"]),
            "GOOSE_MAX_TOKENS": str(row["max_output_tokens"]),
            "GOOSE_SWARM_TELEMETRY_FILE": str(tree / ".swarm/telemetry.jsonl"),
            "GOOSE_PROVIDER_LIFECYCLE_FILE": str(state["provider_lifecycle"]),
            "GOOSE_PROVIDER_LIFECYCLE_STRICT": "true",
            "GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES": "true",
            "GOOSE_BENCH_EXPECTED_PROVIDER": str(row["provider"]),
            "GOOSE_BENCH_SECRET_ENV_NAME": str(row["secret_env"]),
            "GOOSE_BENCH_TOOL_ALLOWLIST": "developer",
            "GOOSE_FAST_MODEL": str(row["model"]),
            "GOOSE_TOOL_SANDBOX_ROOT": str(state["tree"]),
            "GOOSE_TOOL_SANDBOX_HOME": str(tool_home),
            "GOOSE_TOOL_SANDBOX_DENY_ROOT": str(Path.home()),
            "GOOSE_TOOL_SANDBOX_DENY_LOCAL_PORTS": ",".join(
                map(str, denied_local_ports)
            ),
            "GOOSE_BENCH_BUDGET_CONFIG": str(budget_config),
            "GOOSE_BENCH_BUDGET_CONFIG_SHA256": str(state["budget_config_sha256"]),
            "GOOSE_BENCH_BUDGET_LEDGER": str(budget_ledger),
            "GOOSE_BENCH_CAMPAIGN": str(campaign_root),
            "GOOSE_BENCH_ENTRANT": str(row["id"]),
        }
    )
    base_url_env = row.get("base_url_env")
    if base_url_env is not None:
        name = str(base_url_env)
        if name in env:
            raise SystemExit(f"provider base URL cannot overwrite protected env: {name}")
        env[name] = str(row["endpoint_family"])
    return env


def redacted_copy(
    stream: Any, destination: Any, secrets_to_redact: Iterable[str], on_line: Any
) -> None:
    redactions = [value for value in secrets_to_redact if value]
    for raw in iter(stream.readline, ""):
        line = raw
        for value in redactions:
            line = line.replace(value, "[REDACTED]")
        destination.write(line)
        destination.flush()
        on_line(line)


def secret_occurrences(
    paths: Iterable[Path],
    secret_values: Iterable[str],
    excluded_paths: Iterable[Path] = (),
) -> list[str]:
    needles = [value.encode() for value in secret_values if value]
    if not needles:
        return []
    overlap = max(map(len, needles)) - 1
    excluded = {path.resolve() for path in excluded_paths}
    hits: list[str] = []
    files: list[Path] = []
    for path in paths:
        if path.is_file() and path.resolve() not in excluded:
            files.append(path)
        elif path.is_dir():
            files.extend(
                candidate
                for candidate in path.rglob("*")
                if candidate.is_file() and candidate.resolve() not in excluded
            )
    for path in files:
        try:
            with path.open("rb") as stream:
                tail = b""
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    data = tail + chunk
                    if any(needle in data for needle in needles):
                        hits.append(str(path))
                        break
                    tail = data[-overlap:] if overlap else b""
        except OSError:
            hits.append(f"unreadable:{path}")
    return sorted(set(hits))


def persisted_entrant_secret_hits(
    root: Path, campaign: Mapping[str, Any], entrant_id: str
) -> list[str]:
    if not (root / "entrants" / entrant_id).is_dir():
        return [f"missing:{root / 'entrants' / entrant_id}"]
    secret_path = Path(str(campaign["secret_file"]))
    secret_values = parse_secret_file(secret_path).values()
    return secret_occurrences([root], secret_values, [secret_path])


def event_from_line(line: str) -> Dict[str, Any] | None:
    stripped = line.strip()
    if not stripped.startswith("{"):
        return None
    try:
        value = json.loads(stripped)
    except json.JSONDecodeError:
        return None
    if not isinstance(value, dict):
        return None
    nested = value.get("data")
    if isinstance(nested, dict) and "event" in nested:
        return nested
    return value


def provider_state_observer(root: Path, entrant_id: str):
    counters = {"admitted": 0, "terminal": 0, "first_output_at": None}

    def observe(line: str) -> None:
        event = event_from_line(line)
        if not event:
            return
        name = str(event.get("event", ""))
        state = str(event.get("state", ""))
        if name == "provider_request_admitted" or state == "admitted":
            counters["admitted"] += 1
        if name == "provider_request_terminal" or state == "provider_terminal":
            counters["terminal"] += 1
        if counters["first_output_at"] is None and (
            name in {"message", "assistant", "provider_first_item"}
            or event.get("type") in {"message", "assistant"}
        ):
            counters["first_output_at"] = utc_now()
        if name.startswith("provider_request_") or state in {
            "admitted",
            "provider_terminal",
        }:
            update_state(
                root,
                entrant_id,
                admitted_requests=counters["admitted"],
                provider_terminal_requests=counters["terminal"],
                first_output_at=counters["first_output_at"],
                last_provider_event=event,
            )

    return counters, observe


def lifecycle_usage_failure(usage: Any) -> str | None:
    required = {
        "reported_model",
        "input_tokens",
        "output_tokens",
        "total_tokens",
    }
    if not isinstance(usage, dict) or set(usage) != required:
        return "usage evidence does not have the exact terminal schema"
    if not isinstance(usage["reported_model"], str) or not usage["reported_model"]:
        return "usage evidence has no reported model identity"
    counts = (
        usage["input_tokens"],
        usage["output_tokens"],
        usage["total_tokens"],
    )
    if any(
        isinstance(value, bool) or not isinstance(value, int) or value < 0
        for value in counts
    ):
        return "usage evidence has missing, negative, or non-integral token counts"
    if usage["input_tokens"] + usage["output_tokens"] != usage["total_tokens"]:
        return "usage evidence has an inconsistent total token count"
    return None


def smoke_state_observer(root: Path, entrant_id: str):
    counters = {"admitted": 0, "terminal": 0, "first_output_at": None}

    def observe(line: str) -> None:
        event = event_from_line(line)
        if not event:
            return
        name = str(event.get("event", ""))
        state = str(event.get("state", ""))
        if name == "provider_request_admitted" or state == "admitted":
            counters["admitted"] += 1
        if name == "provider_request_terminal" or state == "provider_terminal":
            counters["terminal"] += 1
        if counters["first_output_at"] is None and (
            name in {"message", "assistant", "provider_first_item"}
            or event.get("type") in {"message", "assistant"}
        ):
            counters["first_output_at"] = utc_now()
        if name.startswith("provider_request_") or state in {
            "admitted",
            "provider_terminal",
        }:
            update_smoke_state(
                root,
                entrant_id,
                admitted_requests=counters["admitted"],
                provider_terminal_requests=counters["terminal"],
                first_output_at=counters["first_output_at"],
                last_provider_event=event,
            )

    return counters, observe


def classify_build_exit(
    exit_code: int, admitted_requests: int
) -> tuple[str, str | None]:
    if exit_code == 0:
        return "BUILD_COMPLETE", None
    if admitted_requests == 0:
        return (
            "PRE_ADMISSION_FAILURE",
            f"goose exited {exit_code} before any proven provider admission",
        )
    return (
        "INCOMPLETE",
        f"goose exited {exit_code} after {admitted_requests} admitted request(s); "
        "ambiguous admitted work is never retried",
    )


def lifecycle_summary(
    path: Path, *, expected_provider: str, expected_model: str
) -> Dict[str, Any]:
    summary: Dict[str, Any] = {
        "admitted": 0,
        "terminal": 0,
        "first_output_at": None,
        "malformed_lines": 0,
        "events": 0,
        "transition_errors": [],
        "ambiguous_request_ids": [],
        "request_states": {},
        "terminal_usage": {},
        "valid": True,
    }
    if not path.is_file():
        return summary
    requests: Dict[str, Dict[str, Any]] = {}
    terminal_states = {"provider_terminal", "stream_ambiguous", "error"}
    known_states = {
        "queued",
        "admitted",
        "first_item",
        "usage_reported",
        *terminal_states,
    }
    for line_number, raw in enumerate(
        path.read_text(errors="replace").splitlines(), start=1
    ):
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            summary["malformed_lines"] += 1
            continue
        if not isinstance(event, dict):
            summary["malformed_lines"] += 1
            continue
        summary["events"] += 1
        state = str(event.get("state", ""))
        request_id = event.get("request_id")
        provider = event.get("provider")
        model = event.get("model")
        session = event.get("session")
        timestamp = event.get("timestamp")
        if (
            event.get("schema_version") != 1
            or state not in known_states
            or not isinstance(request_id, str)
            or not request_id
            or not isinstance(provider, str)
            or not provider
            or not isinstance(model, str)
            or not model
            or not isinstance(session, str)
            or not session
            or not isinstance(timestamp, str)
            or not timestamp
        ):
            summary["malformed_lines"] += 1
            summary["transition_errors"].append(
                f"line {line_number}: malformed lifecycle event"
            )
            continue
        usage = event.get("usage")
        usage_problem = (
            lifecycle_usage_failure(usage)
            if state in {"usage_reported", "provider_terminal"}
            else None
        )
        if usage_problem:
            summary["malformed_lines"] += 1
            summary["transition_errors"].append(
                f"line {line_number}: {state} {usage_problem}"
            )
            continue
        if provider != expected_provider or model != expected_model:
            summary["transition_errors"].append(
                f"line {line_number} request {request_id}: lifecycle identity "
                f"{provider}/{model} does not match entrant "
                f"{expected_provider}/{expected_model}"
            )
            continue

        identity = (provider, model, session)
        request = requests.setdefault(
            request_id, {"identity": identity, "states": [], "usage": None}
        )
        states = request["states"]
        transition_error: str | None = None
        if request["identity"] != identity:
            transition_error = "provider, model, or session identity drifted"
        elif state == "queued":
            if states:
                transition_error = "queued was not the first and only queue event"
        elif not states or states[0] != "queued":
            transition_error = f"{state} occurred without queued"
        elif states[-1] in terminal_states:
            transition_error = f"{state} occurred after terminal state {states[-1]}"
        elif state == "admitted":
            if "admitted" in states:
                transition_error = "admitted was duplicated"
            elif states != ["queued"]:
                transition_error = "admitted occurred out of order"
        elif state == "first_item":
            if "admitted" not in states:
                transition_error = "first_item occurred before admission"
            elif "first_item" in states:
                transition_error = "first_item was duplicated"
            elif "usage_reported" in states:
                transition_error = "first_item occurred after usage"
        elif state == "usage_reported":
            if "admitted" not in states:
                transition_error = "usage was reported before admission"
            elif "usage_reported" in states:
                transition_error = "usage_reported was duplicated"
        elif state == "provider_terminal":
            if "admitted" not in states:
                transition_error = "provider_terminal occurred without admission"
            elif "usage_reported" not in states:
                transition_error = "provider_terminal occurred without prior usage"
            elif request["usage"] != usage:
                transition_error = "provider_terminal usage differs from usage_reported"
        elif state == "error" and "admitted" in states:
            transition_error = "error was recorded after admission"

        if transition_error is not None:
            summary["transition_errors"].append(
                f"line {line_number} request {request_id}: {transition_error}"
            )
            continue
        states.append(state)
        if state == "usage_reported":
            request["usage"] = usage
        if state == "admitted":
            summary["admitted"] += 1
        elif state == "provider_terminal":
            summary["terminal"] += 1
            summary["terminal_usage"][request_id] = usage
        elif state == "first_item" and summary["first_output_at"] is None:
            summary["first_output_at"] = timestamp

    ambiguous_request_ids = []
    for request_id, request in requests.items():
        states = request["states"]
        if not states or states[-1] not in terminal_states:
            ambiguous_request_ids.append(request_id)
        elif states[-1] == "stream_ambiguous":
            ambiguous_request_ids.append(request_id)
    summary["request_states"] = {
        request_id: request["states"]
        for request_id, request in sorted(requests.items())
    }
    summary["ambiguous_request_ids"] = sorted(ambiguous_request_ids)
    summary["valid"] = not (
        summary["malformed_lines"]
        or summary["transition_errors"]
        or summary["ambiguous_request_ids"]
    )
    return summary


def lifecycle_failure(summary: Mapping[str, Any]) -> str | None:
    reasons = []
    malformed = int(summary.get("malformed_lines", 0))
    transition_errors = summary.get("transition_errors") or []
    ambiguous_ids = summary.get("ambiguous_request_ids") or []
    if malformed:
        reasons.append(f"{malformed} malformed lifecycle line(s)")
    if transition_errors:
        reasons.append(f"{len(transition_errors)} invalid lifecycle transition(s)")
    if ambiguous_ids:
        reasons.append(f"{len(ambiguous_ids)} ambiguous lifecycle request(s)")
    return "; ".join(reasons) if reasons else None


def entrant_outstanding_reservations(
    campaign: Mapping[str, Any], row: Mapping[str, Any]
) -> tuple[list[str], str | None]:
    ledger_value = campaign.get("budget_ledger")
    if not ledger_value:
        return [], "campaign has no budget ledger"
    ledger_path = Path(str(ledger_value))
    if not ledger_path.is_file():
        return [], f"budget ledger is missing: {ledger_path}"
    try:
        ledger = load_json(ledger_path)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return [], f"budget ledger cannot be read after provider exit: {error}"
    outstanding = ledger.get("outstanding")
    if not isinstance(outstanding, dict):
        return [], "budget ledger outstanding field is malformed"
    request_ids = []
    for request_id, reservation in outstanding.items():
        if not isinstance(reservation, dict):
            return [], "budget ledger contains a malformed reservation"
        if reservation.get("provider") == row.get("provider") and reservation.get(
            "model"
        ) == row.get("model"):
            request_ids.append(str(request_id))
    return sorted(request_ids), None


def remap_paths(value: Any, source: Path, destination: Path) -> Any:
    source_text = str(source.resolve())
    destination_text = str(destination.resolve())
    if isinstance(value, dict):
        return {
            key: remap_paths(nested, source, destination)
            for key, nested in value.items()
        }
    if isinstance(value, list):
        return [remap_paths(nested, source, destination) for nested in value]
    if isinstance(value, str):
        candidate = Path(value)
        if candidate.is_absolute():
            try:
                relative = candidate.resolve().relative_to(source.resolve())
            except (OSError, ValueError):
                pass
            else:
                return str(destination.resolve() / relative)
        if value == source_text or value.startswith(f"{source_text}{os.sep}"):
            return destination_text + value[len(source_text) :]
    return value


def budget_model_profile(
    config: Mapping[str, Any], provider: str, model: str
) -> Mapping[str, Any] | None:
    models = config.get("models")
    if not isinstance(models, dict):
        return None
    profile = models.get(f"{provider}/{model}")
    if (
        not isinstance(profile, dict)
        or profile.get("provider") != provider
        or profile.get("model") != model
    ):
        return None
    return profile


def budget_price(
    profile: Mapping[str, Any], input_tokens: int, output_tokens: int
) -> float | None:
    pricing = profile.get("pricing")
    if not isinstance(pricing, dict):
        return None
    input_rate = pricing.get("input_per_million")
    output_rate = pricing.get("output_per_million")
    threshold = pricing.get("tier_threshold_tokens")
    if threshold is not None and (
        isinstance(threshold, bool) or not isinstance(threshold, int) or threshold < 0
    ):
        return None
    if threshold is not None and input_tokens > threshold:
        input_rate = pricing.get("input_over_threshold_per_million", input_rate)
        output_rate = pricing.get("output_over_threshold_per_million", output_rate)
    if any(
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
        or float(value) < 0
        for value in (input_rate, output_rate)
    ):
        return None
    return (
        input_tokens * float(input_rate) + output_tokens * float(output_rate)
    ) / 1_000_000


def money_equal(left: float, right: float) -> bool:
    return abs(left - right) <= max(1e-9, max(abs(left), abs(right)) * 1e-12)


def budget_ledger_failure(
    ledger: Mapping[str, Any], config: Mapping[str, Any] | None = None
) -> str | None:
    required = {
        "schema_version",
        "currency",
        "total_cap",
        "provider_caps",
        "spent_upper_bound",
        "provider_spent_upper_bound",
        "outstanding",
        "settled",
        "updated_at",
    }
    if not required.issubset(ledger):
        return "budget ledger is missing required fields"
    if ledger.get("schema_version") != 1:
        return "budget ledger schema is not supported"
    if not isinstance(ledger.get("currency"), str) or not ledger["currency"]:
        return "budget ledger currency is malformed"
    if not isinstance(ledger.get("updated_at"), str) or not ledger["updated_at"]:
        return "budget ledger update timestamp is malformed"
    provider_caps = ledger.get("provider_caps")
    provider_spent = ledger.get("provider_spent_upper_bound")
    outstanding = ledger.get("outstanding")
    settled = ledger.get("settled")
    if (
        not isinstance(provider_caps, dict)
        or not isinstance(provider_spent, dict)
        or not isinstance(outstanding, dict)
        or not isinstance(settled, list)
    ):
        return "budget ledger collections are malformed"

    money = [ledger.get("total_cap"), ledger.get("spent_upper_bound")]
    money.extend(provider_caps.values())
    money.extend(provider_spent.values())
    if any(
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
        or float(value) < 0
        for value in money
    ):
        return "budget ledger contains invalid monetary values"
    if set(provider_caps) != set(provider_spent):
        return "budget ledger provider totals differ from its caps"
    if config is not None:
        config_total = config.get("total_cap")
        if (
            config.get("schema_version") != 1
            or ledger.get("currency") != config.get("currency")
            or isinstance(config_total, bool)
            or not isinstance(config_total, (int, float))
            or not money_equal(float(ledger["total_cap"]), float(config_total))
            or provider_caps != config.get("provider_caps")
            or not isinstance(config.get("models"), dict)
        ):
            return "budget ledger does not match its frozen config"
    if float(ledger["spent_upper_bound"]) > float(ledger["total_cap"]):
        return "budget ledger spent total exceeds its cap"

    settled_ids: set[str] = set()
    derived_spent = 0.0
    derived_provider_spent = {provider: 0.0 for provider in provider_caps}
    for row in settled:
        if not isinstance(row, dict):
            return "budget ledger contains a malformed settlement"
        required_settlement = {
            "request_id",
            "provider",
            "model",
            "reported_model",
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "charged_upper_bound_usd",
            "reserved_usd",
            "settled_at_unix_ms",
        }
        if not required_settlement.issubset(row):
            return "budget ledger contains a malformed settlement"
        request_id = row.get("request_id")
        provider = row.get("provider")
        text_fields = (request_id, row.get("model"), row.get("reported_model"))
        token_fields = (
            row.get("input_tokens"),
            row.get("output_tokens"),
            row.get("total_tokens"),
            row.get("settled_at_unix_ms"),
        )
        charged = row.get("charged_upper_bound_usd")
        reserved = row.get("reserved_usd")
        if (
            not all(isinstance(value, str) and value for value in text_fields)
            or provider not in provider_caps
            or any(
                isinstance(value, bool) or not isinstance(value, int) or value < 0
                for value in token_fields
            )
            or row["input_tokens"] + row["output_tokens"] != row["total_tokens"]
            or any(
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(float(value))
                or float(value) < 0
                for value in (charged, reserved)
            )
            or float(charged) > float(reserved) + 1e-9
        ):
            return "budget ledger contains a malformed settlement"
        if config is not None:
            profile = budget_model_profile(config, str(provider), str(row["model"]))
            accepted = profile.get("accepted_reported_models") if profile else None
            context_limit = profile.get("context_limit") if profile else None
            output_limit = profile.get("max_output_tokens") if profile else None
            expected_reserve = (
                budget_price(profile, context_limit, output_limit)
                if profile is not None
                and isinstance(context_limit, int)
                and not isinstance(context_limit, bool)
                and context_limit > 0
                and isinstance(output_limit, int)
                and not isinstance(output_limit, bool)
                and output_limit > 0
                else None
            )
            expected_charge = (
                budget_price(profile, row["input_tokens"], row["output_tokens"])
                if profile is not None
                else None
            )
            if (
                not isinstance(accepted, list)
                or row["reported_model"] not in accepted
                or expected_reserve is None
                or expected_charge is None
                or row["input_tokens"] > context_limit
                or row["output_tokens"] > output_limit
                or not money_equal(float(reserved), expected_reserve)
                or not money_equal(float(charged), expected_charge)
            ):
                return "budget ledger settlement differs from its frozen model profile"
        if request_id in settled_ids:
            return "budget ledger repeats a settled request id"
        settled_ids.add(request_id)
        derived_spent += float(charged)
        derived_provider_spent[str(provider)] += float(charged)
    outstanding_total = 0.0
    outstanding_by_provider = {provider: 0.0 for provider in provider_caps}
    for request_id, reservation in outstanding.items():
        required_reservation = {
            "request_id",
            "provider",
            "model",
            "reserved_usd",
            "input_reserve_tokens",
            "output_reserve_tokens",
            "created_at_unix_ms",
        }
        if (
            not isinstance(request_id, str)
            or not isinstance(reservation, dict)
            or not required_reservation.issubset(reservation)
            or reservation.get("request_id") != request_id
            or request_id in settled_ids
        ):
            return "budget ledger contains a malformed outstanding reservation"
        provider = reservation.get("provider")
        reserved = reservation.get("reserved_usd")
        if (
            provider not in provider_caps
            or not isinstance(reservation.get("model"), str)
            or not reservation.get("model")
            or any(
                isinstance(reservation.get(key), bool)
                or not isinstance(reservation.get(key), int)
                or int(reservation[key]) < 0
                for key in (
                    "input_reserve_tokens",
                    "output_reserve_tokens",
                    "created_at_unix_ms",
                )
            )
            or isinstance(reserved, bool)
            or not isinstance(reserved, (int, float))
            or not math.isfinite(float(reserved))
            or float(reserved) < 0
        ):
            return "budget ledger contains a malformed outstanding reservation"
        if config is not None:
            profile = budget_model_profile(
                config, str(provider), str(reservation["model"])
            )
            context_limit = profile.get("context_limit") if profile else None
            output_limit = profile.get("max_output_tokens") if profile else None
            expected_reserve = (
                budget_price(profile, context_limit, output_limit)
                if profile is not None
                and isinstance(context_limit, int)
                and not isinstance(context_limit, bool)
                and context_limit > 0
                and isinstance(output_limit, int)
                and not isinstance(output_limit, bool)
                and output_limit > 0
                else None
            )
            if (
                expected_reserve is None
                or reservation["input_reserve_tokens"] != context_limit
                or reservation["output_reserve_tokens"] != output_limit
                or not money_equal(float(reserved), expected_reserve)
            ):
                return (
                    "budget ledger outstanding reservation differs from its "
                    "frozen model profile"
                )
        outstanding_total += float(reserved)
        outstanding_by_provider[str(provider)] += float(reserved)
    if not money_equal(float(ledger["spent_upper_bound"]), derived_spent):
        return "budget ledger cumulative spend differs from its settlements"
    if not money_equal(
        float(ledger["spent_upper_bound"]),
        sum(float(value) for value in provider_spent.values()),
    ):
        return "budget ledger provider spend does not sum to cumulative spend"
    for provider, amount in derived_provider_spent.items():
        if not money_equal(float(provider_spent[provider]), amount):
            return f"budget ledger cumulative spend differs for {provider}"
        if (
            amount + outstanding_by_provider[provider]
            > float(provider_caps[provider]) + 1e-9
        ):
            return f"budget ledger reservations exceed the cap for {provider}"
    if derived_spent + outstanding_total > float(ledger["total_cap"]) + 1e-9:
        return "budget ledger reservations exceed the total cap"
    return None


def budget_ledger_descendant_failure(
    initial: Mapping[str, Any],
    current: Mapping[str, Any],
    config: Mapping[str, Any] | None = None,
) -> str | None:
    for ledger in (initial, current):
        failure = budget_ledger_failure(ledger, config)
        if failure:
            return failure
    for key in ("schema_version", "currency", "total_cap", "provider_caps"):
        if current.get(key) != initial.get(key):
            return f"budget ledger changed immutable field {key}"
    if float(current["spent_upper_bound"]) < float(initial["spent_upper_bound"]):
        return "budget ledger cumulative spend decreased across supersession"
    for provider, amount in initial["provider_spent_upper_bound"].items():
        if float(current["provider_spent_upper_bound"].get(provider, -1)) < float(amount):
            return f"budget ledger cumulative spend decreased for {provider}"

    current_settled = {
        str(row["request_id"]): row for row in current["settled"]
    }
    for row in initial["settled"]:
        if current_settled.get(str(row["request_id"])) != row:
            return f"predecessor settlement changed or disappeared: {row['request_id']}"
    for request_id, reservation in initial["outstanding"].items():
        if current["outstanding"].get(request_id) != reservation:
            return f"predecessor reservation changed or disappeared: {request_id}"
    return None


def replacement_reserve_failure(
    ledger: Mapping[str, Any],
    config: Mapping[str, Any],
    rows: Iterable[Mapping[str, Any]],
) -> str | None:
    outstanding = ledger["outstanding"]
    pending_total = sum(float(row["reserved_usd"]) for row in outstanding.values())
    pending_by_provider = {
        provider: sum(
            float(row["reserved_usd"])
            for row in outstanding.values()
            if row["provider"] == provider
        )
        for provider in ledger["provider_caps"]
    }
    for row in rows:
        provider = str(row["provider"])
        model = str(row["model"])
        profile = budget_model_profile(config, provider, model)
        context_limit = profile.get("context_limit") if profile else None
        output_limit = profile.get("max_output_tokens") if profile else None
        reserve = (
            budget_price(profile, context_limit, output_limit)
            if profile is not None
            and isinstance(context_limit, int)
            and not isinstance(context_limit, bool)
            and isinstance(output_limit, int)
            and not isinstance(output_limit, bool)
            else None
        )
        if reserve is None:
            return f"replacement has no valid frozen budget profile for {provider}/{model}"
        pending_total += reserve
        pending_by_provider[provider] += reserve
    if (
        float(ledger["spent_upper_bound"]) + pending_total
        > float(ledger["total_cap"]) + 1e-9
    ):
        return "replacement requests do not fit the remaining total budget envelope"
    for provider, pending in pending_by_provider.items():
        if (
            float(ledger["provider_spent_upper_bound"][provider]) + pending
            > float(ledger["provider_caps"][provider]) + 1e-9
        ):
            return (
                "replacement requests do not fit the remaining provider budget "
                f"envelope for {provider}"
            )
    return None


def campaign_identity(campaign: Mapping[str, Any]) -> Dict[str, Any]:
    publisher = campaign.get("publisher")
    publisher_identity = None
    if isinstance(publisher, dict):
        publisher_identity = {
            key: publisher.get(key)
            for key in (
                "instrument_set_sha256",
                "sanity_target",
                "expected_checks",
                "entries",
            )
        }
    return {
        key: campaign.get(key)
        for key in (
            "schema_version",
            "campaign_id",
            "binary_sha256",
            "entrant_manifest_sha256",
            "budget_config_sha256",
            "instrument_set_sha256",
            "prompt_source_sha256",
            "scorer_version",
            "calibration",
        )
    } | {"publisher": publisher_identity}


def validate_defect_evidence(
    path: Path,
    predecessor: Mapping[str, Any],
    replacement_binary: Path,
    row_ids: set[str],
    secret_values: Iterable[str],
) -> tuple[Dict[str, Any], list[Dict[str, Any]], str]:
    if not path.is_file() or path.is_symlink() or path.stat().st_size > 1024 * 1024:
        raise SystemExit("defect evidence must be one regular JSON file no larger than 1 MiB")
    evidence = load_json(path)
    expected_keys = {
        "schema_version",
        "classification",
        "defect_id",
        "summary",
        "affected_entrants",
        "predecessor_campaign_id",
        "predecessor_binary_sha256",
        "replacement_binary_sha256",
        "fix_source_commit",
        "artifacts",
    }
    if set(evidence) != expected_keys:
        raise SystemExit("defect evidence schema contains missing or unapproved fields")
    if evidence.get("schema_version") != SUPERSESSION_SCHEMA:
        raise SystemExit("defect evidence schema version is not supported")
    if evidence.get("classification") != "infrastructure_defect":
        raise SystemExit("only infrastructure defects can authorize a paid supersession")
    defect_id = evidence.get("defect_id")
    if not isinstance(defect_id, str) or not re.fullmatch(
        r"[A-Za-z0-9][A-Za-z0-9._-]{2,127}", defect_id
    ):
        raise SystemExit("defect evidence has an invalid defect_id")
    summary = evidence.get("summary")
    if not isinstance(summary, str) or not summary.strip() or len(summary) > 2000:
        raise SystemExit("defect evidence requires a bounded non-empty summary")
    affected = evidence.get("affected_entrants")
    if (
        not isinstance(affected, list)
        or not affected
        or any(not isinstance(value, str) for value in affected)
        or len(set(affected)) != len(affected)
        or not set(affected).issubset(row_ids)
    ):
        raise SystemExit("defect evidence has invalid affected entrants")
    replacement_sha = sha256_file(replacement_binary)
    exact = {
        "predecessor_campaign_id": predecessor.get("campaign_id"),
        "predecessor_binary_sha256": predecessor.get("binary_sha256"),
        "replacement_binary_sha256": replacement_sha,
        "fix_source_commit": git_value("rev-parse", "HEAD"),
    }
    for key, expected in exact.items():
        if evidence.get(key) != expected:
            raise SystemExit(f"defect evidence does not bind exact {key}")

    raw_artifacts = evidence.get("artifacts")
    if not isinstance(raw_artifacts, list) or not raw_artifacts:
        raise SystemExit("defect evidence has no supporting artifacts")
    artifacts: list[Dict[str, Any]] = []
    roles: set[str] = set()
    for raw in raw_artifacts:
        if not isinstance(raw, dict) or set(raw) != {"role", "path", "sha256"}:
            raise SystemExit("defect evidence artifact schema is malformed")
        role = raw.get("role")
        if role not in {"root_cause", "regression_test"}:
            raise SystemExit("defect evidence artifact role is not approved")
        source = Path(str(raw.get("path", ""))).expanduser().resolve()
        if (
            not source.is_file()
            or source.is_symlink()
            or source.stat().st_size == 0
            or source.stat().st_size > 10 * 1024 * 1024
        ):
            raise SystemExit(f"defect evidence artifact is not a bounded regular file: {source}")
        digest = sha256_file(source)
        if raw.get("sha256") != digest:
            raise SystemExit(f"defect evidence artifact hash differs: {source}")
        if secret_occurrences([source], secret_values):
            raise SystemExit("defect evidence artifact contains a provider credential")
        roles.add(str(role))
        artifacts.append({"role": role, "source": source, "sha256": digest})
    if roles != {"root_cause", "regression_test"}:
        raise SystemExit("defect evidence requires root-cause and regression-test artifacts")
    return evidence, artifacts, sha256_file(path)


def predecessor_seal(
    root: Path,
    campaign: Mapping[str, Any],
    rows: Iterable[Mapping[str, Any]],
    transition_id: str,
) -> Dict[str, Any]:
    ledger_path = Path(str(campaign.get("budget_ledger", "")))
    ledger = load_json(ledger_path)
    budget_config = load_json(Path(str(campaign.get("budget_config", ""))))
    failure = budget_ledger_failure(ledger, budget_config)
    if failure:
        raise SystemExit(f"predecessor {failure}")
    sealed_entrants: Dict[str, Any] = {}
    for row in rows:
        entrant_id = str(row["id"])
        unit = root / "entrants" / entrant_id
        state_path = state_file(root, entrant_id)
        state = read_state(root, entrant_id)
        lifecycle_path = Path(str(state["provider_lifecycle"]))
        smoke_unit = root / "smoke" / entrant_id
        sealed_entrants[entrant_id] = {
            "state_sha256": sha256_file(state_path),
            "unit_sha256": artifact_tree_sha256(unit),
            "immutable_unit_sha256": artifact_tree_sha256(
                unit,
                excluded_relative_paths={"state.json", "state.lock"},
            ),
            "raw_tree_sha256": hash_tree(Path(str(state["tree"]))),
            "scores_sha256": optional_artifact_tree_sha256(
                root / "scores" / entrant_id
            ),
            "publish_sha256": optional_artifact_tree_sha256(
                root / "publish" / entrant_id
            ),
            "lifecycle_sha256": (
                sha256_file(lifecycle_path) if lifecycle_path.is_file() else None
            ),
            "smoke_unit_sha256": artifact_tree_sha256(smoke_unit),
            "status": state["status"],
            "provider_episode_attempts": int(state.get("provider_episode_attempts", 0)),
            "fixture_seed": state["fixture_seed"],
            "admitted_requests": int(state.get("admitted_requests", 0)),
            "provider_terminal_requests": int(
                state.get("provider_terminal_requests", 0)
            ),
        }
    return {
        "schema_version": SUPERSESSION_SCHEMA,
        "transition_id": transition_id,
        "predecessor_root": str(root.resolve()),
        "campaign_identity": campaign_identity(campaign),
        "campaign_sha256": sha256_file(campaign_file(root)),
        "manager_sha256": sha256_file(root / "manager.json"),
        "budget_ledger_sha256": sha256_file(ledger_path),
        "entrants": sealed_entrants,
    }


def predecessor_seal_failure(root: Path, seal: Mapping[str, Any]) -> str | None:
    try:
        campaign = load_json(campaign_file(root))
        if sha256_file(campaign_file(root)) != seal.get("campaign_sha256"):
            return "predecessor campaign receipt changed"
        if campaign_identity(campaign) != seal.get("campaign_identity"):
            return "predecessor immutable campaign identity changed"
        if sha256_file(root / "manager.json") != seal.get("manager_sha256"):
            return "predecessor manager receipt changed"
        ledger = Path(str(campaign["budget_ledger"]))
        if sha256_file(ledger) != seal.get("budget_ledger_sha256"):
            return "predecessor budget ledger changed after stop"
        sealed_entrants = seal.get("entrants")
        if not isinstance(sealed_entrants, dict):
            return "predecessor seal has no entrants"
        for entrant_id, expected in sealed_entrants.items():
            if not isinstance(expected, dict):
                return f"predecessor seal is malformed for {entrant_id}"
            unit = root / "entrants" / str(entrant_id)
            state = read_state(root, str(entrant_id))
            lifecycle_path = Path(str(state["provider_lifecycle"]))
            lifecycle_sha = sha256_file(lifecycle_path) if lifecycle_path.is_file() else None
            smoke_unit = root / "smoke" / str(entrant_id)
            current = {
                "state_sha256": sha256_file(state_file(root, str(entrant_id))),
                "unit_sha256": artifact_tree_sha256(unit),
                "immutable_unit_sha256": artifact_tree_sha256(
                    unit,
                    excluded_relative_paths={"state.json", "state.lock"},
                ),
                "raw_tree_sha256": hash_tree(Path(str(state["tree"]))),
                "lifecycle_sha256": lifecycle_sha,
                "smoke_unit_sha256": artifact_tree_sha256(smoke_unit),
                "scores_sha256": optional_artifact_tree_sha256(
                    root / "scores" / str(entrant_id)
                ),
                "publish_sha256": optional_artifact_tree_sha256(
                    root / "publish" / str(entrant_id)
                ),
            }
            for key, value in current.items():
                if expected.get(key) != value:
                    return f"predecessor {entrant_id} artifact changed: {key}"
    except (OSError, KeyError, json.JSONDecodeError, SystemExit) as error:
        return f"predecessor seal cannot be verified: {error}"
    return None


def supersession_fault(_stage: str) -> None:
    return None


def predecessor_smoke_terminal_usage(
    root: Path, entrant_id: str, row: Mapping[str, Any]
) -> tuple[Dict[str, Dict[str, Any]], bool, str | None]:
    campaign = load_json(campaign_file(root))
    try:
        state = read_smoke_state(root, entrant_id)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return {}, False, f"smoke state cannot be read: {error}"
    if (
        state.get("entrant") != entrant_id
        or state.get("provider") != row.get("provider")
        or state.get("model") != row.get("model")
        or state.get("status") not in SMOKE_TERMINAL_STATES
    ):
        return {}, False, "smoke state identity or stopped status is invalid"
    launch_attempts = state.get("launch_attempts")
    if (
        isinstance(launch_attempts, bool)
        or not isinstance(launch_attempts, int)
        or launch_attempts < 0
    ):
        return {}, False, "smoke launch count is malformed"
    attempts_root = root / "smoke" / entrant_id / "attempts"
    if not attempts_root.is_dir() or attempts_root.is_symlink():
        return {}, bool(launch_attempts), "smoke attempts root is missing or symbolic"
    expected_names = {f"attempt-{number}" for number in range(1, launch_attempts + 1)}
    actual_names = {path.name for path in attempts_root.iterdir()}
    if actual_names != expected_names:
        return {}, bool(launch_attempts), "smoke attempt directories differ from launch count"
    evidence_hashes = state.get("attempt_evidence_sha256")
    if not isinstance(evidence_hashes, dict):
        return {}, bool(launch_attempts), "smoke evidence index is malformed"

    terminal_usage: Dict[str, Dict[str, Any]] = {}
    for attempt in range(1, launch_attempts + 1):
        attempt_name = f"attempt-{attempt}"
        attempt_root = attempts_root / attempt_name
        if not attempt_root.is_dir() or attempt_root.is_symlink():
            return {}, True, f"{attempt_name} is missing or symbolic"
        lifecycle_path = attempt_root / "provider-lifecycle.jsonl"
        if lifecycle_path.is_symlink():
            return {}, True, f"{attempt_name} lifecycle is symbolic"
        lifecycle = lifecycle_summary(
            lifecycle_path,
            expected_provider=str(row["provider"]),
            expected_model=str(row["model"]),
        )
        lifecycle_problem = lifecycle_failure(lifecycle)
        if lifecycle_problem:
            return {}, True, f"{attempt_name} lifecycle is ambiguous: {lifecycle_problem}"
        if lifecycle["admitted"] != lifecycle["terminal"]:
            return {}, True, f"{attempt_name} has unterminated provider admission"

        evidence_path = attempt_root / "attempt-evidence.json"
        indexed_hash = evidence_hashes.get(attempt_name)
        if evidence_path.exists():
            if evidence_path.is_symlink() or not evidence_path.is_file():
                return {}, True, f"{attempt_name} evidence is not a regular file"
            if indexed_hash != sha256_file(evidence_path):
                return {}, True, f"{attempt_name} evidence hash is not sealed"
            try:
                evidence = load_json(evidence_path)
            except (OSError, json.JSONDecodeError, SystemExit) as error:
                return {}, True, f"{attempt_name} evidence cannot be read: {error}"
            if evidence.get("lifecycle") != lifecycle:
                return {}, True, f"{attempt_name} lifecycle differs from sealed evidence"
            isolation = evidence.get("listener_isolation")
            if not isinstance(isolation, dict):
                return {}, True, f"{attempt_name} has no listener isolation evidence"
            isolation_state = {
                **isolation,
                "attempt_root": str(attempt_root),
            }
            isolation_problem = listener_isolation_failure(
                campaign, row, isolation_state, smoke=True
            )
            if isolation_problem:
                return {}, True, f"{attempt_name} {isolation_problem}"
        elif not (
            attempt == launch_attempts
            and state.get("status") == "STOPPED"
            and state.get("active_attempt") is True
            and state.get("attempt") == attempt
            and Path(str(state.get("attempt_root", ""))).resolve()
            == attempt_root.resolve()
            and Path(str(state.get("provider_lifecycle", ""))).resolve()
            == lifecycle_path.resolve()
            and indexed_hash is None
        ):
            return {}, True, f"{attempt_name} has no sealed or stopped crash evidence"
        else:
            isolation_problem = listener_isolation_failure(
                campaign, row, state, smoke=True
            )
            if isolation_problem:
                return {}, True, f"{attempt_name} {isolation_problem}"

        attempt_usage = lifecycle.get("terminal_usage")
        if not isinstance(attempt_usage, dict):
            return {}, True, f"{attempt_name} terminal usage map is malformed"
        duplicate = set(terminal_usage) & set(attempt_usage)
        if duplicate:
            return {}, True, "smoke request ID was reused across attempts"
        terminal_usage.update(attempt_usage)
    if set(evidence_hashes) - expected_names:
        return {}, bool(launch_attempts), "smoke evidence index contains unknown attempts"
    return terminal_usage, bool(launch_attempts), None


def full_entrant_was_never_started(
    state: Mapping[str, Any], lifecycle: Mapping[str, Any]
) -> bool:
    return (
        state.get("status") == "STOPPED"
        and int(state.get("provider_episode_attempts", 0)) == 0
        and int(state.get("admitted_requests", 0)) == 0
        and int(state.get("provider_terminal_requests", 0)) == 0
        and state.get("score") is None
        and not state.get("verdict")
        and int(lifecycle.get("events", 0)) == 0
        and lifecycle_failure(lifecycle) is None
    )


def smoke_has_proven_pre_admission_activity(state: Mapping[str, Any]) -> bool:
    failure = state.get("failure")
    queued_at = state.get("queued_at")
    return (
        state.get("status") in {"FAILED", "PRE_ADMISSION_FAILURE", "STOPPED"}
        and int(state.get("launch_attempts", 0)) == 0
        and int(state.get("admitted_episodes", 0)) == 0
        and state.get("active_attempt") is False
        and isinstance(queued_at, str)
        and bool(queued_at.strip())
        and isinstance(failure, str)
        and bool(failure.strip())
    )


def validate_stopped_predecessor(
    root: Path,
    campaign: Mapping[str, Any],
    rows: list[Mapping[str, Any]],
    affected_entrants: set[str],
) -> tuple[
    Dict[str, Dict[str, Any]],
    Dict[str, Any],
    Dict[str, list[str]],
    set[str],
]:
    if campaign.get("status") != "STOPPED":
        raise SystemExit("predecessor campaign must be explicitly stopped")
    predecessor_lineage = validated_campaign_lineage(campaign)
    if predecessor_lineage["generation"] != 0:
        raise SystemExit("a supersession successor cannot be superseded again")
    manager = load_json(root / "manager.json")
    if manager.get("status") != "STOPPED":
        raise SystemExit("predecessor manager is not stopped")
    if process_alive(manager.get("pid"), manager.get("identity")):
        raise SystemExit("predecessor manager is still alive")
    manager_pgid = int(manager.get("pgid") or 0)
    if manager_pgid and process_group_members(manager_pgid):
        raise SystemExit("predecessor manager process group is not clean")

    row_ids = {str(row["id"]) for row in rows}
    if not affected_entrants or not affected_entrants.issubset(row_ids):
        raise SystemExit("supersession has invalid affected entrants")
    states: Dict[str, Dict[str, Any]] = {}
    terminal_outstanding: Dict[str, list[str]] = {}
    unstarted: set[str] = set()
    ledger = load_json(Path(str(campaign["budget_ledger"])))
    budget_config = load_json(Path(str(campaign["budget_config"])))
    failure = budget_ledger_failure(ledger, budget_config)
    if failure:
        raise SystemExit(f"predecessor {failure}")
    max_episodes = int(
        load_json(Path(str(campaign["entrant_manifest"])))
        ["spend_policy"]["max_full_episodes_per_model"]
    )

    for row in rows:
        entrant_id = str(row["id"])
        state = read_state(root, entrant_id)
        states[entrant_id] = state
        status = str(state.get("status"))
        if status not in TERMINAL_BUILD_STATES | POST_BUILD_STATES:
            raise SystemExit(f"predecessor entrant is not stopped: {entrant_id}={status}")
        for pid_key, pgid_key, identity_key in (
            ("supervisor_pid", "supervisor_pgid", "supervisor_identity"),
            ("goose_pid", "process_group", "goose_identity"),
            ("publisher_pid", "publisher_pgid", "publisher_identity"),
            ("score_pid", "score_pgid", "score_identity"),
            ("smoke_pid", "smoke_pgid", "smoke_identity"),
        ):
            if process_alive(state.get(pid_key), state.get(identity_key)):
                raise SystemExit(f"predecessor {entrant_id} still owns {pid_key}")
            pgid = int(state.get(pgid_key) or 0)
            if pgid and process_group_members(pgid):
                raise SystemExit(f"predecessor {entrant_id} still owns process group {pgid}")
        smoke_state = read_smoke_state(root, entrant_id)
        if process_alive(
            smoke_state.get("supervisor_pid"), smoke_state.get("supervisor_identity")
        ):
            raise SystemExit(f"predecessor {entrant_id} smoke supervisor is still alive")
        smoke_pgid = int(smoke_state.get("supervisor_pgid") or 0)
        if smoke_pgid and process_group_members(smoke_pgid):
            raise SystemExit(
                f"predecessor {entrant_id} smoke process group is still alive"
            )

        lifecycle = lifecycle_summary(
            Path(str(state["provider_lifecycle"])),
            expected_provider=str(row["provider"]),
            expected_model=str(row["model"]),
        )
        smoke_usage, smoke_launched, smoke_problem = predecessor_smoke_terminal_usage(
            root, entrant_id, row
        )
        if smoke_problem:
            raise SystemExit(
                f"predecessor smoke evidence is ambiguous for {entrant_id}: {smoke_problem}"
            )
        if entrant_id not in affected_entrants:
            if full_entrant_was_never_started(state, lifecycle):
                unstarted.add(entrant_id)
                continue
            if status not in BUILD_SUCCESS_STATES:
                raise SystemExit(
                    f"unsuccessful predecessor entrant was omitted from evidence: {entrant_id}"
                )
            raw_hash = hash_tree(Path(str(state["tree"])))
            if raw_hash != state.get("raw_tree_sha256"):
                raise SystemExit(f"successful predecessor raw tree changed: {entrant_id}")
            continue
        if status in BUILD_SUCCESS_STATES:
            raise SystemExit(
                f"successful build cannot be rerun as an infrastructure defect: {entrant_id}"
            )
        if state.get("score") is not None or state.get("verdict"):
            raise SystemExit(f"scored or outcome-bearing entrant cannot be rerun: {entrant_id}")
        if (
            full_entrant_was_never_started(state, lifecycle)
            and not smoke_launched
            and not smoke_has_proven_pre_admission_activity(smoke_state)
        ):
            raise SystemExit(
                f"defect evidence names an entrant with no smoke or full activity: {entrant_id}"
            )
        attempts = int(state.get("provider_episode_attempts", 0))
        if attempts >= max_episodes:
            raise SystemExit(f"provider episode limit is already exhausted: {entrant_id}")
        lifecycle_problem = lifecycle_failure(lifecycle)
        if lifecycle_problem:
            raise SystemExit(
                f"affected predecessor lifecycle is ambiguous for {entrant_id}: "
                f"{lifecycle_problem}"
            )
        if lifecycle["admitted"] != lifecycle["terminal"]:
            raise SystemExit(f"affected predecessor has unterminated admission: {entrant_id}")
        if int(state.get("admitted_requests", 0)) != int(lifecycle["admitted"]):
            raise SystemExit(f"affected predecessor admission count drifted: {entrant_id}")
        if int(state.get("provider_terminal_requests", 0)) != int(
            lifecycle["terminal"]
        ):
            raise SystemExit(f"affected predecessor terminal count drifted: {entrant_id}")
        outstanding, ledger_error = entrant_outstanding_reservations(campaign, row)
        if ledger_error:
            raise SystemExit(
                f"affected predecessor retains ambiguous budget reservations: {entrant_id}"
            )
        full_terminal_usage = lifecycle.get("terminal_usage")
        if not isinstance(full_terminal_usage, dict):
            raise SystemExit(f"affected predecessor has no terminal usage map: {entrant_id}")
        duplicate_request_ids = set(full_terminal_usage) & set(smoke_usage)
        if duplicate_request_ids:
            raise SystemExit(
                f"affected predecessor reused request IDs across smoke and full work: {entrant_id}"
            )
        terminal_usage = {**smoke_usage, **full_terminal_usage}
        settlements = {
            str(settlement["request_id"]): settlement
            for settlement in ledger["settled"]
            if settlement["provider"] == row["provider"]
            and settlement["model"] == row["model"]
        }
        for request_id, usage in terminal_usage.items():
            settlement = settlements.get(request_id)
            if settlement is None:
                if request_id not in outstanding:
                    raise SystemExit(
                        "affected predecessor terminal request has no preserved "
                        f"accounting evidence: {entrant_id}/{request_id}"
                    )
            elif any(
                settlement[key] != usage[key]
                for key in (
                    "reported_model",
                    "input_tokens",
                    "output_tokens",
                    "total_tokens",
                )
            ):
                raise SystemExit(
                    f"affected predecessor settlement differs from terminal usage: "
                    f"{entrant_id}/{request_id}"
                )
        for request_id in outstanding:
            usage = terminal_usage.get(request_id)
            if not isinstance(usage, dict):
                raise SystemExit(
                    "affected predecessor has an uncorrelated outstanding reserve: "
                    f"{entrant_id}/{request_id}"
                )
            if (
                usage["reported_model"] not in row["accepted_reported_models"]
                or usage["input_tokens"] > int(row["context_limit"])
                or usage["output_tokens"] > int(row["max_output_tokens"])
            ):
                raise SystemExit(
                    "affected predecessor terminal usage differs from the frozen "
                    f"model profile: {entrant_id}/{request_id}"
                )
        terminal_outstanding[entrant_id] = outstanding

    busy = [str(row["vendor_port"]) for row in rows if not port_is_free(int(row["vendor_port"]))]
    if busy:
        raise SystemExit(f"predecessor vendor ports are still occupied: {', '.join(busy)}")
    reserve_problem = replacement_reserve_failure(
        ledger,
        budget_config,
        (
            row
            for row in rows
            if str(row["id"]) in affected_entrants | unstarted
        ),
    )
    if reserve_problem:
        raise SystemExit(reserve_problem)
    return states, ledger, terminal_outstanding, unstarted


def supersession_instrument_failure(
    predecessor: Mapping[str, Any], successor: Mapping[str, Any]
) -> str | None:
    old = predecessor.get("instrument_hashes")
    new = successor.get("instrument_hashes")
    if not isinstance(old, dict) or not isinstance(new, dict):
        return "campaign instrument hashes are missing"
    changed = {
        key
        for key in set(old) | set(new)
        if old.get(key) != new.get(key)
    }
    unapproved = changed - SUPERSESSION_ALLOWED_INSTRUMENT_CHANGES
    if unapproved:
        return f"supersession changed frozen benchmark semantics: {', '.join(sorted(unapproved))}"
    for key in (
        "entrant_manifest_sha256",
        "budget_config_sha256",
        "prompt_source_sha256",
        "scorer_version",
        "calibration",
    ):
        if predecessor.get(key) != successor.get(key):
            return f"supersession changed immutable benchmark field {key}"
    old_publisher = predecessor.get("publisher")
    new_publisher = successor.get("publisher")
    if not isinstance(old_publisher, dict) or not isinstance(new_publisher, dict):
        return "supersession publisher identity is missing"
    for key in (
        "instrument_set_sha256",
        "sanity_target",
        "expected_checks",
        "entries",
    ):
        if old_publisher.get(key) != new_publisher.get(key):
            return f"supersession changed publisher field {key}"
    return None


def copy_evidence_bundle(
    destination: Path,
    evidence_path: Path,
    evidence_sha256: str,
    artifacts: list[Mapping[str, Any]],
) -> list[Dict[str, str]]:
    destination.mkdir(parents=True, exist_ok=False)
    copied_evidence = destination / "defect-evidence.json"
    atomic_copy(evidence_path, copied_evidence, 0o600)
    if sha256_file(copied_evidence) != evidence_sha256:
        raise SystemExit("copied defect evidence changed")
    copied = []
    for index, artifact in enumerate(artifacts):
        role = str(artifact["role"])
        target = destination / f"artifact-{index:02d}-{role}"
        atomic_copy(Path(str(artifact["source"])), target, 0o600)
        if sha256_file(target) != artifact["sha256"]:
            raise SystemExit("copied defect artifact changed")
        copied.append(
            {
                "role": role,
                "path": str(target.relative_to(destination.parent)),
                "sha256": str(artifact["sha256"]),
            }
        )
    return copied


def copy_carried_entrant(
    predecessor_root: Path,
    staged_root: Path,
    target_root: Path,
    entrant_id: str,
    sealed: Mapping[str, Any],
    transition_id: str,
) -> None:
    source = predecessor_root / "entrants" / entrant_id
    destination = staged_root / "entrants" / entrant_id
    shutil.rmtree(destination)
    shutil.copytree(source, destination, symlinks=True)
    if artifact_tree_sha256(destination) != sealed.get("unit_sha256"):
        raise SystemExit(f"carried entrant copy changed: {entrant_id}")
    state = remap_paths(
        load_json(destination / "state.json"), predecessor_root, target_root
    )
    state.update(
        {
            "lineage_role": "carried_success",
            "supersession_transition_id": transition_id,
            "predecessor_state_sha256": sealed["state_sha256"],
            "predecessor_unit_sha256": sealed["unit_sha256"],
            "updated_at": utc_now(),
        }
    )
    atomic_json(destination / "state.json", state)
    if artifact_tree_sha256(
        destination,
        excluded_relative_paths={"state.json", "state.lock"},
    ) != sealed.get("immutable_unit_sha256"):
        raise SystemExit(f"carried entrant immutable payload changed: {entrant_id}")
    for collection in ("scores", "publish"):
        old = predecessor_root / collection / entrant_id
        new = staged_root / collection / entrant_id
        if old.exists():
            if new.exists():
                shutil.rmtree(new)
            shutil.copytree(old, new, symlinks=True)
        expected = sealed.get(f"{collection}_sha256")
        if optional_artifact_tree_sha256(new) != expected:
            raise SystemExit(f"carried entrant {collection} copy changed: {entrant_id}")


def reset_affected_entrant(
    staged_root: Path,
    target_root: Path,
    entrant_id: str,
    predecessor_state: Mapping[str, Any],
    sealed: Mapping[str, Any],
    transition_id: str,
    lineage_role: str = "infrastructure_defect_restart",
) -> None:
    state = remap_paths(read_state(staged_root, entrant_id), staged_root, target_root)
    state.update(
        {
            "status": "PLANNED",
            "provider_episode_attempts": int(
                predecessor_state.get("provider_episode_attempts", 0)
            ),
            "fixture_seed": predecessor_state["fixture_seed"],
            "admitted_requests": 0,
            "provider_terminal_requests": 0,
            "failure": None,
            "lineage_role": lineage_role,
            "supersession_transition_id": transition_id,
            "predecessor_state_sha256": sealed["state_sha256"],
            "predecessor_unit_sha256": sealed["unit_sha256"],
            "predecessor_status": predecessor_state["status"],
            "updated_at": utc_now(),
        }
    )
    atomic_json(state_file(staged_root, entrant_id), state)


def supersession_transition_id(
    predecessor_root: Path,
    target_root: Path,
    evidence_sha256: str,
    replacement_binary_sha256: str,
    predecessor: Mapping[str, Any],
) -> str:
    material = {
        "predecessor_root": str(predecessor_root.resolve()),
        "predecessor_campaign_id": predecessor.get("campaign_id"),
        "target_root": str(target_root.resolve()),
        "evidence_sha256": evidence_sha256,
        "replacement_binary_sha256": replacement_binary_sha256,
        "predecessor_binary_sha256": predecessor.get("binary_sha256"),
    }
    return sha256_bytes(json.dumps(material, sort_keys=True).encode())


def qualification_manifest_failure(
    source: Mapping[str, Any], candidate: Mapping[str, Any]
) -> str | None:
    def normalized(value: Mapping[str, Any]) -> Dict[str, Any]:
        copy = json.loads(json.dumps(value))
        rows = copy.get("entrants")
        if not isinstance(rows, list):
            return copy
        for row in rows:
            if isinstance(row, dict):
                row.pop("endpoint_family", None)
                row.pop("base_url_env", None)
        return copy

    if normalized(source) != normalized(candidate):
        return "qualification restart changed benchmark manifest semantics"
    source_rows = source.get("entrants")
    candidate_rows = candidate.get("entrants")
    if not isinstance(source_rows, list) or not isinstance(candidate_rows, list):
        return "qualification restart entrant manifests are malformed"
    if len(source_rows) != len(candidate_rows):
        return "qualification restart changed the entrant roster"
    for old, new in zip(source_rows, candidate_rows):
        if not isinstance(old, dict) or not isinstance(new, dict):
            return "qualification restart entrant manifests are malformed"
        source_endpoint = {
            "endpoint_family": old.get("endpoint_family"),
            "base_url_env": old.get("base_url_env"),
        }
        target_endpoint = {
            "endpoint_family": new.get("endpoint_family"),
            "base_url_env": new.get("base_url_env"),
        }
        if source_endpoint == target_endpoint:
            continue
        transition = QUALIFICATION_ALLOWED_ENDPOINT_TRANSITIONS.get(
            (str(old.get("provider")), str(old.get("model")))
        )
        if (
            transition is None
            or source_endpoint != transition["source"]
            or target_endpoint != transition["target"]
        ):
            return "qualification restart attempted an unapproved endpoint transition"
    return None


def qualification_publisher_failure(
    source: Mapping[str, Any], target: Mapping[str, Any]
) -> str | None:
    ignored = {"frozen"}
    release_fields = {"commit", "instrument_set_sha256", "tracked_hashes"}
    stable_source = {
        key: value
        for key, value in source.items()
        if key not in ignored | release_fields
    }
    stable_target = {
        key: value
        for key, value in target.items()
        if key not in ignored | release_fields
    }
    if stable_source != stable_target:
        changed = sorted(
            key
            for key in set(stable_source) | set(stable_target)
            if stable_source.get(key) != stable_target.get(key)
        )
        return f"qualification restart changed publisher field {changed[0]}"

    source_release = {
        key: source.get(key) for key in sorted(release_fields)
    }
    target_release = {
        key: target.get(key) for key in sorted(release_fields)
    }
    if source_release == target_release:
        return None

    allowed_source = QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["source"]
    allowed_target = QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["target"]
    if source_release != allowed_source or target_release != allowed_target:
        return "qualification restart attempted an unapproved publisher transition"

    source_hashes = source_release["tracked_hashes"]
    target_hashes = target_release["tracked_hashes"]
    changed_hashes = {
        key
        for key in set(source_hashes) | set(target_hashes)
        if source_hashes.get(key) != target_hashes.get(key)
    }
    if changed_hashes != QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION[
        "changed_tracked_files"
    ]:
        return "qualification restart publisher transition changed the wrong files"
    return None


def qualification_instrument_failure(
    source: Mapping[str, Any], target: Mapping[str, Any]
) -> str | None:
    if source.get("binary_sha256") != target.get("binary_sha256"):
        return "qualification restart changed the frozen Goose binary"
    old_hashes = source.get("instrument_hashes")
    new_hashes = target.get("instrument_hashes")
    if not isinstance(old_hashes, dict) or not isinstance(new_hashes, dict):
        return "qualification restart instrument hashes are missing"
    changed = {
        key
        for key in set(old_hashes) | set(new_hashes)
        if old_hashes.get(key) != new_hashes.get(key)
    }
    unapproved = changed - QUALIFICATION_ALLOWED_INSTRUMENT_CHANGES
    if unapproved:
        return (
            "qualification restart changed frozen benchmark semantics: "
            + ", ".join(sorted(unapproved))
        )
    for key in (
        "budget_config_sha256",
        "prompt_source_sha256",
        "scorer_version",
        "calibration",
        "smoke_max_turns",
        "requested_models",
    ):
        if source.get(key) != target.get(key):
            return f"qualification restart changed immutable benchmark field {key}"
    try:
        source_manifest = load_json(Path(str(source["entrant_manifest"])))
        target_manifest = load_json(Path(str(target["entrant_manifest"])))
    except (OSError, KeyError, json.JSONDecodeError, SystemExit) as error:
        return f"qualification restart manifest cannot be compared: {error}"
    manifest_problem = qualification_manifest_failure(
        source_manifest, target_manifest
    )
    if manifest_problem:
        return manifest_problem
    old_publisher = source.get("publisher")
    new_publisher = target.get("publisher")
    if not isinstance(old_publisher, dict) or not isinstance(new_publisher, dict):
        return "qualification restart publisher identity is missing"
    return qualification_publisher_failure(old_publisher, new_publisher)


def qualification_publisher_runtime_failure(
    source: Mapping[str, Any],
    publish_live: bool,
    website_base_url: str,
    publish_verify_timeout_seconds: float,
    publish_verify_interval_seconds: float,
    publish_process_timeout_seconds: float,
) -> str | None:
    publisher = source.get("publisher")
    if not isinstance(publisher, dict):
        return "qualification source publisher identity is missing"
    if not publish_live:
        return "qualification restart must preserve live publication"
    try:
        base_url = normalized_website_base_url(website_base_url).rstrip("/")
    except SystemExit as error:
        return str(error)
    if (
        publish_verify_timeout_seconds <= 0
        or publish_verify_interval_seconds <= 0
        or publish_process_timeout_seconds <= 0
    ):
        return "qualification restart publisher timing must remain positive"
    expected = {
        "mode": "live",
        "website_base_url": base_url,
        "revalidate_endpoint": f"{base_url}/api/revalidate-benchmarks",
        "verify_timeout_seconds": publish_verify_timeout_seconds,
        "verify_interval_seconds": publish_verify_interval_seconds,
        "process_timeout_seconds": publish_process_timeout_seconds,
    }
    for key, value in expected.items():
        if publisher.get(key) != value:
            return f"qualification restart changed publisher field {key}"
    return None


def qualification_transition_id(
    source_root: Path,
    target_root: Path,
    evidence_sha256: str,
    source: Mapping[str, Any],
    target_manifest_sha256: str,
    target_instrument_set_sha256: str,
) -> str:
    material = {
        "kind": "instrument_qualification_restart",
        "source_root": str(source_root.resolve()),
        "source_campaign_id": source.get("campaign_id"),
        "source_smoke_contract_sha256": source.get("smoke_contract_sha256"),
        "target_root": str(target_root.resolve()),
        "evidence_sha256": evidence_sha256,
        "binary_sha256": source.get("binary_sha256"),
        "target_manifest_sha256": target_manifest_sha256,
        "target_instrument_set_sha256": target_instrument_set_sha256,
    }
    return sha256_bytes(json.dumps(material, sort_keys=True).encode())


def qualification_source_seal(
    root: Path,
    campaign: Mapping[str, Any],
    rows: Iterable[Mapping[str, Any]],
    transition_id: str,
) -> Dict[str, Any]:
    seal = predecessor_seal(root, campaign, rows, transition_id)
    monitor_path = root / "monitor.json"
    seal.update(
        {
            "kind": "instrument_qualification_restart_source",
            "source_smoke_contract_sha256": campaign.get(
                "smoke_contract_sha256"
            ),
            "monitor_sha256": (
                sha256_file(monitor_path) if monitor_path.is_file() else None
            ),
        }
    )
    return seal


def qualification_source_seal_failure(
    root: Path, seal: Mapping[str, Any]
) -> str | None:
    if seal.get("kind") != "instrument_qualification_restart_source":
        return "qualification source seal has the wrong kind"
    problem = predecessor_seal_failure(root, seal)
    if problem:
        return problem
    monitor_path = root / "monitor.json"
    current_monitor = sha256_file(monitor_path) if monitor_path.is_file() else None
    if current_monitor != seal.get("monitor_sha256"):
        return "qualification source monitor receipt changed"
    campaign = load_json(campaign_file(root))
    if campaign.get("smoke_contract_sha256") != seal.get(
        "source_smoke_contract_sha256"
    ):
        return "qualification source smoke contract changed"
    return None


def validate_stopped_qualification_source(
    root: Path,
    campaign: Mapping[str, Any],
    rows: list[Mapping[str, Any]],
) -> tuple[Dict[str, Dict[str, Any]], Dict[str, Any]]:
    if campaign.get("status") != "STOPPED":
        raise SystemExit("qualification source campaign must be explicitly stopped")
    if validated_qualification_history(campaign) is not None:
        raise SystemExit("a qualification restart cannot itself be restarted")
    source_lineage_problem = lineage_failure(root)
    if source_lineage_problem:
        raise SystemExit(
            f"qualification source lineage is invalid: {source_lineage_problem}"
        )
    for label, runtime in (
        ("manager", load_json(root / "manager.json")),
        ("monitor", read_monitor_state(root)),
    ):
        if runtime.get("status") != "STOPPED":
            raise SystemExit(f"qualification source {label} is not stopped")
        if process_alive(runtime.get("pid"), runtime.get("identity")):
            raise SystemExit(f"qualification source {label} is still alive")
        pgid = int(runtime.get("pgid") or 0)
        if pgid and process_group_members(pgid):
            raise SystemExit(
                f"qualification source {label} process group is not clean"
            )

    ledger = load_json(Path(str(campaign["budget_ledger"])))
    budget_config = load_json(Path(str(campaign["budget_config"])))
    ledger_problem = budget_ledger_failure(ledger, budget_config)
    if ledger_problem:
        raise SystemExit(f"qualification source {ledger_problem}")
    if ledger["outstanding"]:
        raise SystemExit("qualification source has outstanding budget reservations")

    states: Dict[str, Dict[str, Any]] = {}
    smoke_terminal_usage: Dict[str, tuple[Mapping[str, Any], Mapping[str, Any]]] = {}
    for row in rows:
        entrant_id = str(row["id"])
        state = read_state(root, entrant_id)
        states[entrant_id] = state
        for pid_key, pgid_key, identity_key in (
            ("supervisor_pid", "supervisor_pgid", "supervisor_identity"),
            ("goose_pid", "process_group", "goose_identity"),
            ("publisher_pid", "publisher_pgid", "publisher_identity"),
            ("score_pid", "score_pgid", "score_identity"),
        ):
            if process_alive(state.get(pid_key), state.get(identity_key)):
                raise SystemExit(
                    f"qualification source {entrant_id} still owns {pid_key}"
                )
            pgid = int(state.get(pgid_key) or 0)
            if pgid and process_group_members(pgid):
                raise SystemExit(
                    f"qualification source {entrant_id} still owns process group {pgid}"
                )
        lifecycle = lifecycle_summary(
            Path(str(state["provider_lifecycle"])),
            expected_provider=str(row["provider"]),
            expected_model=str(row["model"]),
        )
        if not full_entrant_was_never_started(state, lifecycle):
            raise SystemExit(
                "qualification restart is forbidden after any full benchmark "
                f"activity: {entrant_id}"
            )
        tree = Path(str(state["tree"]))
        if not tree.is_dir() or tree.is_symlink() or any(tree.iterdir()):
            raise SystemExit(
                f"qualification source full benchmark tree is not empty: {entrant_id}"
            )
        if (root / "scores" / entrant_id).exists() or (
            root / "publish" / entrant_id
        ).exists():
            raise SystemExit(
                f"qualification source has score or publication artifacts: {entrant_id}"
            )
        smoke_state = read_smoke_state(root, entrant_id)
        if process_alive(
            smoke_state.get("supervisor_pid"), smoke_state.get("supervisor_identity")
        ):
            raise SystemExit(
                f"qualification source {entrant_id} smoke supervisor is still alive"
            )
        smoke_pgid = int(smoke_state.get("supervisor_pgid") or 0)
        if smoke_pgid and process_group_members(smoke_pgid):
            raise SystemExit(
                f"qualification source {entrant_id} smoke process group is not clean"
            )
        usage, _, smoke_problem = predecessor_smoke_terminal_usage(
            root, entrant_id, row
        )
        if smoke_problem:
            raise SystemExit(
                f"qualification source smoke evidence is ambiguous for {entrant_id}: "
                f"{smoke_problem}"
            )
        for request_id, terminal in usage.items():
            if request_id in smoke_terminal_usage:
                raise SystemExit(
                    f"qualification smoke reused request id across entrants: {request_id}"
                )
            smoke_terminal_usage[request_id] = (row, terminal)

    settlements = {
        str(settlement["request_id"]): settlement for settlement in ledger["settled"]
    }
    if set(settlements) != set(smoke_terminal_usage):
        raise SystemExit(
            "qualification source spend is not explained exactly by sealed smoke calls"
        )
    for request_id, (row, usage) in smoke_terminal_usage.items():
        settlement = settlements[request_id]
        if (
            settlement.get("provider") != row["provider"]
            or settlement.get("model") != row["model"]
            or any(
                settlement.get(key) != usage.get(key)
                for key in (
                    "reported_model",
                    "input_tokens",
                    "output_tokens",
                    "total_tokens",
                )
            )
        ):
            raise SystemExit(
                f"qualification smoke settlement differs from terminal usage: {request_id}"
            )
    busy = [
        str(row["vendor_port"])
        for row in rows
        if not port_is_free(int(row["vendor_port"]))
    ]
    if busy:
        raise SystemExit(
            "qualification source vendor ports are still occupied: " + ", ".join(busy)
        )
    reserve_problem = replacement_reserve_failure(ledger, budget_config, rows)
    if reserve_problem:
        raise SystemExit(reserve_problem)
    return states, ledger


def qualification_receipt_failure(
    source_root: Path,
    receipt: Mapping[str, Any],
    source: Mapping[str, Any],
) -> str | None:
    expected_keys = {
        "schema_version",
        "kind",
        "restart_count",
        "transition_id",
        "source_campaign_id",
        "source_smoke_contract_sha256",
        "source_root",
        "target_root",
        "secret_file",
        "publisher_repo",
        "defect_evidence_sha256",
        "defect_artifacts",
        "binary_sha256",
        "source_manifest_sha256",
        "target_manifest_sha256",
        "source_instrument_set_sha256",
        "target_instrument_set_sha256",
        "source_budget_ledger_sha256",
        "source_seal_sha256",
        "entrant_ids",
        "fixture_seeds",
        "source_state_sha256",
        "fresh_all_entrant_smoke_required",
        "full_benchmark_episodes_started",
    }
    publisher = source.get("publisher")
    publisher_repo = publisher.get("repo") if isinstance(publisher, dict) else None
    if (
        set(receipt) != expected_keys
        or receipt.get("schema_version") != QUALIFICATION_RESTART_SCHEMA
        or receipt.get("kind") != "instrument_qualification_restart"
        or receipt.get("restart_count") != 1
        or receipt.get("source_root") != str(source_root.resolve())
        or receipt.get("source_campaign_id") != source.get("campaign_id")
        or receipt.get("source_smoke_contract_sha256")
        != source.get("smoke_contract_sha256")
        or receipt.get("secret_file")
        != str(Path(str(source.get("secret_file", ""))).resolve())
        or receipt.get("publisher_repo")
        != str(Path(str(publisher_repo or "")).resolve())
        or receipt.get("binary_sha256") != source.get("binary_sha256")
        or receipt.get("source_manifest_sha256")
        != source.get("entrant_manifest_sha256")
        or receipt.get("source_instrument_set_sha256")
        != source.get("instrument_set_sha256")
        or receipt.get("fresh_all_entrant_smoke_required") is not True
        or receipt.get("full_benchmark_episodes_started") != 0
    ):
        return "qualification restart receipt is bound to another source"
    for key in (
        "transition_id",
        "target_root",
        "target_manifest_sha256",
        "target_instrument_set_sha256",
        "source_budget_ledger_sha256",
        "source_seal_sha256",
        "defect_evidence_sha256",
    ):
        value = receipt.get(key)
        if not isinstance(value, str) or not value:
            return f"qualification restart receipt has no {key}"
    if not isinstance(receipt.get("entrant_ids"), list) or not isinstance(
        receipt.get("fixture_seeds"), dict
    ) or not isinstance(receipt.get("source_state_sha256"), dict):
        return "qualification restart receipt entrant identity is malformed"
    artifacts = receipt.get("defect_artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        return "qualification restart receipt has no defect artifacts"
    return None


def qualification_fault(_stage: str) -> None:
    return None


def qualification_history_failure(
    root: Path, campaign: Mapping[str, Any]
) -> str | None:
    try:
        history = validated_qualification_history(campaign)
        if history is None:
            return None
        history_path = root / str(history["path"])
        if not history_path.is_file() or history_path.is_symlink():
            return "qualification restart lineage is missing or linked"
        if sha256_file(history_path) != history["sha256"]:
            return "qualification restart lineage hash changed"
        lineage = load_json(history_path)
        expected_lineage_keys = {
            "schema_version",
            "kind",
            "restart_count",
            "transition_id",
            "source_root",
            "source_campaign_id",
            "source_smoke_contract_sha256",
            "source_receipt_sha256",
            "source_seal_sha256",
            "source_budget_ledger_sha256",
            "defect_evidence_sha256",
            "defect_artifacts",
            "target_root",
            "target_campaign_id",
            "target_binary_sha256",
            "target_manifest_sha256",
            "target_instrument_set_sha256",
            "entrant_ids",
            "fixture_seeds",
            "source_state_sha256",
            "fresh_all_entrant_smoke_required",
        }
        if (
            set(lineage) != expected_lineage_keys
            or lineage.get("schema_version") != QUALIFICATION_RESTART_SCHEMA
            or lineage.get("kind") != "instrument_qualification_restart"
            or lineage.get("restart_count") != 1
            or lineage.get("transition_id") != history["transition_id"]
            or lineage.get("source_campaign_id") != history["source_campaign_id"]
            or lineage.get("source_smoke_contract_sha256")
            != history["source_contract_sha256"]
            or lineage.get("target_root") != history["subject_root"]
            or lineage.get("fresh_all_entrant_smoke_required") is not True
        ):
            return "qualification restart lineage is bound to another transition"

        subject_root = Path(str(history["subject_root"])).resolve()
        if root.resolve() != subject_root:
            lineage_pointer = campaign.get("lineage")
            if (
                not isinstance(lineage_pointer, dict)
                or lineage_pointer.get("generation") != 1
            ):
                return "qualification history was copied outside its one-hop successor"
            supersession_path = root / str(lineage_pointer.get("path", ""))
            if not supersession_path.is_file():
                return "qualification successor has no supersession lineage"
            supersession = load_json(supersession_path)
            if supersession.get("predecessor_root") != str(subject_root):
                return "qualification history subject differs from supersession predecessor"
            subject_problem = lineage_failure(
                subject_root, allow_terminal_supersession_receipt=True
            )
            if subject_problem:
                return f"qualification history subject is invalid: {subject_problem}"
            subject_campaign = load_json(campaign_file(subject_root))
            if subject_campaign.get("qualification_history") != history:
                return "qualification history differs from its qualified subject"
            return None

        if lineage.get("target_campaign_id") != campaign.get("campaign_id"):
            return "qualification restart target campaign identity drifted"
        if lineage.get("target_binary_sha256") != campaign.get("binary_sha256"):
            return "qualification restart target binary identity drifted"
        if lineage.get("target_manifest_sha256") != campaign.get(
            "entrant_manifest_sha256"
        ):
            return "qualification restart target manifest identity drifted"
        if lineage.get("target_instrument_set_sha256") != campaign.get(
            "instrument_set_sha256"
        ):
            return "qualification restart target instrument identity drifted"

        source_root = Path(str(lineage.get("source_root", ""))).resolve()
        source_receipt_path = source_root / QUALIFICATION_RESTART_RECEIPT
        if not source_receipt_path.is_file() or source_receipt_path.is_symlink():
            return "qualification source immutable restart receipt is missing"
        if sha256_file(source_receipt_path) != lineage.get("source_receipt_sha256"):
            return "qualification source immutable restart receipt changed"
        source = load_json(campaign_file(source_root))
        receipt = load_json(source_receipt_path)
        receipt_problem = qualification_receipt_failure(
            source_root, receipt, source
        )
        if receipt_problem:
            return receipt_problem
        if (
            receipt.get("transition_id") != lineage.get("transition_id")
            or receipt.get("target_root") != str(root.resolve())
            or receipt.get("source_seal_sha256")
            != lineage.get("source_seal_sha256")
            or receipt.get("source_budget_ledger_sha256")
            != lineage.get("source_budget_ledger_sha256")
            or receipt.get("defect_evidence_sha256")
            != lineage.get("defect_evidence_sha256")
            or receipt.get("target_manifest_sha256")
            != lineage.get("target_manifest_sha256")
            or receipt.get("target_instrument_set_sha256")
            != lineage.get("target_instrument_set_sha256")
            or receipt.get("entrant_ids") != lineage.get("entrant_ids")
            or receipt.get("fixture_seeds") != lineage.get("fixture_seeds")
            or receipt.get("source_state_sha256")
            != lineage.get("source_state_sha256")
        ):
            return "qualification restart receipt differs from target lineage"

        source_seal_path = source_root / QUALIFICATION_RESTART_SEAL
        copied_seal_path = root / "qualification/source-seal.json"
        for candidate in (source_seal_path, copied_seal_path):
            if (
                not candidate.is_file()
                or candidate.is_symlink()
                or sha256_file(candidate) != lineage.get("source_seal_sha256")
            ):
                return "qualification source seal is missing or changed"
        source_seal = load_json(source_seal_path)
        seal_problem = qualification_source_seal_failure(source_root, source_seal)
        if seal_problem:
            return seal_problem
        source_lineage_problem = lineage_failure(
            source_root, allow_terminal_qualification_receipt=True
        )
        if source_lineage_problem:
            return f"qualification source lineage changed: {source_lineage_problem}"

        source_evidence_path = (
            source_root / QUALIFICATION_RESTART_EVIDENCE / "defect-evidence.json"
        )
        target_evidence_path = root / "qualification/evidence/defect-evidence.json"
        for candidate in (source_evidence_path, target_evidence_path):
            if (
                not candidate.is_file()
                or candidate.is_symlink()
                or sha256_file(candidate) != lineage.get("defect_evidence_sha256")
            ):
                return "qualification defect evidence is missing or changed"
        source_artifacts = receipt.get("defect_artifacts")
        target_artifacts = lineage.get("defect_artifacts")
        if not isinstance(source_artifacts, list) or not isinstance(
            target_artifacts, list
        ):
            return "qualification defect artifact records are malformed"
        source_identity = [
            {"role": item.get("role"), "sha256": item.get("sha256")}
            for item in source_artifacts
            if isinstance(item, dict)
        ]
        target_identity = [
            {"role": item.get("role"), "sha256": item.get("sha256")}
            for item in target_artifacts
            if isinstance(item, dict)
        ]
        if source_identity != target_identity or len(source_identity) != len(
            source_artifacts
        ) or len(target_identity) != len(target_artifacts):
            return "qualification defect artifacts differ across the restart"
        for base, artifacts in (
            (source_root, source_artifacts),
            (root / "qualification", target_artifacts),
        ):
            for artifact in artifacts:
                artifact_path = base / str(artifact.get("path", ""))
                if (
                    not artifact_path.is_file()
                    or artifact_path.is_symlink()
                    or sha256_file(artifact_path) != artifact.get("sha256")
                ):
                    return "qualification defect artifact changed"

        source_budget_path = root / "qualification/source-budget-ledger.json"
        if (
            not source_budget_path.is_file()
            or source_budget_path.is_symlink()
            or sha256_file(source_budget_path)
            != lineage.get("source_budget_ledger_sha256")
        ):
            return "qualification source budget snapshot changed"
        initial_ledger = load_json(source_budget_path)
        current_ledger = load_json(Path(str(campaign["budget_ledger"])))
        budget_config = load_json(Path(str(campaign["budget_config"])))
        ledger_problem = budget_ledger_descendant_failure(
            initial_ledger, current_ledger, budget_config
        )
        if ledger_problem:
            return ledger_problem
        instrument_problem = qualification_instrument_failure(source, campaign)
        if instrument_problem:
            return instrument_problem

        manifest = load_json(Path(str(campaign["entrant_manifest"])))
        rows = entrants(manifest)
        row_ids = [str(row["id"]) for row in rows]
        if row_ids != lineage.get("entrant_ids"):
            return "qualification entrant roster differs from target manifest"
        fixture_seeds = lineage.get("fixture_seeds")
        source_hashes = lineage.get("source_state_sha256")
        if not isinstance(fixture_seeds, dict) or not isinstance(source_hashes, dict):
            return "qualification entrant provenance is malformed"
        max_episodes = int(manifest["spend_policy"]["max_full_episodes_per_model"])
        for entrant_id in row_ids:
            state = read_state(root, entrant_id)
            attempts = int(state.get("provider_episode_attempts", -1))
            if (
                state.get("fixture_seed") != fixture_seeds.get(entrant_id)
                or state.get("lineage_role") != "qualification_restart"
                or state.get("qualification_restart_transition_id")
                != lineage.get("transition_id")
                or state.get("qualification_source_state_sha256")
                != source_hashes.get(entrant_id)
                or attempts < 0
                or attempts > max_episodes
            ):
                return f"qualification entrant provenance drifted: {entrant_id}"
    except (OSError, KeyError, ValueError, TypeError, json.JSONDecodeError, SystemExit) as error:
        return f"qualification restart lineage cannot be verified: {error}"
    return None


def lineage_failure(
    root: Path,
    *,
    allow_terminal_qualification_receipt: bool = False,
    allow_terminal_supersession_receipt: bool = False,
) -> str | None:
    try:
        campaign = load_json(campaign_file(root))
        qualification_receipt = root / QUALIFICATION_RESTART_RECEIPT
        if qualification_receipt.exists() and not allow_terminal_qualification_receipt:
            return (
                "campaign has an immutable qualification restart receipt and cannot "
                "run again"
            )
        qualification_problem = qualification_history_failure(root, campaign)
        if qualification_problem:
            return qualification_problem
        lineage_pointer = campaign.get("lineage")
        receipt_at_root = root / SUPERSESSION_RECEIPT
        if not isinstance(lineage_pointer, dict):
            return "campaign lineage pointer is malformed"
        if lineage_pointer.get("generation") == 0:
            validated_campaign_lineage(campaign)
            if receipt_at_root.exists() and not allow_terminal_supersession_receipt:
                return "campaign has an immutable supersession receipt and cannot run again"
            return None
        if receipt_at_root.exists():
            return "one-hop supersession successor has an unexpected successor receipt"
        if lineage_pointer.get("generation") != 1:
            return "campaign lineage generation is not exactly one"
        relative = lineage_pointer.get("path")
        if relative != "lineage/lineage.json":
            return "campaign lineage path is not the frozen path"
        lineage_path = root / str(relative)
        if not lineage_path.is_file() or lineage_path.is_symlink():
            return "campaign lineage receipt is missing or linked"
        if sha256_file(lineage_path) != lineage_pointer.get("sha256"):
            return "campaign lineage receipt hash changed"
        lineage = load_json(lineage_path)
        if lineage.get("schema_version") != SUPERSESSION_SCHEMA:
            return "campaign lineage schema is not supported"
        if lineage.get("generation") != 1:
            return "campaign lineage has an invalid generation"
        smoke_lineage = validated_campaign_lineage(campaign)
        if (
            smoke_lineage["generation"] != 1
            or smoke_lineage["predecessor_campaign_id"]
            != lineage.get("predecessor_campaign_id")
            or smoke_lineage["predecessor_contract_sha256"]
            != lineage.get("predecessor_smoke_contract_sha256")
            or smoke_contract_identity(campaign)
            != lineage.get("successor_smoke_contract_sha256")
            or campaign.get("smoke_contract_sha256")
            != lineage.get("successor_smoke_contract_sha256")
        ):
            return "campaign smoke contract differs from supersession lineage"
        if lineage.get("transition_id") != lineage_pointer.get("transition_id"):
            return "campaign lineage transition id drifted"
        if lineage.get("successor_root") != str(root.resolve()):
            return "campaign lineage is bound to another successor root"
        if lineage.get("successor_binary_sha256") != campaign.get("binary_sha256"):
            return "campaign lineage replacement binary identity drifted"
        binary = Path(str(campaign.get("binary", "")))
        if not binary.is_file() or sha256_file(binary) != campaign.get("binary_sha256"):
            return "campaign replacement binary changed"

        predecessor_root = Path(str(lineage.get("predecessor_root", "")))
        receipt_path = predecessor_root / SUPERSESSION_RECEIPT
        if not receipt_path.is_file() or receipt_path.is_symlink():
            return "predecessor immutable supersession receipt is missing"
        if sha256_file(receipt_path) != lineage.get("predecessor_receipt_sha256"):
            return "predecessor immutable supersession receipt changed"
        receipt = load_json(receipt_path)
        expected_receipt_keys = {
            "schema_version",
            "transition_id",
            "predecessor_campaign_id",
            "predecessor_smoke_contract_sha256",
            "predecessor_root",
            "target_root",
            "secret_file",
            "publisher_repo",
            "defect_evidence_sha256",
            "defect_artifacts",
            "predecessor_binary_sha256",
            "replacement_binary_sha256",
            "entrant_manifest_sha256",
            "predecessor_budget_ledger_sha256",
            "predecessor_seal_sha256",
            "affected_entrants",
            "unstarted_entrants",
            "carried_entrants",
            "predecessor_episode_attempts",
            "predecessor_terminal_outstanding",
            "fresh_all_entrant_smoke_required",
        }
        current_publisher = campaign.get("publisher")
        current_publisher_repo = (
            current_publisher.get("repo")
            if isinstance(current_publisher, dict)
            else None
        )
        if (
            set(receipt) != expected_receipt_keys
            or receipt.get("schema_version") != SUPERSESSION_SCHEMA
            or receipt.get("transition_id") != lineage.get("transition_id")
            or receipt.get("predecessor_root") != str(predecessor_root.resolve())
            or receipt.get("target_root") != str(root.resolve())
            or receipt.get("secret_file") != campaign.get("secret_file")
            or receipt.get("publisher_repo") != current_publisher_repo
            or receipt.get("replacement_binary_sha256")
            != campaign.get("binary_sha256")
            or receipt.get("entrant_manifest_sha256")
            != campaign.get("entrant_manifest_sha256")
            or receipt.get("predecessor_seal_sha256")
            != lineage.get("predecessor_seal_sha256")
            or receipt.get("defect_evidence_sha256")
            != lineage.get("defect_evidence_sha256")
            or receipt.get("affected_entrants") != lineage.get("affected_entrants")
            or receipt.get("unstarted_entrants")
            != lineage.get("unstarted_entrants")
            or receipt.get("carried_entrants") != lineage.get("carried_entrants")
            or receipt.get("predecessor_episode_attempts")
            != lineage.get("predecessor_episode_attempts")
            or receipt.get("predecessor_terminal_outstanding")
            != lineage.get("predecessor_terminal_outstanding")
            or receipt.get("predecessor_smoke_contract_sha256")
            != lineage.get("predecessor_smoke_contract_sha256")
            or receipt.get("fresh_all_entrant_smoke_required") is not True
            or lineage.get("fresh_all_entrant_smoke_required") is not True
        ):
            return "predecessor supersession receipt is bound to another transition"

        seal_path = predecessor_root / "supersession-seal.json"
        copied_seal_path = root / "lineage/predecessor-seal.json"
        expected_seal_sha = lineage.get("predecessor_seal_sha256")
        for candidate in (seal_path, copied_seal_path):
            if not candidate.is_file() or sha256_file(candidate) != expected_seal_sha:
                return "predecessor seal is missing or changed"
        seal = load_json(seal_path)
        sealed_campaign_identity = seal.get("campaign_identity")
        if (
            seal.get("transition_id") != lineage.get("transition_id")
            or not isinstance(sealed_campaign_identity, dict)
            or sealed_campaign_identity.get("campaign_id")
            != receipt.get("predecessor_campaign_id")
            or sealed_campaign_identity.get("binary_sha256")
            != receipt.get("predecessor_binary_sha256")
        ):
            return "predecessor seal transition differs"
        seal_problem = predecessor_seal_failure(predecessor_root, seal)
        if seal_problem:
            return seal_problem
        predecessor_campaign = load_json(campaign_file(predecessor_root))
        instrument_problem = supersession_instrument_failure(
            predecessor_campaign, campaign
        )
        if instrument_problem:
            return instrument_problem

        evidence_path = root / "lineage/evidence/defect-evidence.json"
        if not evidence_path.is_file() or sha256_file(evidence_path) != lineage.get(
            "defect_evidence_sha256"
        ):
            return "copied defect evidence changed"
        lineage_artifacts = lineage.get("defect_artifacts")
        if not isinstance(lineage_artifacts, list) or not lineage_artifacts:
            return "lineage defect artifact records are missing"
        receipt_artifacts = [
            {"role": artifact.get("role"), "sha256": artifact.get("sha256")}
            for artifact in lineage_artifacts
            if isinstance(artifact, dict)
        ]
        if receipt_artifacts != receipt.get("defect_artifacts"):
            return "lineage defect artifacts differ from the immutable receipt"
        for artifact in lineage_artifacts:
            if not isinstance(artifact, dict):
                return "lineage defect artifact record is malformed"
            artifact_path = root / "lineage" / str(artifact.get("path", ""))
            if (
                not artifact_path.is_file()
                or artifact_path.is_symlink()
                or sha256_file(artifact_path) != artifact.get("sha256")
            ):
                return "lineage defect artifact changed"

        initial_ledger_path = root / "lineage/predecessor-budget-ledger.json"
        if not initial_ledger_path.is_file() or sha256_file(
            initial_ledger_path
        ) != lineage.get("predecessor_budget_ledger_sha256"):
            return "predecessor budget snapshot changed"
        if lineage.get("predecessor_budget_ledger_sha256") != receipt.get(
            "predecessor_budget_ledger_sha256"
        ):
            return "predecessor budget snapshot differs from the immutable receipt"
        initial_ledger = load_json(initial_ledger_path)
        current_ledger = load_json(Path(str(campaign["budget_ledger"])))
        budget_config = load_json(Path(str(campaign["budget_config"])))
        ledger_problem = budget_ledger_descendant_failure(
            initial_ledger, current_ledger, budget_config
        )
        if ledger_problem:
            return ledger_problem

        affected = lineage.get("affected_entrants")
        unstarted = lineage.get("unstarted_entrants")
        carried = lineage.get("carried_entrants")
        attempts = lineage.get("predecessor_episode_attempts")
        terminal_outstanding = lineage.get("predecessor_terminal_outstanding")
        if (
            not isinstance(affected, list)
            or not isinstance(unstarted, list)
            or not isinstance(carried, list)
            or not isinstance(attempts, dict)
            or not isinstance(terminal_outstanding, dict)
            or set(affected) & set(unstarted)
            or set(affected) & set(carried)
            or set(unstarted) & set(carried)
        ):
            return "lineage entrant partition is malformed"
        manifest = load_json(Path(str(campaign["entrant_manifest"])))
        manifest_rows = entrants(manifest)
        row_ids = {str(row["id"]) for row in manifest_rows}
        if set(affected) | set(unstarted) | set(carried) != row_ids:
            return "lineage entrant partition differs from the frozen manifest"
        if set(terminal_outstanding) != set(affected):
            return "lineage terminal-outstanding partition differs from affected entrants"
        rows_by_id = {str(row["id"]): row for row in manifest_rows}
        initial_outstanding = initial_ledger["outstanding"]
        for entrant_id, request_ids in terminal_outstanding.items():
            if (
                not isinstance(request_ids, list)
                or request_ids != sorted(set(request_ids))
            ):
                return f"lineage terminal-outstanding ids are malformed: {entrant_id}"
            row = rows_by_id[entrant_id]
            for request_id in request_ids:
                reservation = initial_outstanding.get(request_id)
                if (
                    not isinstance(reservation, dict)
                    or reservation.get("provider") != row["provider"]
                    or reservation.get("model") != row["model"]
                ):
                    return (
                        "lineage terminal-outstanding reserve differs from the "
                        f"predecessor ledger: {entrant_id}/{request_id}"
                    )
        max_episodes = int(manifest["spend_policy"]["max_full_episodes_per_model"])
        seal_entrants = seal.get("entrants")
        if not isinstance(seal_entrants, dict):
            return "predecessor seal entrant records are malformed"
        for entrant_id in sorted(row_ids):
            state = read_state(root, entrant_id)
            expected_attempts = attempts.get(entrant_id)
            if not isinstance(expected_attempts, int):
                return f"lineage has no attempt count for {entrant_id}"
            current_attempts = int(state.get("provider_episode_attempts", -1))
            if entrant_id in affected:
                if state.get("lineage_role") != "infrastructure_defect_restart":
                    return f"affected entrant lineage role drifted: {entrant_id}"
                if current_attempts < expected_attempts or current_attempts > max_episodes:
                    return f"affected entrant attempt count reset or exceeded: {entrant_id}"
            elif entrant_id in unstarted:
                if state.get("lineage_role") != "unstarted_after_infrastructure_defect":
                    return f"unstarted entrant lineage role drifted: {entrant_id}"
                if expected_attempts != 0 or current_attempts != 0:
                    return f"unstarted entrant acquired or reset attempts: {entrant_id}"
            else:
                if state.get("lineage_role") != "carried_success":
                    return f"carried entrant lineage role drifted: {entrant_id}"
                if current_attempts != expected_attempts:
                    return f"carried entrant attempt count changed: {entrant_id}"
                sealed = seal_entrants.get(entrant_id)
                if not isinstance(sealed, dict):
                    return f"carried entrant is absent from predecessor seal: {entrant_id}"
                if hash_tree(Path(str(state["tree"]))) != sealed.get("raw_tree_sha256"):
                    return f"carried predecessor raw tree changed: {entrant_id}"
                unit = root / "entrants" / entrant_id
                if artifact_tree_sha256(
                    unit,
                    excluded_relative_paths={"state.json", "state.lock"},
                ) != sealed.get("immutable_unit_sha256"):
                    return f"carried predecessor immutable payload changed: {entrant_id}"
                if sealed.get("scores_sha256") is not None and (
                    optional_artifact_tree_sha256(root / "scores" / entrant_id)
                    != sealed.get("scores_sha256")
                ):
                    return f"carried predecessor score evidence changed: {entrant_id}"
                if sealed.get("status") == "PUBLISHED" and (
                    optional_artifact_tree_sha256(root / "publish" / entrant_id)
                    != sealed.get("publish_sha256")
                ):
                    return f"carried predecessor publication evidence changed: {entrant_id}"
            if state.get("supersession_transition_id") != lineage.get("transition_id"):
                return f"entrant transition id drifted: {entrant_id}"
    except (OSError, KeyError, ValueError, TypeError, json.JSONDecodeError, SystemExit) as error:
        return f"campaign lineage cannot be verified: {error}"
    return None


def require_lineage(root: Path) -> None:
    failure = lineage_failure(root)
    if failure:
        raise SystemExit(f"cloud campaign lineage refused execution: {failure}")


def supersession_smoke_gate_failure(root: Path) -> str | None:
    campaign = load_json(campaign_file(root))
    lineage = campaign.get("lineage")
    if not isinstance(lineage, dict) or lineage.get("generation") != 1:
        return None
    try:
        require_smoke_proofs(root)
    except SystemExit as error:
        return f"supersession requires a fresh strict all-entrant smoke proof: {error}"
    return None


def recover_existing_supersession(
    root: Path,
    predecessor_root: Path,
    replacement_sha256: str,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
) -> Dict[str, Any] | None:
    if not root.exists():
        return None
    if not campaign_file(root).is_file():
        raise SystemExit("supersession target exists without a campaign receipt")
    campaign = load_json(campaign_file(root))
    lineage = campaign.get("lineage")
    if (
        not isinstance(lineage, dict)
        or lineage.get("generation") != 1
        or campaign.get("binary_sha256") != replacement_sha256
        or campaign.get("secret_file") != str(secret_path)
        or not isinstance(campaign.get("publisher"), dict)
        or campaign["publisher"].get("repo") != str(publisher_repo)
    ):
        raise SystemExit("supersession target already belongs to another transition")
    lineage_path = root / str(lineage.get("path", ""))
    if not lineage_path.is_file():
        raise SystemExit("supersession target has no durable lineage receipt")
    lineage_value = load_json(lineage_path)
    if lineage_value.get("predecessor_root") != str(predecessor_root):
        raise SystemExit("supersession target is bound to another predecessor")
    if manifest_path.is_file() and sha256_file(manifest_path) != campaign.get(
        "entrant_manifest_sha256"
    ):
        raise SystemExit("supersession recovery manifest differs from the committed target")
    failure = lineage_failure(root)
    if failure:
        raise SystemExit(f"existing supersession target is invalid: {failure}")
    return campaign


def supersede_campaign(
    predecessor_root: Path,
    root: Path,
    binary: Path,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
    evidence_path: Path,
    publish_live: bool,
    website_base_url: str = DEFAULT_WEBSITE_BASE_URL,
    publish_verify_timeout_seconds: float = DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    publish_verify_interval_seconds: float = DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    publish_process_timeout_seconds: float = DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
) -> Dict[str, Any]:
    predecessor_root = predecessor_root.resolve()
    root = root.resolve()
    binary = binary.resolve()
    manifest_path = manifest_path.resolve()
    secret_path = secret_path.resolve()
    publisher_repo = publisher_repo.resolve()
    evidence_path = evidence_path.resolve()
    if predecessor_root == root or predecessor_root.parent != root.parent:
        raise SystemExit("supersession roots must be distinct siblings on one filesystem")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit("replacement binary is missing or not executable")
    predecessor = load_json(campaign_file(predecessor_root))
    if validated_campaign_lineage(predecessor)["generation"] != 0:
        raise SystemExit("supersession is limited to one hop")
    if secret_path != Path(str(predecessor.get("secret_file", ""))).resolve():
        raise SystemExit("supersession cannot change the credential source")
    old_publisher = predecessor.get("publisher")
    if (
        not isinstance(old_publisher, dict)
        or publisher_repo != Path(str(old_publisher.get("repo", ""))).resolve()
    ):
        raise SystemExit("supersession cannot change the publisher repository")
    replacement_sha = sha256_file(binary)
    if replacement_sha == predecessor.get("binary_sha256"):
        raise SystemExit("supersession requires a different frozen binary")

    target_lock = root.parent / f".{root.name}.supersession.claim"
    with exclusive_claim(target_lock, blocking=True) as target_claimed:
        if not target_claimed:
            raise SystemExit("cannot claim supersession target")
        existing = recover_existing_supersession(
            root,
            predecessor_root,
            replacement_sha,
            manifest_path,
            secret_path,
            publisher_repo,
        )
        if existing is not None:
            return existing
        if not manifest_path.is_file() or sha256_file(
            manifest_path
        ) != predecessor.get("entrant_manifest_sha256"):
            raise SystemExit("supersession cannot change the frozen entrant manifest")
        manifest = load_json(manifest_path)
        rows = entrants(manifest)
        row_ids = {str(row["id"]) for row in rows}
        secret_values = parse_secret_file(secret_path)
        evidence, artifacts, evidence_sha = validate_defect_evidence(
            evidence_path, predecessor, binary, row_ids, secret_values.values()
        )
        affected = set(evidence["affected_entrants"])
        transition_id = supersession_transition_id(
            predecessor_root, root, evidence_sha, replacement_sha, predecessor
        )
        with exclusive_claim(
            predecessor_root / "locks/manager-launch.claim", blocking=True
        ) as launch_claimed:
            if not launch_claimed:
                raise SystemExit("cannot freeze predecessor manager launch")
            with exclusive_claim(
                predecessor_root / "locks/supersession.claim", blocking=True
            ) as predecessor_claimed:
                if not predecessor_claimed:
                    raise SystemExit("cannot claim predecessor supersession")
                states, predecessor_ledger, terminal_outstanding, unstarted = (
                    validate_stopped_predecessor(
                    predecessor_root, predecessor, rows, affected
                    )
                )
                seal = predecessor_seal(
                    predecessor_root, predecessor, rows, transition_id
                )
                seal_payload = (json.dumps(seal, indent=2, sort_keys=True) + "\n").encode()
                seal_sha = sha256_bytes(seal_payload)
                receipt = {
                    "schema_version": SUPERSESSION_SCHEMA,
                    "transition_id": transition_id,
                    "predecessor_campaign_id": predecessor["campaign_id"],
                    "predecessor_smoke_contract_sha256": predecessor[
                        "smoke_contract_sha256"
                    ],
                    "predecessor_root": str(predecessor_root),
                    "target_root": str(root),
                    "secret_file": str(secret_path),
                    "publisher_repo": str(publisher_repo),
                    "defect_evidence_sha256": evidence_sha,
                    "defect_artifacts": [
                        {"role": artifact["role"], "sha256": artifact["sha256"]}
                        for artifact in artifacts
                    ],
                    "predecessor_binary_sha256": predecessor["binary_sha256"],
                    "replacement_binary_sha256": replacement_sha,
                    "entrant_manifest_sha256": predecessor["entrant_manifest_sha256"],
                    "predecessor_budget_ledger_sha256": sha256_file(
                        Path(str(predecessor["budget_ledger"]))
                    ),
                    "predecessor_seal_sha256": seal_sha,
                    "affected_entrants": sorted(affected),
                    "unstarted_entrants": sorted(unstarted),
                    "carried_entrants": sorted(row_ids - affected - unstarted),
                    "predecessor_episode_attempts": {
                        entrant_id: int(
                            states[entrant_id].get("provider_episode_attempts", 0)
                        )
                        for entrant_id in sorted(row_ids)
                    },
                    "predecessor_terminal_outstanding": terminal_outstanding,
                    "fresh_all_entrant_smoke_required": True,
                }
                receipt_path = predecessor_root / SUPERSESSION_RECEIPT
                write_exclusive_json(receipt_path, receipt)
                write_exclusive_json(
                    predecessor_root / "supersession-seal.json", seal
                )
                supersession_fault("receipt_committed")

                staging_parent = Path(
                    tempfile.mkdtemp(prefix=f".{root.name}.supersession-", dir=root.parent)
                )
                staged_root = staging_parent / root.name
                init_campaign(
                    staged_root,
                    binary,
                    manifest_path,
                    secret_path,
                    publisher_repo,
                    publish_live,
                    website_base_url,
                    publish_verify_timeout_seconds,
                    publish_verify_interval_seconds,
                    publish_process_timeout_seconds,
                )
                supersession_fault("staged_initialized")
                successor = load_json(campaign_file(staged_root))
                qualification_history = validated_qualification_history(predecessor)
                if qualification_history is not None:
                    successor["qualification_history"] = qualification_history
                    atomic_json(campaign_file(staged_root), successor)
                instrument_problem = supersession_instrument_failure(
                    predecessor, successor
                )
                if instrument_problem:
                    raise SystemExit(instrument_problem)

                atomic_copy(
                    Path(str(predecessor["budget_ledger"])),
                    Path(str(successor["budget_ledger"])),
                    0o600,
                )
                if load_json(Path(str(successor["budget_ledger"]))) != predecessor_ledger:
                    raise SystemExit("cumulative predecessor budget did not copy exactly")
                lineage_root = staged_root / "lineage"
                lineage_root.mkdir()
                if qualification_history is not None:
                    source_qualification = predecessor_root / "qualification"
                    target_qualification = staged_root / "qualification"
                    if (
                        not source_qualification.is_dir()
                        or source_qualification.is_symlink()
                    ):
                        raise SystemExit(
                            "qualified predecessor has no immutable qualification bundle"
                        )
                    source_qualification_sha = artifact_tree_sha256(
                        source_qualification
                    )
                    shutil.copytree(source_qualification, target_qualification)
                    if (
                        artifact_tree_sha256(target_qualification)
                        != source_qualification_sha
                    ):
                        raise SystemExit(
                            "qualification history changed while carrying supersession"
                        )
                atomic_copy(
                    predecessor_root / "supersession-seal.json",
                    lineage_root / "predecessor-seal.json",
                    0o600,
                )
                atomic_copy(
                    Path(str(predecessor["budget_ledger"])),
                    lineage_root / "predecessor-budget-ledger.json",
                    0o600,
                )
                copied_artifacts = copy_evidence_bundle(
                    lineage_root / "evidence",
                    evidence_path,
                    evidence_sha,
                    artifacts,
                )
                seal_entrants = seal["entrants"]
                for row in rows:
                    entrant_id = str(row["id"])
                    if entrant_id in affected:
                        reset_affected_entrant(
                            staged_root,
                            root,
                            entrant_id,
                            states[entrant_id],
                            seal_entrants[entrant_id],
                            transition_id,
                        )
                    elif entrant_id in unstarted:
                        reset_affected_entrant(
                            staged_root,
                            root,
                            entrant_id,
                            states[entrant_id],
                            seal_entrants[entrant_id],
                            transition_id,
                            lineage_role="unstarted_after_infrastructure_defect",
                        )
                    else:
                        copy_carried_entrant(
                            predecessor_root,
                            staged_root,
                            root,
                            entrant_id,
                            seal_entrants[entrant_id],
                            transition_id,
                        )

                staged_successor = load_json(campaign_file(staged_root))
                staged_successor.update(
                    {
                        "status": "INITIALIZED",
                        "smoke_status": "PLANNED",
                        "lineage": {
                            "generation": 1,
                            "predecessor_campaign_id": predecessor["campaign_id"],
                            "predecessor_contract_sha256": predecessor[
                                "smoke_contract_sha256"
                            ],
                        },
                    }
                )
                staged_successor = bind_smoke_contract(staged_successor, rows)
                successor_contract = staged_successor["smoke_contract_sha256"]
                for row in rows:
                    entrant_id = str(row["id"])
                    smoke_state = read_smoke_state(staged_root, entrant_id)
                    smoke_state.update(
                        {
                            "status": "PLANNED",
                            "launch_attempts": 0,
                            "admitted_episodes": 0,
                            "active_attempt": False,
                            "attempt_evidence_sha256": {},
                            "smoke_contract_sha256": successor_contract,
                            "budget_settled_baseline_request_ids": staged_successor[
                                "smoke_budget_settled_baselines"
                            ][entrant_id],
                            "budget_outstanding_baseline_request_ids": staged_successor[
                                "smoke_budget_outstanding_baselines"
                            ][entrant_id],
                            "updated_at": utc_now(),
                        }
                    )
                    atomic_json(smoke_state_file(staged_root, entrant_id), smoke_state)
                successor = remap_paths(staged_successor, staged_root, root)
                lineage = {
                    "schema_version": SUPERSESSION_SCHEMA,
                    "generation": 1,
                    "transition_id": transition_id,
                    "predecessor_root": str(predecessor_root),
                    "predecessor_campaign_id": predecessor["campaign_id"],
                    "predecessor_smoke_contract_sha256": predecessor[
                        "smoke_contract_sha256"
                    ],
                    "successor_smoke_contract_sha256": successor_contract,
                    "predecessor_receipt_sha256": sha256_file(receipt_path),
                    "predecessor_seal_sha256": seal_sha,
                    "predecessor_budget_ledger_sha256": sha256_file(
                        lineage_root / "predecessor-budget-ledger.json"
                    ),
                    "defect_evidence_sha256": evidence_sha,
                    "defect_id": evidence["defect_id"],
                    "defect_artifacts": copied_artifacts,
                    "successor_root": str(root),
                    "successor_binary_sha256": replacement_sha,
                    "affected_entrants": receipt["affected_entrants"],
                    "unstarted_entrants": receipt["unstarted_entrants"],
                    "carried_entrants": receipt["carried_entrants"],
                    "predecessor_episode_attempts": receipt[
                        "predecessor_episode_attempts"
                    ],
                    "predecessor_terminal_outstanding": receipt[
                        "predecessor_terminal_outstanding"
                    ],
                    "fresh_all_entrant_smoke_required": receipt[
                        "fresh_all_entrant_smoke_required"
                    ],
                }
                atomic_json(lineage_root / "lineage.json", lineage)
                successor.update(
                    {
                        "lineage": {
                            "generation": 1,
                            "predecessor_campaign_id": predecessor["campaign_id"],
                            "predecessor_contract_sha256": predecessor[
                                "smoke_contract_sha256"
                            ],
                            "transition_id": transition_id,
                            "path": "lineage/lineage.json",
                            "sha256": sha256_file(lineage_root / "lineage.json"),
                        },
                    }
                )
                atomic_json(campaign_file(staged_root), successor)
                supersession_fault("lineage_staged")
                fsync_directory(staged_root)
                os.replace(staged_root, root)
                fsync_directory(root.parent)
                supersession_fault("root_committed")
                failure = lineage_failure(root)
                if failure:
                    raise SystemExit(f"committed supersession failed validation: {failure}")
                with contextlib.suppress(OSError):
                    staging_parent.rmdir()
                return load_json(campaign_file(root))


def qualification_candidate(
    source: Mapping[str, Any],
    binary: Path,
    manifest_path: Path,
    checked: Mapping[str, Any],
) -> tuple[Dict[str, Any], Dict[str, str]]:
    current_hashes = instrument_hashes()
    checked_publisher = checked.get("publisher")
    candidate_publisher = (
        dict(checked_publisher) if isinstance(checked_publisher, dict) else None
    )
    source_publisher = source.get("publisher")
    if isinstance(candidate_publisher, dict) and isinstance(source_publisher, dict):
        for field in QUALIFICATION_PUBLISHER_RUNTIME_FIELDS:
            if field in source_publisher:
                candidate_publisher.setdefault(field, source_publisher[field])
    candidate = dict(source)
    candidate.update(
        {
            "binary_sha256": sha256_file(binary),
            "entrant_manifest": str(manifest_path),
            "entrant_manifest_sha256": sha256_file(manifest_path),
            "instrument_hashes": current_hashes,
            "instrument_set_sha256": sha256_bytes(
                json.dumps(current_hashes, sort_keys=True).encode()
            ),
            "publisher": candidate_publisher,
            "requested_models": checked.get("requested_models"),
        }
    )
    problem = qualification_instrument_failure(source, candidate)
    if problem:
        raise SystemExit(problem)
    return candidate, current_hashes


def qualification_source_evidence_failure(
    source_root: Path, receipt: Mapping[str, Any]
) -> str | None:
    evidence_root = source_root / QUALIFICATION_RESTART_EVIDENCE
    evidence_path = evidence_root / "defect-evidence.json"
    if (
        not evidence_path.is_file()
        or evidence_path.is_symlink()
        or sha256_file(evidence_path) != receipt.get("defect_evidence_sha256")
    ):
        return "qualification source defect evidence changed"
    artifacts = receipt.get("defect_artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        return "qualification source defect artifact records are missing"
    for artifact in artifacts:
        if not isinstance(artifact, dict) or set(artifact) != {
            "role",
            "path",
            "sha256",
        }:
            return "qualification source defect artifact record is malformed"
        path = source_root / str(artifact["path"])
        if (
            not path.is_file()
            or path.is_symlink()
            or sha256_file(path) != artifact.get("sha256")
        ):
            return "qualification source defect artifact changed"
    return None


@contextlib.contextmanager
def qualification_source_claim(root: Path) -> Iterator[bool]:
    with exclusive_claim(
        root / "locks/supersession.claim", blocking=True
    ) as stopped_claimed:
        if not stopped_claimed:
            yield False
            return
        with exclusive_claim(
            root / "locks/qualification-restart.claim", blocking=True
        ) as qualification_claimed:
            yield qualification_claimed


def commit_qualification_evidence_bundle(
    source_root: Path,
    evidence_path: Path,
    evidence_sha256: str,
    artifacts: list[Mapping[str, Any]],
) -> list[Dict[str, str]]:
    destination = source_root / QUALIFICATION_RESTART_EVIDENCE
    expected = [
        {
            "role": artifact["role"],
            "path": str(
                Path(QUALIFICATION_RESTART_EVIDENCE)
                / f"artifact-{index:02d}-{artifact['role']}"
            ),
            "sha256": artifact["sha256"],
        }
        for index, artifact in enumerate(artifacts)
    ]
    if destination.exists():
        if (
            not destination.is_dir()
            or destination.is_symlink()
            or not (destination / "defect-evidence.json").is_file()
            or sha256_file(destination / "defect-evidence.json")
            != evidence_sha256
        ):
            raise SystemExit("qualification evidence bundle differs")
        for artifact in expected:
            path = source_root / artifact["path"]
            if (
                not path.is_file()
                or path.is_symlink()
                or sha256_file(path) != artifact["sha256"]
            ):
                raise SystemExit("qualification defect artifact differs")
        expected_names = {
            "defect-evidence.json",
            *(Path(artifact["path"]).name for artifact in expected),
        }
        if {path.name for path in destination.iterdir()} != expected_names:
            raise SystemExit("qualification evidence bundle has unexpected files")
        return expected

    staging_parent = Path(
        tempfile.mkdtemp(prefix=".qualification-evidence-", dir=source_root)
    )
    staged = staging_parent / QUALIFICATION_RESTART_EVIDENCE
    try:
        copied = copy_evidence_bundle(
            staged, evidence_path, evidence_sha256, artifacts
        )
        if copied != expected:
            raise SystemExit("staged qualification evidence identity drifted")
        qualification_fault("evidence_bundle_staged")
        fsync_directory(staged)
        os.replace(staged, destination)
        fsync_directory(source_root)
        qualification_fault("evidence_bundle_committed")
        return copied
    finally:
        if staged.exists():
            shutil.rmtree(staged)
        with contextlib.suppress(OSError):
            staging_parent.rmdir()


def qualification_restart_campaign(
    source_root: Path,
    root: Path,
    binary: Path,
    manifest_path: Path,
    secret_path: Path,
    publisher_repo: Path,
    evidence_path: Path,
    publish_live: bool,
    website_base_url: str = DEFAULT_WEBSITE_BASE_URL,
    publish_verify_timeout_seconds: float = DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    publish_verify_interval_seconds: float = DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    publish_process_timeout_seconds: float = DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
) -> Dict[str, Any]:
    source_root = source_root.resolve()
    root = root.resolve()
    binary = binary.resolve()
    manifest_path = manifest_path.resolve()
    secret_path = secret_path.resolve()
    publisher_repo = publisher_repo.resolve()
    evidence_path = evidence_path.resolve()
    if source_root == root or source_root.parent != root.parent:
        raise SystemExit(
            "qualification restart roots must be distinct siblings on one filesystem"
        )
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise SystemExit("qualification restart binary is missing or not executable")
    if not manifest_path.is_file() or manifest_path.is_symlink():
        raise SystemExit("qualification restart manifest is missing or linked")
    source = load_json(campaign_file(source_root))
    if secret_path != Path(str(source.get("secret_file", ""))).resolve():
        raise SystemExit("qualification restart cannot change the credential source")
    source_publisher = source.get("publisher")
    if (
        not isinstance(source_publisher, dict)
        or publisher_repo
        != Path(str(source_publisher.get("repo", ""))).resolve()
    ):
        raise SystemExit("qualification restart cannot change the publisher repository")
    if sha256_file(binary) != source.get("binary_sha256"):
        raise SystemExit("qualification restart must keep the exact frozen Goose binary")
    publisher_runtime_problem = qualification_publisher_runtime_failure(
        source,
        publish_live,
        website_base_url,
        publish_verify_timeout_seconds,
        publish_verify_interval_seconds,
        publish_process_timeout_seconds,
    )
    if publisher_runtime_problem:
        raise SystemExit(publisher_runtime_problem)

    target_manifest = load_json(manifest_path)
    target_rows = entrants(target_manifest)
    local_publisher = publisher_snapshot(publisher_repo, target_rows)
    candidate, _ = qualification_candidate(
        source,
        binary,
        manifest_path,
        {
            "publisher": local_publisher,
            "requested_models": source.get("requested_models"),
        },
    )
    target_manifest_sha = str(candidate["entrant_manifest_sha256"])
    target_instrument_sha = str(candidate["instrument_set_sha256"])
    target_lock = root.parent / f".{root.name}.qualification-restart.claim"
    with exclusive_claim(target_lock, blocking=True) as target_claimed:
        if not target_claimed:
            raise SystemExit("cannot claim qualification restart target")
        with exclusive_claim(
            source_root / "locks/manager-launch.claim", blocking=True
        ) as launch_claimed:
            if not launch_claimed:
                raise SystemExit("cannot freeze qualification source manager launch")
            with qualification_source_claim(source_root) as source_claimed:
                if not source_claimed:
                    raise SystemExit("cannot claim qualification source")
                receipt_path = source_root / QUALIFICATION_RESTART_RECEIPT
                if receipt_path.exists():
                    if receipt_path.is_symlink() or not receipt_path.is_file():
                        raise SystemExit(
                            "qualification source receipt is not a regular file"
                        )
                    receipt = load_json(receipt_path)
                    receipt_problem = qualification_receipt_failure(
                        source_root, receipt, source
                    )
                    if receipt_problem:
                        raise SystemExit(receipt_problem)
                    if (
                        receipt.get("target_root") != str(root)
                        or receipt.get("binary_sha256") != sha256_file(binary)
                        or receipt.get("target_manifest_sha256")
                        != target_manifest_sha
                        or receipt.get("target_instrument_set_sha256")
                        != target_instrument_sha
                    ):
                        raise SystemExit(
                            "qualification source has an immutable receipt for another target"
                        )
                    seal_path = source_root / QUALIFICATION_RESTART_SEAL
                    if (
                        not seal_path.is_file()
                        or seal_path.is_symlink()
                        or sha256_file(seal_path)
                        != receipt.get("source_seal_sha256")
                    ):
                        raise SystemExit("qualification source seal changed")
                    seal = load_json(seal_path)
                    seal_problem = qualification_source_seal_failure(
                        source_root, seal
                    )
                    if seal_problem:
                        raise SystemExit(seal_problem)
                    evidence_problem = qualification_source_evidence_failure(
                        source_root, receipt
                    )
                    if evidence_problem:
                        raise SystemExit(evidence_problem)
                    source_lineage_problem = lineage_failure(
                        source_root, allow_terminal_qualification_receipt=True
                    )
                    if source_lineage_problem:
                        raise SystemExit(
                            "qualification source lineage changed after receipt: "
                            f"{source_lineage_problem}"
                        )
                    if root.exists():
                        if not campaign_file(root).is_file():
                            raise SystemExit(
                                "qualification target exists without a campaign receipt"
                            )
                        target_problem = lineage_failure(root)
                        if target_problem:
                            raise SystemExit(
                                "existing qualification target is invalid: "
                                f"{target_problem}"
                            )
                        return load_json(campaign_file(root))
                    checked = preflight(
                        binary, manifest_path, secret_path, publisher_repo
                    )
                    verified_candidate, _ = qualification_candidate(
                        source, binary, manifest_path, checked
                    )
                    if (
                        verified_candidate["entrant_manifest_sha256"]
                        != target_manifest_sha
                        or verified_candidate["instrument_set_sha256"]
                        != target_instrument_sha
                    ):
                        raise SystemExit(
                            "qualification candidate changed before authenticated preflight"
                        )
                    states = {
                        entrant_id: read_state(source_root, entrant_id)
                        for entrant_id in receipt["entrant_ids"]
                    }
                    source_ledger = load_json(Path(str(source["budget_ledger"])))
                else:
                    if root.exists():
                        raise SystemExit(
                            "qualification target exists before its source receipt"
                        )
                    manifest = load_json(Path(str(source["entrant_manifest"])))
                    rows = entrants(manifest)
                    row_ids = {str(row["id"]) for row in rows}
                    states, source_ledger = validate_stopped_qualification_source(
                        source_root, source, rows
                    )
                    secret_values = parse_secret_file(secret_path)
                    evidence, artifacts, evidence_sha = validate_defect_evidence(
                        evidence_path,
                        source,
                        binary,
                        row_ids,
                        secret_values.values(),
                    )
                    if set(evidence["affected_entrants"]) != row_ids:
                        raise SystemExit(
                            "qualification restart evidence must name every entrant"
                        )
                    checked = preflight(
                        binary, manifest_path, secret_path, publisher_repo
                    )
                    verified_candidate, _ = qualification_candidate(
                        source, binary, manifest_path, checked
                    )
                    if (
                        verified_candidate["entrant_manifest_sha256"]
                        != target_manifest_sha
                        or verified_candidate["instrument_set_sha256"]
                        != target_instrument_sha
                    ):
                        raise SystemExit(
                            "qualification candidate changed before authenticated preflight"
                        )
                    transition_id = qualification_transition_id(
                        source_root,
                        root,
                        evidence_sha,
                        source,
                        target_manifest_sha,
                        target_instrument_sha,
                    )
                    seal = qualification_source_seal(
                        source_root, source, rows, transition_id
                    )
                    seal_path = source_root / QUALIFICATION_RESTART_SEAL
                    write_exclusive_json(seal_path, seal)
                    copied_artifacts = commit_qualification_evidence_bundle(
                        source_root, evidence_path, evidence_sha, artifacts
                    )
                    receipt = {
                        "schema_version": QUALIFICATION_RESTART_SCHEMA,
                        "kind": "instrument_qualification_restart",
                        "restart_count": 1,
                        "transition_id": transition_id,
                        "source_campaign_id": source["campaign_id"],
                        "source_smoke_contract_sha256": source[
                            "smoke_contract_sha256"
                        ],
                        "source_root": str(source_root),
                        "target_root": str(root),
                        "secret_file": str(secret_path),
                        "publisher_repo": str(publisher_repo),
                        "defect_evidence_sha256": evidence_sha,
                        "defect_artifacts": copied_artifacts,
                        "binary_sha256": source["binary_sha256"],
                        "source_manifest_sha256": source[
                            "entrant_manifest_sha256"
                        ],
                        "target_manifest_sha256": target_manifest_sha,
                        "source_instrument_set_sha256": source[
                            "instrument_set_sha256"
                        ],
                        "target_instrument_set_sha256": target_instrument_sha,
                        "source_budget_ledger_sha256": sha256_file(
                            Path(str(source["budget_ledger"]))
                        ),
                        "source_seal_sha256": sha256_file(seal_path),
                        "entrant_ids": [str(row["id"]) for row in rows],
                        "fixture_seeds": {
                            entrant_id: states[entrant_id]["fixture_seed"]
                            for entrant_id in sorted(states)
                        },
                        "source_state_sha256": {
                            entrant_id: sha256_file(
                                state_file(source_root, entrant_id)
                            )
                            for entrant_id in sorted(states)
                        },
                        "fresh_all_entrant_smoke_required": True,
                        "full_benchmark_episodes_started": 0,
                    }
                    write_exclusive_json(receipt_path, receipt)
                    qualification_fault("source_receipt_committed")

                staging_parent = Path(
                    tempfile.mkdtemp(
                        prefix=f".{root.name}.qualification-restart-",
                        dir=root.parent,
                    )
                )
                staged_root = staging_parent / root.name
                try:
                    init_campaign(
                        staged_root,
                        binary,
                        manifest_path,
                        secret_path,
                        publisher_repo,
                        publish_live,
                        website_base_url,
                        publish_verify_timeout_seconds,
                        publish_verify_interval_seconds,
                        publish_process_timeout_seconds,
                        verified_preflight=checked,
                    )
                    qualification_fault("staged_initialized")
                    target = load_json(campaign_file(staged_root))
                    instrument_problem = qualification_instrument_failure(
                        source, target
                    )
                    if instrument_problem:
                        raise SystemExit(instrument_problem)
                    atomic_copy(
                        Path(str(source["budget_ledger"])),
                        Path(str(target["budget_ledger"])),
                        0o600,
                    )
                    if load_json(Path(str(target["budget_ledger"]))) != source_ledger:
                        raise SystemExit(
                            "qualification cumulative budget did not copy exactly"
                        )
                    qualification_root = staged_root / "qualification"
                    qualification_root.mkdir()
                    atomic_copy(
                        source_root / QUALIFICATION_RESTART_SEAL,
                        qualification_root / "source-seal.json",
                        0o600,
                    )
                    atomic_copy(
                        Path(str(source["budget_ledger"])),
                        qualification_root / "source-budget-ledger.json",
                        0o600,
                    )
                    shutil.copytree(
                        source_root / QUALIFICATION_RESTART_EVIDENCE,
                        qualification_root / "evidence",
                    )
                    target_artifacts = [
                        {
                            "role": artifact["role"],
                            "path": str(
                                Path("evidence") / Path(str(artifact["path"])).name
                            ),
                            "sha256": artifact["sha256"],
                        }
                        for artifact in receipt["defect_artifacts"]
                    ]
                    lineage = {
                        "schema_version": QUALIFICATION_RESTART_SCHEMA,
                        "kind": "instrument_qualification_restart",
                        "restart_count": 1,
                        "transition_id": receipt["transition_id"],
                        "source_root": str(source_root),
                        "source_campaign_id": source["campaign_id"],
                        "source_smoke_contract_sha256": source[
                            "smoke_contract_sha256"
                        ],
                        "source_receipt_sha256": sha256_file(receipt_path),
                        "source_seal_sha256": receipt["source_seal_sha256"],
                        "source_budget_ledger_sha256": receipt[
                            "source_budget_ledger_sha256"
                        ],
                        "defect_evidence_sha256": receipt[
                            "defect_evidence_sha256"
                        ],
                        "defect_artifacts": target_artifacts,
                        "target_root": str(root),
                        "target_campaign_id": target["campaign_id"],
                        "target_binary_sha256": target["binary_sha256"],
                        "target_manifest_sha256": target[
                            "entrant_manifest_sha256"
                        ],
                        "target_instrument_set_sha256": target[
                            "instrument_set_sha256"
                        ],
                        "entrant_ids": receipt["entrant_ids"],
                        "fixture_seeds": receipt["fixture_seeds"],
                        "source_state_sha256": receipt[
                            "source_state_sha256"
                        ],
                        "fresh_all_entrant_smoke_required": True,
                    }
                    qualification_lineage_path = (
                        staged_root / QUALIFICATION_HISTORY_PATH
                    )
                    atomic_json(qualification_lineage_path, lineage)
                    history = {
                        "restart_count": 1,
                        "transition_id": receipt["transition_id"],
                        "subject_root": str(root),
                        "source_campaign_id": source["campaign_id"],
                        "source_contract_sha256": source[
                            "smoke_contract_sha256"
                        ],
                        "path": QUALIFICATION_HISTORY_PATH,
                        "sha256": sha256_file(qualification_lineage_path),
                    }
                    target.update(
                        {
                            "status": "INITIALIZED",
                            "smoke_status": "PLANNED",
                            "qualification_history": history,
                        }
                    )
                    target = bind_smoke_contract(target, entrants(load_json(manifest_path)))
                    target_contract = target["smoke_contract_sha256"]
                    for entrant_id in receipt["entrant_ids"]:
                        state = remap_paths(
                            read_state(staged_root, entrant_id), staged_root, root
                        )
                        state.update(
                            {
                                "status": "PLANNED",
                                "provider_episode_attempts": 0,
                                "fixture_seed": receipt["fixture_seeds"][entrant_id],
                                "admitted_requests": 0,
                                "provider_terminal_requests": 0,
                                "score": None,
                                "verdict": None,
                                "failure": None,
                                "lineage_role": "qualification_restart",
                                "qualification_restart_transition_id": receipt[
                                    "transition_id"
                                ],
                                "qualification_source_state_sha256": receipt[
                                    "source_state_sha256"
                                ][entrant_id],
                                "updated_at": utc_now(),
                            }
                        )
                        atomic_json(state_file(staged_root, entrant_id), state)
                        smoke_state = remap_paths(
                            read_smoke_state(staged_root, entrant_id),
                            staged_root,
                            root,
                        )
                        smoke_state.update(
                            {
                                "status": "PLANNED",
                                "launch_attempts": 0,
                                "admitted_episodes": 0,
                                "active_attempt": False,
                                "attempt_evidence_sha256": {},
                                "smoke_contract_sha256": target_contract,
                                "budget_settled_baseline_request_ids": target[
                                    "smoke_budget_settled_baselines"
                                ][entrant_id],
                                "budget_outstanding_baseline_request_ids": target[
                                    "smoke_budget_outstanding_baselines"
                                ][entrant_id],
                                "failure": None,
                                "updated_at": utc_now(),
                            }
                        )
                        atomic_json(
                            smoke_state_file(staged_root, entrant_id), smoke_state
                        )
                    target = remap_paths(target, staged_root, root)
                    atomic_json(campaign_file(staged_root), target)
                    qualification_fault("lineage_staged")
                    fsync_directory(staged_root)
                    os.replace(staged_root, root)
                    fsync_directory(root.parent)
                    qualification_fault("root_committed")
                    failure = lineage_failure(root)
                    if failure:
                        raise SystemExit(
                            "committed qualification restart failed validation: "
                            f"{failure}"
                        )
                    return load_json(campaign_file(root))
                finally:
                    if staged_root.exists():
                        shutil.rmtree(staged_root)
                    with contextlib.suppress(OSError):
                        staging_parent.rmdir()


def entrant_budget_requests(
    campaign: Mapping[str, Any], row: Mapping[str, Any]
) -> tuple[list[str], list[str], str | None]:
    ledger_value = campaign.get("budget_ledger")
    if not ledger_value:
        return [], [], "campaign has no budget ledger"
    ledger_path = Path(str(ledger_value))
    if not ledger_path.is_file() or ledger_path.is_symlink():
        return [], [], f"budget ledger is missing or symbolic: {ledger_path}"
    try:
        ledger = load_json(ledger_path)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return [], [], f"budget ledger cannot be read: {error}"
    outstanding = ledger.get("outstanding")
    settled = ledger.get("settled")
    if not isinstance(outstanding, dict) or not isinstance(settled, list):
        return [], [], "budget ledger request collections are malformed"
    outstanding_ids: list[str] = []
    settled_ids: list[str] = []
    for request_id, reservation in outstanding.items():
        if not isinstance(reservation, dict):
            return [], [], "budget ledger contains a malformed reservation"
        if reservation.get("provider") == row.get("provider") and reservation.get(
            "model"
        ) == row.get("model"):
            outstanding_ids.append(str(request_id))
    seen_settled: set[str] = set()
    for settlement in settled:
        if not isinstance(settlement, dict):
            return [], [], "budget ledger contains a malformed settlement"
        request_id = settlement.get("request_id")
        if (
            not isinstance(request_id, str)
            or not request_id
            or request_id in seen_settled
        ):
            return (
                [],
                [],
                "budget ledger contains duplicate or malformed settlement IDs",
            )
        seen_settled.add(request_id)
        if settlement.get("provider") == row.get("provider") and settlement.get(
            "model"
        ) == row.get("model"):
            settled_ids.append(request_id)
    return sorted(outstanding_ids), sorted(settled_ids), None


def bind_smoke_contract(
    campaign: Mapping[str, Any], rows: Iterable[Mapping[str, Any]]
) -> Dict[str, Any]:
    bound = dict(campaign)
    lineage = validated_campaign_lineage(bound)
    settled_baselines: Dict[str, list[str]] = {}
    outstanding_baselines: Dict[str, list[str]] = {}
    for row in rows:
        entrant_id = str(row["id"])
        outstanding, settled, error = entrant_budget_requests(bound, row)
        if error:
            raise SystemExit(f"cannot bind {entrant_id} smoke budget baseline: {error}")
        if outstanding and lineage["generation"] == 0:
            raise SystemExit(
                f"cannot bind {entrant_id} smoke contract with outstanding requests: "
                + ", ".join(outstanding)
            )
        settled_baselines[entrant_id] = settled
        outstanding_baselines[entrant_id] = outstanding
    bound["smoke_budget_settled_baselines"] = settled_baselines
    bound["smoke_budget_outstanding_baselines"] = outstanding_baselines
    bound["smoke_contract_sha256"] = smoke_contract_identity(bound)
    return bound


def current_smoke_budget_requests(
    campaign: Mapping[str, Any], row: Mapping[str, Any]
) -> tuple[list[str], list[str], str | None]:
    outstanding, settled, error = entrant_budget_requests(campaign, row)
    if error:
        return [], [], error
    baselines = campaign.get("smoke_budget_settled_baselines")
    outstanding_baselines = campaign.get("smoke_budget_outstanding_baselines")
    entrant_id = str(row.get("id", ""))
    baseline = baselines.get(entrant_id) if isinstance(baselines, dict) else None
    if (
        not isinstance(baseline, list)
        or any(not isinstance(value, str) or not value for value in baseline)
        or baseline != sorted(set(baseline))
    ):
        return [], [], f"{entrant_id} smoke budget baseline is malformed"
    outstanding_baseline = (
        outstanding_baselines.get(entrant_id)
        if isinstance(outstanding_baselines, dict)
        else None
    )
    if (
        not isinstance(outstanding_baseline, list)
        or any(
            not isinstance(value, str) or not value for value in outstanding_baseline
        )
        or outstanding_baseline != sorted(set(outstanding_baseline))
    ):
        return [], [], f"{entrant_id} smoke outstanding budget baseline is malformed"
    missing = sorted(set(baseline) - set(settled))
    if missing:
        return (
            [],
            [],
            (
                f"{entrant_id} smoke budget baseline settlements disappeared: "
                + ", ".join(missing)
            ),
        )
    missing_outstanding = sorted(set(outstanding_baseline) - set(outstanding))
    if missing_outstanding:
        return (
            [],
            [],
            (
                f"{entrant_id} carried smoke budget reservations disappeared: "
                + ", ".join(missing_outstanding)
            ),
        )
    current_outstanding = sorted(set(outstanding) - set(outstanding_baseline))
    current = sorted(set(settled) - set(baseline))
    return current_outstanding, current, None


def smoke_admission_history(
    root: Path, entrant_id: str, row: Mapping[str, Any]
) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    state = read_smoke_state(root, entrant_id)
    errors: list[str] = []
    try:
        contract = smoke_contract_identity(campaign)
    except SystemExit as error:
        contract = ""
        errors.append(str(error))
    if contract != campaign.get("smoke_contract_sha256"):
        errors.append("campaign smoke contract hash is stale")
    if state.get("smoke_contract_sha256") != contract:
        errors.append("entrant smoke state belongs to a different contract")
    baselines = campaign.get("smoke_budget_settled_baselines")
    expected_baseline = (
        baselines.get(entrant_id) if isinstance(baselines, dict) else None
    )
    if state.get("budget_settled_baseline_request_ids") != expected_baseline:
        errors.append(
            "entrant smoke budget baseline differs from the campaign contract"
        )
    outstanding_baselines = campaign.get("smoke_budget_outstanding_baselines")
    expected_outstanding_baseline = (
        outstanding_baselines.get(entrant_id)
        if isinstance(outstanding_baselines, dict)
        else None
    )
    if (
        state.get("budget_outstanding_baseline_request_ids")
        != expected_outstanding_baseline
    ):
        errors.append(
            "entrant smoke outstanding budget baseline differs from the campaign contract"
        )

    evidence_hashes = state.get("attempt_evidence_sha256")
    if not isinstance(evidence_hashes, dict):
        evidence_hashes = {}
        errors.append("entrant cumulative smoke evidence index is malformed")
    attempts_root = root / "smoke" / entrant_id / "attempts"
    attempts: list[Dict[str, Any]] = []
    seen_names: set[str] = set()
    if not attempts_root.is_dir() or attempts_root.is_symlink():
        errors.append("entrant smoke attempts root is missing or symbolic")
    else:
        for attempt_root in sorted(attempts_root.iterdir()):
            if not attempt_root.name.startswith("attempt-"):
                errors.append(f"unexpected smoke attempt entry: {attempt_root.name}")
                continue
            seen_names.add(attempt_root.name)
            try:
                attempt_number = int(attempt_root.name.removeprefix("attempt-"))
            except ValueError:
                errors.append(f"malformed smoke attempt name: {attempt_root.name}")
                continue
            if (
                attempt_number <= 0
                or not attempt_root.is_dir()
                or attempt_root.is_symlink()
            ):
                errors.append(f"invalid smoke attempt path: {attempt_root.name}")
                continue
            lifecycle_path = attempt_root / "provider-lifecycle.jsonl"
            lifecycle = lifecycle_summary(
                lifecycle_path,
                expected_provider=str(row["provider"]),
                expected_model=str(row["model"]),
            )
            evidence_path = attempt_root / "attempt-evidence.json"
            evidence: Dict[str, Any] | None = None
            evidence_sha: str | None = None
            if evidence_path.is_symlink() or not evidence_path.is_file():
                errors.append(f"{attempt_root.name} has no sealed attempt evidence")
            else:
                try:
                    evidence_sha = sha256_file(evidence_path)
                    evidence = load_json(evidence_path)
                except (OSError, json.JSONDecodeError, SystemExit) as error:
                    errors.append(
                        f"{attempt_root.name} evidence cannot be read: {error}"
                    )
                if evidence_sha != evidence_hashes.get(attempt_root.name):
                    errors.append(
                        f"{attempt_root.name} evidence hash is not sealed in state"
                    )
                if evidence is not None:
                    expected_identity = {
                        "entrant": entrant_id,
                        "attempt": attempt_number,
                        "smoke_contract_sha256": contract,
                    }
                    changed = [
                        key
                        for key, value in expected_identity.items()
                        if evidence.get(key) != value
                    ]
                    if changed:
                        errors.append(
                            f"{attempt_root.name} evidence identity differs: "
                            + ", ".join(changed)
                        )
                    if evidence.get("lifecycle") != lifecycle:
                        errors.append(
                            f"{attempt_root.name} lifecycle differs from sealed evidence"
                        )
                    lifecycle_hash = (
                        sha256_file(lifecycle_path)
                        if lifecycle_path.is_file() and not lifecycle_path.is_symlink()
                        else None
                    )
                    hashes = evidence.get("hashes")
                    if (
                        not isinstance(hashes, dict)
                        or hashes.get("lifecycle") != lifecycle_hash
                    ):
                        errors.append(
                            f"{attempt_root.name} lifecycle hash differs from sealed evidence"
                        )
            attempts.append(
                {
                    "attempt": attempt_number,
                    "name": attempt_root.name,
                    "evidence_sha256": evidence_sha,
                    "admitted": int(lifecycle.get("admitted", 0)),
                    "terminal": int(lifecycle.get("terminal", 0)),
                    "lifecycle_valid": lifecycle_failure(lifecycle) is None,
                }
            )
    extra_indexes = sorted(set(evidence_hashes) - seen_names)
    if extra_indexes:
        errors.append(
            "cumulative smoke evidence index names missing attempts: "
            + ", ".join(extra_indexes)
        )
    outstanding, current_settled, budget_error = current_smoke_budget_requests(
        campaign, row
    )
    if budget_error:
        errors.append(budget_error)
    episodes_admitted = sum(1 for attempt in attempts if attempt["admitted"] > 0)
    if episodes_admitted > 1:
        errors.append("more than one smoke episode admitted provider work")
    if state.get("admitted_episodes") != episodes_admitted:
        errors.append(
            "persisted smoke admission count differs from cumulative evidence"
        )
    return {
        "valid": not errors,
        "errors": errors,
        "contract_sha256": contract,
        "attempts": attempts,
        "episodes_admitted": episodes_admitted,
        "outstanding_request_ids": outstanding,
        "current_settled_request_ids": current_settled,
    }


def smoke_attempt_history_failure(
    root: Path, entrant_id: str, row: Mapping[str, Any]
) -> str | None:
    history = smoke_admission_history(root, entrant_id, row)
    reasons = list(history["errors"])
    if history["episodes_admitted"]:
        reasons.append("a prior smoke episode admitted provider work")
    if history["current_settled_request_ids"]:
        reasons.append(
            "current-campaign provider work is already settled: "
            + ", ".join(history["current_settled_request_ids"])
        )
    if history["outstanding_request_ids"]:
        reasons.append(
            "outstanding budget requests: "
            + ", ".join(history["outstanding_request_ids"])
        )
    return "; ".join(reasons) if reasons else None


def sealed_smoke_admission_history(history: Mapping[str, Any]) -> Dict[str, Any]:
    return {
        "contract_sha256": history.get("contract_sha256"),
        "attempts": history.get("attempts"),
        "episodes_admitted": history.get("episodes_admitted"),
    }


def _regular_file_bytes(path: Path) -> bytes | None:
    if path.is_symlink() or not path.is_file():
        return None
    try:
        return path.read_bytes()
    except OSError:
        return None


def smoke_attempt_evidence(
    root: Path,
    entrant_id: str,
    *,
    exit_code: int | None,
    descendants_clean: bool,
) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    row = manifest_row(root, entrant_id)
    state = read_smoke_state(root, entrant_id)
    reasons: list[str] = []
    try:
        contract = smoke_contract_identity(campaign)
    except SystemExit as error:
        contract = ""
        reasons.append(str(error))
    if contract != campaign.get("smoke_contract_sha256"):
        reasons.append("campaign smoke contract hash is stale")
    if state.get("smoke_contract_sha256") != contract:
        reasons.append("smoke attempt belongs to a different campaign contract")
    baselines = campaign.get("smoke_budget_settled_baselines")
    expected_baseline = (
        baselines.get(entrant_id) if isinstance(baselines, dict) else None
    )
    if state.get("budget_settled_baseline_request_ids") != expected_baseline:
        reasons.append(
            "smoke attempt budget baseline differs from the campaign contract"
        )
    outstanding_baselines = campaign.get("smoke_budget_outstanding_baselines")
    expected_outstanding_baseline = (
        outstanding_baselines.get(entrant_id)
        if isinstance(outstanding_baselines, dict)
        else None
    )
    if (
        state.get("budget_outstanding_baseline_request_ids")
        != expected_outstanding_baseline
    ):
        reasons.append(
            "smoke attempt outstanding budget baseline differs from the campaign contract"
        )
    attempt_root = Path(str(state.get("attempt_root", "")))
    expected_root = (
        root
        / "smoke"
        / entrant_id
        / "attempts"
        / f"attempt-{int(state.get('attempt', 0))}"
    )
    if (
        attempt_root != expected_root
        or not attempt_root.is_dir()
        or attempt_root.is_symlink()
    ):
        reasons.append(
            "smoke attempt root is missing, linked, or not the expected immutable path"
        )
    paths = {
        "tree": Path(str(state.get("tree", ""))),
        "profile": Path(str(state.get("profile", ""))),
        "log": Path(str(state.get("log", ""))),
        "lifecycle": Path(str(state.get("provider_lifecycle", ""))),
        "prompt": Path(str(state.get("prompt", ""))),
        "nonce": Path(str(state.get("nonce_file", ""))),
        "listeners": Path(str(state.get("sandbox_listener_snapshot", ""))),
    }
    for name, path in paths.items():
        try:
            path.resolve().relative_to(attempt_root.resolve())
        except (OSError, ValueError):
            reasons.append(f"smoke {name} path escapes its immutable attempt")
    if (
        paths["tree"] != attempt_root / "tree"
        or paths["profile"] != attempt_root / "profile"
    ):
        reasons.append("smoke tree/profile paths do not match the isolated attempt")
    if paths["nonce"] != paths["tree"] / SMOKE_NONCE_NAME:
        reasons.append("smoke nonce path is not the frozen relative target")
    listener_problem = listener_isolation_failure(
        campaign, row, state, smoke=True
    )
    if listener_problem:
        reasons.append(listener_problem)

    expected_command = str(state.get("expected_command", ""))
    expected_marker = str(state.get("final_marker", ""))
    expected_tool_output = str(state.get("expected_tool_output", ""))
    stream = parse_smoke_stream(
        paths["log"],
        expected_command=expected_command,
        expected_marker=expected_marker,
        expected_tool_output=expected_tool_output,
    )
    if not stream["valid"]:
        reasons.extend(f"stream: {error}" for error in stream["errors"])

    lifecycle = lifecycle_summary(
        paths["lifecycle"],
        expected_provider=str(row["provider"]),
        expected_model=str(row["model"]),
    )
    lifecycle_error = lifecycle_failure(lifecycle)
    if lifecycle_error:
        reasons.append(f"lifecycle: {lifecycle_error}")
    if not lifecycle["admitted"]:
        reasons.append("lifecycle has no proven provider admission")
    if lifecycle["admitted"] != lifecycle["terminal"]:
        reasons.append("not every admitted provider request reached a proven terminal")

    outstanding, settled, budget_error = current_smoke_budget_requests(campaign, row)
    terminal_ids = sorted(
        request_id
        for request_id, states in lifecycle["request_states"].items()
        if states and states[-1] == "provider_terminal"
    )
    if budget_error:
        reasons.append(budget_error)
    if outstanding:
        reasons.append(
            f"{len(outstanding)} provider request(s) retain full budget reserves"
        )
    if not set(terminal_ids).issubset(set(settled)):
        reasons.append(
            "terminal lifecycle request IDs are absent from the shared budget ledger"
        )

    try:
        expected_nonce = bytes.fromhex(str(state.get("nonce_hex", "")))
    except ValueError:
        expected_nonce = b""
        reasons.append("smoke nonce evidence is malformed")
    nonce_bytes = _regular_file_bytes(paths["nonce"])
    if nonce_bytes is None:
        reasons.append("smoke nonce is missing, non-regular, unreadable, or symbolic")
    elif nonce_bytes != expected_nonce:
        reasons.append("smoke nonce bytes differ from the frozen random challenge")

    prompt_bytes = _regular_file_bytes(paths["prompt"])
    expected_prompt = smoke_prompt(expected_command, expected_marker).encode()
    if prompt_bytes != expected_prompt:
        reasons.append("smoke prompt bytes differ from the frozen contract")
    if state.get("expected_command_sha256") != sha256_bytes(expected_command.encode()):
        reasons.append("smoke command hash differs from its persisted command")
    if int(state.get("smoke_max_turns", 0)) != int(campaign.get("smoke_max_turns", 0)):
        reasons.append("smoke turn limit differs from the frozen campaign")
    if int(campaign.get("smoke_max_turns", 0)) != SMOKE_MAX_TURNS:
        reasons.append("frozen campaign smoke turn limit is invalid")
    binary = Path(str(campaign.get("binary", "")))
    if (
        not binary.is_file()
        or binary.is_symlink()
        or sha256_file(binary) != campaign.get("binary_sha256")
    ):
        reasons.append("frozen binary changed before smoke proof")
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        reasons.append(mismatch)
    budget_config = Path(str(campaign.get("budget_config", "")))
    if (
        not budget_config.is_file()
        or budget_config.is_symlink()
        or sha256_file(budget_config) != campaign.get("budget_config_sha256")
    ):
        reasons.append("frozen budget config changed before smoke proof")

    secret_hits: list[str] = []
    try:
        secret_values = parse_secret_file(Path(str(campaign["secret_file"])))
        secret_hits = secret_occurrences(
            [root / "smoke" / entrant_id],
            secret_values.values(),
        )
    except (OSError, KeyError, SystemExit) as error:
        reasons.append(f"secret scan could not be completed: {error}")
    if secret_hits:
        reasons.append("provider credential appeared in smoke-controlled artifacts")
    if exit_code not in {None, 0}:
        reasons.append(f"goose smoke process exited {exit_code}")
    if not descendants_clean:
        reasons.append("background tool descendants survived the smoke process")

    static_hashes: Dict[str, str | None] = {}
    for name in ("log", "lifecycle", "prompt", "nonce", "listeners"):
        path = paths[name]
        static_hashes[name] = (
            sha256_file(path) if path.is_file() and not path.is_symlink() else None
        )
    return {
        "entrant": entrant_id,
        "attempt": int(state.get("attempt", 0)),
        "smoke_contract_sha256": contract,
        "passed": not reasons,
        "reasons": reasons,
        "stream": stream,
        "lifecycle": lifecycle,
        "outstanding_request_ids": outstanding,
        "settled_request_ids": settled,
        "terminal_request_ids": terminal_ids,
        "listener_isolation": {
            key: state.get(key)
            for key in (
                "sandbox_listener_snapshot",
                "sandbox_listener_snapshot_sha256",
                "sandbox_preexisting_listener_ports",
                "sandbox_manifest_vendor_ports",
                "sandbox_denied_local_ports",
                "sandbox_denied_local_ports_sha256",
            )
        },
        "secret_scan_hits": secret_hits,
        "descendants_clean": descendants_clean,
        "exit_code": exit_code,
        "hashes": static_hashes,
        "nonce_sha256": sha256_bytes(nonce_bytes) if nonce_bytes is not None else None,
        "binary_sha256": sha256_file(binary) if binary.is_file() else None,
        "instrument_set_sha256": campaign.get("instrument_set_sha256"),
        "entrant_manifest_sha256": campaign.get("entrant_manifest_sha256"),
        "budget_config_sha256": campaign.get("budget_config_sha256"),
    }


def finalize_smoke_attempt(
    root: Path,
    entrant_id: str,
    *,
    exit_code: int | None,
    descendants_clean: bool,
) -> bool:
    state = read_smoke_state(root, entrant_id)
    evidence = smoke_attempt_evidence(
        root,
        entrant_id,
        exit_code=exit_code,
        descendants_clean=descendants_clean,
    )
    lifecycle = evidence["lifecycle"]
    admitted = int(lifecycle.get("admitted", 0))
    settled = list(evidence.get("settled_request_ids", []))
    admitted_episode = bool(admitted or settled)
    current_attempt_name = f"attempt-{int(state.get('attempt', 0))}"
    evidence_indexes = state.get("attempt_evidence_sha256")
    if not isinstance(evidence_indexes, dict):
        evidence_indexes = {}
        evidence["passed"] = False
        evidence["reasons"].append("cumulative smoke evidence index is malformed")
    else:
        evidence_indexes = dict(evidence_indexes)
    prior_admitted_episodes = 0
    for prior_attempt_name, expected_hash in evidence_indexes.items():
        if prior_attempt_name == current_attempt_name:
            continue
        evidence_path = (
            root
            / "smoke"
            / entrant_id
            / "attempts"
            / prior_attempt_name
            / "attempt-evidence.json"
        )
        if (
            evidence_path.is_symlink()
            or not evidence_path.is_file()
            or sha256_file(evidence_path) != expected_hash
        ):
            evidence["passed"] = False
            evidence["reasons"].append(
                f"prior cumulative evidence is missing or changed: {prior_attempt_name}"
            )
            continue
        prior = load_json(evidence_path)
        prior_lifecycle = prior.get("lifecycle")
        prior_settled = prior.get("settled_request_ids")
        if (
            isinstance(prior_lifecycle, dict)
            and int(prior_lifecycle.get("admitted", 0)) > 0
        ) or (isinstance(prior_settled, list) and bool(prior_settled)):
            prior_admitted_episodes += 1
    admitted_episodes = prior_admitted_episodes + (1 if admitted_episode else 0)
    if admitted_episodes > 1:
        evidence["passed"] = False
        evidence["reasons"].append("more than one smoke episode admitted provider work")

    attempt_evidence_path = Path(str(state["attempt_root"])) / "attempt-evidence.json"
    if attempt_evidence_path.exists():
        try:
            existing_evidence = load_json(attempt_evidence_path)
        except (OSError, json.JSONDecodeError, SystemExit) as error:
            update_smoke_state(
                root,
                entrant_id,
                status="FAILED",
                failure=f"existing smoke attempt evidence cannot be read: {error}",
            )
            return False
        if existing_evidence != evidence:
            update_smoke_state(
                root,
                entrant_id,
                status="FAILED",
                failure="existing smoke attempt evidence differs from reconstruction",
            )
            return False
    else:
        atomic_json(attempt_evidence_path, evidence)
    evidence_indexes[current_attempt_name] = sha256_file(attempt_evidence_path)
    common = {
        "exit_code": exit_code,
        "finished_at": utc_now(),
        "admitted_requests": admitted,
        "provider_terminal_requests": int(lifecycle.get("terminal", 0)),
        "admitted_episodes": admitted_episodes,
        "attempt_evidence": str(attempt_evidence_path),
        "attempt_evidence_sha256": evidence_indexes,
        "lifecycle_events": lifecycle.get("events", 0),
        "lifecycle_malformed_lines": lifecycle.get("malformed_lines", 0),
        "lifecycle_transition_errors": lifecycle.get("transition_errors", []),
        "lifecycle_ambiguous_request_ids": lifecycle.get("ambiguous_request_ids", []),
        "budget_outstanding_request_ids": evidence.get("outstanding_request_ids", []),
        "secret_scan_hits": evidence.get("secret_scan_hits", []),
        "smoke_evidence": evidence,
        "active_attempt": False,
    }
    update_smoke_state(root, entrant_id, status="FINALIZING", **common)
    row = manifest_row(root, entrant_id)
    admission_history = smoke_admission_history(root, entrant_id, row)
    if not admission_history["valid"]:
        update_smoke_state(
            root,
            entrant_id,
            status="FAILED",
            failure="; ".join(admission_history["errors"])[:4000],
            **common,
        )
        return False
    if evidence["passed"]:
        proof_path = Path(str(state["attempt_root"])) / "proof.json"
        proof = {
            "schema_version": SMOKE_PROOF_SCHEMA,
            "created_at": utc_now(),
            "entrant": entrant_id,
            "provider": state["provider"],
            "model": state["model"],
            "attempt": state["attempt"],
            "passed": True,
            "smoke_contract_sha256": state["smoke_contract_sha256"],
            "smoke_max_turns": state["smoke_max_turns"],
            "expected_command_sha256": state["expected_command_sha256"],
            "prompt_sha256": state["prompt_sha256"],
            "admission_history": sealed_smoke_admission_history(admission_history),
            "evidence": evidence,
        }
        if proof_path.exists():
            existing = load_json(proof_path)
            comparable = {
                key: value for key, value in proof.items() if key != "created_at"
            }
            existing_comparable = {
                key: value for key, value in existing.items() if key != "created_at"
            }
            if existing_comparable != comparable:
                update_smoke_state(
                    root,
                    entrant_id,
                    status="FAILED",
                    failure="existing smoke proof differs from reconstructed evidence",
                    **common,
                )
                return False
        else:
            atomic_json(proof_path, proof)
        try:
            secret_values = parse_secret_file(
                Path(str(load_json(campaign_file(root))["secret_file"]))
            )
            sealed_secret_hits = secret_occurrences(
                [root / "smoke" / entrant_id], secret_values.values()
            )
        except (OSError, KeyError, SystemExit) as error:
            sealed_secret_hits = [f"scan-error:{error}"]
        if sealed_secret_hits:
            update_smoke_state(
                root,
                entrant_id,
                status="FAILED",
                failure="provider credential appeared in sealed smoke artifacts",
                sealed_secret_scan_hits=sealed_secret_hits,
                **common,
            )
            return False
        update_smoke_state(
            root,
            entrant_id,
            status="PASS",
            proof=str(proof_path),
            proof_sha256=sha256_file(proof_path),
            failure=None,
            **common,
        )
        return True

    unambiguous_pre_admission = (
        not admitted_episode
        and not lifecycle_failure(lifecycle)
        and not evidence.get("outstanding_request_ids")
        and descendants_clean
    )
    status = "PRE_ADMISSION_FAILURE" if unambiguous_pre_admission else "FAILED"
    update_smoke_state(
        root,
        entrant_id,
        status=status,
        failure="; ".join(evidence["reasons"])[:4000],
        **common,
    )
    return False


def smoke_proof_mismatch(
    root: Path, entrant_id: str, row: Mapping[str, Any]
) -> str | None:
    campaign = load_json(campaign_file(root))
    try:
        state = read_smoke_state(root, entrant_id)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return f"smoke state cannot be read: {error}"
    try:
        contract = smoke_contract_identity(campaign)
    except SystemExit as error:
        return str(error)
    if campaign.get("smoke_contract_sha256") != contract:
        return "campaign smoke contract hash is stale"
    if state.get("smoke_contract_sha256") != contract:
        return "smoke state belongs to a different campaign contract"
    if state.get("status") != "PASS":
        return f"smoke status is {state.get('status')}, not PASS"
    proof_path = Path(str(state.get("proof", "")))
    expected_proof = (
        root
        / "smoke"
        / entrant_id
        / "attempts"
        / f"attempt-{int(state.get('attempt', 0))}"
        / "proof.json"
    )
    if (
        proof_path != expected_proof
        or proof_path.is_symlink()
        or not proof_path.is_file()
    ):
        return "smoke proof is missing, linked, or outside its immutable attempt"
    try:
        proof_sha = sha256_file(proof_path)
        proof = load_json(proof_path)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        return f"smoke proof cannot be read: {error}"
    if proof_sha != state.get("proof_sha256"):
        return "smoke proof hash differs from the sealed state"
    expected_fields = {
        "schema_version": SMOKE_PROOF_SCHEMA,
        "entrant": entrant_id,
        "provider": row.get("provider"),
        "model": row.get("model"),
        "attempt": state.get("attempt"),
        "passed": True,
        "smoke_contract_sha256": contract,
        "smoke_max_turns": campaign.get("smoke_max_turns"),
        "expected_command_sha256": state.get("expected_command_sha256"),
        "prompt_sha256": state.get("prompt_sha256"),
    }
    changed = [key for key, value in expected_fields.items() if proof.get(key) != value]
    if changed:
        return f"smoke proof identity differs: {', '.join(changed)}"
    if campaign.get("smoke_max_turns") != SMOKE_MAX_TURNS:
        return "campaign smoke max-turns contract changed"
    if sha256_bytes(str(state.get("expected_command", "")).encode()) != state.get(
        "expected_command_sha256"
    ):
        return "persisted smoke command differs from its hash"

    evidence = proof.get("evidence")
    if not isinstance(evidence, dict) or evidence.get("passed") is not True:
        return "smoke proof has no passing evidence"
    if (
        evidence.get("entrant") != entrant_id
        or evidence.get("attempt") != state.get("attempt")
        or evidence.get("smoke_contract_sha256") != contract
    ):
        return "smoke attempt evidence belongs to a different contract"
    attempt_evidence = Path(str(state.get("attempt_evidence", "")))
    expected_attempt_evidence = proof_path.parent / "attempt-evidence.json"
    indexes = state.get("attempt_evidence_sha256")
    attempt_name = f"attempt-{int(state.get('attempt', 0))}"
    try:
        sealed_attempt_evidence = (
            load_json(attempt_evidence)
            if attempt_evidence.is_file() and not attempt_evidence.is_symlink()
            else None
        )
    except (OSError, json.JSONDecodeError, SystemExit):
        sealed_attempt_evidence = None
    if (
        attempt_evidence != expected_attempt_evidence
        or attempt_evidence.is_symlink()
        or not attempt_evidence.is_file()
        or not isinstance(indexes, dict)
        or sha256_file(attempt_evidence) != indexes.get(attempt_name)
        or sealed_attempt_evidence != evidence
    ):
        return "sealed smoke attempt evidence is missing or changed"
    paths = {
        "log": Path(str(state.get("log", ""))),
        "lifecycle": Path(str(state.get("provider_lifecycle", ""))),
        "prompt": Path(str(state.get("prompt", ""))),
        "nonce": Path(str(state.get("nonce_file", ""))),
        "listeners": Path(str(state.get("sandbox_listener_snapshot", ""))),
    }
    hashes = evidence.get("hashes")
    if not isinstance(hashes, dict):
        return "smoke proof artifact hashes are malformed"
    for name, path in paths.items():
        if path.is_symlink() or not path.is_file():
            return f"smoke {name} evidence is missing or symbolic"
        if sha256_file(path) != hashes.get(name):
            return f"smoke {name} evidence changed after PASS"
    listener_problem = listener_isolation_failure(campaign, row, state, smoke=True)
    if listener_problem:
        return listener_problem

    stream = parse_smoke_stream(
        paths["log"],
        expected_command=str(state.get("expected_command", "")),
        expected_marker=str(state.get("final_marker", "")),
        expected_tool_output=str(state.get("expected_tool_output", "")),
    )
    if not stream["valid"]:
        return f"smoke stream no longer validates: {'; '.join(stream['errors'])}"
    lifecycle = lifecycle_summary(
        paths["lifecycle"],
        expected_provider=str(row["provider"]),
        expected_model=str(row["model"]),
    )
    if (
        lifecycle_failure(lifecycle)
        or not lifecycle["admitted"]
        or lifecycle["admitted"] != lifecycle["terminal"]
    ):
        return "smoke lifecycle no longer proves admitted terminal requests"
    if lifecycle != evidence.get("lifecycle"):
        return "smoke lifecycle evidence differs from the sealed proof"

    nonce = _regular_file_bytes(paths["nonce"])
    try:
        expected_nonce = bytes.fromhex(str(state.get("nonce_hex", "")))
    except ValueError:
        return "persisted smoke nonce is malformed"
    if nonce != expected_nonce or sha256_bytes(expected_nonce) != evidence.get(
        "nonce_sha256"
    ):
        return "smoke nonce bytes differ from the sealed proof"
    expected_prompt = smoke_prompt(
        str(state.get("expected_command", "")), str(state.get("final_marker", ""))
    ).encode()
    if _regular_file_bytes(paths["prompt"]) != expected_prompt:
        return "smoke prompt differs from the sealed contract"

    binary = Path(str(campaign.get("binary", "")))
    if (
        binary.is_symlink()
        or not binary.is_file()
        or sha256_file(binary) != campaign.get("binary_sha256")
        or campaign.get("binary_sha256") != evidence.get("binary_sha256")
    ):
        return "frozen binary differs from the smoke proof"
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        return mismatch
    if campaign.get("instrument_set_sha256") != evidence.get("instrument_set_sha256"):
        return "frozen instrument identity differs from the smoke proof"
    budget_config = Path(str(campaign.get("budget_config", "")))
    if (
        budget_config.is_symlink()
        or not budget_config.is_file()
        or sha256_file(budget_config) != campaign.get("budget_config_sha256")
        or campaign.get("budget_config_sha256") != evidence.get("budget_config_sha256")
    ):
        return "frozen budget config differs from the smoke proof"

    _, settled, budget_error = current_smoke_budget_requests(campaign, row)
    if budget_error:
        return budget_error
    terminal_ids = evidence.get("terminal_request_ids")
    if not isinstance(terminal_ids, list) or not set(terminal_ids).issubset(
        set(settled)
    ):
        return "shared budget ledger lost the smoke request settlements"
    history = smoke_admission_history(root, entrant_id, row)
    if not history["valid"]:
        return "cumulative smoke admission evidence is invalid: " + "; ".join(
            history["errors"]
        )
    if sealed_smoke_admission_history(history) != proof.get("admission_history"):
        return "cumulative smoke admission evidence differs from the sealed proof"
    try:
        secrets_map = parse_secret_file(Path(str(campaign["secret_file"])))
    except (OSError, KeyError, SystemExit) as error:
        return f"smoke proof secret scan cannot be repeated: {error}"
    secret_hits = secret_occurrences(
        [root / "smoke" / entrant_id],
        secrets_map.values(),
    )
    if secret_hits:
        return "provider credential appeared in sealed smoke artifacts"
    return None


def require_smoke_proofs(root: Path, pristine_entrant: str | None = None) -> None:
    campaign = load_json(campaign_file(root))
    manifest_path = Path(str(campaign.get("entrant_manifest", "")))
    if (
        manifest_path.is_symlink()
        or not manifest_path.is_file()
        or sha256_file(manifest_path) != campaign.get("entrant_manifest_sha256")
    ):
        raise SystemExit("frozen entrant manifest changed before smoke gate")
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    if len(rows) != 5:
        raise SystemExit("cloud builds require exactly five smoke contracts")
    if smoke_max_turns(manifest) != campaign.get("smoke_max_turns"):
        raise SystemExit("campaign smoke max-turns differs from the frozen manifest")
    try:
        contract = smoke_contract_identity(campaign)
    except SystemExit as error:
        raise SystemExit(f"cloud smoke contract is invalid: {error}") from None
    if campaign.get("smoke_contract_sha256") != contract:
        raise SystemExit("campaign smoke contract identity is stale")
    if campaign.get("smoke_status") != "PASS":
        raise SystemExit(
            f"campaign smoke status is {campaign.get('smoke_status')}, not PASS"
        )
    raw_before = campaign.get("smoke_raw_tree_sha256_before")
    raw_after = campaign.get("smoke_raw_tree_sha256_after")
    if not isinstance(raw_before, dict) or raw_before != raw_after:
        raise SystemExit("smoke did not preserve all five raw benchmark trees")
    for entrant_id in [pristine_entrant] if pristine_entrant is not None else []:
        if entrant_id not in raw_before:
            raise SystemExit(f"smoke raw-tree evidence has no entrant: {entrant_id}")
        current_hash = sha256_tree_exact(root / "entrants" / entrant_id / "tree")
        if current_hash != raw_before[entrant_id]:
            raise SystemExit(f"raw benchmark tree changed before build: {entrant_id}")
    proof_hashes = campaign.get("smoke_proof_sha256")
    if not isinstance(proof_hashes, dict):
        raise SystemExit("campaign has no sealed smoke proof index")
    failures = []
    for row in rows:
        entrant_id = str(row["id"])
        mismatch = smoke_proof_mismatch(root, entrant_id, row)
        state = read_smoke_state(root, entrant_id)
        if proof_hashes.get(entrant_id) != state.get("proof_sha256"):
            mismatch = (
                mismatch or "campaign smoke proof index differs from entrant state"
            )
        if mismatch:
            failures.append(f"{entrant_id}: {mismatch}")
    if failures:
        raise SystemExit(
            "cloud builds require five untampered smoke PASS proofs: "
            + "; ".join(failures)
        )


def recover_smoke_entrant(root: Path, entrant_id: str) -> bool:
    state = read_smoke_state(root, entrant_id)
    if state.get("status") in SMOKE_TERMINAL_STATES:
        return state.get("status") == "PASS"
    if state.get("status") == "PLANNED":
        return False
    if process_alive(state.get("supervisor_pid"), state.get("supervisor_identity")):
        return False
    clean = stop_recorded_group(
        state.get("supervisor_pid"),
        state.get("supervisor_pgid"),
        state.get("supervisor_identity"),
    )
    if state.get("active_attempt"):
        return finalize_smoke_attempt(
            root,
            entrant_id,
            exit_code=state.get("exit_code"),
            descendants_clean=clean,
        )
    row = manifest_row(root, entrant_id)
    ambiguity = smoke_attempt_history_failure(root, entrant_id, row)
    update_smoke_state(
        root,
        entrant_id,
        status="FAILED" if ambiguity or not clean else "PRE_ADMISSION_FAILURE",
        failure=ambiguity or "smoke supervisor disappeared before provider admission",
        supervisor_pid=None,
        supervisor_pgid=None,
        supervisor_identity=None,
    )
    return False


@contextlib.contextmanager
def provider_lane(root: Path, lane: str) -> Iterator[None]:
    path = root / "locks" / f"{lane}.lock"
    with path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


@contextlib.contextmanager
def exclusive_claim(path: Path, blocking: bool = False) -> Iterator[bool]:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as lock:
        flags = fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB)
        try:
            fcntl.flock(lock.fileno(), flags)
        except BlockingIOError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def smoke_supervise(root: Path, entrant_id: str) -> int:
    with exclusive_claim(root / "locks" / f"smoke-{entrant_id}.claim") as claimed:
        if not claimed:
            return 0
        return smoke_supervise_claimed(root, entrant_id)


def smoke_supervise_claimed(root: Path, entrant_id: str) -> int:
    require_lineage(root)
    campaign = load_json(campaign_file(root))
    row = manifest_row(root, entrant_id)
    state = read_smoke_state(root, entrant_id)
    if state.get("status") not in SMOKE_RETRYABLE_STATES:
        return 0 if state.get("status") == "PASS" else 1
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        update_smoke_state(
            root, entrant_id, status="PRE_ADMISSION_FAILURE", failure=mismatch
        )
        return 2
    try:
        contract = smoke_contract_identity(campaign)
    except SystemExit as error:
        update_smoke_state(
            root, entrant_id, status="PRE_ADMISSION_FAILURE", failure=str(error)
        )
        return 2
    if campaign.get("smoke_contract_sha256") != contract:
        update_smoke_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure="campaign smoke contract hash is stale",
        )
        return 2
    ambiguity = smoke_attempt_history_failure(root, entrant_id, row)
    if ambiguity:
        update_smoke_state(root, entrant_id, status="FAILED", failure=ambiguity)
        return 2
    try:
        secret_values = parse_secret_file(Path(str(campaign["secret_file"])))
    except (OSError, KeyError, SystemExit) as error:
        update_smoke_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure=f"smoke credential preflight failed: {error}",
        )
        return 2
    secret_value = secret_values.get(str(row["secret_env"]), "")
    if not secret_value:
        update_smoke_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure="missing smoke provider credential",
        )
        return 2

    update_smoke_state(
        root,
        entrant_id,
        status="WAITING_PROVIDER_LANE",
        active_attempt=False,
        supervisor_pid=os.getpid(),
        supervisor_pgid=os.getpgrp(),
        supervisor_identity=process_identity(os.getpid()),
        queued_at=utc_now(),
        failure=None,
    )
    with provider_lane(root, str(row["provider_lane"])):
        state = read_smoke_state(root, entrant_id)
        if (
            state.get("status") != "WAITING_PROVIDER_LANE"
            or int(state.get("supervisor_pid", -1)) != os.getpid()
        ):
            return 0
        try:
            state = prepare_smoke_attempt(root, entrant_id, row)
        except SystemExit as error:
            update_smoke_state(root, entrant_id, status="FAILED", failure=str(error))
            return 2
        binary = Path(str(campaign["binary"]))
        if (
            binary.is_symlink()
            or not binary.is_file()
            or not os.access(binary, os.X_OK)
            or sha256_file(binary) != campaign.get("binary_sha256")
        ):
            return (
                2
                if not finalize_smoke_attempt(
                    root, entrant_id, exit_code=None, descendants_clean=True
                )
                else 0
            )
        mismatch = instrument_mismatch(campaign)
        if mismatch:
            update_smoke_state(root, entrant_id, launch_failure=mismatch)
            finalize_smoke_attempt(
                root, entrant_id, exit_code=None, descendants_clean=True
            )
            return 2
        try:
            state = persist_listener_isolation(
                root, row, state, smoke=True
            )
        except SystemExit as error:
            update_smoke_state(root, entrant_id, launch_failure=str(error))
            finalize_smoke_attempt(
                root, entrant_id, exit_code=None, descendants_clean=True
            )
            return 2
        env = child_env(row, state, secret_value)
        command = smoke_goose_command(
            binary,
            row,
            Path(str(state["prompt"])).read_text(),
            int(state["smoke_max_turns"]),
        )
        sanitized_command = [
            "[PROMPT]" if value == Path(str(state["prompt"])).read_text() else value
            for value in command
        ]
        log_path = Path(str(state["log"]))
        lifecycle_path = Path(str(state["provider_lifecycle"]))
        lifecycle_path.unlink(missing_ok=True)
        _, observe = smoke_state_observer(root, entrant_id)
        update_smoke_state(
            root,
            entrant_id,
            status="RUNNING",
            started_at=utc_now(),
            command=sanitized_command,
            binary_sha256=campaign["binary_sha256"],
            instrument_set_sha256=campaign["instrument_set_sha256"],
            failure=None,
        )
        exit_code: int | None = None
        try:
            with log_path.open("x", buffering=1) as log:
                proc = subprocess.Popen(
                    command,
                    cwd=Path(str(state["tree"])),
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    bufsize=1,
                )
                update_smoke_state(
                    root,
                    entrant_id,
                    goose_pid=proc.pid,
                    goose_identity=process_identity(proc.pid),
                    process_group=os.getpgrp(),
                )
                assert proc.stdout is not None
                redacted_copy(proc.stdout, log, secret_values.values(), observe)
                exit_code = proc.wait()
        except OSError as error:
            update_smoke_state(
                root,
                entrant_id,
                launch_failure=f"{type(error).__name__}: {error}",
            )
        descendants_clean = stop_group_members(os.getpgrp(), {os.getpid()})
        passed = finalize_smoke_attempt(
            root,
            entrant_id,
            exit_code=exit_code,
            descendants_clean=descendants_clean,
        )
        return 0 if passed else 1


def supervise(root: Path, entrant_id: str) -> int:
    with exclusive_claim(root / "locks" / f"entrant-{entrant_id}.claim") as claimed:
        if not claimed:
            return 0
        return supervise_claimed(root, entrant_id)


def supervise_claimed(root: Path, entrant_id: str) -> int:
    require_smoke_proofs(root, pristine_entrant=entrant_id)
    campaign = load_json(campaign_file(root))
    if campaign.get("status") == "STOPPED" or (root / SUPERSESSION_RECEIPT).exists():
        return 2
    lineage_problem = lineage_failure(root)
    if lineage_problem:
        update_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure=f"campaign lineage refused provider admission: {lineage_problem}",
        )
        return 2
    smoke_problem = supersession_smoke_gate_failure(root)
    if smoke_problem:
        update_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure=smoke_problem,
        )
        return 2
    instrument_bench = campaign_instrument_path(
        campaign, "evals/swarm-bench/bench/cloud_sb7.py"
    ).parent
    if str(instrument_bench) not in sys.path:
        sys.path.insert(0, str(instrument_bench))
    from vendor_service_v3 import serve  # noqa: PLC0415

    row = manifest_row(root, entrant_id)
    state = read_state(root, entrant_id)
    if state["status"] not in RETRYABLE_BUILD_STATES:
        return 0
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        update_state(root, entrant_id, status="PRE_ADMISSION_FAILURE", failure=mismatch)
        return 2
    secret_path = Path(str(campaign["secret_file"]))
    secret_values = parse_secret_file(secret_path)
    secret_name = str(row["secret_env"])
    secret_value = secret_values.get(secret_name, "")
    if not secret_value:
        update_state(
            root,
            entrant_id,
            status="PRE_ADMISSION_FAILURE",
            failure="missing credential",
        )
        return 2

    update_state(
        root,
        entrant_id,
        status="WAITING_PROVIDER_LANE",
        supervisor_pid=os.getpid(),
        supervisor_pgid=os.getpgrp(),
        queued_at=utc_now(),
    )
    with provider_lane(root, str(row["provider_lane"])):
        state = read_state(root, entrant_id)
        if (
            state.get("status") != "WAITING_PROVIDER_LANE"
            or int(state.get("supervisor_pid", -1)) != os.getpid()
        ):
            return 0
        port = int(state["vendor_port"])
        if not port_is_free(port):
            update_state(
                root,
                entrant_id,
                status="PRE_ADMISSION_FAILURE",
                failure=f"vendor port {port} occupied before provider admission",
            )
            return 2

        trace = Path(str(state["vendor_trace"]))
        trace.unlink(missing_ok=True)
        server = serve(port, trace, seed=str(state["fixture_seed"]))
        prompt = build_prompt(port, campaign)
        prompt_path = Path(str(state["tree"])).parent / "prompt.txt"
        prompt_path.write_text(prompt)
        prompt_sha = sha256_bytes(prompt.encode())
        binary = Path(str(campaign["binary"]))
        if sha256_file(binary) != campaign["binary_sha256"]:
            server.shutdown()
            update_state(
                root,
                entrant_id,
                status="PRE_ADMISSION_FAILURE",
                failure="frozen binary hash changed before admission",
            )
            return 2

        try:
            state = persist_listener_isolation(
                root, row, state, smoke=False
            )
        except SystemExit as error:
            server.shutdown()
            update_state(
                root,
                entrant_id,
                status="PRE_ADMISSION_FAILURE",
                failure=str(error),
            )
            return 2

        telemetry = Path(str(state["tree"])) / ".swarm/telemetry.jsonl"
        telemetry.parent.mkdir(parents=True, exist_ok=True)
        telemetry.write_text("")
        lifecycle_path = Path(str(state["provider_lifecycle"]))
        lifecycle_path.unlink(missing_ok=True)
        env = child_env(row, state, secret_value)
        cmd = build_goose_command(binary, row, prompt)
        log_path = Path(str(state["build_log"]))
        counters, observe = provider_state_observer(root, entrant_id)
        started = time.time()
        manifest = load_json(Path(str(campaign["entrant_manifest"])))
        max_episodes = int(manifest["spend_policy"]["max_full_episodes_per_model"])
        episode_attempt = int(state.get("provider_episode_attempts", 0)) + 1
        if episode_attempt > max_episodes:
            server.shutdown()
            update_state(
                root,
                entrant_id,
                status="INCOMPLETE",
                failure="provider episode limit exhausted before process creation",
            )
            return 2
        update_state(
            root,
            entrant_id,
            status="BUILD_RUNNING",
            started_at=utc_now(),
            prompt_sha256=prompt_sha,
            provider_episode_attempts=episode_attempt,
            command=[
                str(binary),
                "run",
                "--quiet",
                "--provider",
                row["provider"],
                "--model",
                row["model"],
                "--output-format",
                "stream-json",
                "-t",
                "[PROMPT]",
            ],
        )
        try:
            with log_path.open("a", buffering=1) as log:
                proc = subprocess.Popen(
                    cmd,
                    cwd=Path(str(state["tree"])),
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    bufsize=1,
                )
                update_state(
                    root, entrant_id, goose_pid=proc.pid, process_group=os.getpgrp()
                )
                assert proc.stdout is not None
                redacted_copy(proc.stdout, log, secret_values.values(), observe)
                exit_code = proc.wait()
        finally:
            server.shutdown()
        descendants_clean = stop_group_members(os.getpgrp(), {os.getpid()})
        secret_hits = persisted_entrant_secret_hits(root, campaign, entrant_id)

        elapsed = round(time.time() - started, 3)
        lifecycle = lifecycle_summary(
            lifecycle_path,
            expected_provider=str(row["provider"]),
            expected_model=str(row["model"]),
        )
        counters["admitted"] = lifecycle["admitted"]
        counters["terminal"] = lifecycle["terminal"]
        counters["first_output_at"] = lifecycle["first_output_at"]
        completed = exit_code == 0
        status, failure = classify_build_exit(exit_code, counters["admitted"])
        isolation_problem = listener_isolation_failure(
            campaign, row, read_state(root, entrant_id), smoke=False
        )
        outstanding_ids, budget_error = entrant_outstanding_reservations(campaign, row)
        if budget_error:
            status = "INCOMPLETE"
            failure = budget_error
            completed = False
        elif outstanding_ids:
            status = "INCOMPLETE"
            failure = (
                f"{len(outstanding_ids)} provider request(s) retain full budget reserves; "
                "admission or terminal usage is ambiguous and the episode is never retried"
            )
            completed = False
        elif lifecycle_failure(lifecycle) is not None:
            status = "INCOMPLETE"
            failure = f"provider lifecycle evidence is invalid: {lifecycle_failure(lifecycle)}"
            completed = False
        elif isolation_problem:
            status = "INCOMPLETE"
            failure = isolation_problem
            completed = False
        elif completed and counters["admitted"] == 0:
            status = "INCOMPLETE"
            failure = "goose exited successfully without a proven provider admission"
            completed = False
        elif completed and counters["admitted"] != counters["terminal"]:
            status = "INCOMPLETE"
            failure = (
                f"{counters['admitted']} requests admitted but only "
                f"{counters['terminal']} reached proven provider terminal"
            )
            completed = False
        elif secret_hits:
            status = "INCOMPLETE"
            failure = "provider credential appeared in benchmark-controlled artifacts"
            completed = False
        elif completed and not descendants_clean:
            status = "INCOMPLETE"
            failure = "background tool descendants survived the build process"
            completed = False
        elif lineage_failure(root) is not None:
            status = "INCOMPLETE"
            failure = f"campaign lineage changed during build: {lineage_failure(root)}"
            completed = False
        tree_hash = hash_tree(Path(str(state["tree"])))
        final = update_state(
            root,
            entrant_id,
            status=status,
            exit_code=exit_code,
            failure=failure,
            finished_at=utc_now(),
            elapsed_seconds=elapsed,
            admitted_requests=counters["admitted"],
            provider_terminal_requests=counters["terminal"],
            lifecycle_events=lifecycle["events"],
            lifecycle_malformed_lines=lifecycle["malformed_lines"],
            lifecycle_transition_errors=lifecycle["transition_errors"],
            lifecycle_ambiguous_request_ids=lifecycle["ambiguous_request_ids"],
            budget_outstanding_request_ids=outstanding_ids,
            secret_scan_hits=secret_hits,
            first_output_at=counters["first_output_at"],
            raw_tree_sha256=tree_hash,
        )
        atomic_json(Path(str(state["tree"])).parent / "build-manifest.json", final)
        return 0 if completed else 1


def hash_tree(root: Path) -> str:
    h = hashlib.sha256()
    if not root.is_dir():
        return h.hexdigest()
    ignored = {".DS_Store", "telemetry.jsonl"}
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.name in ignored:
            continue
        rel = str(path.relative_to(root)).encode()
        h.update(len(rel).to_bytes(8, "big"))
        h.update(rel)
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                h.update(chunk)
    return h.hexdigest()


def launch_detached(cmd: list[str], log_path: Path) -> subprocess.Popen[Any]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log = log_path.open("a")
    try:
        return subprocess.Popen(
            cmd,
            cwd=REPO,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            close_fds=True,
        )
    finally:
        log.close()


def launch_supervisor(root: Path, entrant_id: str) -> subprocess.Popen[Any]:
    unit = root / "entrants" / entrant_id
    campaign = load_json(campaign_file(root))
    proc = launch_detached(
        [
            sys.executable,
            str(campaign["coordinator"]),
            "_supervise",
            "--root",
            str(root),
            "--entrant",
            entrant_id,
        ],
        unit / "logs/supervisor.log",
    )
    update_state(
        root,
        entrant_id,
        supervisor_pid=proc.pid,
        supervisor_pgid=proc.pid,
        supervisor_identity=process_identity(proc.pid),
        launched_at=utc_now(),
    )
    return proc


def launch_smoke_supervisor(root: Path, entrant_id: str) -> subprocess.Popen[Any]:
    unit = root / "smoke" / entrant_id
    campaign = load_json(campaign_file(root))
    state = read_smoke_state(root, entrant_id)
    launch = int(state.get("supervisor_launches", 0)) + 1
    proc = launch_detached(
        [
            sys.executable,
            str(campaign["coordinator"]),
            "_smoke_supervise",
            "--root",
            str(root),
            "--entrant",
            entrant_id,
        ],
        unit / f"supervisor-{launch}.log",
    )
    update_smoke_state(
        root,
        entrant_id,
        supervisor_launches=launch,
        supervisor_pid=proc.pid,
        supervisor_pgid=proc.pid,
        supervisor_identity=process_identity(proc.pid),
        supervisor_log=str(unit / f"supervisor-{launch}.log"),
        supervisor_launched_at=utc_now(),
    )
    return proc


def process_identity(pid: Any) -> str | None:
    try:
        value = int(pid)
    except (TypeError, ValueError):
        return None
    proc = subprocess.run(
        ["ps", "-p", str(value), "-o", "stat=", "-o", "lstart="],
        text=True,
        capture_output=True,
        check=False,
    )
    raw = proc.stdout.strip()
    if proc.returncode != 0 or not raw:
        return None
    fields = raw.split(maxsplit=1)
    if not fields or fields[0].startswith("Z"):
        return None
    return fields[1] if len(fields) > 1 else "running"


def process_alive(pid: Any, expected_identity: Any = None) -> bool:
    identity = process_identity(pid)
    if identity is None:
        return False
    return expected_identity is None or identity == str(expected_identity)


def process_group_members(pgid: int) -> list[tuple[int, str]]:
    proc = subprocess.run(
        ["ps", "-axo", "pid=,pgid=,stat="],
        text=True,
        capture_output=True,
        check=False,
        start_new_session=True,
    )
    members: list[tuple[int, str]] = []
    for raw in proc.stdout.splitlines():
        fields = raw.split()
        if len(fields) < 3:
            continue
        try:
            pid, candidate = int(fields[0]), int(fields[1])
        except ValueError:
            continue
        if candidate == pgid and not fields[2].startswith("Z"):
            members.append((pid, fields[2]))
    return members


def stop_group_members(
    pgid: int, excluded_pids: set[int] | None = None, grace_seconds: float = 5.0
) -> bool:
    excluded = excluded_pids or set()
    deadline = time.monotonic() + grace_seconds
    signaled: set[int] = set()
    while time.monotonic() < deadline:
        members = [pid for pid, _ in process_group_members(pgid) if pid not in excluded]
        if not members:
            return True
        for pid in members:
            if pid in signaled:
                continue
            with contextlib.suppress(ProcessLookupError):
                os.kill(pid, signal.SIGTERM)
            signaled.add(pid)
        time.sleep(0.2)
    members = [pid for pid, _ in process_group_members(pgid) if pid not in excluded]
    for pid in members:
        with contextlib.suppress(ProcessLookupError):
            os.kill(pid, signal.SIGKILL)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if not [pid for pid, _ in process_group_members(pgid) if pid not in excluded]:
            return True
        time.sleep(0.2)
    return not [pid for pid, _ in process_group_members(pgid) if pid not in excluded]


def wait_for_builds(
    root: Path,
    row_ids: list[str],
    supervisors: Mapping[str, subprocess.Popen[Any]] | None = None,
) -> bool:
    handles = dict(supervisors or {})
    campaign = load_json(campaign_file(root))
    while True:
        states = [read_state(root, entrant_id) for entrant_id in row_ids]
        for entrant_id, proc in list(handles.items()):
            if proc.poll() is not None:
                proc.wait()
                handles.pop(entrant_id, None)
        if all(
            state["status"] in TERMINAL_BUILD_STATES | POST_BUILD_STATES
            for state in states
        ):
            return all(state["status"] in BUILD_SUCCESS_STATES for state in states)
        for state in states:
            if state[
                "status"
            ] not in TERMINAL_BUILD_STATES | POST_BUILD_STATES and state.get(
                "supervisor_pid"
            ):
                if not process_alive(
                    state["supervisor_pid"], state.get("supervisor_identity")
                ):
                    pgid = int(state.get("supervisor_pgid") or 0)
                    group_clean = not process_group_members(pgid) or stop_group(pgid)
                    row = manifest_row(root, str(state["entrant"]))
                    lifecycle = lifecycle_summary(
                        Path(str(state["provider_lifecycle"])),
                        expected_provider=str(row["provider"]),
                        expected_model=str(row["model"]),
                    )
                    outstanding_ids, budget_error = entrant_outstanding_reservations(
                        campaign, row
                    )
                    ambiguous = bool(
                        lifecycle["admitted"]
                        or lifecycle_failure(lifecycle)
                        or outstanding_ids
                        or budget_error
                        or not group_clean
                    )
                    reasons = ["supervisor disappeared; silence is not success"]
                    if lifecycle["admitted"]:
                        reasons.append(
                            f"{lifecycle['admitted']} request(s) were admitted"
                        )
                    if outstanding_ids:
                        reasons.append(
                            f"{len(outstanding_ids)} request(s) retain full budget reserves"
                        )
                    if budget_error:
                        reasons.append(budget_error)
                    if not group_clean:
                        reasons.append("owned process group survived cleanup")
                    update_state(
                        root,
                        str(state["entrant"]),
                        status="INCOMPLETE" if ambiguous else "PRE_ADMISSION_FAILURE",
                        failure="; ".join(reasons),
                        admitted_requests=lifecycle["admitted"],
                        provider_terminal_requests=lifecycle["terminal"],
                        lifecycle_events=lifecycle["events"],
                        lifecycle_malformed_lines=lifecycle["malformed_lines"],
                        lifecycle_transition_errors=lifecycle["transition_errors"],
                        lifecycle_ambiguous_request_ids=lifecycle[
                            "ambiguous_request_ids"
                        ],
                        budget_outstanding_request_ids=outstanding_ids,
                    )
        time.sleep(10)


def wait_for_smokes(
    root: Path,
    row_ids: list[str],
    supervisors: Mapping[str, subprocess.Popen[Any]] | None = None,
    *,
    poll_seconds: float = 1.0,
) -> bool:
    handles = dict(supervisors or {})
    while True:
        for entrant_id, proc in list(handles.items()):
            if proc.poll() is not None:
                proc.wait()
                handles.pop(entrant_id, None)
        states = [read_smoke_state(root, entrant_id) for entrant_id in row_ids]
        if all(state.get("status") in SMOKE_TERMINAL_STATES for state in states):
            return all(state.get("status") == "PASS" for state in states)
        for state in states:
            if state.get("status") in SMOKE_TERMINAL_STATES:
                continue
            if not process_alive(
                state.get("supervisor_pid"), state.get("supervisor_identity")
            ):
                recover_smoke_entrant(root, str(state["entrant"]))
        time.sleep(poll_seconds)


def smoke(root: Path) -> int:
    with exclusive_claim(root / "locks/smoke-run.claim", blocking=True) as claimed:
        if not claimed:
            raise SystemExit("cannot claim cloud contract smoke run")
        require_lineage(root)
        campaign = load_json(campaign_file(root))
        manifest = load_json(Path(str(campaign["entrant_manifest"])))
        rows = entrants(manifest)
        if len(rows) != 5:
            raise SystemExit("cloud SB7 smoke requires exactly five frozen entrants")
        build_states = [read_state(root, str(row["id"])) for row in rows]
        dirty_builds = [
            str(state["entrant"])
            for state in build_states
            if state.get("status") != "PLANNED"
            or int(state.get("provider_episode_attempts", 0)) != 0
            or int(state.get("admitted_requests", 0)) != 0
        ]
        if dirty_builds:
            raise SystemExit(
                "contract smoke must precede every raw benchmark build: "
                + ", ".join(dirty_builds)
            )
        raw_before = {
            str(row["id"]): sha256_tree_exact(
                root / "entrants" / str(row["id"]) / "tree"
            )
            for row in rows
        }
        update_campaign(
            root,
            smoke_status="RUNNING",
            smoke_started_at=utc_now(),
            smoke_raw_tree_sha256_before=raw_before,
            smoke_failure=None,
        )
        row_ids = [str(row["id"]) for row in rows]
        for entrant_id in row_ids:
            recover_smoke_entrant(root, entrant_id)
        supervisors: Dict[str, subprocess.Popen[Any]] = {}
        for entrant_id in row_ids:
            state = read_smoke_state(root, entrant_id)
            if state.get("status") in SMOKE_RETRYABLE_STATES:
                supervisors[entrant_id] = launch_smoke_supervisor(root, entrant_id)
        passed = wait_for_smokes(root, row_ids, supervisors)
        raw_after = {
            entrant_id: sha256_tree_exact(root / "entrants" / entrant_id / "tree")
            for entrant_id in row_ids
        }
        if raw_after != raw_before:
            passed = False
            for entrant_id in row_ids:
                if raw_after[entrant_id] != raw_before[entrant_id]:
                    state = read_smoke_state(root, entrant_id)
                    if state.get("status") != "FAILED":
                        update_smoke_state(
                            root,
                            entrant_id,
                            status="FAILED",
                            failure="raw benchmark tree changed during isolated smoke",
                        )
        proof_hashes = {
            entrant_id: read_smoke_state(root, entrant_id).get("proof_sha256")
            for entrant_id in row_ids
        }
        update_campaign(
            root,
            smoke_status="PASS" if passed else "ATTENTION",
            smoke_finished_at=utc_now(),
            smoke_raw_tree_sha256_after=raw_after,
            smoke_proof_sha256=proof_hashes,
            smoke_failure=None if passed else "one or more contract smokes failed",
        )
        if passed:
            require_smoke_proofs(root)
        return 0 if passed else 1


def clone_for_score(root: Path, entrant_id: str, attempt: int) -> Path:
    raw = root / "entrants" / entrant_id / "tree"
    dest = root / "scores" / entrant_id / f"attempt-{attempt}" / "tree"
    if dest.exists():
        raise SystemExit(
            f"score clone already exists; attempts are never overwritten: {dest}"
        )
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(
        raw,
        dest,
        ignore=shutil.ignore_patterns(
            ".swarm",
            "__pycache__",
            ".pytest_cache",
            "*.pyc",
            "graded-sb7-db",
            "sb7-empty-db",
            "sb7-combined-db",
            "sb7-shots",
            "sb7-tokens.json",
            "sb7-expect.json",
            "vendor-trace-sb7.jsonl",
        ),
    )
    return dest


def next_score_attempt(root: Path, entrant_id: str, state: Mapping[str, Any]) -> int:
    attempts_root = root / "scores" / entrant_id
    disk_attempts = []
    if attempts_root.is_dir():
        for path in attempts_root.glob("attempt-*"):
            with contextlib.suppress(ValueError):
                disk_attempts.append(int(path.name.removeprefix("attempt-")))
    return max([int(state.get("score_attempts", 0)), *disk_attempts], default=0) + 1


def stop_recorded_group(
    pid: Any,
    pgid: Any,
    identity: Any,
    *,
    grace_seconds: float = 5.0,
) -> bool:
    actual_identity = process_identity(pid)
    if actual_identity is not None:
        if identity is not None and actual_identity != str(identity):
            return True
        return stop_group(int(pgid or pid), grace_seconds=grace_seconds)
    group = int(pgid or 0)
    if group and process_group_members(group):
        return stop_group(group, grace_seconds=grace_seconds)
    return True


def recover_interrupted_scoring(root: Path) -> None:
    for state in status_rows(root):
        if state["status"] != "SCORING":
            continue
        clean = stop_recorded_group(
            state.get("score_pid"),
            state.get("score_pgid"),
            state.get("score_identity"),
        )
        update_state(
            root,
            str(state["entrant"]),
            status="SCORE_FAILED" if clean else "INCOMPLETE",
            failure=(
                "scorer supervision disappeared; the immutable attempt is retained and "
                "a fresh attempt is required"
                if clean
                else "interrupted scorer process group survived recovery"
            ),
            score_recovered_at=utc_now(),
            score_pid=None,
            score_pgid=None,
            score_identity=None,
        )


def recover_dead_manager(root: Path) -> bool:
    manager = load_json(root / "manager.json")
    pid = manager.get("pid")
    identity = manager.get("identity")
    actual_identity = process_identity(pid)
    if actual_identity is not None and (
        identity is None or actual_identity == str(identity)
    ):
        return False
    if actual_identity is None and not stop_recorded_group(
        pid, manager.get("pgid"), identity
    ):
        raise SystemExit("dead manager's process group survived recovery")
    recover_interrupted_scoring(root)
    recover_interrupted_publication(root)
    manager_state(
        root,
        status="RECOVERED",
        recovered_at=utc_now(),
        pid=None,
        pgid=None,
        identity=None,
    )
    return True


def manager_restart_mismatch(root: Path) -> str | None:
    try:
        require_smoke_proofs(root)
    except SystemExit as error:
        return f"smoke gate refused manager recovery: {error}"
    campaign = load_json(campaign_file(root))
    if campaign.get("status") not in RESTARTABLE_CAMPAIGN_STATES - {"ATTENTION"}:
        return f"campaign is not autonomously restart-safe: {campaign.get('status')}"
    manager = load_json(root / "manager.json")
    if process_alive(manager.get("pid"), manager.get("identity")):
        return "manager is still alive"
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    max_episodes = int(manifest["spend_policy"]["max_full_episodes_per_model"])
    for row in entrants(manifest):
        entrant_id = str(row["id"])
        state = read_state(root, entrant_id)
        status = str(state.get("status"))
        supervisor_alive = process_alive(
            state.get("supervisor_pid"), state.get("supervisor_identity")
        )
        could_relaunch = status in RETRYABLE_BUILD_STATES
        abandoned_active = (
            status not in TERMINAL_BUILD_STATES | POST_BUILD_STATES
            and not supervisor_alive
        )
        if not could_relaunch and not abandoned_active:
            continue
        lifecycle = lifecycle_summary(
            Path(str(state.get("provider_lifecycle", ""))),
            expected_provider=str(row["provider"]),
            expected_model=str(row["model"]),
        )
        outstanding, budget_error = entrant_outstanding_reservations(campaign, row)
        reasons = []
        if lifecycle.get("admitted"):
            reasons.append(f"{lifecycle['admitted']} provider request(s) admitted")
        if lifecycle_failure(lifecycle):
            reasons.append(f"lifecycle is ambiguous: {lifecycle_failure(lifecycle)}")
        if outstanding:
            reasons.append(
                f"outstanding full-budget reserves: {', '.join(outstanding)}"
            )
        if budget_error:
            reasons.append(budget_error)
        attempts = int(state.get("provider_episode_attempts", 0))
        if could_relaunch and attempts >= max_episodes:
            reasons.append("provider episode allowance is exhausted")
        if reasons:
            return f"{entrant_id} is not pre-admission restart-safe: " + "; ".join(
                reasons
            )
    return None


def recover_interrupted_publication(root: Path) -> None:
    for state in status_rows(root):
        if state["status"] not in INTERRUPTED_PUBLICATION_STATES:
            continue
        clean = stop_recorded_group(
            state.get("publisher_pid"),
            state.get("publisher_pgid"),
            state.get("publisher_identity"),
        )
        if not clean:
            raise SystemExit(
                f"{state['entrant']} publisher process group survived recovery"
            )
        update_state(
            root,
            str(state["entrant"]),
            status="PUBLISH_FAILED",
            publisher_pid=None,
            publisher_pgid=None,
            publisher_identity=None,
            publisher_recovered_at=utc_now(),
            failure=(
                "publication was interrupted; deterministic remote receipt must be "
                "checked before any retry"
            ),
        )


class PublicationError(RuntimeError):
    pass


def redact_text(value: str, redactions: Iterable[str]) -> str:
    redacted = value
    for secret_value in redactions:
        if secret_value:
            redacted = redacted.replace(secret_value, "[REDACTED]")
    return redacted


def pinned_publisher_env_values(campaign: Mapping[str, Any]) -> Dict[str, str]:
    publisher = campaign.get("publisher")
    if not isinstance(publisher, dict):
        raise PublicationError("campaign has no pinned publisher")
    try:
        current, values = read_publisher_env(Path(str(publisher["repo"])))
    except (OSError, SystemExit) as error:
        raise PublicationError(
            f"pinned publisher environment cannot be read: {error}"
        ) from None
    for field in (
        "env_file",
        "env_file_mode",
        "env_file_sha256",
        "sanity_target",
    ):
        if current.get(field) != publisher.get(field):
            raise PublicationError(
                f"pinned publisher environment changed after freeze: {field}"
            )
    return values


def publisher_environment(
    campaign: Mapping[str, Any], include_credentials: bool = False
) -> tuple[Dict[str, str], list[str]]:
    publisher = campaign.get("publisher")
    if not isinstance(publisher, dict):
        raise PublicationError("campaign has no pinned publisher")
    values = pinned_publisher_env_values(campaign)
    env = {key: value for key, value in os.environ.items() if key in SAFE_ENV_NAMES}
    if include_credentials:
        env.update(
            {
                "SANITY_WRITE_TOKEN": values["SANITY_WRITE_TOKEN"],
                "NEXT_PUBLIC_SANITY_PROJECT_ID": values[
                    "NEXT_PUBLIC_SANITY_PROJECT_ID"
                ],
                "NEXT_PUBLIC_SANITY_DATASET": values.get(
                    "NEXT_PUBLIC_SANITY_DATASET", "production"
                ),
            }
        )
    redactions = sorted(
        {
            value
            for value in values.values()
            if isinstance(value, str) and len(value) >= 8
        },
        key=len,
        reverse=True,
    )
    return env, redactions


def run_logged_process(
    cmd: list[str],
    cwd: Path,
    env: Mapping[str, str],
    log_path: Path,
    timeout_seconds: float,
    redactions: Iterable[str],
    on_started: Any = None,
) -> Dict[str, Any]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    proc = subprocess.Popen(
        cmd,
        cwd=cwd,
        env=dict(env),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
        close_fds=True,
    )
    assert proc.stdout is not None
    selector: selectors.BaseSelector | None = None
    try:
        if on_started is not None:
            on_started(proc)
        os.set_blocking(proc.stdout.fileno(), False)
        selector = selectors.DefaultSelector()
        selector.register(proc.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout_seconds
        pending = ""
        output_hash = hashlib.sha256()
        timed_out = False
        redaction_values = list(redactions)

        def persist(text_value: str, log: Any) -> None:
            safe = redact_text(text_value, redaction_values)
            log.write(safe)
            log.flush()
            output_hash.update(safe.encode())

        with log_path.open("w", buffering=1) as log:
            while selector.get_map() or proc.poll() is None:
                if (
                    not timed_out
                    and proc.poll() is None
                    and time.monotonic() >= deadline
                ):
                    timed_out = True
                    stop_group(proc.pid, grace_seconds=5.0)
                for key, _ in selector.select(timeout=0.25):
                    try:
                        chunk = os.read(key.fd, 65536)
                    except BlockingIOError:
                        continue
                    if not chunk:
                        with contextlib.suppress(Exception):
                            selector.unregister(key.fileobj)
                        continue
                    pending += chunk.decode("utf-8", errors="replace")
                    while "\n" in pending:
                        line, pending = pending.split("\n", 1)
                        persist(f"{line}\n", log)
                if timed_out and proc.poll() is not None and not selector.get_map():
                    break
            if pending:
                persist(pending, log)
        exit_code = proc.wait()
        return {
            "exit_code": exit_code,
            "timed_out": timed_out,
            "log": str(log_path),
            "log_sha256": output_hash.hexdigest(),
            "pid": proc.pid,
        }
    except BaseException:
        if proc.poll() is None:
            stop_group(proc.pid, grace_seconds=5.0)
        with contextlib.suppress(Exception):
            proc.wait(timeout=5)
        raise
    finally:
        if selector is not None:
            selector.close()
        proc.stdout.close()


def publish_entry(campaign: Mapping[str, Any], entrant_id: str) -> Dict[str, str]:
    publisher = campaign.get("publisher")
    if not isinstance(publisher, dict):
        raise PublicationError("campaign has no pinned publisher")
    entries = publisher.get("entries")
    if not isinstance(entries, dict) or not isinstance(entries.get(entrant_id), dict):
        raise PublicationError(f"campaign publisher has no entrant: {entrant_id}")
    return dict(entries[entrant_id])


def publication_stage(root: Path, entrant_id: str) -> Path:
    campaign = load_json(campaign_file(root))
    state = read_state(root, entrant_id)
    attempt = int(state.get("score_attempts", 0))
    if attempt <= 0:
        raise PublicationError(f"{entrant_id} has no successful score attempt to stage")
    target = root / "publish" / entrant_id / f"attempt-{attempt}"
    runs = target / "runs"
    artifact_manifest = target / "artifact-manifest.json"
    source_verdict = Path(str(state.get("verdict", "")))
    if not source_verdict.is_file():
        raise PublicationError(f"scored verdict is missing: {source_verdict}")
    verdict = load_json(source_verdict)
    rep = verdict.get("rep", 0)
    if not isinstance(rep, int) or rep < 0:
        raise PublicationError("scored verdict has an invalid repetition index")
    source_shots = source_verdict.parent / "tree" / "sb7-shots"
    if not source_shots.is_dir():
        raise PublicationError(f"scorer screenshots are missing: {source_shots}")
    linked = [path for path in source_shots.rglob("*") if path.is_symlink()]
    if linked:
        raise PublicationError("scorer screenshot evidence contains a symbolic link")

    if target.exists():
        if not runs.is_dir() or not artifact_manifest.is_file():
            raise PublicationError(f"partial publication stage is present: {target}")
        artifact = load_json(artifact_manifest)
        actual_hash = hash_tree(runs)
        if (
            artifact.get("entrant") != entrant_id
            or artifact.get("score_attempt") != attempt
            or artifact.get("source_verdict_sha256") != sha256_file(source_verdict)
            or artifact.get("runs_sha256") != actual_hash
            or artifact.get("instrument_set_sha256")
            != campaign.get("instrument_set_sha256")
            or artifact.get("publisher_instrument_set_sha256")
            != campaign.get("publisher", {}).get("instrument_set_sha256")
        ):
            raise PublicationError(f"publication stage changed after sealing: {target}")
        update_state(
            root,
            entrant_id,
            publish_stage=str(runs),
            publish_stage_sha256=actual_hash,
            publish_artifact_manifest=str(artifact_manifest),
            publish_rep=rep,
        )
        return runs

    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(tempfile.mkdtemp(prefix=f".{target.name}.", dir=target.parent))
    try:
        temporary_runs = temporary / "runs"
        temporary_runs.mkdir()
        shutil.copy2(source_verdict, temporary_runs / f"{entrant_id}.json")
        shutil.copytree(
            source_shots,
            temporary_runs / f"{entrant_id}-r{rep}" / "sb7-shots",
        )
        runs_hash = hash_tree(temporary_runs)
        artifact = {
            "schema_version": 1,
            "entrant": entrant_id,
            "score_attempt": attempt,
            "rep": rep,
            "created_at": utc_now(),
            "source_verdict": str(source_verdict),
            "source_verdict_sha256": sha256_file(source_verdict),
            "runs_sha256": runs_hash,
            "instrument_set_sha256": campaign.get("instrument_set_sha256"),
            "binary_sha256": campaign.get("binary_sha256"),
            "publisher_commit": campaign.get("publisher", {}).get("commit"),
            "publisher_instrument_set_sha256": campaign.get("publisher", {}).get(
                "instrument_set_sha256"
            ),
            "files": {
                str(path.relative_to(temporary_runs)): sha256_file(path)
                for path in sorted(temporary_runs.rglob("*"))
                if path.is_file()
            },
        }
        atomic_json(temporary / "artifact-manifest.json", artifact)
        os.replace(temporary, target)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)

    update_state(
        root,
        entrant_id,
        publish_stage=str(runs),
        publish_stage_sha256=runs_hash,
        publish_artifact_manifest=str(artifact_manifest),
        publish_rep=rep,
    )
    return runs


def run_publisher(
    root: Path, entrant_id: str, runs: Path, live: bool
) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    mismatch = publisher_mismatch(campaign)
    if mismatch:
        raise PublicationError(mismatch)
    mismatch = frozen_publisher_mismatch(campaign)
    if mismatch:
        raise PublicationError(mismatch)
    publisher = campaign["publisher"]
    repo = Path(str(publisher["repo"]))
    frozen_repo = Path(str(publisher["frozen"]["root"]))
    node = str(publisher["node"]["path"])
    cmd = [
        node,
        str(frozen_repo / str(publisher["script"])),
        "--runs",
        str(runs),
        "--manifest",
        str(frozen_repo / str(publisher["manifest"])),
        "--only",
        entrant_id,
    ]
    phase = "live" if live else "dry-run"
    if live:
        cmd.append("--live")
    env, redactions = publisher_environment(campaign, include_credentials=live)
    state = read_state(root, entrant_id)
    attempt = int(state["score_attempts"])
    log_path = (
        root / "publish" / entrant_id / f"attempt-{attempt}" / f"publisher-{phase}.log"
    )

    def started(proc: subprocess.Popen[Any]) -> None:
        update_state(
            root,
            entrant_id,
            publisher_pid=proc.pid,
            publisher_pgid=proc.pid,
            publisher_identity=process_identity(proc.pid),
            publisher_phase=phase,
            publisher_started_at=utc_now(),
        )

    result = run_logged_process(
        cmd,
        cwd=repo,
        env=env,
        log_path=log_path,
        timeout_seconds=float(publisher["process_timeout_seconds"]),
        redactions=redactions,
        on_started=started,
    )
    update_state(
        root,
        entrant_id,
        publisher_pid=None,
        publisher_pgid=None,
        publisher_identity=None,
        publisher_finished_at=utc_now(),
    )
    return result


def publisher_plan_from_log(log_path: Path, runs: Path) -> list[Dict[str, Any]]:
    if not log_path.is_file():
        raise PublicationError(f"publisher dry-run log is missing: {log_path}")
    pattern = re.compile(
        r'^\s*shot\s+(?P<name>\S+)\s+·\s+"(?P<caption>[^"]*)"\s+·\s+'
        r"(?P<file>.+)\s+\([0-9.]+KB\)$"
    )
    root = runs.resolve()
    plan: list[Dict[str, Any]] = []
    for raw in log_path.read_text(errors="replace").splitlines():
        match = pattern.match(raw)
        if not match:
            continue
        source = Path(match.group("file")).resolve()
        try:
            source.relative_to(root)
        except ValueError:
            raise PublicationError(
                f"publisher planned a screenshot outside the sealed stage: {source}"
            ) from None
        if not source.is_file():
            raise PublicationError(f"publisher planned screenshot is missing: {source}")
        plan.append(
            {
                "name": match.group("name"),
                "caption": match.group("caption"),
                "source": str(source),
                "sha1": sha1_file(source),
                "sha256": sha256_file(source),
            }
        )
    if not plan:
        raise PublicationError("publisher dry-run emitted no screenshot plan")
    if len({row["name"] for row in plan}) != len(plan):
        raise PublicationError("publisher dry-run repeated a screenshot plan name")
    return plan


def sanity_document(
    campaign: Mapping[str, Any], document_id: str
) -> Dict[str, Any] | None:
    publisher = campaign["publisher"]
    values = pinned_publisher_env_values(campaign)
    token = values.get("SANITY_WRITE_TOKEN", "")
    project_id = values.get("NEXT_PUBLIC_SANITY_PROJECT_ID", "")
    dataset = values.get("NEXT_PUBLIC_SANITY_DATASET", "production")
    if not token or not re.fullmatch(r"[a-z0-9-]+", project_id):
        raise PublicationError("Sanity receipt credentials are missing or malformed")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", dataset):
        raise PublicationError("Sanity receipt dataset is malformed")
    encoded_dataset = urllib.parse.quote(dataset, safe="")
    encoded_id = urllib.parse.quote(document_id, safe="")
    url = (
        f"https://{project_id}.api.sanity.io/v2025-02-19/data/doc/"
        f"{encoded_dataset}/{encoded_id}"
    )
    request = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "User-Agent": "goose-sb7-cloud-publisher/1",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            status = int(getattr(response, "status", response.getcode()))
            raw = response.read(10 * 1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        raise PublicationError(
            f"Sanity receipt read returned HTTP {error.code}"
        ) from None
    except Exception as error:
        raise PublicationError(
            f"Sanity receipt read failed: {type(error).__name__}"
        ) from None
    if status != 200 or len(raw) > 10 * 1024 * 1024:
        raise PublicationError(f"Sanity receipt read returned invalid HTTP {status}")
    try:
        result = json.loads(raw)
    except json.JSONDecodeError:
        raise PublicationError("Sanity receipt read returned invalid JSON") from None
    documents = result.get("documents") if isinstance(result, dict) else None
    if not isinstance(documents, list):
        raise PublicationError("Sanity receipt read omitted its documents array")
    matches = [
        row
        for row in documents
        if isinstance(row, dict) and row.get("_id") == document_id
    ]
    if len(matches) > 1:
        raise PublicationError(f"Sanity receipt read duplicated document {document_id}")
    return dict(matches[0]) if matches else None


def verdict_tier_mean(verdict: Mapping[str, Any], letter: str) -> float:
    tiers = verdict.get("tiers")
    if not isinstance(tiers, dict):
        raise PublicationError("scored verdict has no tier evidence")
    tier = tiers.get(letter)
    if isinstance(tier, (int, float)) and not isinstance(tier, bool):
        return float(tier)
    if isinstance(tier, dict) and isinstance(tier.get("mean"), (int, float)):
        return float(tier["mean"])
    raise PublicationError(f"scored verdict has no {letter} tier mean")


def same_number(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return False
    try:
        return abs(float(left) - float(right)) < 1e-12
    except (TypeError, ValueError):
        return False


def public_publication_identity(
    campaign: Mapping[str, Any], verdict: Mapping[str, Any]
) -> Dict[str, Any]:
    publisher = campaign.get("publisher")
    target = QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["target"]
    if not isinstance(publisher, dict) or any(
        publisher.get(field) != target[field]
        for field in ("commit", "instrument_set_sha256", "tracked_hashes")
    ):
        raise PublicationError(
            "publisher is not the exact frozen stable-board correction"
        )
    raw_scorer = verdict.get("scorer_version")
    raw_calibration = verdict.get("calibration")
    if (
        raw_scorer
        != QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION["raw_scorer_version"]
        or campaign.get("scorer_version") != raw_scorer
        or not isinstance(raw_calibration, str)
        or not re.search(r"uncalibrated|rc-grade", raw_calibration, re.IGNORECASE)
    ):
        raise PublicationError(
            "raw hermetic verdict does not match the frozen RC publication mapping"
        )
    return {
        "scorer_version": QUALIFICATION_ALLOWED_PUBLISHER_TRANSITION[
            "public_scorer_version"
        ],
        "calibration_absent": True,
    }


def raw_publication_identity_sha256(verdict: Mapping[str, Any]) -> str:
    identity = {
        "scorer_version": verdict.get("scorer_version"),
        "calibration": verdict.get("calibration"),
    }
    return sha256_bytes(
        json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    )


def rendered_publication_expected(
    campaign: Mapping[str, Any],
    entry: Mapping[str, str],
    verdict: Mapping[str, Any],
) -> Dict[str, Any]:
    public = public_publication_identity(campaign, verdict)
    return {
        "doc_id": entry["doc_id"],
        "label": entry["label"],
        "model": entry["model"],
        "score": float(verdict["score"]),
        **public,
    }


def remote_publication_receipt(
    campaign: Mapping[str, Any],
    entry: Mapping[str, str],
    verdict: Mapping[str, Any],
    screenshot_plan: list[Mapping[str, Any]],
) -> Dict[str, Any]:
    public = public_publication_identity(campaign, verdict)
    raw_identity_sha256 = raw_publication_identity_sha256(verdict)
    document = sanity_document(campaign, entry["doc_id"])
    if document is None:
        return {
            "checked_at": utc_now(),
            "doc_id": entry["doc_id"],
            "matched": False,
            "reasons": ["stable document does not exist"],
            "expected_public_identity": public,
            "raw_verdict_identity_sha256": raw_identity_sha256,
        }

    reasons: list[str] = []
    exact_fields = {
        "_id": entry["doc_id"],
        "_type": "benchmarkRun",
        "label": entry["label"],
        "model": entry["model"],
        "baseline": True,
        "scorerVersion": public["scorer_version"],
        "excellent": bool(verdict.get("excellent")),
    }
    for field, expected in exact_fields.items():
        if document.get(field) != expected:
            reasons.append(f"document field {field} differs")
    if "calibration" in document:
        reasons.append("document field calibration must be absent")
    notes = document.get("notes")
    if isinstance(notes, str) and re.search(
        r"sb-7\.0-rc|\bcalibration\b|\buncalibrated\b|\brc-grade\b",
        notes,
        re.IGNORECASE,
    ):
        reasons.append("document notes retain forbidden RC/calibration residue")

    numeric_fields = {
        "score": verdict.get("score"),
        "tierA": verdict_tier_mean(verdict, "A"),
        "tierB": verdict_tier_mean(verdict, "B"),
        "tierC": verdict_tier_mean(verdict, "C"),
        "tierD": verdict_tier_mean(verdict, "D"),
        "wallSecs": math.floor(float(verdict.get("agent", {}).get("secs", -1)) + 0.5),
    }
    if isinstance(verdict.get("inner"), (int, float)):
        numeric_fields["scoreInner"] = verdict["inner"]
    excellence = verdict.get("excellence")
    if isinstance(excellence, dict):
        numeric_fields["excellenceFraction"] = excellence.get("fraction")
        numeric_fields["excellenceEMean"] = excellence.get("e_mean")
    critical = verdict.get("critical")
    if isinstance(critical, dict):
        numeric_fields["criticalMultiplier"] = critical.get("multiplier")
        numeric_fields["criticalFloor"] = critical.get("floor")
        numeric_fields["preSeverityScore"] = critical.get("pre_severity_score")
    for field, expected in numeric_fields.items():
        if not same_number(document.get(field), expected):
            reasons.append(f"document numeric field {field} differs")

    expected_checks = []
    raw_checks = verdict.get("checks")
    if not isinstance(raw_checks, list):
        raise PublicationError("scored verdict has no checks array")
    for row in raw_checks:
        if not isinstance(row, dict):
            raise PublicationError("scored verdict contains a malformed check")
        expected_checks.append(
            {
                "check": str(row.get("check", ""))[:60],
                "tier": row.get("tier"),
                "score": row.get("score"),
                "detail": str(row.get("detail", ""))[:220],
            }
        )
    actual_checks = document.get("checksSummary")
    if not isinstance(actual_checks, list) or len(actual_checks) != len(
        expected_checks
    ):
        reasons.append("document checksSummary count differs")
    else:
        for index, (actual, expected) in enumerate(zip(actual_checks, expected_checks)):
            if not isinstance(actual, dict):
                reasons.append(f"document check {index} is malformed")
                continue
            if (
                actual.get("check") != expected["check"]
                or actual.get("tier") != expected["tier"]
                or not same_number(actual.get("score"), expected["score"])
                or actual.get("detail") != expected["detail"]
            ):
                reasons.append(f"document check {index} differs")

    if isinstance(excellence, dict):
        expected_gates = [
            {
                "name": str(row.get("name")),
                "ok": bool(row.get("ok")),
                **(
                    {}
                    if row.get("value") is None
                    else {"value": float(row.get("value"))}
                ),
            }
            for row in excellence.get("conditions", [])
            if isinstance(row, dict)
        ]
        actual_gates = document.get("gateConditions")
        if not isinstance(actual_gates, list) or len(actual_gates) != len(
            expected_gates
        ):
            reasons.append("document gateConditions count differs")
        else:
            for index, (actual, expected) in enumerate(
                zip(actual_gates, expected_gates)
            ):
                if not isinstance(actual, dict):
                    reasons.append(f"document gate condition {index} is malformed")
                    continue
                if (
                    actual.get("name") != expected["name"]
                    or actual.get("ok") != expected["ok"]
                ):
                    reasons.append(f"document gate condition {index} differs")
                if "value" in expected and not same_number(
                    actual.get("value"), expected["value"]
                ):
                    reasons.append(f"document gate condition {index} value differs")

    if isinstance(critical, dict):
        expected_critical = [
            {
                "check": str(row.get("check")),
                "score": row.get("score"),
                "factor": row.get("factor"),
                "why": str(row.get("why")),
            }
            for row in critical.get("rows", [])
            if isinstance(row, dict)
        ]
        actual_critical = document.get("criticalRows")
        if not isinstance(actual_critical, list) or len(actual_critical) != len(
            expected_critical
        ):
            reasons.append("document criticalRows count differs")
        else:
            for index, (actual, expected) in enumerate(
                zip(actual_critical, expected_critical)
            ):
                if not isinstance(actual, dict):
                    reasons.append(f"document critical row {index} is malformed")
                    continue
                if (
                    actual.get("check") != expected["check"]
                    or not same_number(actual.get("score"), expected["score"])
                    or not same_number(actual.get("factor"), expected["factor"])
                    or actual.get("why") != expected["why"]
                ):
                    reasons.append(f"document critical row {index} differs")

    screenshots = document.get("screenshots")
    if not isinstance(screenshots, list) or len(screenshots) != len(screenshot_plan):
        reasons.append("document screenshot count differs")
    else:
        for index, (actual, planned) in enumerate(zip(screenshots, screenshot_plan)):
            asset = actual.get("asset") if isinstance(actual, dict) else None
            asset_ref = asset.get("_ref") if isinstance(asset, dict) else None
            if not isinstance(asset_ref, str):
                reasons.append(f"document screenshot {index} has no asset reference")
                continue
            asset_doc = sanity_document(campaign, asset_ref)
            asset_sha1 = str((asset_doc or {}).get("sha1hash", "")).lower()
            expected_sha1 = str(planned["sha1"]).lower()
            asset_id_matches = asset_ref.startswith(f"image-{expected_sha1}-")
            if asset_sha1 != expected_sha1 and not asset_id_matches:
                reasons.append(f"document screenshot {index} bytes differ")
            if actual.get("caption") != planned.get("caption"):
                reasons.append(f"document screenshot {index} caption differs")

    canonical = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    return {
        "checked_at": utc_now(),
        "doc_id": entry["doc_id"],
        "matched": not reasons,
        "reasons": reasons[:100],
        "document_sha256": sha256_bytes(canonical),
        "revision": document.get("_rev"),
        "updated_at": document.get("_updatedAt"),
        "checks": len(actual_checks) if isinstance(actual_checks, list) else None,
        "screenshots": len(screenshots) if isinstance(screenshots, list) else None,
        "expected_public_identity": public,
        "raw_verdict_identity_sha256": raw_identity_sha256,
    }


def revalidate_publication(
    campaign: Mapping[str, Any], entry: Mapping[str, str]
) -> Dict[str, Any]:
    publisher = campaign["publisher"]
    values = pinned_publisher_env_values(campaign)
    token = values.get("SANITY_WRITE_TOKEN", "")
    if not token:
        raise PublicationError("SANITY_WRITE_TOKEN is missing for revalidation")
    run_path = f"/agentic-benchmarks/run/{entry['doc_id']}"
    payload = json.dumps({"runIds": [entry["doc_id"]]}).encode()
    request = urllib.request.Request(
        str(publisher["revalidate_endpoint"]),
        data=payload,
        headers={
            "Content-Type": "application/json",
            "x-reval-key": sha256_bytes(token.encode()),
            "User-Agent": "goose-sb7-cloud-publisher/1",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            status = int(getattr(response, "status", response.getcode()))
            raw = response.read(1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        raise PublicationError(
            f"benchmark revalidation returned HTTP {error.code}"
        ) from None
    except Exception as error:
        raise PublicationError(
            f"benchmark revalidation failed: {type(error).__name__}"
        ) from None
    if status != 200 or len(raw) > 1024 * 1024:
        raise PublicationError(f"benchmark revalidation returned invalid HTTP {status}")
    try:
        result = json.loads(raw)
    except json.JSONDecodeError:
        raise PublicationError("benchmark revalidation returned invalid JSON") from None
    expected_paths = {"/agentic-benchmarks", run_path}
    returned = result.get("revalidated") if isinstance(result, dict) else None
    if not isinstance(returned, list) or not expected_paths.issubset(set(returned)):
        raise PublicationError(
            "benchmark revalidation omitted the board or stable run path"
        )
    return {
        "at": utc_now(),
        "status": status,
        "endpoint": str(publisher["revalidate_endpoint"]),
        "paths": sorted(expected_paths),
        "response_sha256": sha256_bytes(raw),
    }


class RenderedEvidenceParser(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.visible: list[str] = []
        self.json_ld: list[str] = []
        self._ignored_depth = 0
        self._json_ld_depth = 0
        self._json_ld_buffer: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        lowered = tag.lower()
        if lowered in {"script", "style"}:
            self._ignored_depth += 1
        if lowered == "script" and dict(attrs).get("type") == "application/ld+json":
            self._json_ld_depth = self._ignored_depth
            self._json_ld_buffer = []

    def handle_endtag(self, tag: str) -> None:
        lowered = tag.lower()
        if lowered == "script" and self._json_ld_depth:
            self.json_ld.append("".join(self._json_ld_buffer))
            self._json_ld_depth = 0
            self._json_ld_buffer = []
        if lowered in {"script", "style"} and self._ignored_depth:
            self._ignored_depth -= 1

    def handle_data(self, data: str) -> None:
        if self._json_ld_depth:
            self._json_ld_buffer.append(data)
        elif not self._ignored_depth:
            self.visible.append(data)


def json_ld_objects(parser: RenderedEvidenceParser) -> Iterator[Mapping[str, Any]]:
    def walk(value: Any) -> Iterator[Mapping[str, Any]]:
        if isinstance(value, dict):
            yield value
            for nested in value.values():
                yield from walk(nested)
        elif isinstance(value, list):
            for nested in value:
                yield from walk(nested)

    for raw in parser.json_ld:
        try:
            value = json.loads(raw)
        except json.JSONDecodeError:
            continue
        yield from walk(value)


def rendered_publication_matches(
    campaign: Mapping[str, Any],
    board_html: str,
    run_html: str,
    website_base_url: str,
    entry: Mapping[str, str],
    verdict: Mapping[str, Any],
) -> tuple[bool, Dict[str, Any]]:
    score = float(verdict["score"])
    score_text = f"{score:.4f}"
    public = public_publication_identity(campaign, verdict)
    scorer = str(public["scorer_version"])
    run_url = f"{website_base_url.rstrip('/')}/agentic-benchmarks/run/{entry['doc_id']}"

    board_parser = RenderedEvidenceParser()
    board_parser.feed(board_html)
    board_name = f"{entry['label']} — {score_text}"
    board_item = any(
        item.get("@type") == "ListItem"
        and item.get("url") == run_url
        and item.get("name") == board_name
        for item in json_ld_objects(board_parser)
    )

    run_parser = RenderedEvidenceParser()
    run_parser.feed(run_html)
    run_text = " ".join(" ".join(run_parser.visible).split())
    expected_visible = [
        f"{entry['label']} — {score_text} on {scorer}",
        entry["model"],
        f"scorer {scorer}",
    ]
    missing_visible = [value for value in expected_visible if value not in run_text]
    forbidden_residue = []
    raw_scorer = str(verdict["scorer_version"])
    if raw_scorer != scorer and re.search(re.escape(raw_scorer), run_html, re.IGNORECASE):
        forbidden_residue.append(raw_scorer)
    if re.search(
        r"\bcalibration\b|\buncalibrated\b|\brc-grade\b",
        run_html,
        re.IGNORECASE,
    ):
        forbidden_residue.append("calibration disclosure")

    dataset = False
    for item in json_ld_objects(run_parser):
        if item.get("@type") != "Dataset" or item.get("url") != run_url:
            continue
        measured = item.get("variableMeasured")
        if not isinstance(measured, list):
            continue
        values = [row.get("value") for row in measured if isinstance(row, dict)]
        try:
            score_present = any(abs(float(value) - score) < 1e-12 for value in values)
        except (TypeError, ValueError):
            score_present = False
        if scorer in str(item.get("name", "")) and score_present:
            dataset = True
            break

    reasons = []
    if not board_item:
        reasons.append("board JSON-LD lacks the exact stable run URL, label and score")
    if missing_visible:
        reasons.append(
            f"run page lacks exact visible fields: {', '.join(missing_visible)}"
        )
    if forbidden_residue:
        reasons.append(
            "run page retains forbidden RC/calibration residue: "
            + ", ".join(forbidden_residue)
        )
    if not dataset:
        reasons.append("run Dataset JSON-LD lacks the exact URL, scorer and score")
    return not reasons, {
        "board_item_exact": board_item,
        "run_visible_exact": not missing_visible,
        "run_dataset_exact": dataset,
        "run_public_identity_exact": not forbidden_residue,
        "reasons": reasons,
    }


def fetch_rendered_page(url: str) -> tuple[int, str, Dict[str, str]]:
    request = urllib.request.Request(
        url,
        headers={
            "Cache-Control": "no-cache",
            "User-Agent": "goose-sb7-cloud-publisher/1",
        },
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        status = int(getattr(response, "status", response.getcode()))
        raw = response.read(10 * 1024 * 1024 + 1)
        if len(raw) > 10 * 1024 * 1024:
            raise PublicationError(f"rendered page is unexpectedly large: {url}")
        content_type = response.headers.get_content_charset() or "utf-8"
        headers = {
            key: response.headers.get(key, "")
            for key in ("x-cache", "x-nextjs-cache", "age")
            if response.headers.get(key)
        }
    return status, raw.decode(content_type, errors="replace"), headers


def verify_rendered_publication(
    campaign: Mapping[str, Any],
    entry: Mapping[str, str],
    verdict: Mapping[str, Any],
) -> Dict[str, Any]:
    publisher = campaign["publisher"]
    base_url = str(publisher["website_base_url"]).rstrip("/")
    board_url = f"{base_url}/agentic-benchmarks"
    run_url = f"{board_url}/run/{entry['doc_id']}"
    timeout_seconds = float(publisher["verify_timeout_seconds"])
    interval_seconds = float(publisher["verify_interval_seconds"])
    deadline = time.monotonic() + timeout_seconds
    attempts = 0
    last: Dict[str, Any] = {}
    while True:
        attempts += 1
        try:
            board_status, board_html, board_headers = fetch_rendered_page(board_url)
            run_status, run_html, run_headers = fetch_rendered_page(run_url)
            matched, checks = rendered_publication_matches(
                campaign, board_html, run_html, base_url, entry, verdict
            )
            last = {
                "attempt": attempts,
                "board_status": board_status,
                "run_status": run_status,
                "board_html_sha256": sha256_bytes(board_html.encode()),
                "run_html_sha256": sha256_bytes(run_html.encode()),
                "board_headers": board_headers,
                "run_headers": run_headers,
                **checks,
            }
            if board_status == 200 and run_status == 200 and matched:
                return {
                    **last,
                    "verified_at": utc_now(),
                    "board_url": board_url,
                    "run_url": run_url,
                    "expected": rendered_publication_expected(
                        campaign, entry, verdict
                    ),
                    "raw_verdict_identity_sha256": (
                        raw_publication_identity_sha256(verdict)
                    ),
                }
        except Exception as error:
            last = {
                "attempt": attempts,
                "error": f"{type(error).__name__}: {str(error)[:300]}",
            }
        now = time.monotonic()
        if now >= deadline:
            raise PublicationError(
                f"rendered verification timed out after {attempts} attempt(s): "
                f"{json.dumps(last, sort_keys=True)}"
            )
        time.sleep(min(interval_seconds, max(0.0, deadline - now)))


def publication_failed(
    root: Path, entrant_id: str, stage: str, error: BaseException
) -> None:
    campaign = load_json(campaign_file(root))
    redactions: list[str] = []
    try:
        _, redactions = publisher_environment(campaign)
    except (OSError, SystemExit, PublicationError):
        pass
    safe = redact_text(str(error), redactions)[:1000]
    update_state(
        root,
        entrant_id,
        status="PUBLISH_FAILED",
        publication_failure_stage=stage,
        publisher_pid=None,
        publisher_pgid=None,
        publisher_identity=None,
        failure=f"publication {stage} failed: {safe}",
    )


def publish_one(root: Path, entrant_id: str) -> bool:
    state = read_state(root, entrant_id)
    if state["status"] == "PUBLISHED":
        return True
    if state["status"] not in POST_BUILD_STATES:
        return False
    campaign = load_json(campaign_file(root))
    if campaign.get("status") == "STOPPED" or (root / SUPERSESSION_RECEIPT).exists():
        return False
    lineage_problem = lineage_failure(root)
    if lineage_problem:
        update_state(
            root,
            entrant_id,
            status="PUBLISH_FAILED",
            failure=f"campaign lineage refused publication: {lineage_problem}",
        )
        return False
    smoke_problem = supersession_smoke_gate_failure(root)
    if smoke_problem:
        update_state(root, entrant_id, status="PUBLISH_FAILED", failure=smoke_problem)
        return False
    isolation_problem = listener_isolation_failure(
        campaign, state, state, smoke=False
    )
    if isolation_problem:
        update_state(
            root, entrant_id, status="INCOMPLETE", failure=isolation_problem
        )
        return False
    secret_hits = persisted_entrant_secret_hits(root, campaign, entrant_id)
    if secret_hits:
        update_state(
            root,
            entrant_id,
            status="INCOMPLETE",
            secret_scan_hits=secret_hits,
            failure="provider credential appeared in benchmark-controlled artifacts",
        )
        return False
    entry = publish_entry(campaign, entrant_id)
    stage = "stage"
    try:
        runs = publication_stage(root, entrant_id)
        state = read_state(root, entrant_id)
        update_state(
            root,
            entrant_id,
            publication_attempts=int(state.get("publication_attempts", 0)) + 1,
        )
        screenshot_plan = state.get("publisher_plan")
        if not isinstance(screenshot_plan, list) or not screenshot_plan:
            stage = "dry-run-validation"
            update_state(
                root,
                entrant_id,
                status="PUBLISH_VALIDATING",
                failure=None,
            )
            dry_run = run_publisher(root, entrant_id, runs, live=False)
            if dry_run["exit_code"] != 0 or dry_run["timed_out"]:
                update_state(root, entrant_id, publisher_dry_run=dry_run)
                raise PublicationError(
                    "pinned publisher dry-run validation did not complete successfully"
                )
            screenshot_plan = publisher_plan_from_log(Path(dry_run["log"]), runs)
            update_state(
                root,
                entrant_id,
                status="PUBLISH_VALIDATED",
                publisher_dry_run=dry_run,
                publisher_plan=screenshot_plan,
            )

        stage = "pre-write-receipt"
        pre_write_receipt = remote_publication_receipt(
            campaign, entry, load_json(runs / f"{entrant_id}.json"), screenshot_plan
        )
        update_state(
            root,
            entrant_id,
            publisher_pre_write_receipt=pre_write_receipt,
        )
        state = read_state(root, entrant_id)
        if pre_write_receipt["matched"]:
            update_state(
                root,
                entrant_id,
                status="PUBLISHED_UNVERIFIED",
                publisher_live_succeeded_at=(
                    state.get("publisher_live_succeeded_at") or utc_now()
                ),
                publisher_remote_receipt=pre_write_receipt,
                publisher_write_adopted=True,
            )
        elif state.get("publisher_live_succeeded_at"):
            raise PublicationError(
                "stable Sanity document diverged after a proven matching live receipt"
            )
        else:
            stage = "live-write"
            require_lineage(root)
            secret_hits = persisted_entrant_secret_hits(root, campaign, entrant_id)
            if secret_hits:
                raise PublicationError(
                    "provider credential appeared before the live publication boundary"
                )
            update_state(root, entrant_id, status="PUBLISHING")
            live = run_publisher(root, entrant_id, runs, live=True)
            live_succeeded_at = None
            if live["exit_code"] == 0 and not live["timed_out"]:
                live_succeeded_at = utc_now()
            update_state(
                root,
                entrant_id,
                publisher_live=live,
                **(
                    {"publisher_live_succeeded_at": live_succeeded_at}
                    if live_succeeded_at
                    else {}
                ),
            )
            stage = "post-write-receipt"
            post_write_receipt = remote_publication_receipt(
                campaign,
                entry,
                load_json(runs / f"{entrant_id}.json"),
                screenshot_plan,
            )
            update_state(
                root,
                entrant_id,
                publisher_post_write_receipt=post_write_receipt,
            )
            if not post_write_receipt["matched"]:
                if live["exit_code"] != 0 or live["timed_out"]:
                    raise PublicationError(
                        "publisher exited ambiguously and the stable document does not "
                        "match the sealed scorer evidence"
                    )
                raise PublicationError(
                    "publisher exited successfully but the stable document does not "
                    "match the sealed scorer evidence"
                )
            update_state(
                root,
                entrant_id,
                status="PUBLISHED_UNVERIFIED",
                publisher_live_succeeded_at=live_succeeded_at or utc_now(),
                publisher_remote_receipt=post_write_receipt,
                publisher_write_adopted=(live["exit_code"] != 0 or live["timed_out"]),
            )

        stage = "revalidation"
        update_state(root, entrant_id, status="REVALIDATING")
        revalidation = revalidate_publication(campaign, entry)
        update_state(
            root,
            entrant_id,
            status="REVALIDATED",
            revalidation=revalidation,
        )

        stage = "rendered-verification"
        update_state(root, entrant_id, status="VERIFYING_RENDERED")
        verdict = load_json(runs / f"{entrant_id}.json")
        rendered = verify_rendered_publication(campaign, entry, verdict)
        update_state(
            root,
            entrant_id,
            status="PUBLISHED",
            published_at=utc_now(),
            published_url=rendered["run_url"],
            rendered_verification=rendered,
            publication_failure_stage=None,
            failure=None,
        )
        return True
    except (Exception, SystemExit) as error:
        publication_failed(root, entrant_id, stage, error)
        return False


def verdict_failure(
    result: Mapping[str, Any], campaign: Mapping[str, Any]
) -> str | None:
    score = result.get("score")
    if (
        not isinstance(score, (int, float))
        or isinstance(score, bool)
        or not math.isfinite(float(score))
        or not 0 <= float(score) <= 1
    ):
        return "hermetic verdict has no finite score from 0 to 1"
    if result.get("scorer_version") != campaign.get("scorer_version"):
        return "hermetic verdict scorer version differs from the frozen campaign"
    calibration = result.get("calibration")
    if not isinstance(calibration, str) or not calibration.strip():
        return "hermetic verdict has no calibration truth"
    if str(result["scorer_version"]).endswith("-rc") and not re.search(
        r"uncalibrated|rc-grade", calibration, re.IGNORECASE
    ):
        return "hermetic rc verdict does not disclose uncalibrated/rc-grade status"
    publisher = campaign.get("publisher")
    expected_checks = (
        publisher.get("expected_checks") if isinstance(publisher, dict) else None
    )
    checks = result.get("checks")
    if not isinstance(checks, list) or len(checks) != expected_checks:
        return (
            f"hermetic verdict check count differs from frozen publisher contract: "
            f"expected {expected_checks}, found "
            f"{len(checks) if isinstance(checks, list) else '<missing>'}"
        )
    return None


def score_one(root: Path, entrant_id: str) -> bool:
    state = read_state(root, entrant_id)
    if state["status"] in POST_BUILD_STATES - {"SCORING", "SCORE_FAILED"}:
        return True
    if state["status"] not in {"BUILD_COMPLETE", "SCORE_FAILED"}:
        return False
    campaign = load_json(campaign_file(root))
    if campaign.get("status") == "STOPPED" or (root / SUPERSESSION_RECEIPT).exists():
        return False
    lineage_problem = lineage_failure(root)
    if lineage_problem:
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            failure=f"campaign lineage refused scoring: {lineage_problem}",
        )
        return False
    smoke_problem = supersession_smoke_gate_failure(root)
    if smoke_problem:
        update_state(root, entrant_id, status="SCORE_FAILED", failure=smoke_problem)
        return False
    isolation_problem = listener_isolation_failure(
        campaign, state, state, smoke=False
    )
    if isolation_problem:
        update_state(
            root, entrant_id, status="INCOMPLETE", failure=isolation_problem
        )
        return False
    secret_hits = persisted_entrant_secret_hits(root, campaign, entrant_id)
    if secret_hits:
        update_state(
            root,
            entrant_id,
            status="INCOMPLETE",
            secret_scan_hits=secret_hits,
            failure="provider credential appeared in benchmark-controlled artifacts",
        )
        return False
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        update_state(root, entrant_id, status="SCORE_FAILED", failure=mismatch)
        return False
    raw = Path(str(state["tree"]))
    if hash_tree(raw) != state.get("raw_tree_sha256"):
        update_state(
            root,
            entrant_id,
            status="INCOMPLETE",
            failure="raw tree changed before scoring",
        )
        return False
    score_attempt = next_score_attempt(root, entrant_id, state)
    score_tree = clone_for_score(root, entrant_id, score_attempt)
    score_dir = score_tree.parent
    verdict = score_dir / "verdict.json"
    log_path = score_dir / "score.log"
    cmd = [
        sys.executable,
        str(campaign_instrument_path(campaign, "evals/swarm-bench/bench/score_sb7.py")),
        "--tree",
        str(score_tree),
        "--port",
        str(state["vendor_port"]),
        "--seed",
        str(state["fixture_seed"]),
        "--json-out",
        str(verdict),
    ]
    update_state(
        root,
        entrant_id,
        status="SCORING",
        score_started_at=utc_now(),
        score_attempts=score_attempt,
    )
    proc: subprocess.Popen[Any] | None = None
    try:
        with log_path.open("w") as log:
            proc = subprocess.Popen(
                cmd,
                cwd=Path(str(campaign["instrument_root"])),
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                close_fds=True,
            )
            update_state(
                root,
                entrant_id,
                score_pid=proc.pid,
                score_pgid=proc.pid,
                score_identity=process_identity(proc.pid),
            )
            exit_code = proc.wait()
    except BaseException:
        if proc is not None:
            stop_recorded_group(proc.pid, proc.pid, process_identity(proc.pid))
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            failure="hermetic scorer supervision failed; immutable attempt retained",
            score_pid=None,
            score_pgid=None,
            score_identity=None,
        )
        raise
    if exit_code != 0 or not verdict.is_file():
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            score_exit_code=exit_code,
            failure="hermetic scorer failed; raw build remains sealed",
            score_pid=None,
            score_pgid=None,
            score_identity=None,
        )
        return False
    try:
        result = load_json(verdict)
    except (OSError, json.JSONDecodeError, SystemExit) as error:
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            score_exit_code=exit_code,
            failure=f"hermetic scorer emitted an unreadable verdict: {error}",
        )
        return False
    invalid = verdict_failure(result, campaign)
    if invalid:
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            score_exit_code=exit_code,
            failure=invalid,
        )
        return False
    update_state(
        root,
        entrant_id,
        status="SCORED",
        score_exit_code=exit_code,
        score_finished_at=utc_now(),
        score=result.get("score"),
        scorer_version=result.get("scorer_version"),
        calibration=result.get("calibration"),
        calibrated=result.get("calibrated"),
        verdict=str(verdict),
        score_pid=None,
        score_pgid=None,
        score_identity=None,
    )
    return True


def score_all(root: Path, row_ids: list[str], finalize_campaign: bool = True) -> bool:
    require_lineage(root)
    recover_interrupted_scoring(root)
    manager_state(root, status="SCORING", active_entrant=None, failure=None)
    update_campaign(root, status="SCORING", score_started_at=utc_now())
    for entrant_id in row_ids:
        if not score_one(root, entrant_id):
            failure = f"scoring failed: {entrant_id}"
            manager_state(
                root,
                status="ATTENTION",
                failure=failure,
                active_entrant=entrant_id,
            )
            update_campaign(root, status="ATTENTION", failure=failure)
            return False
        manager_state(root, status="PUBLISHING", active_entrant=entrant_id)
        if not publish_one(root, entrant_id):
            failure = f"publication failed: {entrant_id}"
            manager_state(
                root,
                status="ATTENTION",
                failure=failure,
                active_entrant=entrant_id,
            )
            update_campaign(root, status="ATTENTION", failure=failure)
            return False
    if finalize_campaign:
        manager_state(
            root,
            status="PUBLISHED",
            active_entrant=None,
            finished_at=utc_now(),
            failure=None,
        )
        update_campaign(
            root,
            status="PUBLISHED",
            finished_at=utc_now(),
            failure=None,
        )
    return True


def manage(root: Path) -> int:
    with exclusive_claim(root / "locks/manager-run.claim") as claimed:
        if not claimed:
            return 0
        return manage_claimed(root)


def manage_claimed(root: Path) -> int:
    campaign = load_json(campaign_file(root))
    if campaign.get("status") == "STOPPED" or (root / SUPERSESSION_RECEIPT).exists():
        return 2
    recover_interrupted_scoring(root)
    lineage_problem = lineage_failure(root)
    if lineage_problem:
        manager_state(root, status="ATTENTION", failure=lineage_problem)
        update_campaign(root, status="ATTENTION", failure=lineage_problem)
        return 2
    require_smoke_proofs(root)
    recover_interrupted_scoring(root)
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    row_ids = [str(row["id"]) for row in entrants(manifest)]
    manager_state(
        root,
        status="RUNNING",
        pid=os.getpid(),
        pgid=os.getpgrp(),
        started_at=utc_now(),
    )
    update_campaign(root, status="RUNNING", started_at=utc_now())
    supervisors: dict[str, subprocess.Popen[Any]] = {}
    for entrant_id in row_ids:
        state = read_state(root, entrant_id)
        if state["status"] in RETRYABLE_BUILD_STATES:
            supervisors[entrant_id] = launch_supervisor(root, entrant_id)
    builds_ok = wait_for_builds(root, row_ids, supervisors)
    if builds_ok:
        update_campaign(root, status="BUILD_COMPLETE", build_finished_at=utc_now())
    completed_ids = [
        entrant_id
        for entrant_id in row_ids
        if read_state(root, entrant_id)["status"] in BUILD_SUCCESS_STATES
    ]
    if completed_ids and not score_all(
        root, completed_ids, finalize_campaign=builds_ok
    ):
        return 1
    if not builds_ok:
        failed = [
            entrant_id
            for entrant_id in row_ids
            if read_state(root, entrant_id)["status"] not in BUILD_SUCCESS_STATES
        ]
        manager_state(
            root,
            status="ATTENTION",
            failure=f"builds did not complete: {', '.join(failed)}",
            active_entrant=None,
        )
        update_campaign(root, status="ATTENTION")
        return 1
    return 0


def start(root: Path) -> int:
    with exclusive_claim(root / "locks/manager-launch.claim", blocking=True) as claimed:
        if not claimed:
            raise SystemExit("cannot claim cloud benchmark manager launch")
        campaign = load_json(campaign_file(root))
        require_lineage(root)
        if campaign["status"] not in RESTARTABLE_CAMPAIGN_STATES:
            raise SystemExit(f"campaign cannot start from {campaign['status']}")
        require_smoke_proofs(root)
        current = load_json(root / "manager.json")
        if current.get("pid") and process_alive(
            current["pid"], current.get("identity")
        ):
            raise SystemExit(f"manager is already running as pid {current['pid']}")
        recover_dead_manager(root)
        proc = launch_detached(
            [
                sys.executable,
                str(campaign["coordinator"]),
                "_manage",
                "--root",
                str(root),
            ],
            root / "manager.log",
        )
        manager_state(
            root,
            status="STARTING",
            pid=proc.pid,
            pgid=proc.pid,
            identity=process_identity(proc.pid),
            launched_at=utc_now(),
        )
    print(f"started cloud SB7 manager pid={proc.pid} root={root}")
    return 0


def stop_runtime_groups_for_attention(root: Path) -> list[str]:
    failures: list[str] = []
    manager = load_json(root / "manager.json")
    if manager.get("pgid") and not stop_recorded_group(
        manager.get("pid"), manager.get("pgid"), manager.get("identity")
    ):
        failures.append(f"manager-pgid={manager.get('pgid')}")
    for state in status_rows(root):
        if state.get("supervisor_pgid") and not stop_recorded_group(
            state.get("supervisor_pid"),
            state.get("supervisor_pgid"),
            state.get("supervisor_identity"),
        ):
            failures.append(
                f"{state.get('entrant')}:supervisor-pgid={state.get('supervisor_pgid')}"
            )
        if state.get("publisher_pgid") and not stop_recorded_group(
            state.get("publisher_pid"),
            state.get("publisher_pgid"),
            state.get("publisher_identity"),
        ):
            failures.append(
                f"{state.get('entrant')}:publisher-pgid={state.get('publisher_pgid')}"
            )
        if state.get("score_pgid") and not stop_recorded_group(
            state.get("score_pid"),
            state.get("score_pgid"),
            state.get("score_identity"),
        ):
            failures.append(
                f"{state.get('entrant')}:score-pgid={state.get('score_pgid')}"
            )
    return failures


def published_campaign_mismatch(root: Path) -> str | None:
    campaign = load_json(campaign_file(root))
    manager = load_json(root / "manager.json")
    if campaign.get("status") != "PUBLISHED" or manager.get("status") != "PUBLISHED":
        return "campaign and manager have not both committed PUBLISHED"
    try:
        require_smoke_proofs(root)
    except SystemExit as error:
        return f"published campaign smoke proof failed: {error}"
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    rows = entrants(manifest)
    if len(rows) != 5:
        return "published campaign does not contain exactly five entrants"
    for row in rows:
        entrant_id = str(row["id"])
        state = read_state(root, entrant_id)
        if state.get("status") != "PUBLISHED":
            return f"{entrant_id} is {state.get('status')}, not PUBLISHED"
        attempt = int(state.get("score_attempts", 0))
        verdict_path = Path(str(state.get("verdict", "")))
        expected_verdict = (
            root / "scores" / entrant_id / f"attempt-{attempt}" / "verdict.json"
        )
        if (
            attempt <= 0
            or verdict_path != expected_verdict
            or verdict_path.is_symlink()
            or not verdict_path.is_file()
        ):
            return f"{entrant_id} sealed hermetic verdict is missing"
        try:
            verdict = load_json(verdict_path)
        except (OSError, json.JSONDecodeError, SystemExit) as error:
            return f"{entrant_id} hermetic verdict cannot be read: {error}"
        invalid = verdict_failure(verdict, campaign)
        if invalid:
            return f"{entrant_id} hermetic verdict is invalid: {invalid}"
        if not same_number(state.get("score"), verdict.get("score")):
            return f"{entrant_id} persisted score differs from its hermetic verdict"
        stage = Path(str(state.get("publish_stage", "")))
        expected_stage = root / "publish" / entrant_id / f"attempt-{attempt}" / "runs"
        artifact_path = Path(str(state.get("publish_artifact_manifest", "")))
        if (
            stage != expected_stage
            or stage.is_symlink()
            or not stage.is_dir()
            or hash_tree(stage) != state.get("publish_stage_sha256")
            or artifact_path != expected_stage.parent / "artifact-manifest.json"
            or artifact_path.is_symlink()
            or not artifact_path.is_file()
        ):
            return f"{entrant_id} sealed publication stage is missing or changed"
        artifact = load_json(artifact_path)
        if (
            artifact.get("entrant") != entrant_id
            or artifact.get("score_attempt") != attempt
            or artifact.get("source_verdict_sha256") != sha256_file(verdict_path)
            or artifact.get("runs_sha256") != state.get("publish_stage_sha256")
        ):
            return f"{entrant_id} publication artifact manifest differs"
        receipt = state.get("publisher_remote_receipt")
        try:
            public_identity = public_publication_identity(campaign, verdict)
        except PublicationError as error:
            return f"{entrant_id} public publication identity is invalid: {error}"
        if (
            not isinstance(receipt, dict)
            or receipt.get("matched") is not True
            or receipt.get("expected_public_identity") != public_identity
            or receipt.get("raw_verdict_identity_sha256")
            != raw_publication_identity_sha256(verdict)
        ):
            return f"{entrant_id} has no matching stable-document receipt"
        revalidation = state.get("revalidation")
        entry = publish_entry(campaign, entrant_id)
        expected_run_path = f"/agentic-benchmarks/run/{entry['doc_id']}"
        if (
            not isinstance(revalidation, dict)
            or revalidation.get("status") != 200
            or not {"/agentic-benchmarks", expected_run_path}.issubset(
                set(revalidation.get("paths", []))
            )
        ):
            return f"{entrant_id} has no complete revalidation receipt"
        rendered = state.get("rendered_verification")
        expected_rendered = rendered_publication_expected(
            campaign, entry, verdict
        )
        if (
            not isinstance(rendered, dict)
            or rendered.get("board_status") != 200
            or rendered.get("run_status") != 200
            or rendered.get("board_item_exact") is not True
            or rendered.get("run_visible_exact") is not True
            or rendered.get("run_dataset_exact") is not True
            or rendered.get("run_public_identity_exact") is not True
            or rendered.get("expected") != expected_rendered
            or rendered.get("raw_verdict_identity_sha256")
            != raw_publication_identity_sha256(verdict)
            or state.get("published_url") != rendered.get("run_url")
        ):
            return f"{entrant_id} rendered board/run verification is incomplete"
    return None


def monitor_attention(root: Path, failure: str) -> tuple[bool, int]:
    cleanup_failures = stop_runtime_groups_for_attention(root)
    if cleanup_failures:
        failure = failure + "; owned groups survived: " + ", ".join(cleanup_failures)
    monitor_state(
        root,
        status="ATTENTION",
        failure=failure[:4000],
        exit_code=1,
        finished_at=utc_now(),
    )
    manager_state(root, status="ATTENTION", failure=failure[:4000])
    update_campaign(root, status="ATTENTION", failure=failure[:4000])
    return True, 1


def monitor_tick(root: Path) -> tuple[bool, int]:
    campaign = load_json(campaign_file(root))
    manager = load_json(root / "manager.json")
    if campaign.get("status") == "PUBLISHED" and manager.get("status") == "PUBLISHED":
        mismatch = published_campaign_mismatch(root)
        if mismatch:
            return monitor_attention(root, mismatch)
        monitor_state(
            root,
            status="PUBLISHED",
            failure=None,
            exit_code=0,
            finished_at=utc_now(),
        )
        return True, 0
    if campaign.get("status") == "STOPPED" or manager.get("status") == "STOPPED":
        monitor_state(
            root,
            status="STOPPED",
            failure=None,
            exit_code=2,
            finished_at=utc_now(),
        )
        return True, 2
    if campaign.get("status") == "ATTENTION" or manager.get("status") == "ATTENTION":
        return monitor_attention(
            root,
            str(
                campaign.get("failure")
                or manager.get("failure")
                or "campaign needs attention"
            ),
        )
    try:
        require_smoke_proofs(root)
    except SystemExit as error:
        return monitor_attention(root, f"smoke proof gate failed: {error}")
    if process_alive(manager.get("pid"), manager.get("identity")):
        monitor_state(
            root,
            status="RUNNING",
            manager_pid=manager.get("pid"),
            manager_identity=manager.get("identity"),
            manager_alive=True,
            failure=None,
        )
        return False, 0
    mismatch = manager_restart_mismatch(root)
    if mismatch:
        return monitor_attention(root, mismatch)
    try:
        recover_dead_manager(root)
        start(root)
    except SystemExit as error:
        return monitor_attention(root, f"manager recovery failed: {error}")
    restarted = load_json(root / "manager.json")
    current_monitor = read_monitor_state(root)
    monitor_state(
        root,
        status="RUNNING",
        manager_pid=restarted.get("pid"),
        manager_identity=restarted.get("identity"),
        manager_alive=True,
        restarts=int(current_monitor.get("restarts", 0)) + 1,
        last_restart_at=utc_now(),
        failure=None,
    )
    return False, 0


def wait_for_monitor_detachment(
    timeout_seconds: float = MONITOR_DETACH_TIMEOUT_SECONDS,
    poll_seconds: float = 0.05,
) -> int:
    deadline = time.monotonic() + timeout_seconds
    while True:
        parent_pid = os.getppid()
        if parent_pid == 1:
            return parent_pid
        if time.monotonic() >= deadline:
            raise SystemExit(
                f"detached monitor still has parent pid {parent_pid} after "
                f"{timeout_seconds:.1f}s"
            )
        time.sleep(poll_seconds)


def monitor_campaign(root: Path, poll_seconds: float = 10.0) -> int:
    with exclusive_claim(root / "locks/monitor-run.claim") as claimed:
        if not claimed:
            return 0
        require_lineage(root)
        campaign = load_json(campaign_file(root))
        try:
            contract = smoke_contract_identity(campaign)
        except SystemExit as error:
            monitor_attention(root, f"monitor smoke contract failed: {error}")
            return 1
        try:
            parent_pid = wait_for_monitor_detachment()
        except SystemExit as error:
            monitor_attention(root, f"monitor detachment proof failed: {error}")
            return 1
        monitor_state(
            root,
            status="RUNNING",
            pid=os.getpid(),
            pgid=os.getpgrp(),
            identity=process_identity(os.getpid()),
            parent_pid=parent_pid,
            session_id=os.getsid(0),
            detached_session=os.getsid(0) == os.getpid(),
            smoke_contract_sha256=contract,
            started_at=utc_now(),
            failure=None,
        )
        while True:
            try:
                terminal, exit_code = monitor_tick(root)
            except (Exception, SystemExit) as error:
                monitor_attention(
                    root, f"monitor crashed while evaluating campaign state: {error}"
                )
                return 1
            if terminal:
                return exit_code
            time.sleep(poll_seconds)


def monitor_start(root: Path) -> int:
    with exclusive_claim(root / "locks/monitor-launch.claim", blocking=True) as claimed:
        if not claimed:
            raise SystemExit("cannot claim cloud benchmark monitor launch")
        campaign = load_json(campaign_file(root))
        if campaign.get("status") in {"PUBLISHED", "STOPPED"}:
            raise SystemExit(
                f"campaign monitor cannot start from {campaign.get('status')}"
            )
        require_smoke_proofs(root)
        current = read_monitor_state(root)
        if process_alive(current.get("pid"), current.get("identity")):
            raise SystemExit(f"monitor is already running as pid {current.get('pid')}")
        if current.get("pid") and not stop_recorded_group(
            current.get("pid"), current.get("pgid"), current.get("identity")
        ):
            raise SystemExit("dead monitor's owned process group survived recovery")
        monitor_state(
            root,
            status="RECOVERED" if current.get("pid") else "IDLE",
            pid=None,
            pgid=None,
            identity=None,
            recovered_at=utc_now() if current.get("pid") else None,
        )
        proc = launch_detached(
            [
                sys.executable,
                str(campaign["coordinator"]),
                "_monitor",
                "--root",
                str(root),
            ],
            root / "monitor.log",
        )
        monitor_state(
            root,
            status="STARTING",
            pid=proc.pid,
            pgid=proc.pid,
            identity=process_identity(proc.pid),
            smoke_contract_sha256=campaign["smoke_contract_sha256"],
            launched_at=utc_now(),
            failure=None,
        )
    print(f"started cloud SB7 monitor pid={proc.pid} root={root}")
    return 0


def stop_group(pgid: int, grace_seconds: float = 15.0) -> bool:
    try:
        os.killpg(pgid, signal.SIGTERM)
    except ProcessLookupError:
        return True
    deadline = time.monotonic() + grace_seconds
    while time.monotonic() < deadline:
        if not process_group_members(pgid):
            return True
        time.sleep(0.2)
    if not process_group_members(pgid):
        return True
    with contextlib.suppress(ProcessLookupError):
        os.killpg(pgid, signal.SIGKILL)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if not process_group_members(pgid):
            return True
        time.sleep(0.2)
    return not process_group_members(pgid)


def stop(root: Path) -> int:
    with exclusive_claim(root / "locks/supersession.claim", blocking=True) as claimed:
        if not claimed:
            raise SystemExit("cannot claim cloud campaign stop")
        return stop_claimed(root)


def stop_claimed(root: Path) -> int:
    if (root / SUPERSESSION_RECEIPT).exists() or (
        root / QUALIFICATION_RESTART_RECEIPT
    ).exists():
        return 0
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    failures = []
    for row in entrants(manifest):
        entrant_id = str(row["id"])
        smoke_path = smoke_state_file(root, entrant_id)
        if smoke_path.is_file():
            smoke_state = read_smoke_state(root, entrant_id)
            smoke_pgid = smoke_state.get("supervisor_pgid")
            if (
                smoke_pgid
                and (
                    process_alive(
                        smoke_state.get("supervisor_pid"),
                        smoke_state.get("supervisor_identity"),
                    )
                    or process_group_members(int(smoke_pgid))
                )
                and not stop_recorded_group(
                    smoke_state.get("supervisor_pid"),
                    smoke_pgid,
                    smoke_state.get("supervisor_identity"),
                )
            ):
                failures.append(f"{entrant_id}:smoke-pgid={smoke_pgid}")
            if smoke_state.get("status") not in SMOKE_TERMINAL_STATES:
                update_smoke_state(
                    root, entrant_id, status="STOPPED", stopped_at=utc_now()
                )
        state = read_state(root, entrant_id)
        publisher_pgid = state.get("publisher_pgid")
        if (
            state["status"] in INTERRUPTED_PUBLICATION_STATES
            and publisher_pgid
            and not stop_recorded_group(
                state.get("publisher_pid"),
                publisher_pgid,
                state.get("publisher_identity"),
            )
        ):
            failures.append(f"{entrant_id}:publisher-pgid={publisher_pgid}")
        pgid = state.get("supervisor_pgid")
        if (
            state["status"] not in TERMINAL_BUILD_STATES | POST_BUILD_STATES
            and pgid
            and process_group_members(int(pgid))
            and not stop_group(int(pgid))
        ):
            failures.append(f"{entrant_id}:pgid={pgid}")
        if state["status"] not in TERMINAL_BUILD_STATES | {"PUBLISHED"}:
            update_state(root, entrant_id, status="STOPPED", stopped_at=utc_now())
    manager = load_json(root / "manager.json")
    pgid = manager.get("pgid")
    if (
        manager.get("status") not in {"PUBLISHED", "ATTENTION", "STOPPED"}
        and pgid
        and process_group_members(int(pgid))
        and not stop_group(int(pgid))
    ):
        failures.append(f"manager:pgid={pgid}")
    manager_state(root, status="STOPPED", stop_failures=failures)
    monitor = read_monitor_state(root)
    monitor_pgid = monitor.get("pgid")
    if (
        monitor.get("status") not in MONITOR_TERMINAL_STATES
        and monitor_pgid
        and (
            process_alive(monitor.get("pid"), monitor.get("identity"))
            or process_group_members(int(monitor_pgid))
        )
        and not stop_recorded_group(
            monitor.get("pid"), monitor_pgid, monitor.get("identity")
        )
    ):
        failures.append(f"monitor:pgid={monitor_pgid}")
    monitor_state(root, status="STOPPED", stop_failures=failures)
    if failures:
        raise SystemExit(f"owned process groups survived stop: {', '.join(failures)}")
    busy = []
    for row in entrants(manifest):
        port = int(row["vendor_port"])
        if not port_is_free(port):
            busy.append(port)
    if busy:
        raise SystemExit(f"owned vendor ports survived stop: {busy}")
    update_campaign(root, status="STOPPED", stopped_at=utc_now())
    return 0


def status_rows(root: Path) -> list[Dict[str, Any]]:
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    return [read_state(root, str(row["id"])) for row in entrants(manifest)]


def print_status(root: Path) -> None:
    campaign = load_json(campaign_file(root))
    manager = load_json(root / "manager.json")
    monitor = read_monitor_state(root)
    print(
        f"campaign={campaign.get('status')} manager={manager.get('status')} "
        f"pid={manager.get('pid')} alive={process_alive(manager.get('pid'))} "
        f"smoke={campaign.get('smoke_status')} monitor={monitor.get('status')} "
        f"monitor_alive={process_alive(monitor.get('pid'), monitor.get('identity'))}"
    )
    budget_path = campaign.get("budget_ledger")
    if budget_path and Path(str(budget_path)).is_file():
        budget = load_json(Path(str(budget_path)))
        print(
            f"budget=${float(budget.get('spent_upper_bound', 0)):.4f}/"
            f"${float(budget.get('total_cap', 0)):.2f} "
            f"outstanding={len(budget.get('outstanding', {}))}"
        )
    for state in status_rows(root):
        score = state.get("score")
        suffix = f" score={100 * float(score):.2f}%" if score is not None else ""
        elapsed = state.get("elapsed_seconds")
        elapsed_text = f" elapsed={elapsed}s" if elapsed is not None else ""
        print(
            f"{state['entrant']:<26} {state['status']:<24}"
            f" admitted={state.get('admitted_requests', 0)}"
            f" terminal={state.get('provider_terminal_requests', 0)}"
            f"{elapsed_text}{suffix}"
        )
        if state.get("failure"):
            print(f"  failure: {state['failure']}")


def results(root: Path) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    rows = []
    for state in status_rows(root):
        row = {
            "entrant": state["entrant"],
            "provider": state["provider"],
            "model": state["model"],
            "status": state["status"],
            "score": state.get("score"),
            "elapsed_seconds": state.get("elapsed_seconds"),
            "fixture_seed": state["fixture_seed"],
            "vendor_port": state["vendor_port"],
            "verdict": state.get("verdict"),
            "publish_doc_id": state.get("publish_doc_id"),
            "published_url": state.get("published_url"),
            "published_at": state.get("published_at"),
            "rendered_verification": state.get("rendered_verification"),
            "failure": state.get("failure"),
        }
        rows.append(row)
    budget = None
    if campaign.get("budget_ledger") and Path(str(campaign["budget_ledger"])).is_file():
        budget = load_json(Path(str(campaign["budget_ledger"])))
    return {
        "campaign": campaign["campaign_id"],
        "status": campaign["status"],
        "smoke_status": campaign.get("smoke_status"),
        "smoke_contract_sha256": campaign.get("smoke_contract_sha256"),
        "smoke_proof_sha256": campaign.get("smoke_proof_sha256"),
        "binary_sha256": campaign["binary_sha256"],
        "instrument_set_sha256": campaign["instrument_set_sha256"],
        "budget": budget,
        "results": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    def root_arg(command: argparse.ArgumentParser) -> None:
        command.add_argument("--root", type=Path, default=DEFAULT_ROOT)

    p_preflight = sub.add_parser("preflight")
    p_preflight.add_argument("--binary", type=Path, required=True)
    p_preflight.add_argument("--manifest", type=Path, default=DEFAULT_ENTRANTS)
    p_preflight.add_argument("--secrets", type=Path, default=DEFAULT_SECRET_FILE)
    p_preflight.add_argument("--publisher-repo", type=Path, required=True)

    p_init = sub.add_parser("init")
    root_arg(p_init)
    p_init.add_argument("--binary", type=Path, required=True)
    p_init.add_argument("--manifest", type=Path, default=DEFAULT_ENTRANTS)
    p_init.add_argument("--secrets", type=Path, default=DEFAULT_SECRET_FILE)
    p_init.add_argument("--publisher-repo", type=Path, required=True)
    p_init.add_argument("--publish-live", action="store_true")
    p_init.add_argument("--website-base-url", default=DEFAULT_WEBSITE_BASE_URL)
    p_init.add_argument(
        "--publish-verify-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    )
    p_init.add_argument(
        "--publish-verify-interval-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    )
    p_init.add_argument(
        "--publish-process-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
    )

    p_supersede = sub.add_parser("supersede")
    root_arg(p_supersede)
    p_supersede.add_argument("--from-root", type=Path, required=True)
    p_supersede.add_argument("--binary", type=Path, required=True)
    p_supersede.add_argument("--manifest", type=Path, default=DEFAULT_ENTRANTS)
    p_supersede.add_argument("--secrets", type=Path, default=DEFAULT_SECRET_FILE)
    p_supersede.add_argument("--publisher-repo", type=Path, required=True)
    p_supersede.add_argument("--defect-evidence", type=Path, required=True)
    p_supersede.add_argument("--publish-live", action="store_true")
    p_supersede.add_argument("--website-base-url", default=DEFAULT_WEBSITE_BASE_URL)
    p_supersede.add_argument(
        "--publish-verify-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    )
    p_supersede.add_argument(
        "--publish-verify-interval-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    )
    p_supersede.add_argument(
        "--publish-process-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
    )

    p_qualification_restart = sub.add_parser("qualification-restart")
    root_arg(p_qualification_restart)
    p_qualification_restart.add_argument("--from-root", type=Path, required=True)
    p_qualification_restart.add_argument("--binary", type=Path, required=True)
    p_qualification_restart.add_argument(
        "--manifest", type=Path, default=DEFAULT_ENTRANTS
    )
    p_qualification_restart.add_argument(
        "--secrets", type=Path, default=DEFAULT_SECRET_FILE
    )
    p_qualification_restart.add_argument("--publisher-repo", type=Path, required=True)
    p_qualification_restart.add_argument("--defect-evidence", type=Path, required=True)
    p_qualification_restart.add_argument("--publish-live", action="store_true")
    p_qualification_restart.add_argument(
        "--website-base-url", default=DEFAULT_WEBSITE_BASE_URL
    )
    p_qualification_restart.add_argument(
        "--publish-verify-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_TIMEOUT_SECONDS,
    )
    p_qualification_restart.add_argument(
        "--publish-verify-interval-seconds",
        type=float,
        default=DEFAULT_PUBLISH_VERIFY_INTERVAL_SECONDS,
    )
    p_qualification_restart.add_argument(
        "--publish-process-timeout-seconds",
        type=float,
        default=DEFAULT_PUBLISH_PROCESS_TIMEOUT_SECONDS,
    )

    for name in (
        "smoke",
        "monitor-start",
        "start",
        "status",
        "watch",
        "results",
        "stop",
        "score",
        "resume",
    ):
        root_arg(sub.add_parser(name))

    p_smoke_supervise = sub.add_parser("_smoke_supervise")
    root_arg(p_smoke_supervise)
    p_smoke_supervise.add_argument("--entrant", required=True)
    root_arg(sub.add_parser("_monitor"))
    p_supervise = sub.add_parser("_supervise")
    root_arg(p_supervise)
    p_supervise.add_argument("--entrant", required=True)
    root_arg(sub.add_parser("_manage"))

    args = parser.parse_args()
    if args.command == "preflight":
        value = preflight(
            args.binary.resolve(),
            args.manifest.resolve(),
            args.secrets.resolve(),
            args.publisher_repo.resolve(),
        )
        print(json.dumps(value, indent=2))
        return 0
    if args.command == "init":
        value = init_campaign(
            args.root.resolve(),
            args.binary.resolve(),
            args.manifest.resolve(),
            args.secrets.resolve(),
            args.publisher_repo.resolve(),
            args.publish_live,
            args.website_base_url,
            args.publish_verify_timeout_seconds,
            args.publish_verify_interval_seconds,
            args.publish_process_timeout_seconds,
        )
        print(f"initialized {value['campaign_id']} at {args.root.resolve()}")
        return 0
    if args.command == "supersede":
        value = supersede_campaign(
            args.from_root,
            args.root,
            args.binary,
            args.manifest,
            args.secrets,
            args.publisher_repo,
            args.defect_evidence,
            args.publish_live,
            args.website_base_url,
            args.publish_verify_timeout_seconds,
            args.publish_verify_interval_seconds,
            args.publish_process_timeout_seconds,
        )
        print(f"superseded into {value['campaign_id']} at {args.root.resolve()}")
        return 0
    if args.command == "qualification-restart":
        value = qualification_restart_campaign(
            args.from_root,
            args.root,
            args.binary,
            args.manifest,
            args.secrets,
            args.publisher_repo,
            args.defect_evidence,
            args.publish_live,
            args.website_base_url,
            args.publish_verify_timeout_seconds,
            args.publish_verify_interval_seconds,
            args.publish_process_timeout_seconds,
        )
        print(
            f"qualification restarted into {value['campaign_id']} "
            f"at {args.root.resolve()}"
        )
        return 0
    root = args.root.resolve()
    if args.command == "smoke":
        return smoke(root)
    if args.command == "_smoke_supervise":
        return smoke_supervise(root, args.entrant)
    if args.command == "monitor-start":
        return monitor_start(root)
    if args.command == "_monitor":
        return monitor_campaign(root)
    if args.command == "start":
        return start(root)
    if args.command == "_manage":
        return manage(root)
    if args.command == "_supervise":
        return supervise(root, args.entrant)
    if args.command == "status":
        print_status(root)
        return 0
    if args.command == "watch":
        try:
            while True:
                print_status(root)
                manager = load_json(root / "manager.json")
                campaign = load_json(campaign_file(root))
                if (
                    manager.get("status") in {"PUBLISHED", "ATTENTION", "STOPPED"}
                    or campaign.get("status") == "PUBLISHED"
                ):
                    return 0
                if not process_alive(manager.get("pid"), manager.get("identity")):
                    start(root)
                print()
                time.sleep(20)
        except KeyboardInterrupt:
            return 130
    if args.command == "results":
        print(json.dumps(results(root), indent=2))
        return 0
    if args.command == "stop":
        return stop(root)
    if args.command == "score":
        ids = [state["entrant"] for state in status_rows(root)]
        return 0 if score_all(root, ids) else 1
    if args.command == "resume":
        require_lineage(root)
        recover_dead_manager(root)
        recover_interrupted_publication(root)
        campaign = load_json(campaign_file(root))
        for state in status_rows(root):
            if state["status"] == "PRE_ADMISSION_FAILURE":
                row = manifest_row(root, str(state["entrant"]))
                lifecycle = lifecycle_summary(
                    Path(str(state["provider_lifecycle"])),
                    expected_provider=str(row["provider"]),
                    expected_model=str(row["model"]),
                )
                outstanding_ids, budget_error = entrant_outstanding_reservations(
                    campaign, row
                )
                pgid = int(state.get("supervisor_pgid") or 0)
                members = process_group_members(pgid) if pgid else []
                if (
                    lifecycle["admitted"]
                    or lifecycle_failure(lifecycle)
                    or outstanding_ids
                    or budget_error
                    or members
                ):
                    update_state(
                        root,
                        state["entrant"],
                        status="INCOMPLETE",
                        failure="resume denied: reconstructed provider/process evidence is ambiguous",
                        admitted_requests=lifecycle["admitted"],
                        provider_terminal_requests=lifecycle["terminal"],
                        lifecycle_transition_errors=lifecycle["transition_errors"],
                        lifecycle_ambiguous_request_ids=lifecycle[
                            "ambiguous_request_ids"
                        ],
                        budget_outstanding_request_ids=outstanding_ids,
                    )
                    raise SystemExit(
                        f"{state['entrant']} has ambiguous evidence and cannot be retried"
                    )
                update_state(root, state["entrant"], status="PLANNED", failure=None)
            elif state["status"] in {"INCOMPLETE", "STOPPED"}:
                raise SystemExit(
                    f"{state['entrant']} is {state['status']}; admitted or operator-stopped "
                    "work cannot be resumed as a fresh paid attempt"
                )
            elif state["status"] == "SCORING":
                update_state(
                    root,
                    state["entrant"],
                    status="SCORE_FAILED",
                    failure="scorer was interrupted; raw build remains sealed",
                )
        update_campaign(root, status="ATTENTION")
        manager_state(
            root,
            status="ATTENTION",
            pid=None,
            pgid=None,
            identity=None,
        )
        return start(root)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
