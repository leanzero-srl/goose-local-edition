#!/usr/bin/env python3
"""Persistent, build-only SB7 cloud benchmark coordinator.

The build and score state machines are deliberately separate.  A build owns one
immutable binary, fixture seed, vendor port, profile root, process group and raw
tree.  Scoring always operates on a disposable clone and never mutates the raw
tree.  Provider lanes sharing one credential serialize unless their manifest
explicitly records independently proven concurrency.
"""

from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, Mapping


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
DEFAULT_ENTRANTS = HERE / "cloud-sb7-entrants.json"
DEFAULT_SECRET_FILE = (
    Path.home()
    / ".agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env"
)
DEFAULT_ROOT = Path.home() / "goose-builds/cloud-sb7-20260823"
TERMINAL_BUILD_STATES = {
    "BUILD_COMPLETE",
    "INCOMPLETE",
    "PRE_ADMISSION_FAILURE",
    "STOPPED",
}
RETRYABLE_BUILD_STATES = {"PLANNED", "PRE_ADMISSION_FAILURE"}
CAMPAIGN_SCHEMA = 1
REQUIRED_BINARY_MARKERS = (
    "GOOSE_PROVIDER_LIFECYCLE_FILE",
    "GOOSE_PROVIDER_LIFECYCLE_STRICT",
    "GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES",
    "GOOSE_BENCH_BUDGET_CONFIG",
    "GOOSE_BENCH_BUDGET_CONFIG_SHA256",
    "GOOSE_BENCH_BUDGET_LEDGER",
)


def utc_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


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
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(raw)


def load_json(path: Path) -> Dict[str, Any]:
    with path.open() as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise SystemExit(f"expected an object in {path}")
    return value


def parse_secret_file(path: Path) -> Dict[str, str]:
    if not path.is_file():
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
            raise SystemExit(f"SB7 vendor ports must be >= 8899: {entrant_id} -> {port}")
        seen_ids.add(entrant_id)
        seen_ports.add(port)
        out.append(dict(raw))
    return out


def spend_policy(manifest: Mapping[str, Any], rows: Iterable[Mapping[str, Any]]) -> Dict[str, Any]:
    policy = manifest.get("spend_policy")
    if not isinstance(policy, dict):
        raise SystemExit("entrant manifest has no spend_policy")
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
            pricing.get("input_over_threshold_per_million", pricing["input_per_million"])
        )
        output_rate = float(
            pricing.get("output_over_threshold_per_million", pricing["output_per_million"])
        )
        if input_rate < 0 or output_rate < 0:
            raise SystemExit(f"pricing rates must be non-negative: {row['id']}")
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


def read_state(root: Path, entrant_id: str) -> Dict[str, Any]:
    return load_json(state_file(root, entrant_id))


def update_state(root: Path, entrant_id: str, **changes: Any) -> Dict[str, Any]:
    path = state_file(root, entrant_id)
    state = load_json(path)
    state.update(changes)
    state["updated_at"] = utc_now()
    atomic_json(path, state)
    return state


def manager_state(root: Path, **changes: Any) -> Dict[str, Any]:
    path = root / "manager.json"
    state = load_json(path) if path.is_file() else {"schema_version": CAMPAIGN_SCHEMA}
    state.update(changes)
    state["updated_at"] = utc_now()
    atomic_json(path, state)
    return state


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


def instrument_mismatch(campaign: Mapping[str, Any]) -> str | None:
    expected = campaign.get("instrument_hashes")
    current = instrument_hashes()
    if current == expected:
        return None
    expected = expected if isinstance(expected, dict) else {}
    changed = sorted(
        key
        for key in set(expected) | set(current)
        if expected.get(key) != current.get(key)
    )
    return f"instrument changed after freeze: {', '.join(changed)}"


def fetch_json(url: str, headers: Mapping[str, str]) -> Dict[str, Any]:
    request = urllib.request.Request(url, headers=dict(headers))
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            value = json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(f"authenticated roster failed: HTTP {error.code} from {url}") from None
    except Exception as error:
        raise SystemExit(
            f"authenticated roster failed: {type(error).__name__} from {url}"
        ) from None
    if not isinstance(value, dict):
        raise SystemExit(f"authenticated roster returned a non-object: {url}")
    return value


def authenticated_rosters(secret_values: Mapping[str, str]) -> Dict[str, set[str]]:
    required = ("ZHIPU_API_KEY", "GOOGLE_API_KEY", "DEEPSEEK_API_KEY")
    missing = [name for name in required if not secret_values.get(name)]
    if missing:
        raise SystemExit(f"secret file is missing variables: {', '.join(missing)}")

    zai = fetch_json(
        "https://api.z.ai/api/paas/v4/models",
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
    return {
        "zai_api": {str(row.get("id", "")) for row in zai.get("data", [])},
        "google": {
            str(row.get("name", "")).split("/")[-1]
            for row in google.get("models", [])
        },
        "custom_deepseek": {
            str(row.get("id", "")) for row in deepseek.get("data", [])
        },
    }


def validate_rosters(rows: Iterable[Mapping[str, Any]], rosters: Mapping[str, set[str]]) -> None:
    for row in rows:
        provider = str(row["provider"])
        model = str(row["model"])
        if model not in rosters.get(provider, set()):
            raise SystemExit(
                f"exact model is not in the authenticated {provider} roster: {model}"
            )


def preflight(binary: Path, manifest_path: Path, secret_path: Path) -> Dict[str, Any]:
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    spend_policy(manifest, rows)
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
        raise SystemExit("cloud benchmark source worktree must be clean before it is frozen")
    secret_values = parse_secret_file(secret_path)
    rosters = authenticated_rosters(secret_values)
    validate_rosters(rows, rosters)
    busy = [str(row["vendor_port"]) for row in rows if not port_is_free(int(row["vendor_port"]))]
    if busy:
        raise SystemExit(f"vendor ports are already occupied: {', '.join(busy)}")
    return {
        "checked_at": utc_now(),
        "binary_sha256": sha256_file(binary),
        "models": {key: sorted(value) for key, value in rosters.items()},
        "requested_models": [str(row["model"]) for row in rows],
        "ports_free": True,
        "credential_file_mode": f"{secret_path.stat().st_mode & 0o777:04o}",
    }


def init_campaign(
    root: Path, binary: Path, manifest_path: Path, secret_path: Path
) -> Dict[str, Any]:
    if campaign_file(root).exists():
        existing = load_json(campaign_file(root))
        if existing.get("status") in {"INITIALIZED", "RUNNING", "BUILD_COMPLETE", "SCORING"}:
            return existing
        raise SystemExit(f"campaign already exists with status {existing.get('status')}: {root}")

    checked = preflight(binary, manifest_path, secret_path)
    manifest = load_json(manifest_path)
    rows = entrants(manifest)
    policy = spend_policy(manifest, rows)
    root.mkdir(parents=True, exist_ok=False)
    (root / "instrument").mkdir()
    (root / "entrants").mkdir()
    (root / "locks").mkdir()
    (root / "scores").mkdir()
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
    hashes = instrument_hashes()
    prompt_source = (REPO / "evals/swarm-bench/spec-build-sb7.md").read_bytes()
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
        "instrument_hashes": hashes,
        "instrument_set_sha256": sha256_bytes(
            json.dumps(hashes, sort_keys=True).encode()
        ),
        "prompt_source_sha256": sha256_bytes(prompt_source),
        "secret_file": str(secret_path),
        "preflight": {
            key: value for key, value in checked.items() if key != "models"
        },
        "requested_models": checked["requested_models"],
        "scorer_version": manifest.get("suite"),
        "calibration": manifest.get("calibration"),
    }
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
            "attempt": 1,
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
        }
        atomic_json(state_file(root, entrant_id), state)
    manager_state(root, status="IDLE", pid=None, pgid=None)
    return campaign


def manifest_row(root: Path, entrant_id: str) -> Dict[str, Any]:
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    for row in entrants(manifest):
        if row["id"] == entrant_id:
            return row
    raise SystemExit(f"unknown entrant in campaign: {entrant_id}")


def build_prompt(port: int) -> str:
    from vendor_service_v3 import API_KEY, DOCS_PATH  # noqa: PLC0415

    spec = (REPO / "evals/swarm-bench/spec-build-sb7.md").read_text()
    return (
        spec.replace("{DOCS_URL}", f"http://127.0.0.1:{port}{DOCS_PATH}")
        .replace("{BASE_URL}", f"http://127.0.0.1:{port}")
        .replace("{API_KEY}", API_KEY)
    )


SAFE_ENV_NAMES = {
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
    "COLORTERM",
    "SSH_AUTH_SOCK",
    "GIT_ASKPASS",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "NO_COLOR",
}


def child_env(
    row: Mapping[str, Any], state: Mapping[str, Any], secret_value: str
) -> Dict[str, str]:
    env = {key: value for key, value in os.environ.items() if key in SAFE_ENV_NAMES}
    env.update(
        {
            str(row["secret_env"]): secret_value,
            "GOOSE_PATH_ROOT": str(state["profile"]),
            "GOOSE_PROVIDER": str(row["provider"]),
            "GOOSE_MODEL": str(row["model"]),
            "GOOSE_MODE": "auto",
            "GOOSE_DISABLE_SESSION_NAMING": "true",
            "GOOSE_THINKING_EFFORT": str(row["thinking_effort"]),
            "GOOSE_CONTEXT_LIMIT": str(row["context_limit"]),
            "GOOSE_MAX_TOKENS": str(row["max_output_tokens"]),
            "GOOSE_SWARM_TELEMETRY_FILE": str(
                Path(str(state["tree"])) / ".swarm/telemetry.jsonl"
            ),
            "GOOSE_PROVIDER_LIFECYCLE_FILE": str(state["provider_lifecycle"]),
            "GOOSE_PROVIDER_LIFECYCLE_STRICT": "true",
            "GOOSE_PROVIDER_TERMINAL_SAFE_RETRIES": "true",
            "GOOSE_BENCH_BUDGET_CONFIG": str(
                Path(str(state["tree"])).parents[2] / "instrument/budget-config.json"
            ),
            "GOOSE_BENCH_BUDGET_CONFIG_SHA256": str(
                state["budget_config_sha256"]
            ),
            "GOOSE_BENCH_BUDGET_LEDGER": str(
                Path(str(state["tree"])).parents[2] / "budget-ledger.json"
            ),
            "GOOSE_BENCH_CAMPAIGN": str(Path(str(state["tree"])).parents[2]),
            "GOOSE_BENCH_ENTRANT": str(row["id"]),
        }
    )
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


def classify_build_exit(exit_code: int, admitted_requests: int) -> tuple[str, str | None]:
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


def lifecycle_summary(path: Path) -> Dict[str, Any]:
    summary: Dict[str, Any] = {
        "admitted": 0,
        "terminal": 0,
        "first_output_at": None,
        "malformed_lines": 0,
        "events": 0,
    }
    if not path.is_file():
        return summary
    for raw in path.read_text(errors="replace").splitlines():
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            summary["malformed_lines"] += 1
            continue
        if not isinstance(event, dict):
            summary["malformed_lines"] += 1
            continue
        summary["events"] += 1
        state = event.get("state")
        if state == "admitted":
            summary["admitted"] += 1
        elif state == "provider_terminal":
            summary["terminal"] += 1
        elif state == "first_item" and summary["first_output_at"] is None:
            summary["first_output_at"] = event.get("at") or event.get("timestamp")
    return summary


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
        if (
            reservation.get("provider") == row.get("provider")
            and reservation.get("model") == row.get("model")
        ):
            request_ids.append(str(request_id))
    return sorted(request_ids), None


@contextlib.contextmanager
def provider_lane(root: Path, lane: str) -> Iterator[None]:
    path = root / "locks" / f"{lane}.lock"
    with path.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def supervise(root: Path, entrant_id: str) -> int:
    from vendor_service_v3 import serve  # noqa: PLC0415

    campaign = load_json(campaign_file(root))
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
        update_state(root, entrant_id, status="PRE_ADMISSION_FAILURE", failure="missing credential")
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
        prompt = build_prompt(port)
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

        telemetry = Path(str(state["tree"])) / ".swarm/telemetry.jsonl"
        telemetry.parent.mkdir(parents=True, exist_ok=True)
        telemetry.write_text("")
        lifecycle_path = Path(str(state["provider_lifecycle"]))
        lifecycle_path.unlink(missing_ok=True)
        env = child_env(row, state, secret_value)
        cmd = [
            str(binary),
            "run",
            "--provider",
            str(row["provider"]),
            "--model",
            str(row["model"]),
            "--output-format",
            "stream-json",
            "-t",
            prompt,
        ]
        log_path = Path(str(state["build_log"]))
        counters, observe = provider_state_observer(root, entrant_id)
        started = time.time()
        update_state(
            root,
            entrant_id,
            status="BUILD_RUNNING",
            started_at=utc_now(),
            prompt_sha256=prompt_sha,
            command=[str(binary), "run", "--provider", row["provider"], "--model", row["model"], "--output-format", "stream-json", "-t", "[PROMPT]"],
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
                update_state(root, entrant_id, goose_pid=proc.pid, process_group=os.getpgrp())
                assert proc.stdout is not None
                redacted_copy(proc.stdout, log, secret_values.values(), observe)
                exit_code = proc.wait()
        finally:
            server.shutdown()

        elapsed = round(time.time() - started, 3)
        lifecycle = lifecycle_summary(lifecycle_path)
        counters["admitted"] = lifecycle["admitted"]
        counters["terminal"] = lifecycle["terminal"]
        counters["first_output_at"] = lifecycle["first_output_at"]
        completed = exit_code == 0
        status, failure = classify_build_exit(exit_code, counters["admitted"])
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
        elif completed and lifecycle["malformed_lines"]:
            status = "INCOMPLETE"
            failure = "provider lifecycle ledger contains malformed evidence"
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
            budget_outstanding_request_ids=outstanding_ids,
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


def launch_supervisor(root: Path, entrant_id: str) -> int:
    unit = root / "entrants" / entrant_id
    proc = launch_detached(
        [sys.executable, str(Path(__file__).resolve()), "_supervise", "--root", str(root), "--entrant", entrant_id],
        unit / "logs/supervisor.log",
    )
    update_state(
        root,
        entrant_id,
        supervisor_pid=proc.pid,
        supervisor_pgid=proc.pid,
        launched_at=utc_now(),
    )
    return proc.pid


def process_alive(pid: Any) -> bool:
    try:
        os.kill(int(pid), 0)
    except (OSError, TypeError, ValueError):
        return False
    return True


def process_group_members(pgid: int) -> list[tuple[int, str]]:
    proc = subprocess.run(
        ["ps", "-axo", "pid=,pgid=,stat="],
        text=True,
        capture_output=True,
        check=False,
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


def wait_for_builds(root: Path, row_ids: list[str]) -> bool:
    while True:
        states = [read_state(root, entrant_id) for entrant_id in row_ids]
        if all(state["status"] in TERMINAL_BUILD_STATES for state in states):
            return all(state["status"] == "BUILD_COMPLETE" for state in states)
        for state in states:
            if state["status"] not in TERMINAL_BUILD_STATES and state.get("supervisor_pid"):
                if not process_alive(state["supervisor_pid"]):
                    update_state(
                        root,
                        str(state["entrant"]),
                        status="INCOMPLETE" if state.get("admitted_requests") else "PRE_ADMISSION_FAILURE",
                        failure="supervisor disappeared; silence is not success",
                    )
        time.sleep(10)


def clone_for_score(root: Path, entrant_id: str, attempt: int) -> Path:
    raw = root / "entrants" / entrant_id / "tree"
    dest = root / "scores" / entrant_id / f"attempt-{attempt}" / "tree"
    if dest.exists():
        raise SystemExit(f"score clone already exists; attempts are never overwritten: {dest}")
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


def score_one(root: Path, entrant_id: str) -> bool:
    state = read_state(root, entrant_id)
    if state["status"] == "SCORED":
        return True
    if state["status"] not in {"BUILD_COMPLETE", "SCORE_FAILED"}:
        return False
    campaign = load_json(campaign_file(root))
    mismatch = instrument_mismatch(campaign)
    if mismatch:
        update_state(root, entrant_id, status="SCORE_FAILED", failure=mismatch)
        return False
    raw = Path(str(state["tree"]))
    if hash_tree(raw) != state.get("raw_tree_sha256"):
        update_state(root, entrant_id, status="INCOMPLETE", failure="raw tree changed before scoring")
        return False
    score_attempt = int(state.get("score_attempts", 0)) + 1
    score_tree = clone_for_score(root, entrant_id, score_attempt)
    score_dir = score_tree.parent
    verdict = score_dir / "verdict.json"
    log_path = score_dir / "score.log"
    cmd = [
        sys.executable,
        str(HERE / "score_sb7.py"),
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
    with log_path.open("w") as log:
        proc = subprocess.run(cmd, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, check=False)
    if proc.returncode != 0 or not verdict.is_file():
        update_state(
            root,
            entrant_id,
            status="SCORE_FAILED",
            score_exit_code=proc.returncode,
            failure="hermetic scorer failed; raw build remains sealed",
        )
        return False
    result = load_json(verdict)
    update_state(
        root,
        entrant_id,
        status="SCORED",
        score_exit_code=proc.returncode,
        score_finished_at=utc_now(),
        score=result.get("score"),
        scorer_version=result.get("scorer_version"),
        calibrated=result.get("calibrated"),
        verdict=str(verdict),
    )
    return True


def score_all(root: Path, row_ids: list[str]) -> bool:
    manager_state(root, status="SCORING")
    for entrant_id in row_ids:
        if not score_one(root, entrant_id):
            manager_state(root, status="ATTENTION", failure=f"scoring failed: {entrant_id}")
            return False
    manager_state(root, status="SCORED", finished_at=utc_now())
    campaign = load_json(campaign_file(root))
    campaign["status"] = "SCORED"
    campaign["finished_at"] = utc_now()
    atomic_json(campaign_file(root), campaign)
    return True


def manage(root: Path) -> int:
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    row_ids = [str(row["id"]) for row in entrants(manifest)]
    manager_state(
        root,
        status="RUNNING",
        pid=os.getpid(),
        pgid=os.getpgrp(),
        started_at=utc_now(),
    )
    campaign["status"] = "RUNNING"
    campaign["started_at"] = utc_now()
    atomic_json(campaign_file(root), campaign)
    for entrant_id in row_ids:
        state = read_state(root, entrant_id)
        if state["status"] in RETRYABLE_BUILD_STATES:
            launch_supervisor(root, entrant_id)
    builds_ok = wait_for_builds(root, row_ids)
    if not builds_ok:
        manager_state(root, status="ATTENTION", failure="one or more builds did not complete")
        campaign = load_json(campaign_file(root))
        campaign["status"] = "ATTENTION"
        atomic_json(campaign_file(root), campaign)
        return 1
    campaign = load_json(campaign_file(root))
    campaign["status"] = "BUILD_COMPLETE"
    campaign["build_finished_at"] = utc_now()
    atomic_json(campaign_file(root), campaign)
    return 0 if score_all(root, row_ids) else 1


def start(root: Path) -> int:
    campaign = load_json(campaign_file(root))
    if campaign["status"] not in {"INITIALIZED", "ATTENTION"}:
        raise SystemExit(f"campaign cannot start from {campaign['status']}")
    current = load_json(root / "manager.json")
    if current.get("pid") and process_alive(current["pid"]):
        raise SystemExit(f"manager is already running as pid {current['pid']}")
    proc = launch_detached(
        [sys.executable, str(Path(__file__).resolve()), "_manage", "--root", str(root)],
        root / "manager.log",
    )
    manager_state(root, status="STARTING", pid=proc.pid, pgid=proc.pid, launched_at=utc_now())
    print(f"started cloud SB7 manager pid={proc.pid} root={root}")
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
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    failures = []
    for row in entrants(manifest):
        entrant_id = str(row["id"])
        state = read_state(root, entrant_id)
        pgid = state.get("supervisor_pgid")
        if pgid and process_alive(pgid) and not stop_group(int(pgid)):
            failures.append(f"{entrant_id}:pgid={pgid}")
        if state["status"] not in TERMINAL_BUILD_STATES | {"SCORED", "SCORE_FAILED"}:
            update_state(root, entrant_id, status="STOPPED", stopped_at=utc_now())
    manager = load_json(root / "manager.json")
    pgid = manager.get("pgid")
    if pgid and process_alive(pgid) and not stop_group(int(pgid)):
        failures.append(f"manager:pgid={pgid}")
    manager_state(root, status="STOPPED", stop_failures=failures)
    if failures:
        raise SystemExit(f"owned process groups survived stop: {', '.join(failures)}")
    busy = []
    for row in entrants(manifest):
        port = int(row["vendor_port"])
        if not port_is_free(port):
            busy.append(port)
    if busy:
        raise SystemExit(f"owned vendor ports survived stop: {busy}")
    return 0


def status_rows(root: Path) -> list[Dict[str, Any]]:
    campaign = load_json(campaign_file(root))
    manifest = load_json(Path(str(campaign["entrant_manifest"])))
    return [read_state(root, str(row["id"])) for row in entrants(manifest)]


def print_status(root: Path) -> None:
    campaign = load_json(campaign_file(root))
    manager = load_json(root / "manager.json")
    print(
        f"campaign={campaign.get('status')} manager={manager.get('status')} "
        f"pid={manager.get('pid')} alive={process_alive(manager.get('pid'))}"
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
            "failure": state.get("failure"),
        }
        rows.append(row)
    budget = None
    if campaign.get("budget_ledger") and Path(str(campaign["budget_ledger"])).is_file():
        budget = load_json(Path(str(campaign["budget_ledger"])))
    return {
        "campaign": campaign["campaign_id"],
        "status": campaign["status"],
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

    p_init = sub.add_parser("init")
    root_arg(p_init)
    p_init.add_argument("--binary", type=Path, required=True)
    p_init.add_argument("--manifest", type=Path, default=DEFAULT_ENTRANTS)
    p_init.add_argument("--secrets", type=Path, default=DEFAULT_SECRET_FILE)

    for name in ("start", "status", "watch", "results", "stop", "score", "resume"):
        root_arg(sub.add_parser(name))

    p_supervise = sub.add_parser("_supervise")
    root_arg(p_supervise)
    p_supervise.add_argument("--entrant", required=True)
    root_arg(sub.add_parser("_manage"))

    args = parser.parse_args()
    if args.command == "preflight":
        value = preflight(args.binary.resolve(), args.manifest.resolve(), args.secrets.resolve())
        print(json.dumps(value, indent=2))
        return 0
    if args.command == "init":
        value = init_campaign(
            args.root.resolve(),
            args.binary.resolve(),
            args.manifest.resolve(),
            args.secrets.resolve(),
        )
        print(f"initialized {value['campaign_id']} at {args.root.resolve()}")
        return 0
    root = args.root.resolve()
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
                if manager.get("status") in {"SCORED", "ATTENTION", "STOPPED"}:
                    return 0
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
        for state in status_rows(root):
            if state["status"] == "PRE_ADMISSION_FAILURE":
                update_state(root, state["entrant"], status="PLANNED", failure=None)
            elif state["status"] in {"INCOMPLETE", "STOPPED"}:
                raise SystemExit(
                    f"{state['entrant']} is {state['status']}; admitted or operator-stopped "
                    "work cannot be resumed as a fresh paid attempt"
                )
        return start(root)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
