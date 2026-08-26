#!/usr/bin/env python3
"""Detached, restart-safe terminal closure for an immutable SB7 run.

The live run is read-only. This controller authenticates its already-running processes from the
launch receipt, waits for natural terminal evidence, seals the complete raw tree, scores only a
disposable clone with the frozen CLI scorer, then invokes the dedicated exact-ID publisher.
"""

from __future__ import annotations

import argparse
import contextlib
import ctypes
import datetime as dt
import fcntl
import hashlib
import importlib.util
import json
import math
import os
import pathlib
import re
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Iterable, Mapping, Sequence


USAGE_POLICY_PATH = pathlib.Path(__file__).with_name("usage_impairment.py")
USAGE_POLICY_SPEC = importlib.util.spec_from_file_location(
    "v21_r4_usage_impairment", USAGE_POLICY_PATH
)
if USAGE_POLICY_SPEC is None or USAGE_POLICY_SPEC.loader is None:
    raise RuntimeError("V21-r4 usage impairment policy could not be loaded")
usage_policy = importlib.util.module_from_spec(USAGE_POLICY_SPEC)
USAGE_POLICY_SPEC.loader.exec_module(usage_policy)


SCHEMA_VERSION = 1
SEED_RE = re.compile(r"^[0-9a-f]{16}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
GIT_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
RUN_ID_RE = re.compile(r"^swarm-\d{8}-\d{9}$")
# BEGIN TERMINAL_CLOSURE_RUN_BINDING
V19_GENERATION = "v21-r4"
V19_LIVE_ROOT = pathlib.Path(
    "/Users/mihaiperdum/goose-builds/local-sb7-engine-v21-r4"
)
V19_RUN_DIR = V19_LIVE_ROOT / "swarm-3node-qwen38-brainwaves-r0"
V19_STATE_DIR = pathlib.Path(
    "/Users/mihaiperdum/goose-builds/local-sb7-engine-v21-r4-terminal-closure-r4"
)
V19_BOUND_CONFIG = V19_STATE_DIR / "config.json"
V19_SCORE_LOCK = pathlib.Path(
    "/Users/mihaiperdum/goose-builds/local-sb7-engine-v21-r4-score.lock"
)
V19_LAUNCHER = V19_LIVE_ROOT / "launch_local_v21_r4.py"
V19_LAUNCHER_SHA256 = "548587275a4a49ca32cc66549abbb4d51b77cfdbce84ae8a53cd4dcdbec8ec44"
V19_FLEET_SEAL = V19_LIVE_ROOT / "fleet-seal.json"
V19_TARGET_DOCUMENT_ID = "brun-fleet-qwen38-brainwaves-sb70"
V19_PROTECTED_DOCUMENT_IDS = frozenset(
    {"brun-fleet-qwen38-sb70", "brun-fleet-qwen-sb70"}
)
V19_EXACT_MODEL_ALIASES = frozenset(
    {
        "gabee-qwen3.8-27b-brainwaves-mxfp8-mlx",
        "mihai-qwen3.8-27b-brainwaves-mxfp8-mlx",
        "workhorse-qwen3.8-27b-brainwaves-mxfp8-mlx",
    }
)
V19_EXACT_CONTEXT_BY_ROLE = {"local": 262_144, "workhorse": 262_144, "mac": 135_936}
V19_EXPECTED_ARTIFACT_PATH_SHA256 = "a08b6e855ac5fc1045921c633a902189d73be4584ee685cd4a49834d6850b136"
V19_EXPECTED_QUANTIZATION_SHA256 = "267f67a60ca5f10733c610ff4be23d9bf6107a66de92ba1f37757c1d8aaec767"
V19_EXPECTED_PLANNER_MODEL = "workhorse-qwen3.8-27b-brainwaves-mxfp8-mlx"
V19_ROLE_PREFIXES = {
    "local": "mihai-",
    "workhorse": "workhorse-",
    "mac": "gabee-",
}
V19_CANDIDATE_COMMIT = "8aac1f30063ffa09c0fb1b133e04bdfb6788729b"
V19_CANDIDATE_TREE = "82e52238125d48b6356b9d0f804354f4af02cb69"
V19_BINARY = V19_LIVE_ROOT / "bin/goose-833d3e37c54"
V19_BINARY_SHA256 = "833d3e37c5432266b58b6cd93d763911abbfc653c82e5d4d077fb6b2dd6b253f"
V19_INSTRUMENT_MANIFEST_SHA256 = (
    "26a476d64112d4c4664084da3e3cb7286973a649a9492bf68db2141f0dc26f13"
)
V19_INSTRUMENT_RECORDED_BINARY = {
    "path": "/Users/mihaiperdum/goose-builds/local-sb7-engine-v21-r2/bin/goose-fb9c8dec683",
    "sha256": "fb9c8dec683ec5a6ab5b8ce9a51623b0267c3cb70e2162d56ebb423032b0472a",
}
V19_PUBLISHER_COMMIT = "3fe730a58f81e63b3279d7b2cb5a11dd27f27b57"
V19_PUBLISHER_SHA256 = "43eecfa4f91a1c3b72e0c29e12ace88d1797f4b65c1d32bcb456e5f78287799f"
V19_USAGE_POLICY_SHA256 = "4cb200a193191a799a44673fd8bd2755ced118b994c5132340ee62e7d4bd052b"
V19_PUBLISHER_MARKER = "Brainwaves v21"
V19_PUBLISHER_ROOT = pathlib.Path("/Users/mihaiperdum/Projects/LeanZero-website")
V19_PUBLISHER_PATH = (
    V19_STATE_DIR / "closure-instrument" / "seed-fleet-brainwaves-sb70.mjs"
)
V19_USAGE_POLICY_PATH = V19_STATE_DIR / "closure-instrument" / "usage_impairment.py"
V19_BOUND_LAUNCH_SHA256 = "ebc79bd7ccf2ee119242226ab2e1421ae8dff4252af8f097b8e2b278c3e097ff"
V19_BOUND_RUN_ID = "swarm-20260826-013036983"
V19_BOUND_PROCESSES = {
    "harness": {
        "pid": 75049,
        "identity_sha256": "f3a1d41309176154b47bfd5a351701c90da156d3494e6bf350d9f6c1bcd1eca1",
    },
    "goose": {
        "pid": 75054,
        "identity_sha256": "93d4f8a0570710a0c15ce50f7f279e4a12af2cf32624f497e2a28fb8e7bd975e",
    },
    "monitor": {
        "pid": 75045,
        "identity_sha256": "06bcba6ce61f86cbf082823a2aa6ce5f3ab470ac063b041fee812057bb4c7b99",
    },
}
# END TERMINAL_CLOSURE_RUN_BINDING
SENSITIVE_NAME_RE = re.compile(r"(?:token|secret|authorization|api[_-]?key|password)", re.I)
SENSITIVE_TEXT_PATTERNS = (
    re.compile(r"(authorization\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+", re.I),
    re.compile(r"(x-reval-key\s*[:=]\s*)[^\s,;]+", re.I),
    re.compile(r"((?:api[_-]?key|token|secret|password)\s*[:=]\s*)[^\s,;]+", re.I),
    re.compile(r"bearer\s+[A-Za-z0-9._~+/=-]+", re.I),
)
TERMINAL_PHASES = {"complete", "failed", "stopped"}
RECURRENCE_OBSERVATION_REASON = (
    "measured full-stream recurrence grew while structured output did not progress"
)
RECURRENCE_OBSERVATION_SOURCE = "full-stream-reasoning-recurrence-meter"
SB7_TIERS = frozenset({"A", "B", "C", "D", "J", "V", "P", "T", "X", "R", "E"})
PROBE_DEGRADATION_RE = re.compile(
    r"(?:probe[\s_-]*(?:unavailable|error)|_probe_error|harness[\s_-]*(?:missing|failure)|"
    r"product_probe[^\n]*(?:missing|failed|error|unavailable))",
    re.I,
)
PLAYWRIGHT_SMOKE_SOURCE = r"""
const { createRequire } = require('node:module');
const { join, sep } = require('node:path');
const { realpathSync } = require('node:fs');

const moduleRoot = process.argv[1];
const expectedVersion = process.argv[2];
const probeScript = process.argv[3];
const load = createRequire(join(moduleRoot, '__terminal_closure__.cjs'));
const probeLoad = createRequire(probeScript);

(async () => {
  const packageJson = load(join(moduleRoot, 'package.json'));
  if (packageJson.version !== expectedVersion) {
    throw new Error(`Playwright version ${packageJson.version} differs from ${expectedVersion}`);
  }
  const expectedPackage = realpathSync(join(moduleRoot, 'package.json'));
  const resolvedPackage = realpathSync(probeLoad.resolve('playwright/package.json'));
  if (resolvedPackage !== expectedPackage) {
    throw new Error(`probe resolved an unpinned Playwright package: ${resolvedPackage}`);
  }
  const resolvedEntry = realpathSync(probeLoad.resolve('playwright'));
  const realModuleRoot = realpathSync(moduleRoot);
  if (!resolvedEntry.startsWith(`${realModuleRoot}${sep}`)) {
    throw new Error(`probe resolved an unpinned Playwright module: ${resolvedEntry}`);
  }
  const playwright = probeLoad('playwright');
  const browser = await playwright.chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent('<!doctype html><title>terminal-closure-playwright-smoke</title>');
    if (await page.title() !== 'terminal-closure-playwright-smoke') {
      throw new Error('Playwright browser smoke page returned the wrong title');
    }
  } finally {
    await browser.close();
  }
  process.stdout.write(JSON.stringify({
    ok: true,
    browser: 'chromium',
    headless: true,
    pinnedModule: true,
    modulePackage: resolvedPackage,
    version: packageJson.version,
  }));
})().catch((error) => {
  process.stderr.write(`${error && error.message ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
"""
SCORER_GATE_SOURCE = r"""
import ctypes
import hashlib
import json
import os
import runpy
import signal
import struct
import subprocess
import sys

gate_fd = int(sys.argv[1])
gate_token = os.read(gate_fd, 1)
os.close(gate_fd)
if gate_token != b"1":
    raise SystemExit(126)

spawn_journal = sys.argv[2]
scorer_argv = sys.argv[3:]
real_popen = subprocess.Popen


def process_identity_sha256(pid):
    probe = real_popen(
        ["ps", "-p", str(pid), "-o", "pid=", "-o", "lstart=", "-o", "comm="],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    stdout, _ = probe.communicate()
    identity = stdout.strip()
    if probe.returncode != 0 or not identity:
        return None
    return hashlib.sha256(identity).hexdigest()


def process_birth_sha256(pid):
    if sys.platform == "darwin":
        buffer = ctypes.create_string_buffer(136)
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        proc_pidinfo = libproc.proc_pidinfo
        proc_pidinfo.argtypes = [
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint64,
            ctypes.c_void_p,
            ctypes.c_int,
        ]
        proc_pidinfo.restype = ctypes.c_int
        if proc_pidinfo(pid, 3, 0, buffer, len(buffer)) != len(buffer):
            return None
        observed_pid = struct.unpack_from("=I", buffer.raw, 12)[0]
        started_seconds, started_microseconds = struct.unpack_from(
            "=QQ", buffer.raw, 120
        )
        if observed_pid != pid or started_seconds <= 0:
            return None
        birth = f"{pid}:{started_seconds}:{started_microseconds}".encode()
        return hashlib.sha256(birth).hexdigest()
    if sys.platform.startswith("linux"):
        try:
            boot_id = open("/proc/sys/kernel/random/boot_id", "rb").read().strip()
            stat_line = open(f"/proc/{pid}/stat", "rb").read().strip()
            fields = stat_line[stat_line.rfind(b")") + 2 :].split()
            start_ticks = fields[19]
        except (OSError, IndexError):
            return None
        return hashlib.sha256(
            str(pid).encode() + b":" + boot_id + b":" + start_ticks
        ).hexdigest()
    return None


def append_owned_identity(pid, identity, birth):
    entry = json.dumps(
        {
            "pid": pid,
            "identity_sha256s": [identity],
            "birth_sha256s": [birth],
        },
        sort_keys=True,
        separators=(",", ":"),
    ).encode() + b"\n"
    output = os.open(spawn_journal, os.O_WRONLY | os.O_APPEND)
    try:
        if os.write(output, entry) != len(entry):
            raise RuntimeError("spawn identity journal write was incomplete")
        os.fsync(output)
    finally:
        os.close(output)


def terminate_failed_spawn(process):
    try:
        process_group = os.getpgid(process.pid)
    except OSError:
        process_group = None
    separate_group = process_group == process.pid
    try:
        if separate_group:
            os.killpg(process_group, signal.SIGKILL)
        elif process.poll() is None:
            process.kill()
    except OSError:
        pass
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try:
            process.kill()
        except OSError:
            pass
        process.wait()


class TrackedPopen(real_popen):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        try:
            identity = process_identity_sha256(self.pid)
            birth = process_birth_sha256(self.pid)
            if identity is None or birth is None:
                if self.poll() is None:
                    raise RuntimeError("spawned process identity could not be authenticated")
                return
            append_owned_identity(self.pid, identity, birth)
        except BaseException:
            terminate_failed_spawn(self)
            raise


subprocess.Popen = TrackedPopen
sys.argv = scorer_argv
runpy.run_path(scorer_argv[0], run_name="__main__")
"""


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


def process_birth_sha256(pid: int) -> str | None:
    if sys.platform == "darwin":
        buffer = ctypes.create_string_buffer(136)
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        proc_pidinfo = libproc.proc_pidinfo
        proc_pidinfo.argtypes = [
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint64,
            ctypes.c_void_p,
            ctypes.c_int,
        ]
        proc_pidinfo.restype = ctypes.c_int
        if proc_pidinfo(pid, 3, 0, buffer, len(buffer)) != len(buffer):
            return None
        observed_pid = struct.unpack_from("=I", buffer.raw, 12)[0]
        started_seconds, started_microseconds = struct.unpack_from(
            "=QQ", buffer.raw, 120
        )
        if observed_pid != pid or started_seconds <= 0:
            return None
        birth = f"{pid}:{started_seconds}:{started_microseconds}".encode()
        return sha256_bytes(birth)
    if sys.platform.startswith("linux"):
        try:
            boot_id = pathlib.Path("/proc/sys/kernel/random/boot_id").read_bytes().strip()
            stat_line = pathlib.Path(f"/proc/{pid}/stat").read_bytes().strip()
            fields = stat_line[stat_line.rfind(b")") + 2 :].split()
            start_ticks = fields[19]
        except (OSError, IndexError):
            return None
        return sha256_bytes(str(pid).encode() + b":" + boot_id + b":" + start_ticks)
    return None


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


def create_once(path: pathlib.Path, payload: bytes, mode: int = 0o400) -> bool:
    ensure_secure_dir(path.parent)
    if path.is_symlink():
        raise ClosureError(f"create-only target is symbolic: {path}")
    if path.exists():
        if not path.is_file() or path.read_bytes() != payload:
            raise ClosureError(f"create-only target already exists with different content: {path}")
        if stat.S_IMODE(path.stat().st_mode) != mode:
            raise ClosureError(f"create-only target mode changed: {path}")
        return False
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{os.urandom(6).hex()}.tmp")
    descriptor = os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, mode)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        temporary.chmod(mode)
        try:
            os.link(temporary, path, follow_symlinks=False)
            created = True
        except FileExistsError:
            if path.is_symlink() or not path.is_file() or path.read_bytes() != payload:
                raise ClosureError(
                    f"create-only target won a race with different content: {path}"
                )
            if stat.S_IMODE(path.stat().st_mode) != mode:
                raise ClosureError(f"create-only target mode changed: {path}")
            created = False
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        return created
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


def decode_json_object(payload: bytes, identity: str) -> dict[str, Any]:
    def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise ClosureError(f"{identity} repeats JSON key {key!r}")
            value[key] = item
        return value

    try:
        value = json.loads(payload, object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ClosureError(f"{identity} is malformed JSON") from error
    if not isinstance(value, dict):
        raise ClosureError(f"{identity} is not a JSON object")
    return value


def read_private_bytes(
    path: pathlib.Path,
    *,
    immutable: bool = False,
    maximum_bytes: int = 32 * 1024 * 1024,
) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise ClosureError(f"private evidence is missing or linked: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        mode = stat.S_IMODE(before.st_mode)
        if not stat.S_ISREG(before.st_mode):
            raise ClosureError(f"private evidence is not a regular file: {path}")
        if mode & 0o077:
            raise ClosureError(f"private evidence is group/world accessible: {path}")
        if immutable and mode & 0o222:
            raise ClosureError(f"immutable evidence is writable: {path}")
        if before.st_size > maximum_bytes:
            raise ClosureError(f"private evidence exceeds its size bound: {path}")
        chunks: list[bytes] = []
        size = 0
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, maximum_bytes + 1 - size))
            if not chunk:
                break
            chunks.append(chunk)
            size += len(chunk)
            if size > maximum_bytes:
                raise ClosureError(f"private evidence exceeds its size bound: {path}")
        after_descriptor = os.fstat(descriptor)
        after_path = path.lstat()
    finally:
        os.close(descriptor)
    identity_before = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
    )
    descriptor_after = (
        after_descriptor.st_dev,
        after_descriptor.st_ino,
        after_descriptor.st_mode,
        after_descriptor.st_size,
        after_descriptor.st_mtime_ns,
    )
    path_after = (
        after_path.st_dev,
        after_path.st_ino,
        after_path.st_mode,
        after_path.st_size,
        after_path.st_mtime_ns,
    )
    if identity_before != descriptor_after or identity_before != path_after:
        raise ClosureError(f"private evidence changed while it was read: {path}")
    return b"".join(chunks)


def read_private_json(path: pathlib.Path, *, immutable: bool = False) -> dict[str, Any]:
    return decode_json_object(
        read_private_bytes(path, immutable=immutable), str(path)
    )


def read_private_jsonl_first(path: pathlib.Path) -> tuple[dict[str, Any], str]:
    if path.is_symlink() or not path.is_file():
        raise ClosureError(f"first-trace evidence is missing or linked: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        if stat.S_IMODE(before.st_mode) & 0o077:
            raise ClosureError(f"first-trace evidence is group/world accessible: {path}")
        line = b""
        while b"\n" not in line and len(line) <= 1024 * 1024:
            chunk = os.read(descriptor, 64 * 1024)
            if not chunk:
                break
            line += chunk
        if b"\n" not in line or len(line) > 1024 * 1024:
            raise ClosureError(f"first-trace evidence lacks a bounded complete row: {path}")
        first = line.split(b"\n", 1)[0]
        after_path = path.lstat()
        if (
            before.st_dev,
            before.st_ino,
            before.st_mode,
        ) != (
            after_path.st_dev,
            after_path.st_ino,
            after_path.st_mode,
        ):
            raise ClosureError(f"first-trace evidence identity changed: {path}")
    finally:
        os.close(descriptor)
    return decode_json_object(first, f"{path}:1"), sha256_bytes(first)


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


def stable_process_receipt(
    pid: int, timeout_seconds: float = 2, stable_seconds: float = 0.15
) -> dict[str, Any] | None:
    deadline = time.monotonic() + timeout_seconds
    previous: dict[str, Any] | None = None
    stable_since = time.monotonic()
    while time.monotonic() < deadline:
        observed = safe_process_receipt(pid)
        if observed is None:
            return None
        if observed != previous:
            previous = observed
            stable_since = time.monotonic()
        elif time.monotonic() - stable_since >= stable_seconds:
            return observed
        time.sleep(0.02)
    return None


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


def terminal_completion_assessment(
    rows: Sequence[Mapping[str, Any]], expected_run_id: str
) -> dict[str, Any]:
    started_indices = [
        index for index, row in enumerate(rows) if row.get("event") == "run_started"
    ]
    complete_indices = [
        index for index, row in enumerate(rows) if row.get("event") == "complete_result"
    ]
    overview_indices = [
        index for index, row in enumerate(rows) if row.get("event") == "run_overview"
    ]
    finished_indices = [
        index for index, row in enumerate(rows) if row.get("event") == "run_finished"
    ]
    started_identity_exact = (
        len(started_indices) == 1
        and rows[started_indices[0]].get("run_id") == expected_run_id
    )
    marker_order_exact = (
        len(started_indices) == 1
        and len(complete_indices) == 1
        and len(finished_indices) == 1
        and started_indices[0] < complete_indices[0] < finished_indices[0]
        and len(overview_indices) == 1
        and started_indices[0] < overview_indices[0] < complete_indices[0]
    )
    if overview_indices:
        terminal_phase = "post_overview"
    elif complete_indices:
        terminal_phase = "post_complete_result"
    else:
        terminal_phase = "pre_complete_result"
    last_row = rows[-1] if rows else {}
    last_event = last_row.get("event")
    last_seq = last_row.get("seq")
    return {
        "run_started_count": len(started_indices),
        "complete_result_count": len(complete_indices),
        "run_overview_count": len(overview_indices),
        "run_finished_count": len(finished_indices),
        "started_identity_exact": started_identity_exact,
        "marker_order_exact": marker_order_exact,
        "terminal_complete": started_identity_exact and marker_order_exact,
        "terminal_phase": terminal_phase,
        "last_event": last_event if isinstance(last_event, str) else None,
        "last_seq": last_seq if isinstance(last_seq, int) else None,
    }


def monitor_completion_assessment(
    rows: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    started_indices = [
        index for index, row in enumerate(rows) if row.get("event") == "monitor_started"
    ]
    detected_indices = [
        index
        for index, row in enumerate(rows)
        if row.get("event") == "incident_detected"
    ]
    captured_indices = [
        index
        for index, row in enumerate(rows)
        if row.get("event") == "incident_captured"
    ]
    stop_indices = [
        index
        for index, row in enumerate(rows)
        if row.get("event") == "stop_after_capture"
    ]
    completed_indices = [
        index
        for index, row in enumerate(rows)
        if row.get("event") == "monitor_completed"
    ]

    clean_terminal = (
        len(started_indices) == 1
        and len(completed_indices) == 1
        and not detected_indices
        and not captured_indices
        and not stop_indices
        and started_indices[0] < completed_indices[0] == len(rows) - 1
        and rows[completed_indices[0]].get("outcome") == "run_finished"
    )
    if clean_terminal:
        return {
            "classification": "clean_terminal",
            "publication_fatal": False,
            "terminal_index": completed_indices[0],
            "reason": None,
        }

    observation_only = False
    if (
        len(started_indices) == 1
        and len(detected_indices) == 1
        and len(captured_indices) == 1
        and not stop_indices
        and not completed_indices
        and started_indices[0] < detected_indices[0] < captured_indices[0]
        and captured_indices[0] == len(rows) - 1
    ):
        monitor_started = rows[started_indices[0]]
        detected = rows[detected_indices[0]]
        captured = rows[captured_indices[0]]
        evidence = detected.get("evidence")
        recurrence_share = monitor_started.get("recurrence_share")
        repeated_windows = monitor_started.get("repeated_windows")
        confirmations = monitor_started.get("confirmations")
        observation_only = (
            monitor_started.get("stop_on_incident") is False
            and monitor_started.get("recurrence_source")
            == RECURRENCE_OBSERVATION_SOURCE
            and detected.get("reason") == RECURRENCE_OBSERVATION_REASON
            and isinstance(evidence, Mapping)
            and evidence.get("source") == RECURRENCE_OBSERVATION_SOURCE
            and evidence.get("tail_reasoning_used") is False
            and finite_number(recurrence_share, 0, 1)
            and finite_number(evidence.get("repeat_share"), float(recurrence_share), 1)
            and evidence.get("repeat_share_gate") == recurrence_share
            and isinstance(repeated_windows, int)
            and not isinstance(repeated_windows, bool)
            and repeated_windows > 0
            and isinstance(evidence.get("repeated_windows"), int)
            and not isinstance(evidence.get("repeated_windows"), bool)
            and evidence["repeated_windows"] >= repeated_windows
            and isinstance(confirmations, int)
            and not isinstance(confirmations, bool)
            and confirmations >= 2
            and evidence.get("required_corroborations") == confirmations
            and isinstance(evidence.get("corroboration_streak"), int)
            and not isinstance(evidence.get("corroboration_streak"), bool)
            and evidence["corroboration_streak"] >= confirmations
            and finite_number(evidence.get("thinking_growth"), 0)
            and evidence["thinking_growth"] > 0
            and finite_number(evidence.get("structured_output_growth"))
            and evidence["structured_output_growth"] <= 0
            and isinstance(captured.get("incident_dir"), str)
            and bool(captured["incident_dir"])
        )
    if observation_only:
        return {
            "classification": "observation_only",
            "publication_fatal": False,
            "detected_index": detected_indices[0],
            "terminal_index": captured_indices[0],
            "reason": RECURRENCE_OBSERVATION_REASON,
        }

    return {
        "classification": "publication_fatal",
        "publication_fatal": True,
        "terminal_index": None,
        "reason": "monitor terminal evidence is incomplete or contains a material incident",
    }


def path_is_within(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def validate_observation_capture(
    run_dir: pathlib.Path,
    monitor_rows: Sequence[Mapping[str, Any]],
    assessment: Mapping[str, Any],
) -> dict[str, Any]:
    if assessment.get("classification") != "observation_only":
        raise ClosureError("monitor observation capture requested for another classification")
    detected_index = assessment.get("detected_index")
    captured_index = assessment.get("terminal_index")
    if (
        not isinstance(detected_index, int)
        or isinstance(detected_index, bool)
        or not isinstance(captured_index, int)
        or isinstance(captured_index, bool)
        or not 0 <= detected_index < captured_index < len(monitor_rows)
    ):
        raise ClosureError("monitor observation indices are malformed")
    detected = monitor_rows[detected_index]
    captured = monitor_rows[captured_index]
    capture_root = run_dir.resolve() / ".swarm-monitor" / "incidents"
    incident_dir = pathlib.Path(str(captured.get("incident_dir", "")))
    if (
        not incident_dir.is_absolute()
        or capture_root.is_symlink()
        or incident_dir.is_symlink()
        or not incident_dir.is_dir()
        or incident_dir.parent.resolve() != capture_root.resolve()
    ):
        raise ClosureError("monitor observation capture escaped its incident root")

    captured_paths = list(incident_dir.rglob("*"))
    if any(path.is_symlink() for path in captured_paths):
        raise ClosureError("monitor observation capture contains a symbolic link")
    if any(not path.is_dir() and not path.is_file() for path in captured_paths):
        raise ClosureError("monitor observation capture contains a special file")
    files = {
        path.relative_to(incident_dir).as_posix(): path
        for path in captured_paths
        if path.is_file()
    }
    required_files = {"incident.json", "manifest.sha256", "CAPTURE_COMPLETE"}
    if not required_files.issubset(files):
        raise ClosureError("monitor observation capture is incomplete")

    manifest_payload = files["manifest.sha256"].read_bytes()
    if (
        not manifest_payload
        or len(manifest_payload) > 1024 * 1024
        or not manifest_payload.endswith(b"\n")
    ):
        raise ClosureError("monitor observation capture manifest is malformed")
    manifest: dict[str, str] = {}
    for raw_line in manifest_payload.splitlines():
        try:
            digest, relative = raw_line.decode("utf-8").split("  ", 1)
        except (UnicodeDecodeError, ValueError) as error:
            raise ClosureError(
                "monitor observation capture manifest is malformed"
            ) from error
        relative_path = pathlib.PurePosixPath(relative)
        if (
            not SHA256_RE.fullmatch(digest)
            or not relative
            or relative_path.is_absolute()
            or ".." in relative_path.parts
            or relative in manifest
        ):
            raise ClosureError("monitor observation capture manifest is malformed")
        manifest[relative] = digest
    captured_evidence_files = set(files) - {"manifest.sha256", "CAPTURE_COMPLETE"}
    if set(manifest) != captured_evidence_files:
        raise ClosureError("monitor observation capture manifest inventory differs")
    for relative, digest in manifest.items():
        if sha256_file(files[relative]) != digest:
            raise ClosureError("monitor observation capture manifest hash differs")

    incident = read_json(files["incident.json"])
    capture_complete = read_json(files["CAPTURE_COMPLETE"])
    if (
        incident.get("reason") != RECURRENCE_OBSERVATION_REASON
        or incident.get("reason") != detected.get("reason")
        or canonical_json(incident.get("evidence"))
        != canonical_json(detected.get("evidence"))
        or incident.get("run_dir") != str(run_dir.resolve())
        or not isinstance(incident.get("pid"), int)
        or isinstance(incident.get("pid"), bool)
        or capture_complete.get("pid") != incident.get("pid")
        or not isinstance(capture_complete.get("captured_at"), str)
    ):
        raise ClosureError("monitor observation capture identity differs")
    return {
        "schema_version": SCHEMA_VERSION,
        "classification": "observation_only",
        "reason": RECURRENCE_OBSERVATION_REASON,
        "incident_detected_sha256": sha256_bytes(canonical_json(detected)),
        "incident_captured_sha256": sha256_bytes(canonical_json(captured)),
        "incident_json_sha256": sha256_file(files["incident.json"]),
        "incident_manifest_sha256": sha256_bytes(manifest_payload),
        "capture_complete_sha256": sha256_file(files["CAPTURE_COMPLETE"]),
    }


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


def validate_playwright_runtime(
    runtime: Mapping[str, Any], render_node: pathlib.Path, render_node_sha256: str
) -> dict[str, Any]:
    required = {
        "module_root",
        "module_tree_sha256",
        "version",
        "browsers_json",
        "browsers_json_sha256",
        "browser_name",
        "browser_revision",
        "installed_browsers_path",
        "browser_directory",
        "browser_tree_sha256",
        "executable",
        "executable_sha256",
    }
    if set(runtime) != required:
        raise ClosureError("Playwright runtime contract fields changed")
    for field in (
        "module_tree_sha256",
        "browsers_json_sha256",
        "browser_tree_sha256",
        "executable_sha256",
    ):
        if not re.fullmatch(r"[0-9a-f]{64}", str(runtime.get(field, ""))):
            raise ClosureError(f"Playwright runtime {field} must be a SHA-256")
    if render_node.is_symlink() or not render_node.is_file():
        raise ClosureError("Playwright Node runtime is not a regular file")
    if sha256_file(render_node) != render_node_sha256:
        raise ClosureError("Playwright Node runtime hash changed")

    configured_module_root = pathlib.Path(str(runtime["module_root"]))
    if configured_module_root.is_symlink() or not configured_module_root.is_dir():
        raise ClosureError("Playwright module root is not a real directory")
    module_root = configured_module_root.resolve()
    module_manifest = tree_manifest(module_root)
    if module_manifest["tree_sha256"] != runtime["module_tree_sha256"]:
        raise ClosureError("Playwright module tree hash changed")
    package_json_path = module_root / "package.json"
    if package_json_path.is_symlink() or not package_json_path.is_file():
        raise ClosureError("Playwright package manifest is missing")
    package_json = read_json(package_json_path)
    if package_json.get("version") != runtime["version"]:
        raise ClosureError("Playwright package version changed")

    browsers_json_relative = pathlib.Path(str(runtime["browsers_json"]))
    if browsers_json_relative.is_absolute() or ".." in browsers_json_relative.parts:
        raise ClosureError("Playwright browsers.json path escaped its module root")
    browsers_json_path = module_root / browsers_json_relative
    if (
        browsers_json_path.is_symlink()
        or not browsers_json_path.is_file()
        or not path_is_within(browsers_json_path, module_root)
        or sha256_file(browsers_json_path) != runtime["browsers_json_sha256"]
    ):
        raise ClosureError("Playwright browsers.json hash changed")
    browsers_json = read_json(browsers_json_path)
    browser_rows = [
        row
        for row in browsers_json.get("browsers", [])
        if isinstance(row, dict) and row.get("name") == runtime["browser_name"]
    ]
    if len(browser_rows) != 1 or str(browser_rows[0].get("revision")) != str(
        runtime["browser_revision"]
    ):
        raise ClosureError("Playwright browser revision differs from browsers.json")
    expected_directory = (
        str(runtime["browser_name"]).replace("-", "_")
        + "-"
        + str(runtime["browser_revision"])
    )
    if runtime["browser_directory"] != expected_directory:
        raise ClosureError("Playwright browser directory differs from its pinned revision")

    configured_browsers_path = pathlib.Path(str(runtime["installed_browsers_path"]))
    if configured_browsers_path.is_symlink() or not configured_browsers_path.is_dir():
        raise ClosureError("installed Playwright browser root is not a real directory")
    installed_browsers_path = configured_browsers_path.resolve()
    browser_dir = installed_browsers_path / expected_directory
    if browser_dir.is_symlink() or not browser_dir.is_dir():
        raise ClosureError("pinned Playwright browser revision is missing")
    browser_manifest = tree_manifest(browser_dir)
    if browser_manifest["tree_sha256"] != runtime["browser_tree_sha256"]:
        raise ClosureError("Playwright browser runtime tree hash changed")

    executable_relative = pathlib.Path(str(runtime["executable"]))
    if executable_relative.is_absolute() or ".." in executable_relative.parts:
        raise ClosureError("Playwright executable escaped its pinned browser directory")
    executable = browser_dir / executable_relative
    if (
        executable.is_symlink()
        or not executable.is_file()
        or not path_is_within(executable, browser_dir)
        or not os.access(executable, os.X_OK)
        or sha256_file(executable) != runtime["executable_sha256"]
    ):
        raise ClosureError("Playwright browser executable hash/mode changed")
    return {
        "module_root": str(module_root),
        "module_search_path": str(module_root.parent),
        "module_tree_sha256": module_manifest["tree_sha256"],
        "version": runtime["version"],
        "browser_name": runtime["browser_name"],
        "browser_revision": str(runtime["browser_revision"]),
        "browser_directory": expected_directory,
        "browser_dir": str(browser_dir),
        "browser_tree_sha256": browser_manifest["tree_sha256"],
        "executable": str(executable),
        "executable_sha256": runtime["executable_sha256"],
    }


def prepare_playwright_browser_view(
    runtime_info: Mapping[str, Any], view_root: pathlib.Path
) -> pathlib.Path:
    ensure_secure_dir(view_root)
    browser_link = view_root / str(runtime_info["browser_directory"])
    browser_dir = pathlib.Path(str(runtime_info["browser_dir"])).resolve()
    if os.path.lexists(browser_link):
        if not browser_link.is_symlink() or browser_link.resolve() != browser_dir:
            raise ClosureError("private Playwright browser view identity changed")
    else:
        os.symlink(browser_dir, browser_link, target_is_directory=True)
        directory = os.open(view_root, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    return view_root


def create_playwright_node_wrapper(
    wrapper_path: pathlib.Path,
    render_node: pathlib.Path,
    runtime_info: Mapping[str, Any],
    browser_view: pathlib.Path,
) -> str:
    payload = (
        f"#!{sys.executable}\n"
        "import os\n"
        "import sys\n"
        "\n"
        "environment = dict(os.environ)\n"
        f"environment['NODE_PATH'] = {str(runtime_info['module_search_path'])!r}\n"
        f"environment['PLAYWRIGHT_BROWSERS_PATH'] = {str(browser_view)!r}\n"
        f"node = {str(render_node)!r}\n"
        "os.execve(node, [node, *sys.argv[1:]], environment)\n"
    ).encode()
    if os.path.lexists(wrapper_path):
        if (
            wrapper_path.is_symlink()
            or not wrapper_path.is_file()
            or wrapper_path.read_bytes() != payload
        ):
            raise ClosureError("private Playwright Node wrapper identity changed")
        wrapper_path.chmod(0o500)
    else:
        atomic_write(wrapper_path, payload, 0o500)
    return sha256_file(wrapper_path)


def smoke_playwright_runtime(
    runtime_info: Mapping[str, Any],
    render_node: pathlib.Path,
    runtime_home: pathlib.Path,
    runtime_tmp: pathlib.Path,
    browser_view: pathlib.Path,
    probe_script: pathlib.Path,
    timeout_seconds: float = 60,
) -> dict[str, Any]:
    for directory in (runtime_home, runtime_tmp):
        ensure_secure_dir(directory)
    if browser_view.is_symlink() or not browser_view.is_dir():
        raise ClosureError("private Playwright browser view is not a real directory")
    if probe_script.is_symlink() or not probe_script.is_file():
        raise ClosureError("frozen Playwright product probe is not a regular file")
    inherited_path = safe_environment().get("PATH", "")
    pinned_path = str(render_node.parent)
    if inherited_path:
        pinned_path += os.pathsep + inherited_path
    environment = safe_environment(
        {
            "HOME": str(runtime_home),
            "TMPDIR": str(runtime_tmp),
            "NODE_PATH": str(runtime_info["module_search_path"]),
            "PATH": pinned_path,
            "PLAYWRIGHT_BROWSERS_PATH": str(browser_view),
        }
    )
    process = subprocess.Popen(
        [
            str(render_node),
            "-e",
            PLAYWRIGHT_SMOKE_SOURCE,
            str(runtime_info["module_root"]),
            str(runtime_info["version"]),
            str(probe_script),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as error:
        terminate_process_group(process.pid)
        stdout, stderr = process.communicate()
        raise ClosureError("pinned Playwright browser smoke launch timed out") from error
    if process.returncode != 0:
        detail = redact_text(stderr.decode("utf-8", "replace")[:600]).strip()
        raise ClosureError(
            "pinned Playwright browser smoke launch failed"
            + (f": {detail}" if detail else "")
        )
    try:
        receipt = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise ClosureError("pinned Playwright browser smoke receipt is malformed") from error
    if receipt != {
        "ok": True,
        "browser": "chromium",
        "headless": True,
        "pinnedModule": True,
        "modulePackage": str(
            (pathlib.Path(str(runtime_info["module_root"])) / "package.json").resolve()
        ),
        "version": runtime_info["version"],
    }:
        raise ClosureError("pinned Playwright browser smoke receipt differs")
    return receipt


def preflight_playwright_runtime(
    runtime: Mapping[str, Any],
    render_node: pathlib.Path,
    render_node_sha256: str,
    probe_script: pathlib.Path,
) -> dict[str, Any]:
    before = validate_playwright_runtime(runtime, render_node, render_node_sha256)
    with tempfile.TemporaryDirectory(prefix="sb7-playwright-preflight-") as temporary:
        runtime_root = pathlib.Path(temporary)
        runtime_home = runtime_root / "home"
        runtime_tmp = runtime_root / "tmp"
        browser_view = prepare_playwright_browser_view(
            before, runtime_root / "playwright-browsers"
        )
        resolution_receipt = smoke_playwright_runtime(
            before,
            render_node,
            runtime_home,
            runtime_tmp,
            browser_view,
            probe_script,
        )
    after = validate_playwright_runtime(runtime, render_node, render_node_sha256)
    if before != after:
        raise ClosureError("Playwright runtime changed during its preflight launch")
    return {**before, "resolution_receipt": resolution_receipt}


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
    for field in ("probe_unavailable", "harness_missing"):
        evidence = score.get(field)
        if not isinstance(evidence, list):
            raise ClosureError(f"score {field} evidence is missing or malformed")
        if evidence:
            raise ClosureError(f"score contains degraded product-probe evidence in {field}")
    if not isinstance(score.get("sched_unreached"), list):
        raise ClosureError("score sched_unreached evidence is missing or malformed")
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
        if row.get("unavailable") is True:
            raise ClosureError(f"score check {name} is probe-unavailable")
        evidence_text = "\n".join(
            str(row.get(field, "")) for field in ("detail", "consequence", "reason")
        )
        if PROBE_DEGRADATION_RE.search(evidence_text):
            raise ClosureError(f"score check {name} contains degraded product-probe evidence")
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
    usage_contract = expected.get("usage_contract")
    if usage_contract is None:
        if set(telemetry["nodes"]) != set(expected["telemetry_nodes"]):
            raise ClosureError("score telemetry node identity differs")
    else:
        try:
            usage_policy.validate_score_telemetry(
                telemetry,
                usage_contract,
                expected_nodes=expected["telemetry_nodes"],
            )
        except usage_policy.UsageEvidenceError as error:
            raise ClosureError(f"score usage evidence is invalid: {error}") from error
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


def validate_v19_config(config: dict[str, Any], *, allow_unarmed: bool) -> None:
    expected = config["expected"]
    publication = config["publication"]
    publisher = config["publisher"]
    if config.get("closure_generation") != V19_GENERATION:
        raise ClosureError("v19 closure generation identity changed")
    serialized_config = canonical_json(config).decode("utf-8")
    if any(
        stale in serialized_config
        for stale in (
            "local-sb7-engine-v17",
            "local-sb7-engine-v18",
            "local-sb7-engine-v19",
            "local-sb7-engine-v20",
            "local-sb7-engine-v21-r1",
        )
    ):
        raise ClosureError("v21 closure config contains stale run provenance")
    if any(
        stale in serialized_config
        for stale in (
            "Brainwaves v17",
            "Brainwaves v18",
            "Brainwaves v19",
            "Brainwaves v20",
        )
    ):
        raise ClosureError("v21 closure config contains stale publication provenance")
    exact_paths = {
        "live_root": V19_LIVE_ROOT,
        "run_dir": V19_RUN_DIR,
        "state_dir": V19_STATE_DIR,
        "score_lock_path": V19_SCORE_LOCK,
        "bound_config_path": V19_BOUND_CONFIG,
    }
    for field, exact in exact_paths.items():
        if config.get(field) != str(exact):
            raise ClosureError(f"v19 {field} differs from its exact closure path")
    exact_expected = {
        "candidate_commit": V19_CANDIDATE_COMMIT,
        "candidate_tree": V19_CANDIDATE_TREE,
        "binary_path": str(V19_BINARY),
        "binary_sha256": V19_BINARY_SHA256,
        "launch_controller_path": str(V19_LAUNCHER),
    }
    for field, exact in exact_expected.items():
        if expected.get(field) != exact:
            raise ClosureError(f"v19 expected.{field} changed")
    if expected.get("launch_controller_sha256") != V19_LAUNCHER_SHA256:
        raise ClosureError("v19 launcher source hash changed")
    if expected.get("instrument_manifest_sha256") != V19_INSTRUMENT_MANIFEST_SHA256:
        raise ClosureError("v19 instrument manifest hash changed")
    if publication.get("target_document_id") != V19_TARGET_DOCUMENT_ID:
        raise ClosureError("v19 publication target changed")
    if publication.get("protected_document_ids") != sorted(
        V19_PROTECTED_DOCUMENT_IDS
    ):
        raise ClosureError("v19 protected publication identities changed")
    if publication.get("provenance_marker") != V19_PUBLISHER_MARKER:
        raise ClosureError("v19 publication provenance marker changed")
    if publisher.get("git_commit") != V19_PUBLISHER_COMMIT:
        raise ClosureError("v19 publisher Git commit changed")
    if publisher.get("sha256") != V19_PUBLISHER_SHA256:
        raise ClosureError("v19 publisher source hash changed")
    if publisher.get("site_root") != str(V19_PUBLISHER_ROOT):
        raise ClosureError("v19 publisher repository path changed")
    allowed_publisher_paths = {
        V19_PUBLISHER_PATH.resolve(),
        (
            V19_STATE_DIR
            / "closure-instrument"
            / "seed-fleet-brainwaves-sb70.mjs"
        ).resolve(),
    }
    if pathlib.Path(str(publisher.get("path", ""))).resolve() not in allowed_publisher_paths:
        raise ClosureError("v19 publisher path changed")
    armed = config.get("armed")
    if not isinstance(armed, bool):
        raise ClosureError("v19 closure armed state is not boolean")
    dynamic_hashes = (
        "launch_sha256",
        "run_started_sha256",
        "trace_header_sha256",
        "fleet_seal_sha256",
        "fleet_binding_sha256",
    )
    if not armed:
        if not allow_unarmed:
            raise ClosureError("v19 closure config is unarmed")
        if config.get("binding") is not None:
            raise ClosureError("unarmed v19 closure config already contains binding evidence")
        if expected.get("run_id") is not None or expected.get("fixture_seed") is not None:
            raise ClosureError("unarmed v19 closure config already contains run identity")
        if expected.get("models") is not None or expected.get("instrument_files") not in (
            None,
            {},
        ):
            raise ClosureError("unarmed v19 closure config already contains live inventory")
        if any(expected.get(field) is not None for field in dynamic_hashes):
            raise ClosureError("unarmed v19 closure config already contains live evidence hashes")
        return
    if not RUN_ID_RE.fullmatch(str(expected.get("run_id", ""))):
        raise ClosureError("armed v19 closure run_id is malformed")
    if expected.get("run_id") != V19_BOUND_RUN_ID:
        raise ClosureError("armed v19 closure run_id differs from its frozen successor binding")
    if not SEED_RE.fullmatch(str(expected.get("fixture_seed", ""))):
        raise ClosureError("armed v19 closure fixture_seed must be exact 16-hex")
    if any(not SHA256_RE.fullmatch(str(expected.get(field, ""))) for field in dynamic_hashes):
        raise ClosureError("armed v19 closure lacks an exact bound evidence hash")
    models = expected.get("models")
    if (
        not isinstance(models, list)
        or len(models) != 3
        or len(set(models)) != 3
        or any(not isinstance(model, str) or not model for model in models)
    ):
        raise ClosureError("armed v19 closure model inventory is malformed")
    instrument_files = expected.get("instrument_files")
    if (
        not isinstance(instrument_files, dict)
        or not instrument_files
        or any(
            not isinstance(relative, str)
            or not relative
            or pathlib.Path(relative).is_absolute()
            or ".." in pathlib.Path(relative).parts
            or not SHA256_RE.fullmatch(str(digest))
            for relative, digest in instrument_files.items()
        )
    ):
        raise ClosureError("armed v19 closure instrument inventory is malformed")
    binding = config.get("binding")
    if not isinstance(binding, dict):
        raise ClosureError("armed v19 closure lacks binding evidence")
    expected_binding = {
        "generation": V19_GENERATION,
        "launch_path": str(V19_LIVE_ROOT / "launch.json"),
        "instrument_manifest_path": str(V19_LIVE_ROOT / "instrument-manifest.json"),
        "run_log_path": str(V19_RUN_DIR / "run.jsonl"),
        "trace_path": str(
            V19_LIVE_ROOT / "trace-swarm-3node-qwen38-brainwaves-r0.jsonl"
        ),
        "fleet_seal_path": str(V19_FLEET_SEAL),
    }
    for field, exact in expected_binding.items():
        if binding.get(field) != exact:
            raise ClosureError(f"armed v19 binding {field} changed")
    if binding.get("template_sha256") is None or not SHA256_RE.fullmatch(
        str(binding.get("template_sha256"))
    ):
        raise ClosureError("armed v19 binding template hash is malformed")
    if binding.get("launch_sha256") != expected["launch_sha256"]:
        raise ClosureError("armed v19 binding launch hash differs")
    if binding.get("launch_sha256") != V19_BOUND_LAUNCH_SHA256:
        raise ClosureError("armed v19 binding launch hash differs from its frozen successor binding")
    if binding.get("instrument_manifest_sha256") != expected[
        "instrument_manifest_sha256"
    ]:
        raise ClosureError("armed v19 binding manifest hash differs")
    if binding.get("run_started_sha256") != expected["run_started_sha256"]:
        raise ClosureError("armed v19 binding run_started hash differs")
    if binding.get("trace_header_sha256") != expected["trace_header_sha256"]:
        raise ClosureError("armed v19 binding trace-header hash differs")
    if binding.get("fleet_seal_sha256") != expected["fleet_seal_sha256"]:
        raise ClosureError("armed v19 binding fleet-seal hash differs")
    fleet_binding = binding.get("fleet_binding")
    validate_v19_fleet_binding(
        fleet_binding,
        fleet_seal_sha256=expected["fleet_seal_sha256"],
        model_ids=models,
    )
    fleet_binding_sha256 = sha256_bytes(canonical_json(fleet_binding))
    if (
        fleet_binding_sha256 != expected["fleet_binding_sha256"]
        or binding.get("fleet_binding_sha256") != fleet_binding_sha256
    ):
        raise ClosureError("armed v19 fleet binding hash differs")


def validate_config(config: dict[str, Any], *, allow_unarmed: bool = False) -> None:
    required_sections = {
        "expected",
        "publication",
        "publisher",
        "runtime",
        "usage_policy",
    }
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
        raise ClosureError("closure state must be outside the immutable live run root")
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
    if target in protected or protected != sorted(V19_PROTECTED_DOCUMENT_IDS):
        raise ClosureError("protected benchmark document set changed")
    if config["expected"]["vendor_port"] != 18970:
        raise ClosureError("advertised/scoring port must remain 18970")
    if config["expected"].get("entrant") != "swarm-3node-qwen38-brainwaves":
        raise ClosureError("entrant identity changed")
    if config["expected"].get("raw_scorer_version") != "sb-7.0-rc":
        raise ClosureError("raw scorer convention changed")
    if config["expected"].get("check_count") != 91:
        raise ClosureError("frozen scorer check count changed")
    if sorted(config["expected"].get("telemetry_nodes") or []) != [
        "gabee",
        "mihai",
        "workhorse",
    ]:
        raise ClosureError("telemetry node contract changed")
    for field in ("controller_sha256",):
        if not re.fullmatch(r"[0-9a-f]{64}", str(config.get(field, ""))):
            raise ClosureError(f"{field} must be a frozen SHA-256")
    for field in ("sha256", "node_sha256", "package_lock_sha256", "package_json_sha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(config["publisher"].get(field, ""))):
            raise ClosureError(f"publisher.{field} must be a frozen SHA-256")
    usage_policy = config["usage_policy"]
    allowed_usage_policy_paths = {
        V19_USAGE_POLICY_PATH.resolve(),
        (V19_STATE_DIR / "closure-instrument" / "usage_impairment.py").resolve(),
    }
    if (
        not isinstance(usage_policy, dict)
        or pathlib.Path(str(usage_policy.get("path", ""))).resolve()
        not in allowed_usage_policy_paths
        or usage_policy.get("sha256") != V19_USAGE_POLICY_SHA256
    ):
        raise ClosureError("configured immutable usage policy changed")
    playwright_runtime = config["runtime"].get("playwright")
    if not isinstance(playwright_runtime, dict):
        raise ClosureError("runtime.playwright contract is missing")
    for field in (
        "module_tree_sha256",
        "browsers_json_sha256",
        "browser_tree_sha256",
        "executable_sha256",
    ):
        if not re.fullmatch(r"[0-9a-f]{64}", str(playwright_runtime.get(field, ""))):
            raise ClosureError(f"runtime.playwright.{field} must be a frozen SHA-256")
    for field in ("module_root", "installed_browsers_path"):
        runtime_path = pathlib.Path(str(playwright_runtime.get(field, ""))).resolve()
        if path_is_within(runtime_path, live_root):
            raise ClosureError(f"runtime.playwright.{field} must be outside the live run tree")
    if int(config.get("max_score_attempts", 0)) < 1:
        raise ClosureError("max_score_attempts must be positive")
    if int(config.get("max_publish_attempts", 0)) < 1:
        raise ClosureError("max_publish_attempts must be positive")
    if float(config.get("score_timeout_seconds", 0)) <= 0:
        raise ClosureError("score_timeout_seconds must be positive")
    if config.get("closure_generation") == V19_GENERATION or "armed" in config:
        validate_v19_config(config, allow_unarmed=allow_unarmed)


def verify_immutable_file(
    path: pathlib.Path, expected_sha256: str, *, require_read_only: bool = True
) -> None:
    if path.is_symlink() or not path.is_file():
        raise ClosureError(f"immutable input is missing or linked: {path}")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ClosureError(f"immutable input is not a regular file: {path}")
        if require_read_only and stat.S_IMODE(before.st_mode) & 0o222:
            raise ClosureError(f"immutable input is writable: {path}")
        digest_state = hashlib.sha256()
        while True:
            block = os.read(descriptor, 1024 * 1024)
            if not block:
                break
            digest_state.update(block)
        after_descriptor = os.fstat(descriptor)
        after_path = path.lstat()
    finally:
        os.close(descriptor)
    before_identity = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
        before.st_mtime_ns,
    )
    if before_identity != (
        after_descriptor.st_dev,
        after_descriptor.st_ino,
        after_descriptor.st_mode,
        after_descriptor.st_size,
        after_descriptor.st_mtime_ns,
    ) or before_identity != (
        after_path.st_dev,
        after_path.st_ino,
        after_path.st_mode,
        after_path.st_size,
        after_path.st_mtime_ns,
    ):
        raise ClosureError(f"immutable input changed while it was hashed: {path}")
    if digest_state.hexdigest() != expected_sha256:
        raise ClosureError(f"immutable input hash changed: {path}")


def git_head(repository: pathlib.Path) -> str:
    completed = subprocess.run(
        ["git", "-C", str(repository), "rev-parse", "HEAD"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env=safe_environment(),
    )
    head = completed.stdout.decode("ascii", "strict").strip()
    if completed.returncode != 0 or not GIT_COMMIT_RE.fullmatch(head):
        raise ClosureError("publisher Git identity could not be authenticated")
    return head


def valid_v19_model_id(model_id: Any, role: Any) -> bool:
    prefix = V19_ROLE_PREFIXES.get(role)
    if (
        not prefix
        or not isinstance(model_id, str)
        or len(model_id) > 200
        or re.fullmatch(r"[a-z0-9][a-z0-9._-]*", model_id) is None
        or not model_id.startswith(prefix)
    ):
        return False
    return model_id in V19_EXACT_MODEL_ALIASES


def validate_v19_fleet_seal(
    seal: Mapping[str, Any], launch: Mapping[str, Any]
) -> list[str]:
    models = seal.get("models")
    model_ids = seal.get("model_ids")
    exact_model_aliases_verified = seal.get("exact_model_aliases_verified")
    if (
        seal.get("schema_version") != SCHEMA_VERSION
        or seal.get("source") != "authenticated-live-lm-studio-preflight"
        or not isinstance(models, list)
        or len(models) != 3
        or not isinstance(model_ids, list)
        or len(model_ids) != 3
        or len(set(model_ids)) != 3
        or seal.get("api_model_ids") != sorted(model_ids)
        or set(model_ids) != V19_EXACT_MODEL_ALIASES
        or exact_model_aliases_verified != sorted(V19_EXACT_MODEL_ALIASES)
        or "prior_model_aliases_reused" in seal
        or "protected_prior_aliases_reused" in seal
    ):
        raise ClosureError("v19 authenticated fleet seal is malformed or stale")
    identifiers: list[str] = []
    paths: set[str] = set()
    quantizations: set[bytes] = set()
    roles: set[str] = set()
    for row in models:
        if not isinstance(row, dict):
            raise ClosureError("v19 fleet seal model row is malformed")
        identifier = row.get("identifier")
        role = row.get("role")
        if (
            not valid_v19_model_id(identifier, role)
            or not isinstance(row.get("path"), str)
            or not row.get("path")
            or not isinstance(row.get("contextLength"), int)
            or row["contextLength"] != V19_EXACT_CONTEXT_BY_ROLE.get(role)
            or row.get("parallel") != 2
            or not isinstance(row.get("quantization"), dict)
            or not isinstance(row["quantization"].get("bits"), int)
            or row["quantization"]["bits"] <= 0
        ):
            raise ClosureError("v19 fleet seal model row changed")
        identifiers.append(identifier)
        roles.add(role)
        paths.add(row["path"])
        quantizations.add(canonical_json(row["quantization"]))
    if (
        roles != set(V19_ROLE_PREFIXES)
        or sorted(identifiers) != sorted(model_ids)
        or len(paths) != 1
        or len(quantizations) != 1
        or seal.get("planner_model") != V19_EXPECTED_PLANNER_MODEL
    ):
        raise ClosureError("v19 fleet seal physical/model identity does not reconcile")
    launch_seal = launch.get("fleet_seal")
    if (
        not isinstance(launch_seal, dict)
        or launch_seal.get("path") != str(V19_FLEET_SEAL)
        or sorted(launch_seal.get("model_ids") or []) != sorted(model_ids)
        or launch_seal.get("planner_model") != seal.get("planner_model")
    ):
        raise ClosureError("v19 launch/fleet-seal identity differs")
    return sorted(model_ids)


def v19_fleet_binding(
    seal: Mapping[str, Any], fleet_seal_sha256: str
) -> dict[str, Any]:
    if not SHA256_RE.fullmatch(fleet_seal_sha256):
        raise ClosureError("v19 fleet binding lacks its sealed source hash")
    rows: list[dict[str, Any]] = []
    for model in seal["models"]:
        artifact = {
            "artifact_path_sha256": sha256_bytes(str(model["path"]).encode()),
            "quantization_sha256": sha256_bytes(canonical_json(model["quantization"])),
        }
        rows.append(
            {
                "model_id": model["identifier"],
                "role": model["role"],
                **artifact,
                "context_length": model["contextLength"],
                "parallel": model["parallel"],
                "artifact_identity_sha256": sha256_bytes(canonical_json(artifact)),
            }
        )
    rows.sort(key=lambda row: (row["role"], row["model_id"]))
    return {
        "schema_version": SCHEMA_VERSION,
        "fleet_seal_sha256": fleet_seal_sha256,
        "model_ids": sorted(str(model_id) for model_id in seal["model_ids"]),
        "models": rows,
    }


def validate_v19_fleet_binding(
    binding: Any,
    *,
    fleet_seal_sha256: str,
    model_ids: list[str],
) -> None:
    if (
        not isinstance(binding, dict)
        or set(binding)
        != {"schema_version", "fleet_seal_sha256", "model_ids", "models"}
        or binding.get("schema_version") != SCHEMA_VERSION
        or binding.get("fleet_seal_sha256") != fleet_seal_sha256
        or binding.get("model_ids") != sorted(model_ids)
        or not isinstance(binding.get("models"), list)
        or len(binding["models"]) != 3
    ):
        raise ClosureError("v19 fleet binding identity is malformed")
    roles: set[str] = set()
    bound_model_ids: list[str] = []
    artifact_identities: set[str] = set()
    for row in binding["models"]:
        if not isinstance(row, dict) or set(row) != {
            "model_id",
            "role",
            "artifact_path_sha256",
            "context_length",
            "parallel",
            "quantization_sha256",
            "artifact_identity_sha256",
        }:
            raise ClosureError("v19 fleet binding model row is malformed")
        role = row.get("role")
        model_id = row.get("model_id")
        artifact = {
            "artifact_path_sha256": row.get("artifact_path_sha256"),
            "quantization_sha256": row.get("quantization_sha256"),
        }
        if (
            not valid_v19_model_id(model_id, role)
            or not SHA256_RE.fullmatch(str(artifact["artifact_path_sha256"]))
            or not isinstance(row.get("context_length"), int)
            or row["context_length"] != V19_EXACT_CONTEXT_BY_ROLE.get(role)
            or row.get("parallel") != 2
            or artifact["artifact_path_sha256"] != V19_EXPECTED_ARTIFACT_PATH_SHA256
            or artifact["quantization_sha256"] != V19_EXPECTED_QUANTIZATION_SHA256
            or row.get("artifact_identity_sha256")
            != sha256_bytes(canonical_json(artifact))
        ):
            raise ClosureError("v19 fleet binding model row changed")
        roles.add(role)
        bound_model_ids.append(model_id)
        artifact_identities.add(row["artifact_identity_sha256"])
    if (
        roles != set(V19_ROLE_PREFIXES)
        or sorted(bound_model_ids) != sorted(model_ids)
        or len(set(bound_model_ids)) != 3
        or len(artifact_identities) != 1
    ):
        raise ClosureError("v19 fleet binding physical/artifact identity differs")


def v19_binding_evidence(
    template_path: pathlib.Path, config: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, Any]]:
    validate_config(config, allow_unarmed=True)
    if config.get("armed") is not False:
        raise ClosureError("v19 binder requires the unarmed template")
    if template_path.is_symlink() or not template_path.is_file():
        raise ClosureError("v19 closure template is missing or linked")
    if sha256_file(pathlib.Path(__file__).resolve()) != config["controller_sha256"]:
        raise ClosureError("v19 closure controller differs from its unarmed template")
    launcher = pathlib.Path(config["expected"]["launch_controller_path"])
    verify_immutable_file(
        launcher, config["expected"]["launch_controller_sha256"]
    )
    publisher = pathlib.Path(config["publisher"]["path"])
    if publisher.resolve() != V19_PUBLISHER_PATH.resolve():
        raise ClosureError("v19 binder requires the accepted publisher source path")
    verify_immutable_file(
        publisher, config["publisher"]["sha256"], require_read_only=False
    )
    if git_head(pathlib.Path(config["publisher"]["site_root"])) != config[
        "publisher"
    ]["git_commit"]:
        raise ClosureError("v19 publisher repository is not at the accepted commit")

    launch_path = V19_LIVE_ROOT / "launch.json"
    manifest_path = V19_LIVE_ROOT / "instrument-manifest.json"
    launch_payload = read_private_bytes(launch_path)
    manifest_payload = read_private_bytes(manifest_path, immutable=True)
    launch = decode_json_object(launch_payload, str(launch_path))
    manifest = decode_json_object(manifest_payload, str(manifest_path))
    launch_sha256 = sha256_bytes(launch_payload)
    manifest_sha256 = sha256_bytes(manifest_payload)
    expected = config["expected"]
    if launch_sha256 != V19_BOUND_LAUNCH_SHA256:
        raise ClosureError("v19 launch receipt differs from its frozen successor binding")
    if launch.get("schema_version") != SCHEMA_VERSION:
        raise ClosureError("v19 launch receipt schema changed")
    if manifest_sha256 != expected["instrument_manifest_sha256"]:
        raise ClosureError("v19 instrument manifest differs from its unarmed pin")
    if launch.get("launch_controller_sha256") != expected[
        "launch_controller_sha256"
    ]:
        raise ClosureError("v19 launch receipt used a different launcher")
    candidate = launch.get("candidate")
    if not isinstance(candidate, dict) or candidate.get("commit") != V19_CANDIDATE_COMMIT:
        raise ClosureError("v19 launch candidate commit changed")
    if candidate.get("tree") != V19_CANDIDATE_TREE:
        raise ClosureError("v19 launch candidate tree changed")
    if (
        launch.get("binary")
        != {"path": str(V19_BINARY), "sha256": V19_BINARY_SHA256}
        or launch.get("vendor_port") != 18970
        or launch.get("entrant") != expected["entrant"]
        or launch.get("publication_document_id") != V19_TARGET_DOCUMENT_ID
        or launch.get("instrument_manifest_sha256") != manifest_sha256
    ):
        raise ClosureError("v19 launch static identity differs")
    verify_immutable_file(V19_BINARY, V19_BINARY_SHA256)
    if (
        manifest.get("schema_version") != SCHEMA_VERSION
        or manifest.get("candidate_commit") != V19_CANDIDATE_COMMIT
        or manifest.get("candidate_tree") != V19_CANDIDATE_TREE
        or manifest.get("binary") != V19_INSTRUMENT_RECORDED_BINARY
    ):
        raise ClosureError("v19 instrument manifest identity differs")
    policy = manifest.get("sb7_policy")
    if not isinstance(policy, dict) or policy != {
        "spec_and_scorer_unchanged_from_v6": True,
        "website_surface": "stable-sb7",
        "publish_from_run_build_auto_score": False,
        "entrant": expected["entrant"],
        "publication_document_id": V19_TARGET_DOCUMENT_ID,
        "protected_document_ids": sorted(V19_PROTECTED_DOCUMENT_IDS),
    }:
        raise ClosureError("v19 instrument publication policy changed")
    instrument_files = manifest.get("files")
    if not isinstance(instrument_files, dict) or not instrument_files:
        raise ClosureError("v19 instrument manifest has no frozen files")
    for relative, digest in instrument_files.items():
        relative_path = pathlib.Path(str(relative))
        if (
            relative_path.is_absolute()
            or ".." in relative_path.parts
            or not SHA256_RE.fullmatch(str(digest))
        ):
            raise ClosureError("v19 instrument manifest contains an invalid path/hash")
        verify_immutable_file(V19_LIVE_ROOT / "instrument" / relative_path, digest)

    fleet_receipt = launch.get("fleet_seal")
    if not isinstance(fleet_receipt, dict) or fleet_receipt.get("path") != str(
        V19_FLEET_SEAL
    ):
        raise ClosureError("v19 launch lacks its exact fleet-seal path")
    fleet_payload = read_private_bytes(V19_FLEET_SEAL)
    fleet_sha256 = sha256_bytes(fleet_payload)
    if fleet_receipt.get("sha256") != fleet_sha256:
        raise ClosureError("v19 fleet seal differs from its launch receipt")
    fleet = decode_json_object(fleet_payload, str(V19_FLEET_SEAL))
    models = validate_v19_fleet_seal(fleet, launch)
    fleet_binding = v19_fleet_binding(fleet, fleet_sha256)
    validate_v19_fleet_binding(
        fleet_binding,
        fleet_seal_sha256=fleet_sha256,
        model_ids=models,
    )
    fleet_binding_sha256 = sha256_bytes(canonical_json(fleet_binding))

    run_log_path = V19_RUN_DIR / "run.jsonl"
    run_started, run_started_sha256 = read_private_jsonl_first(run_log_path)
    if (
        run_started.get("event") != "run_started"
        or run_started.get("seq") != 0
        or run_started.get("assured") is not False
        or run_started.get("working_dir") != str(V19_RUN_DIR)
        or run_started.get("telemetry_file")
        != str(V19_RUN_DIR / ".swarm/telemetry.jsonl")
        or run_started.get("endpoint") != "http://localhost:1234"
        or run_started.get("max_attempts") != 3
        or run_started.get("max_turns") != 100000
        or not RUN_ID_RE.fullmatch(str(run_started.get("run_id", "")))
    ):
        raise ClosureError("v19 run_started identity is malformed")
    run_pool = run_started.get("pool")
    run_models = (
        sorted(str(row.get("model_id")) for row in run_pool)
        if isinstance(run_pool, list) and all(isinstance(row, dict) for row in run_pool)
        else []
    )
    launch_started = launch.get("run_started_identity")
    if (
        len(run_models) != 3
        or run_models != models
        or not isinstance(launch_started, dict)
        or launch_started.get("run_id") != V19_BOUND_RUN_ID
        or launch_started.get("run_id") != run_started["run_id"]
        or sorted(launch_started.get("pool_models") or []) != models
        or launch_started.get("planner_model") != fleet.get("planner_model")
        or run_started.get("planner_model") != fleet.get("planner_model")
    ):
        raise ClosureError("v19 launch/run_started/fleet identity differs")
    for role in ("harness", "goose", "monitor"):
        receipt = launch.get(role)
        expected_receipt = V19_BOUND_PROCESSES.get(role)
        if (
            not isinstance(receipt, dict)
            or not isinstance(expected_receipt, dict)
            or receipt.get("pid") != expected_receipt.get("pid")
            or receipt.get("identity_sha256")
            != expected_receipt.get("identity_sha256")
            or not validate_authenticated_process(role, receipt)
        ):
            raise ClosureError(f"v19 {role} is not live at the binding boundary")

    trace_path = V19_LIVE_ROOT / f"trace-{expected['entrant']}-r0.jsonl"
    trace_header, trace_header_sha256 = read_private_jsonl_first(trace_path)
    fixture_seed = trace_header.get("fixture_seed")
    if (
        trace_header.get("trace_header") != "meridian-v3"
        or trace_header.get("seq") != 1
        or not isinstance(fixture_seed, str)
        or not SEED_RE.fullmatch(fixture_seed)
    ):
        raise ClosureError("v19 first trace row lacks its original exact fixture_seed")
    binding = {
        "generation": V19_GENERATION,
        "template_sha256": sha256_file(template_path),
        "launch_path": str(launch_path),
        "launch_sha256": launch_sha256,
        "instrument_manifest_path": str(manifest_path),
        "instrument_manifest_sha256": manifest_sha256,
        "run_log_path": str(run_log_path),
        "run_started_sha256": run_started_sha256,
        "trace_path": str(trace_path),
        "trace_header_sha256": trace_header_sha256,
        "fleet_seal_path": str(V19_FLEET_SEAL),
        "fleet_seal_sha256": fleet_sha256,
        "fleet_binding_sha256": fleet_binding_sha256,
        "fleet_binding": fleet_binding,
    }
    observed = {
        "run_id": run_started["run_id"],
        "fixture_seed": fixture_seed,
        "models": models,
        "instrument_files": instrument_files,
        "binding": binding,
    }
    return observed, binding


def bind_v19(template_path: pathlib.Path) -> tuple[pathlib.Path, bool]:
    template_path = template_path.resolve()
    template = load_config(template_path)
    observed, binding = v19_binding_evidence(template_path, template)
    armed = json.loads(json.dumps(template))
    armed["armed"] = True
    armed["expected"].update(
        {
            "run_id": observed["run_id"],
            "fixture_seed": observed["fixture_seed"],
            "models": observed["models"],
            "instrument_files": observed["instrument_files"],
            "launch_sha256": binding["launch_sha256"],
            "instrument_manifest_sha256": binding[
                "instrument_manifest_sha256"
            ],
            "run_started_sha256": binding["run_started_sha256"],
            "trace_header_sha256": binding["trace_header_sha256"],
            "fleet_seal_sha256": binding["fleet_seal_sha256"],
            "fleet_binding_sha256": binding["fleet_binding_sha256"],
        }
    )
    armed["binding"] = {**binding, "bound_at": utc_now()}
    validate_config(armed)
    target = pathlib.Path(armed["bound_config_path"])
    if target.exists():
        existing = read_private_json(target, immutable=True)
        validate_config(existing)
        comparable_existing = json.loads(json.dumps(existing))
        comparable_new = json.loads(json.dumps(armed))
        comparable_existing.get("binding", {}).pop("bound_at", None)
        comparable_new.get("binding", {}).pop("bound_at", None)
        if comparable_existing != comparable_new:
            raise ClosureError("v19 closure was already bound to different evidence")
        return target, False
    payload = json.dumps(armed, indent=2, sort_keys=True).encode() + b"\n"
    created = create_once(target, payload, 0o400)
    persisted = read_private_json(target, immutable=True)
    if canonical_json(persisted) != canonical_json(armed):
        raise ClosureError("v19 armed closure config differs after create-only binding")
    return target, created


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

    def expected_fixture_seed(self, observed: Any = None) -> str:
        configured = self.config["expected"].get("fixture_seed")
        if configured is None:
            configured = observed
        if not isinstance(configured, str) or not SEED_RE.fullmatch(configured):
            raise ClosureError("closure lacks an exact 16-hex fixture_seed")
        if observed is not None and observed != configured:
            raise ClosureError("fixture_seed differs from the armed closure identity")
        return configured

    def bound_trace_header(self) -> dict[str, Any] | None:
        expected_hash = self.config["expected"].get("trace_header_sha256")
        if expected_hash is None:
            return None
        trace_path = self.live_root / f"trace-{self.config['expected']['entrant']}-r0.jsonl"
        header, digest = read_private_jsonl_first(trace_path)
        if digest != expected_hash:
            raise ClosureError("first trace evidence changed after v19 binding")
        if header.get("trace_header") != "meridian-v3" or header.get("seq") != 1:
            raise ClosureError("first trace evidence header identity changed")
        self.expected_fixture_seed(header.get("fixture_seed"))
        return header

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

    def authenticated_v19_fleet_binding(
        self, launch: Mapping[str, Any]
    ) -> dict[str, Any]:
        expected = self.config["expected"]
        fleet_payload = read_private_bytes(V19_FLEET_SEAL)
        fleet_seal_sha256 = sha256_bytes(fleet_payload)
        if fleet_seal_sha256 != expected["fleet_seal_sha256"]:
            raise ClosureError("bound fleet seal changed")
        fleet = decode_json_object(fleet_payload, str(V19_FLEET_SEAL))
        model_ids = validate_v19_fleet_seal(fleet, launch)
        if model_ids != sorted(expected["models"]):
            raise ClosureError("bound v19 model inventory changed")
        binding = v19_fleet_binding(fleet, fleet_seal_sha256)
        validate_v19_fleet_binding(
            binding,
            fleet_seal_sha256=fleet_seal_sha256,
            model_ids=model_ids,
        )
        binding_sha256 = sha256_bytes(canonical_json(binding))
        if (
            binding_sha256 != expected["fleet_binding_sha256"]
            or self.config["binding"].get("fleet_binding_sha256")
            != binding_sha256
            or canonical_json(self.config["binding"].get("fleet_binding"))
            != canonical_json(binding)
        ):
            raise ClosureError("bound v19 fleet binding changed")
        return binding

    def validate_frozen_inputs(self) -> tuple[dict[str, Any], dict[str, Any]]:
        expected = self.config["expected"]
        launch_path = self.live_root / "launch.json"
        manifest_path = self.live_root / "instrument-manifest.json"
        if sha256_file(launch_path) != expected["launch_sha256"]:
            raise ClosureError("launch receipt changed")
        if sha256_file(manifest_path) != expected["instrument_manifest_sha256"]:
            raise ClosureError("instrument manifest changed")
        launch = read_json(launch_path)
        manifest = read_json(manifest_path)
        if self.config.get("closure_generation") == V19_GENERATION:
            verify_immutable_file(
                pathlib.Path(expected["launch_controller_path"]),
                expected["launch_controller_sha256"],
            )
            if launch.get("launch_controller_sha256") != expected[
                "launch_controller_sha256"
            ]:
                raise ClosureError("launch receipt used a different frozen launcher")
            if launch.get("candidate", {}).get("tree") != expected["candidate_tree"]:
                raise ClosureError("candidate tree changed")
            if launch.get("binary", {}).get("path") != expected["binary_path"]:
                raise ClosureError("frozen binary path changed")
            run_started, run_started_sha256 = read_private_jsonl_first(
                self.run_dir / "run.jsonl"
            )
            if (
                run_started_sha256 != expected["run_started_sha256"]
                or run_started.get("run_id") != expected["run_id"]
            ):
                raise ClosureError("bound run_started identity changed")
            self.authenticated_v19_fleet_binding(launch)
            self.bound_trace_header()
        if launch.get("publication_document_id") != self.config["publication"]["target_document_id"]:
            raise ClosureError("launch publication identity changed")
        policy = manifest.get("sb7_policy") or {}
        if policy.get("publication_document_id") != self.config["publication"]["target_document_id"]:
            raise ClosureError("instrument publication identity changed")
        if policy.get("protected_document_ids") != self.config["publication"][
            "protected_document_ids"
        ]:
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
        if self.config.get("closure_generation") == V19_GENERATION and git_head(
            pathlib.Path(self.config["publisher"]["site_root"])
        ) != self.config["publisher"]["git_commit"]:
            raise ClosureError("guarded publisher Git commit changed")
        if sha256_file(pathlib.Path(__file__).resolve()) != self.config["controller_sha256"]:
            raise ClosureError("terminal closure controller hash changed")
        node = pathlib.Path(self.config["publisher"]["node"])
        if node.is_symlink() or sha256_file(node) != self.config["publisher"]["node_sha256"]:
            raise ClosureError("publisher/render Node runtime hash changed")
        validate_playwright_runtime(
            self.config["runtime"]["playwright"],
            node,
            self.config["publisher"]["node_sha256"],
        )
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
            if not goose_alive:
                self.require_terminal_completion_markers()
            if goose_alive and not harness_alive:
                raise ClosureError("authenticated harness exited while Goose remains alive")
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
        terminal_completion = self.require_terminal_completion_markers(run_rows)
        started = [row for row in run_rows if row.get("event") == "run_started"]
        finished = [row for row in run_rows if row.get("event") == "run_finished"]
        if len(started) != 1 or len(finished) != 1:
            raise ClosureError("run log lacks exactly one run_started and one run_finished")
        if started[0].get("run_id") != self.config["expected"]["run_id"]:
            raise ClosureError("run_started id differs from the launch receipt")
        monitor_terminal_row = self.monitor_terminal_row(terminal_completion)
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
        seed = self.expected_fixture_seed(seed)
        trace_header = self.bound_trace_header()
        if trace_header is not None and trace_header.get("fixture_seed") != seed:
            raise ClosureError("raw auto-verdict fixture_seed differs from first trace evidence")
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
            "monitor_classification": monitor_terminal_row["classification"],
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

    def require_terminal_completion_markers(
        self, run_rows: Sequence[Mapping[str, Any]] | None = None
    ) -> dict[str, Any]:
        run_log_path = self.run_dir / "run.jsonl"
        rows = list(run_rows) if run_rows is not None else read_jsonl(run_log_path)
        assessment = terminal_completion_assessment(
            rows, self.config["expected"]["run_id"]
        )
        if assessment["terminal_complete"]:
            return assessment
        diagnosis = {
            "schema_version": SCHEMA_VERSION,
            "run_id": self.config["expected"]["run_id"],
            "reason": "authenticated-goose-exited-without-complete-terminal-markers",
            "run_log_sha256": sha256_file(run_log_path),
            **assessment,
            "scoring_forbidden": True,
            "publication_forbidden": True,
            "terminal_reconstruction_forbidden": True,
        }
        diagnosis_path = self.state_dir / "terminal-incomplete-diagnosis.json"
        payload = json.dumps(diagnosis, indent=2, sort_keys=True).encode() + b"\n"
        create_once(diagnosis_path, payload, 0o400)
        self.events.emit(
            "terminal_incomplete",
            run_id=diagnosis["run_id"],
            terminal_phase=diagnosis["terminal_phase"],
            complete_result_count=diagnosis["complete_result_count"],
            run_overview_count=diagnosis["run_overview_count"],
            run_finished_count=diagnosis["run_finished_count"],
        )
        raise ClosureError(
            "terminal-incomplete: authenticated Goose exited without exactly one "
            "ordered complete_result and run_finished; scoring, publication, and "
            "terminal reconstruction are forbidden"
        )

    def monitor_terminal_row(
        self, terminal_completion: Mapping[str, Any]
    ) -> dict[str, Any]:
        monitor_rows = read_jsonl(self.run_dir / ".swarm-monitor" / "watch.jsonl")
        assessment = monitor_completion_assessment(monitor_rows)
        if assessment["publication_fatal"]:
            raise ClosureError(str(assessment["reason"]))
        terminal_index = assessment["terminal_index"]
        if assessment["classification"] == "clean_terminal":
            terminal = monitor_rows[terminal_index]
            return {
                "schema_version": SCHEMA_VERSION,
                "classification": "clean_terminal",
                "outcome": "run_finished",
                "monitor_completed_sha256": sha256_bytes(canonical_json(terminal)),
            }
        if terminal_completion.get("terminal_complete") is not True:
            raise ClosureError(
                "observation-only monitor incident requires authenticated engine terminal evidence"
            )
        return validate_observation_capture(
            self.run_dir, monitor_rows, assessment
        )

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

    def sealed_usage_contract(self, raw_seal: Mapping[str, Any]) -> dict[str, Any]:
        if raw_seal.get("tree_sha256") != tree_manifest(self.run_dir).get("tree_sha256"):
            raise ClosureError("usage contract requested for a tree outside its raw seal")
        try:
            contract = usage_policy.usage_contract_from_run_dir(
                self.run_dir,
                run_id=self.config["expected"]["run_id"],
                expected_nodes=self.config["expected"]["telemetry_nodes"],
                expected_models=self.config["expected"]["models"],
            )
        except usage_policy.UsageEvidenceError as error:
            raise ClosureError(f"sealed usage evidence is invalid: {error}") from error
        if raw_seal.get("tree_sha256") != tree_manifest(self.run_dir).get("tree_sha256"):
            raise ClosureError("raw tree changed while deriving its usage contract")
        contract["raw_tree_sha256"] = raw_seal["tree_sha256"]
        path = self.state_dir / "usage-contract.json"
        payload = canonical_json(contract) + b"\n"
        create_once(path, payload)
        if read_private_bytes(path, immutable=True) != payload:
            raise ClosureError("sealed usage contract changed after creation")
        return contract

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
        inventory_path = attempt / "descendants.json"
        spawn_journal_path = attempt / "spawn-journal.txt"
        clone = attempt / "tree"
        render_wrapper = attempt / "runtime" / "playwright-node"
        if any(
            path.is_symlink()
            for path in (
                result_path,
                score_path,
                seal_path,
                inventory_path,
                spawn_journal_path,
                render_wrapper,
            )
        ):
            raise ClosureError("successful scoring evidence contains a symbolic link")
        if clone.is_symlink() or not clone.is_dir():
            raise ClosureError("successful scoring clone is not a real directory")
        if (
            not result_path.is_file()
            or not score_path.is_file()
            or not seal_path.is_file()
            or not inventory_path.is_file()
            or not spawn_journal_path.is_file()
        ):
            return None
        result = read_json(result_path)
        configured_seed = self.config["expected"].get("fixture_seed")
        if (
            result.get("exit_code") == 0
            and result.get("scorer_exit_code") == 0
            and configured_seed is not None
            and result.get("fixture_seed") != configured_seed
        ):
            raise ClosureError(
                "successful score worker receipt used a different fixture_seed"
            )
        if (
            result.get("exit_code") != 0
            or result.get("scorer_exit_code") != 0
            or result.get("descendants_clean") is not True
            or result.get("descendant_cleanup_proven") is not True
            or result.get("descendants_survived_scorer") != 0
            or result.get("fixture_seed") is None
            or (
                configured_seed is not None
                and result.get("fixture_seed") != configured_seed
            )
            or result.get("port") != self.config["expected"]["vendor_port"]
            or result.get("scorer_sha256")
            != self.config["expected"]["instrument_files"][
                "evals/swarm-bench/bench/score_sb7.py"
            ]
            or result.get("playwright_module_tree_sha256")
            != self.config["runtime"]["playwright"]["module_tree_sha256"]
            or result.get("playwright_browser_tree_sha256")
            != self.config["runtime"]["playwright"]["browser_tree_sha256"]
            or result.get("playwright_executable_sha256")
            != self.config["runtime"]["playwright"]["executable_sha256"]
            or not re.fullmatch(
                r"[0-9a-f]{64}", str(result.get("descendant_inventory_sha256", ""))
            )
            or sha256_file(inventory_path)
            != result.get("descendant_inventory_sha256")
            or sha256_file(spawn_journal_path)
            != result.get("spawn_journal_sha256")
            or not render_wrapper.is_file()
            or stat.S_IMODE(render_wrapper.stat().st_mode) != 0o500
            or sha256_file(render_wrapper)
            != result.get("playwright_node_wrapper_sha256")
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
        descendant_receipts = validate_attempt_process_inventory(inventory_path)
        journal_receipts = read_spawn_journal_receipts(spawn_journal_path)
        inventoried = {receipt["pid"]: receipt for receipt in descendant_receipts}
        if any(
            receipt["pid"] not in inventoried
            or not set(receipt["identity_sha256s"]).issubset(
                set(inventoried[receipt["pid"]]["identity_sha256s"])
            )
            or not set(receipt["birth_sha256s"]).issubset(
                set(inventoried[receipt["pid"]]["birth_sha256s"])
            )
            for receipt in journal_receipts
        ):
            raise ClosureError("successful scoring attempt lost journaled identity evidence")
        descendant_statuses = [
            process_receipt_status(receipt) for receipt in descendant_receipts
        ]
        if "unavailable" in descendant_statuses:
            raise ClosureError(
                "successful scoring attempt descendant identity probe is unavailable"
            )
        if "match" in descendant_statuses:
            raise ClosureError("successful scoring attempt still has a live descendant")
        if not port_is_available(int(self.config["expected"]["vendor_port"])):
            raise ClosureError("successful scoring attempt did not leave its port isolated")
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

    def prove_attempt_cleanup_before_retry(
        self, attempt_dir: pathlib.Path, result: Mapping[str, Any]
    ) -> None:
        cleanup = cleanup_persisted_attempt_processes(
            attempt_dir / "descendants.json",
            attempt_dir / "spawn-journal.txt",
            int(self.config["expected"]["vendor_port"]),
        )
        if result.get("descendant_cleanup_proven") is not True:
            raise ClosureError(
                f"{attempt_dir.name} did not prove descendant cleanup; refusing retry"
            )
        if not cleanup["cleanup_proven"]:
            raise ClosureError(
                f"{attempt_dir.name} has live descendants or a contaminated port; refusing retry"
            )

    def start_score_attempt(
        self,
        attempt: int,
        terminal: dict[str, Any],
        raw_seal: dict[str, Any],
    ) -> tuple[pathlib.Path, dict[str, Any]]:
        attempt_dir = self.state_dir / "scoring" / f"attempt-{attempt}"
        clone = self.clone_for_attempt(attempt_dir, raw_seal)
        usage_contract = self.sealed_usage_contract(raw_seal)
        job = {
            "schema_version": SCHEMA_VERSION,
            "attempt": attempt,
            "clone": str(clone),
            "raw_tree": str(self.run_dir),
            "raw_tree_sha256": raw_seal["tree_sha256"],
            "seed": self.expected_fixture_seed(terminal.get("fixture_seed")),
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
            "playwright_runtime": self.config["runtime"]["playwright"],
            "playwright_probe": str(
                self.live_root
                / "instrument/evals/swarm-bench/bench/product_probe_v3.mjs"
            ),
            "playwright_probe_sha256": self.config["expected"]["instrument_files"][
                "evals/swarm-bench/bench/product_probe_v3.mjs"
            ],
            "score_contract": {
                "raw_scorer_version": self.config["expected"]["raw_scorer_version"],
                "check_count": self.config["expected"]["check_count"],
                "telemetry_nodes": self.config["expected"]["telemetry_nodes"],
                "models": self.config["expected"]["models"],
                "usage_contract": usage_contract,
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
        receipt = stable_process_receipt(worker.pid)
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
                descendant_cleanup = cleanup_persisted_attempt_processes(
                    attempt_dir / "descendants.json",
                    attempt_dir / "spawn-journal.txt",
                    int(self.config["expected"]["vendor_port"]),
                )
                if descendant_cleanup["inventory_present"]:
                    self.events.emit(
                        "orphaned_attempt_descendants_cleaned",
                        attempt=attempt_dir.name,
                        live_before=descendant_cleanup["live_before_cleanup"],
                        live_after=descendant_cleanup["live_after_cleanup"],
                        cleanup_proven=descendant_cleanup["cleanup_proven"],
                    )
                recovered = {
                    "schema_version": SCHEMA_VERSION,
                    "attempt": int(attempt_dir.name.removeprefix("attempt-")),
                    "completed_at": utc_now(),
                    "exit_code": 125,
                    "failure": (
                        "score worker exited and attempt descendant cleanup could not be proven"
                        if not descendant_cleanup["cleanup_proven"]
                        else "score worker exited without a durable result; attempt rejected"
                        if not group_alive or scorer_alive
                        else "score worker exited and an unauthenticated process group remains; "
                        "attempt rejected without signalling it"
                    ),
                    "descendant_cleanup_proven": descendant_cleanup["cleanup_proven"],
                    "descendants_clean": descendant_cleanup["cleanup_proven"],
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
        usage_contract = self.sealed_usage_contract(raw_seal)
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
                    self.prove_attempt_cleanup_before_retry(attempt_dir, result)
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
                self.prove_attempt_cleanup_before_retry(attempt_dir, result)
            if successful is None:
                raise ClosureError("authoritative scorer exhausted its bounded attempts")
        score_path, worker_result = successful
        score = read_json(score_path)
        self.validate_score(score, terminal, usage_contract)
        parent_owned = {"entrant", "rep", "agent", "actual_pool", "actual_nodes", "vendor_port", "closure"}
        overlap = sorted(parent_owned & set(score))
        if overlap:
            raise ClosureError(f"raw scorer attempted to supply parent-owned fields: {overlap}")
        expected_seed = self.expected_fixture_seed(terminal.get("fixture_seed"))
        if worker_result.get("fixture_seed") != expected_seed:
            raise ClosureError("score worker receipt used a different fixture_seed")
        auto = read_json(self.run_dir / "verdict.json")
        fleet_closure: dict[str, Any] = {}
        fleet_provenance: dict[str, Any] = {}
        if self.config.get("closure_generation") == V19_GENERATION:
            fleet_binding = self.authenticated_v19_fleet_binding(launch)
            fleet_binding_sha256 = sha256_bytes(canonical_json(fleet_binding))
            fleet_closure = {
                "fleet_seal_sha256": self.config["expected"][
                    "fleet_seal_sha256"
                ],
                "fleet_binding_sha256": fleet_binding_sha256,
            }
            fleet_provenance = {
                "fleet_binding": fleet_binding,
                "fleet_binding_sha256": fleet_binding_sha256,
            }
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
                    "playwright_module_tree_sha256": worker_result[
                        "playwright_module_tree_sha256"
                    ],
                    "playwright_browser_tree_sha256": worker_result[
                        "playwright_browser_tree_sha256"
                    ],
                    "playwright_executable_sha256": worker_result[
                        "playwright_executable_sha256"
                    ],
                    "playwright_node_wrapper_sha256": worker_result[
                        "playwright_node_wrapper_sha256"
                    ],
                    "auto_verdict_sha256": terminal["auto_verdict_sha256"],
                    **fleet_closure,
                },
            }
        )
        try:
            score["telemetry"].update(
                usage_policy.public_usage_receipt(usage_contract)
            )
        except usage_policy.UsageEvidenceError as error:
            raise ClosureError(f"public usage receipt is invalid: {error}") from error
        usage_contract_sha256 = sha256_bytes(canonical_json(usage_contract))
        score["closure"]["usage_contract_sha256"] = usage_contract_sha256
        if score.get("fixture_seed") != expected_seed:
            raise ClosureError("authoritative verdict used a different fixture_seed")
        authoritative_path = self.state_dir / "authoritative-verdict.json"
        if authoritative_path.is_file():
            existing_authoritative = read_json(authoritative_path)
            if canonical_json(existing_authoritative) != canonical_json(score):
                raise ClosureError("authoritative verdict differs from its durable receipt")
        else:
            atomic_json(authoritative_path, score)
        provenance = {
            "schema_version": SCHEMA_VERSION,
            "fixture_seed": expected_seed,
            "raw_tree_sha256": raw_seal["tree_sha256"],
            "scorer_sha256": worker_result["scorer_sha256"],
            "score_tree_sha256": worker_result["score_tree_sha256"],
            "playwright_module_tree_sha256": worker_result[
                "playwright_module_tree_sha256"
            ],
            "playwright_browser_tree_sha256": worker_result[
                "playwright_browser_tree_sha256"
            ],
            "playwright_executable_sha256": worker_result[
                "playwright_executable_sha256"
            ],
            "playwright_node_wrapper_sha256": worker_result[
                "playwright_node_wrapper_sha256"
            ],
            "candidate_commit": launch["candidate"]["commit"],
            "usage_contract_sha256": usage_contract_sha256,
            "usage_contract": usage_contract,
            "engine_events": terminal["engine_events"],
            "run_started_at": terminal["run_started_at"],
            "run_finished_at": terminal["run_finished_at"],
            **fleet_provenance,
            "authoritative_verdict_sha256": sha256_file(authoritative_path),
        }
        self.validate_v19_publication_fleet(score, provenance)
        provenance_path = self.state_dir / "scoring-provenance.json"
        if provenance_path.is_file():
            existing_provenance = read_json(provenance_path)
            if canonical_json(existing_provenance) != canonical_json(provenance):
                raise ClosureError("scoring provenance differs from its durable receipt")
        else:
            atomic_json(provenance_path, provenance)
        current_raw = tree_manifest(self.run_dir)
        if not manifests_equal(raw_seal, current_raw):
            raise ClosureError("raw tree changed during authoritative scoring")
        return authoritative_path, provenance

    def validate_v19_publication_fleet(
        self,
        authoritative: Mapping[str, Any],
        provenance: Mapping[str, Any],
    ) -> None:
        if self.config.get("closure_generation") != V19_GENERATION:
            return
        expected = self.config["expected"]
        fleet_binding = provenance.get("fleet_binding")
        validate_v19_fleet_binding(
            fleet_binding,
            fleet_seal_sha256=expected["fleet_seal_sha256"],
            model_ids=expected["models"],
        )
        fleet_binding_sha256 = sha256_bytes(canonical_json(fleet_binding))
        score_closure = authoritative.get("closure")
        actual_pool = authoritative.get("actual_pool")
        if (
            fleet_binding_sha256 != expected["fleet_binding_sha256"]
            or provenance.get("fleet_binding_sha256") != fleet_binding_sha256
            or not isinstance(score_closure, dict)
            or score_closure.get("fleet_seal_sha256")
            != expected["fleet_seal_sha256"]
            or score_closure.get("fleet_binding_sha256")
            != fleet_binding_sha256
            or not isinstance(actual_pool, list)
            or sorted(actual_pool) != sorted(expected["models"])
        ):
            raise ClosureError("v19 authoritative fleet provenance differs")
        usage_contract = provenance.get("usage_contract")
        telemetry = authoritative.get("telemetry")
        if not isinstance(usage_contract, dict) or not isinstance(telemetry, dict):
            raise ClosureError("authoritative usage impairment provenance is missing")
        usage_contract_sha256 = sha256_bytes(canonical_json(usage_contract))
        if (
            provenance.get("usage_contract_sha256") != usage_contract_sha256
            or score_closure.get("usage_contract_sha256")
            != usage_contract_sha256
            or usage_contract.get("raw_tree_sha256")
            != provenance.get("raw_tree_sha256")
        ):
            raise ClosureError("authoritative usage impairment provenance differs")
        public_receipt = usage_policy.public_usage_receipt(usage_contract)
        if any(telemetry.get(field) != value for field, value in public_receipt.items()):
            raise ClosureError("authoritative telemetry usage disclosure differs")
        raw_telemetry = dict(telemetry)
        for field in usage_policy.PUBLIC_RECEIPT_FIELDS:
            raw_telemetry.pop(field, None)
        try:
            usage_policy.validate_score_telemetry(
                raw_telemetry,
                usage_contract,
                expected_nodes=expected["telemetry_nodes"],
            )
        except usage_policy.UsageEvidenceError as error:
            raise ClosureError(
                f"authoritative completed-call telemetry differs: {error}"
            ) from error

    def validate_score(
        self,
        score: dict[str, Any],
        terminal: dict[str, Any],
        usage_contract: Mapping[str, Any],
    ) -> None:
        expected_seed = self.expected_fixture_seed(terminal.get("fixture_seed"))
        expected = dict(self.config["expected"])
        expected["usage_contract"] = usage_contract
        validate_sb7_score_payload(score, expected, expected_seed)
        raw_score = read_json(self.run_dir / "verdict.json")
        if raw_score.get("fixture_seed") != expected_seed:
            raise ClosureError("raw auto-verdict fixture_seed changed before publication")
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
                process_receipt = stable_process_receipt(publisher_process.pid)
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
        if receipt.get("protected_document_ids") != self.config["publication"][
            "protected_document_ids"
        ]:
            raise ClosureError("publisher protected-document receipt differs")
        if receipt.get("protected_before_sha256") != receipt.get("protected_after_sha256"):
            raise ClosureError("protected document receipts changed")
        authoritative_path = self.state_dir / "authoritative-verdict.json"
        authoritative = read_json(authoritative_path)
        expected_seed = self.expected_fixture_seed(authoritative.get("fixture_seed"))
        provenance = read_json(self.state_dir / "scoring-provenance.json")
        if provenance.get("fixture_seed") != expected_seed:
            raise ClosureError("publication provenance used a different fixture_seed")
        self.validate_v19_publication_fleet(authoritative, provenance)
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
            "fixture_seed": self.expected_fixture_seed(terminal.get("fixture_seed")),
            "scorer_sha256": provenance["scorer_sha256"],
            "score_tree_sha256": provenance["score_tree_sha256"],
            "playwright_module_tree_sha256": provenance[
                "playwright_module_tree_sha256"
            ],
            "playwright_browser_tree_sha256": provenance[
                "playwright_browser_tree_sha256"
            ],
            "playwright_executable_sha256": provenance[
                "playwright_executable_sha256"
            ],
            "playwright_node_wrapper_sha256": provenance[
                "playwright_node_wrapper_sha256"
            ],
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


def process_parent_pairs() -> list[tuple[int, int]]:
    completed = subprocess.run(
        ["ps", "-axo", "pid=", "-o", "ppid="],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if completed.returncode != 0:
        raise ClosureError("could not inventory scorer descendants")
    pairs: list[tuple[int, int]] = []
    for raw_line in completed.stdout.splitlines():
        fields = raw_line.split()
        if len(fields) != 2:
            continue
        try:
            pid, parent_pid = (int(field) for field in fields)
        except ValueError:
            continue
        if pid > 1 and parent_pid >= 0:
            pairs.append((pid, parent_pid))
    return pairs


def pid_exists(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def pid_requires_cleanup(pid: int) -> bool:
    try:
        completed = subprocess.run(
            ["ps", "-p", str(pid), "-o", "stat="],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        return pid_exists(pid)
    state = completed.stdout.strip()
    if completed.returncode == 0 and state:
        return not state.startswith(b"Z")
    return pid_exists(pid)


def process_receipt_status(receipt: Mapping[str, Any]) -> str:
    pid = receipt.get("pid")
    births = receipt.get("birth_sha256s")
    if births is None and isinstance(receipt.get("birth_sha256"), str):
        births = [receipt["birth_sha256"]]
    if births is not None:
        if (
            not isinstance(pid, int)
            or pid <= 1
            or not isinstance(births, list)
            or not births
            or any(
                not isinstance(birth, str)
                or not re.fullmatch(r"[0-9a-f]{64}", birth)
                for birth in births
            )
        ):
            return "invalid"
        try:
            observed_birth = process_birth_sha256(pid)
        except Exception:
            observed_birth = None
        if observed_birth is None:
            return "unavailable" if pid_requires_cleanup(pid) else "absent"
        return "match" if observed_birth in births else "mismatch"
    identities = receipt.get("identity_sha256s")
    if identities is None and isinstance(receipt.get("identity_sha256"), str):
        identities = [receipt["identity_sha256"]]
    if (
        not isinstance(pid, int)
        or pid <= 1
        or not isinstance(identities, list)
        or not identities
        or any(
            not isinstance(identity, str)
            or not re.fullmatch(r"[0-9a-f]{64}", identity)
            for identity in identities
        )
    ):
        return "invalid"
    try:
        observed = safe_process_receipt(pid)
    except Exception:
        observed = None
    if observed is None:
        return "unavailable" if pid_requires_cleanup(pid) else "absent"
    return "match" if observed["identity_sha256"] in identities else "mismatch"


def process_receipt_matches(receipt: Mapping[str, Any]) -> bool:
    return process_receipt_status(receipt) == "match"


def validate_attempt_process_inventory(path: pathlib.Path) -> list[dict[str, Any]]:
    if path.is_symlink():
        raise ClosureError("attempt descendant inventory is symbolic")
    if not path.is_file():
        return []
    inventory = read_json(path)
    if (
        not isinstance(inventory, dict)
        or inventory.get("schema_version") != SCHEMA_VERSION
        or not isinstance(inventory.get("root_pid"), int)
        or not isinstance(inventory.get("processes"), list)
        or len(inventory["processes"]) > 4096
    ):
        raise ClosureError("attempt descendant inventory is malformed")
    receipts: list[dict[str, Any]] = []
    seen: set[int] = set()
    for receipt in inventory["processes"]:
        if not isinstance(receipt, dict):
            raise ClosureError("attempt descendant receipt is malformed")
        pid = receipt.get("pid")
        identities = receipt.get("identity_sha256s")
        births = receipt.get("birth_sha256s")
        if (
            not isinstance(pid, int)
            or pid <= 1
            or pid in seen
            or not isinstance(identities, list)
            or not identities
            or len(identities) > 32
            or len(set(identities)) != len(identities)
            or not isinstance(births, list)
            or not births
            or len(births) > 32
            or len(set(births)) != len(births)
            or any(
                not isinstance(identity, str)
                or not re.fullmatch(r"[0-9a-f]{64}", identity)
                for identity in identities
            )
            or any(
                not isinstance(birth, str)
                or not re.fullmatch(r"[0-9a-f]{64}", birth)
                for birth in births
            )
        ):
            raise ClosureError("attempt descendant receipt is malformed")
        seen.add(pid)
        receipts.append(
            {
                "pid": pid,
                "identity_sha256s": identities,
                "birth_sha256s": births,
            }
        )
    if inventory["root_pid"] not in seen:
        raise ClosureError("attempt descendant inventory lost its scorer root")
    return receipts


def signal_authenticated_receipts(
    receipts: Sequence[Mapping[str, Any]], process_signal: signal.Signals
) -> int:
    signalled = 0
    for receipt in receipts:
        pid = receipt.get("pid")
        if not isinstance(pid, int) or pid in {os.getpid(), os.getppid()}:
            continue
        if not process_receipt_matches(receipt):
            continue
        try:
            os.kill(pid, process_signal)
            signalled += 1
        except (ProcessLookupError, PermissionError):
            continue
    return signalled


def terminate_authenticated_receipts(
    receipts: Sequence[Mapping[str, Any]], grace_seconds: float = 3
) -> dict[str, Any]:
    probe_unavailable = False

    def live_receipts() -> list[dict[str, Any]]:
        nonlocal probe_unavailable
        live: list[dict[str, Any]] = []
        for receipt in receipts:
            status = process_receipt_status(receipt)
            if status == "unavailable":
                probe_unavailable = True
            elif status == "match":
                live.append(dict(receipt))
        return live

    live_before = live_receipts()
    signalled = signal_authenticated_receipts(live_before, signal.SIGTERM)
    deadline = time.monotonic() + grace_seconds
    live_after = live_receipts()
    while live_after and time.monotonic() < deadline:
        time.sleep(0.05)
        live_after = live_receipts()
    if live_after:
        signalled += signal_authenticated_receipts(live_after, signal.SIGKILL)
        kill_deadline = time.monotonic() + min(grace_seconds, 2)
        while live_after and time.monotonic() < kill_deadline:
            time.sleep(0.05)
            live_after = live_receipts()
    return {
        "live_before_cleanup": len(live_before),
        "live_after_cleanup": len(live_after),
        "signals_sent": signalled,
        "identity_probe_unavailable": probe_unavailable,
    }


def read_spawn_journal_receipts(path: pathlib.Path) -> list[dict[str, Any]]:
    if path.is_symlink():
        raise ClosureError("attempt spawn journal is symbolic")
    if not path.is_file():
        return []
    payload = path.read_bytes()
    if len(payload) > 128 * 1024 or (payload and not payload.endswith(b"\n")):
        raise ClosureError("attempt spawn journal is malformed")
    identities_by_pid: dict[int, list[str]] = {}
    births_by_pid: dict[int, list[str]] = {}
    for raw_line in payload.splitlines():
        try:
            entry = json.loads(raw_line)
        except json.JSONDecodeError as error:
            raise ClosureError("attempt spawn journal is malformed") from error
        if not isinstance(entry, dict) or set(entry) != {
            "pid",
            "identity_sha256s",
            "birth_sha256s",
        }:
            raise ClosureError("attempt spawn journal is malformed")
        pid = entry["pid"]
        identities = entry["identity_sha256s"]
        births = entry["birth_sha256s"]
        if (
            not isinstance(pid, int)
            or pid <= 1
            or not isinstance(identities, list)
            or not identities
            or len(identities) > 32
            or not isinstance(births, list)
            or not births
            or len(births) > 32
            or any(
                not isinstance(identity, str)
                or not re.fullmatch(r"[0-9a-f]{64}", identity)
                for identity in identities
            )
            or any(
                not isinstance(birth, str)
                or not re.fullmatch(r"[0-9a-f]{64}", birth)
                for birth in births
            )
        ):
            raise ClosureError("attempt spawn journal is malformed")
        owned = identities_by_pid.setdefault(pid, [])
        for identity in identities:
            if identity not in owned:
                owned.append(identity)
        owned_births = births_by_pid.setdefault(pid, [])
        for birth in births:
            if birth not in owned_births:
                owned_births.append(birth)
    if len(identities_by_pid) > 4096:
        raise ClosureError("attempt spawn journal is too large")
    return [
        {
            "pid": pid,
            "identity_sha256s": identities_by_pid[pid],
            "birth_sha256s": births_by_pid[pid],
        }
        for pid in sorted(identities_by_pid)
    ]


class AttemptProcessTracker:
    def __init__(
        self,
        root_receipt: Mapping[str, Any],
        inventory_path: pathlib.Path,
        spawn_journal_path: pathlib.Path,
        poll_seconds: float = 0.05,
    ) -> None:
        if not process_receipt_matches(root_receipt):
            raise ClosureError("scorer root could not be authenticated for descendant containment")
        self.root_pid = int(root_receipt["pid"])
        root_birth = process_birth_sha256(self.root_pid)
        if root_birth is None:
            raise ClosureError("scorer root birth identity could not be authenticated")
        self.inventory_path = inventory_path
        self.spawn_journal_path = spawn_journal_path
        self.poll_seconds = poll_seconds
        self.processes: dict[int, dict[str, Any]] = {
            self.root_pid: {
                "pid": self.root_pid,
                "identity_sha256s": [str(root_receipt["identity_sha256"])],
                "birth_sha256s": [root_birth],
            }
        }
        self.lock = threading.Lock()
        self.stop_event = threading.Event()
        self.error: str | None = None
        self.thread = threading.Thread(target=self._run, daemon=True)
        self._persist()

    def _persist(self) -> None:
        with self.lock:
            payload = {
                "schema_version": SCHEMA_VERSION,
                "root_pid": self.root_pid,
                "updated_at": utc_now(),
                "processes": [self.processes[pid] for pid in sorted(self.processes)],
            }
        atomic_json(self.inventory_path, payload)

    def _merge_receipt(self, receipt: Mapping[str, Any]) -> bool:
        pid = receipt.get("pid")
        identities = receipt.get("identity_sha256s")
        if identities is None and isinstance(receipt.get("identity_sha256"), str):
            identities = [receipt["identity_sha256"]]
        births = receipt.get("birth_sha256s")
        if births is None and isinstance(receipt.get("birth_sha256"), str):
            births = [receipt["birth_sha256"]]
        if (
            not isinstance(pid, int)
            or not isinstance(identities, list)
            or not isinstance(births, list)
        ):
            raise ClosureError("attempt descendant receipt is malformed")
        changed = False
        with self.lock:
            stored = self.processes.setdefault(
                pid,
                {"pid": pid, "identity_sha256s": [], "birth_sha256s": []},
            )
            for identity in identities:
                if identity not in stored["identity_sha256s"]:
                    stored["identity_sha256s"].append(identity)
                    changed = True
            for birth in births:
                if birth not in stored["birth_sha256s"]:
                    stored["birth_sha256s"].append(birth)
                    changed = True
        return changed

    def _record_probe_unavailable(self, pid: int) -> None:
        if self.error is None:
            self.error = f"descendant birth identity probe unavailable for live pid {pid}"

    def _live_receipts(self) -> list[dict[str, Any]]:
        live: list[dict[str, Any]] = []
        for receipt in self.receipts():
            status = process_receipt_status(receipt)
            if status == "unavailable":
                self._record_probe_unavailable(int(receipt["pid"]))
            elif status == "match":
                live.append(receipt)
        return live

    def scan(self) -> None:
        pairs = process_parent_pairs()
        changed = False
        for receipt in read_spawn_journal_receipts(self.spawn_journal_path):
            changed = self._merge_receipt(receipt) or changed
        active = {receipt["pid"] for receipt in self._live_receipts()}
        while True:
            discovered = False
            for pid, parent_pid in pairs:
                if pid in active or parent_pid not in active:
                    continue
                receipt = stable_process_receipt(pid, timeout_seconds=0.5)
                if receipt is None:
                    continue
                try:
                    birth = process_birth_sha256(pid)
                except Exception:
                    birth = None
                if birth is None:
                    if pid_requires_cleanup(pid):
                        self._record_probe_unavailable(pid)
                    continue
                changed = self._merge_receipt(
                    {**receipt, "birth_sha256": birth}
                ) or changed
                active.add(pid)
                discovered = True
            if not discovered:
                break
        if changed:
            self._persist()

    def _run(self) -> None:
        try:
            while not self.stop_event.wait(self.poll_seconds):
                self.scan()
        except BaseException as error:
            if self.error is None:
                self.error = f"{type(error).__name__}: {redact_text(error)}"

    def start(self) -> None:
        self.scan()
        self.thread.start()

    def receipts(self) -> list[dict[str, Any]]:
        with self.lock:
            return [dict(self.processes[pid]) for pid in sorted(self.processes)]

    def cleanup(self, port: int, grace_seconds: float = 3) -> dict[str, Any]:
        self.scan()
        live_before = self._live_receipts()
        deadline = time.monotonic() + grace_seconds
        signals_sent = 0
        while True:
            self.scan()
            live = self._live_receipts()
            if not live or time.monotonic() >= deadline:
                break
            signals_sent += signal_authenticated_receipts(live, signal.SIGTERM)
            time.sleep(0.05)
        live = self._live_receipts()
        if live:
            signals_sent += signal_authenticated_receipts(live, signal.SIGKILL)
            kill_deadline = time.monotonic() + min(grace_seconds, 2)
            while live and time.monotonic() < kill_deadline:
                self.scan()
                time.sleep(0.05)
                live = self._live_receipts()
        self.stop_event.set()
        self.thread.join(timeout=2)
        self.scan()
        live_after = self._live_receipts()
        port_free = port_is_available(port)
        cleanup_proven = not live_after and port_free and self.error is None
        self._persist()
        return {
            "observed_count": len(self.receipts()),
            "live_before_cleanup": len(live_before),
            "live_after_cleanup": len(live_after),
            "signals_sent": signals_sent,
            "port_free_after_cleanup": port_free,
            "cleanup_proven": cleanup_proven,
            "tracker_error": self.error,
        }


def cleanup_persisted_attempt_processes(
    inventory_path: pathlib.Path,
    spawn_journal_path: pathlib.Path,
    port: int,
    grace_seconds: float = 3,
) -> dict[str, Any]:
    receipts = validate_attempt_process_inventory(inventory_path)
    by_pid = {receipt["pid"]: receipt for receipt in receipts}
    for journal_receipt in read_spawn_journal_receipts(spawn_journal_path):
        stored = by_pid.get(journal_receipt["pid"])
        if stored is None:
            stored = {
                "pid": journal_receipt["pid"],
                "identity_sha256s": [],
                "birth_sha256s": [],
            }
            receipts.append(stored)
            by_pid[journal_receipt["pid"]] = stored
        for identity in journal_receipt["identity_sha256s"]:
            if identity not in stored["identity_sha256s"]:
                stored["identity_sha256s"].append(identity)
        for birth in journal_receipt["birth_sha256s"]:
            if birth not in stored["birth_sha256s"]:
                stored["birth_sha256s"].append(birth)
    if not receipts:
        port_free = port_is_available(port)
        return {
            "inventory_present": False,
            "live_before_cleanup": 0,
            "live_after_cleanup": 0,
            "signals_sent": 0,
            "identity_probe_unavailable": False,
            "port_free_after_cleanup": port_free,
            "cleanup_proven": port_free,
        }
    cleanup = terminate_authenticated_receipts(receipts, grace_seconds)
    port_free = port_is_available(port)
    return {
        "inventory_present": True,
        **cleanup,
        "port_free_after_cleanup": port_free,
        "cleanup_proven": (
            cleanup["live_after_cleanup"] == 0
            and not cleanup["identity_probe_unavailable"]
            and port_free
        ),
    }


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
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(process_group_id, signal.SIGTERM)
    deadline = time.monotonic() + grace_seconds
    while process_group_exists(process_group_id) and time.monotonic() < deadline:
        time.sleep(0.1)
    if process_group_exists(process_group_id):
        with contextlib.suppress(ProcessLookupError, PermissionError):
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
    playwright_runtime = job.get("playwright_runtime")
    if not isinstance(playwright_runtime, dict):
        raise ClosureError("score worker lacks its pinned Playwright runtime contract")
    playwright_info = validate_playwright_runtime(
        playwright_runtime, render_node, str(job["render_node_sha256"])
    )
    if tree_manifest(clone)["tree_sha256"] != job["raw_tree_sha256"]:
        raise ClosureError("score worker clone differs from the raw seal")
    score_contract = job.get("score_contract")
    if not isinstance(score_contract, dict):
        raise ClosureError("score worker lacks its usage contract")
    configured_usage_contract = score_contract.get("usage_contract")
    if not isinstance(configured_usage_contract, dict):
        raise ClosureError("score worker usage contract is malformed")
    try:
        observed_usage_contract = usage_policy.usage_contract_from_run_dir(
            clone,
            run_id=configured_usage_contract.get("run_id"),
            expected_nodes=score_contract.get("telemetry_nodes") or [],
            expected_models=score_contract.get("models") or [],
        )
    except usage_policy.UsageEvidenceError as error:
        raise ClosureError(f"score worker sealed usage evidence is invalid: {error}") from error
    observed_usage_contract["raw_tree_sha256"] = job["raw_tree_sha256"]
    if canonical_json(observed_usage_contract) != canonical_json(
        configured_usage_contract
    ):
        raise ClosureError("score worker usage contract differs from its sealed clone")
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
        browser_view = prepare_playwright_browser_view(
            playwright_info, runtime_root / "playwright-browsers"
        )
        probe_script = pathlib.Path(str(job["playwright_probe"]))
        if (
            probe_script.is_symlink()
            or not probe_script.is_file()
            or not path_is_within(
                probe_script, pathlib.Path(str(job["instrument_root"]))
            )
            or sha256_file(probe_script) != job["playwright_probe_sha256"]
        ):
            raise ClosureError("score worker frozen Playwright product probe changed")
        playwright_smoke_before = smoke_playwright_runtime(
            playwright_info,
            render_node,
            runtime_home,
            runtime_tmp,
            browser_view,
            probe_script,
        )
        render_wrapper = runtime_root / "playwright-node"
        render_wrapper_sha256 = create_playwright_node_wrapper(
            render_wrapper, render_node, playwright_info, browser_view
        )
        score_environment = safe_environment(
            {
                "HOME": str(runtime_home),
                "TMPDIR": str(runtime_tmp),
                "GOOSE_SWARM_RENDER_NODE": str(render_wrapper),
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
            spawn_journal_path = attempt_dir / "spawn-journal.txt"
            if spawn_journal_path.is_symlink():
                raise ClosureError("attempt spawn journal is symbolic")
            spawn_journal_descriptor = os.open(
                spawn_journal_path, os.O_CREAT | os.O_APPEND | os.O_WRONLY, 0o600
            )
            os.fchmod(spawn_journal_descriptor, 0o600)
            os.close(spawn_journal_descriptor)
            gate_read, gate_write = os.pipe()
            os.set_inheritable(gate_read, True)
            try:
                scorer_process = subprocess.Popen(
                    [
                        sys.executable,
                        "-B",
                        "-u",
                        "-c",
                        SCORER_GATE_SOURCE,
                        str(gate_read),
                        str(spawn_journal_path),
                        *command[3:],
                    ],
                    cwd=scorer.parent,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    env=score_environment,
                    start_new_session=True,
                    pass_fds=(gate_read,),
                )
            except BaseException:
                os.close(gate_write)
                raise
            finally:
                os.close(gate_read)
            started_epoch = time.time()
            deadline_epoch = started_epoch + float(job["timeout_seconds"])
            scorer_receipt = stable_process_receipt(scorer_process.pid)
            scorer_state = {
                "schema_version": SCHEMA_VERSION,
                "pid": scorer_process.pid,
                "process_group_id": scorer_process.pid,
                "started_at": utc_now(),
                "started_epoch": started_epoch,
                "deadline_epoch": deadline_epoch,
            }
            atomic_json(attempt_dir / "scorer-state.json", scorer_state)
            if scorer_receipt is None:
                os.close(gate_write)
                terminate_process_group(scorer_process.pid)
                scorer_process.wait()
                raise ClosureError(
                    "scorer did not remain alive long enough to authenticate containment"
                )
            atomic_json(attempt_dir / "scorer.pid.json", scorer_receipt)
            descendant_inventory_path = attempt_dir / "descendants.json"
            try:
                descendant_tracker = AttemptProcessTracker(
                    scorer_receipt,
                    descendant_inventory_path,
                    spawn_journal_path,
                )
                descendant_tracker.start()
            except BaseException:
                os.close(gate_write)
                terminate_process_group(scorer_process.pid)
                scorer_process.wait()
                raise
            try:
                os.write(gate_write, b"1")
            finally:
                os.close(gate_write)
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
            try:
                while scorer_process.poll() is None:
                    if stop_path.exists():
                        termination_reason = (
                            "closure stop requested during authoritative scoring"
                        )
                        break
                    if time.time() >= deadline_epoch:
                        termination_reason = (
                            "authoritative scorer exceeded its frozen timeout"
                        )
                        break
                    time.sleep(0.5)
            finally:
                if scorer_process.poll() is None:
                    terminate_process_group(scorer_process.pid)
                scorer_exit = scorer_process.wait()
                descendant_cleanup = descendant_tracker.cleanup(port)
                reader.join(timeout=5)
            if reader.is_alive():
                raise ClosureError("score output channel did not reach EOF")
            if reader_outcome.get("error"):
                raise ClosureError(f"score output capture failed: {reader_outcome['error']}")
            score_log.flush()
            os.fsync(score_log.fileno())
        verify_instrument_inventory(job)
        if sha256_file(render_node) != job["render_node_sha256"]:
            raise ClosureError("render Node runtime changed during authoritative scoring")
        playwright_after = validate_playwright_runtime(
            playwright_runtime, render_node, str(job["render_node_sha256"])
        )
        if playwright_after != playwright_info:
            raise ClosureError("Playwright runtime changed during authoritative scoring")
        playwright_smoke_after = smoke_playwright_runtime(
            playwright_after,
            render_node,
            runtime_home,
            runtime_tmp,
            browser_view,
            probe_script,
        )
        if playwright_smoke_after != playwright_smoke_before:
            raise ClosureError("Playwright resolution changed during authoritative scoring")
        if (
            render_wrapper.is_symlink()
            or not render_wrapper.is_file()
            or stat.S_IMODE(render_wrapper.stat().st_mode) != 0o500
            or sha256_file(render_wrapper) != render_wrapper_sha256
        ):
            raise ClosureError("private Playwright Node wrapper changed during scoring")
        accepted_exit = scorer_exit
        failure = termination_reason
        if termination_reason:
            accepted_exit = 75 if stop_path.exists() else 124
        descendants_survived_scorer = descendant_cleanup["live_before_cleanup"]
        descendant_cleanup_proven = descendant_cleanup["cleanup_proven"]
        if descendants_survived_scorer:
            accepted_exit = 70
            failure = "authoritative scorer left descendant processes; attempt rejected"
        if not descendant_cleanup_proven:
            accepted_exit = 70
            failure = "authoritative scorer descendant cleanup could not be proven"
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
            "playwright_module_tree_sha256": playwright_info[
                "module_tree_sha256"
            ],
            "playwright_browser_tree_sha256": playwright_info[
                "browser_tree_sha256"
            ],
            "playwright_executable_sha256": playwright_info[
                "executable_sha256"
            ],
            "playwright_node_wrapper_sha256": render_wrapper_sha256,
            "raw_tree_sha256": job["raw_tree_sha256"],
            "fixture_seed": seed,
            "port": port,
            "descendants_clean": (
                descendants_survived_scorer == 0 and descendant_cleanup_proven
            ),
            "descendant_cleanup_proven": descendant_cleanup_proven,
            "descendants_observed": descendant_cleanup["observed_count"],
            "descendants_survived_scorer": descendants_survived_scorer,
            "descendants_live_after_cleanup": descendant_cleanup[
                "live_after_cleanup"
            ],
            "descendant_cleanup_signals": descendant_cleanup["signals_sent"],
            "descendant_tracker_error": descendant_cleanup["tracker_error"],
            "port_free_after_cleanup": descendant_cleanup[
                "port_free_after_cleanup"
            ],
            "descendant_inventory_sha256": sha256_file(descendant_inventory_path),
            "spawn_journal_sha256": sha256_file(spawn_journal_path),
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


def materialize_closure_instrument_snapshot(
    state_dir: pathlib.Path,
    source_files: Mapping[str, Mapping[str, Any]],
    frozen_config: Mapping[str, Any],
) -> pathlib.Path:
    ensure_secure_dir(state_dir)
    instrument = state_dir / "closure-instrument"
    if instrument.is_symlink() or (instrument.exists() and not instrument.is_dir()):
        raise ClosureError("closure instrument target is not a real directory")
    expected_names = set(source_files) | {"config.json"}
    if (
        "manifest.json" in expected_names
        or len(expected_names) != len(source_files) + 1
        or any(pathlib.Path(name).name != name or not name for name in expected_names)
    ):
        raise ClosureError("closure instrument source inventory is malformed")
    if not instrument.exists():
        for name, contract in source_files.items():
            source = pathlib.Path(str(contract.get("path", ""))).resolve()
            expected_sha256 = contract.get("sha256")
            if (
                source.is_symlink()
                or not source.is_file()
                or not SHA256_RE.fullmatch(str(expected_sha256))
                or sha256_file(source) != expected_sha256
            ):
                raise ClosureError(f"closure instrument source changed: {name}")
        temporary = state_dir / (
            f".closure-instrument.{os.getpid()}.{os.urandom(6).hex()}.tmp"
        )
        ensure_secure_dir(temporary)
        for name, contract in source_files.items():
            source = pathlib.Path(str(contract["path"])).resolve()
            mode = int(contract.get("mode", 0o400))
            if mode & 0o222:
                raise ClosureError(f"closure instrument source mode is writable: {name}")
            atomic_write(temporary / name, source.read_bytes(), mode)
        atomic_json(temporary / "config.json", frozen_config, 0o400)
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "created_at": utc_now(),
            "files": {
                name: sha256_file(temporary / name) for name in sorted(expected_names)
            },
        }
        atomic_json(temporary / "manifest.json", manifest, 0o400)
        os.replace(temporary, instrument)
        directory = os.open(state_dir, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    manifest_path = instrument / "manifest.json"
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise ClosureError("closure instrument snapshot is incomplete")
    manifest = read_json(manifest_path)
    if manifest.get("schema_version") != SCHEMA_VERSION or set(
        manifest.get("files") or {}
    ) != expected_names:
        raise ClosureError("closure instrument inventory changed")
    for name, contract in source_files.items():
        if manifest["files"].get(name) != contract.get("sha256"):
            raise ClosureError(f"closure instrument source binding changed: {name}")
    actual_names = {path.name for path in instrument.iterdir()}
    if actual_names != expected_names | {"manifest.json"}:
        raise ClosureError("closure instrument contains unsealed files or bytecode cache")
    for name, digest in manifest["files"].items():
        path = instrument / name
        if (
            path.is_symlink()
            or not path.is_file()
            or path.stat().st_mode & 0o222
            or sha256_file(path) != digest
        ):
            raise ClosureError(f"closure instrument changed: {name}")
    if manifest_path.stat().st_mode & 0o222:
        raise ClosureError("closure instrument manifest became writable")
    persisted_config = read_private_json(instrument / "config.json", immutable=True)
    if canonical_json(persisted_config) != canonical_json(frozen_config):
        raise ClosureError("closure instrument config differs from its bound source")
    return instrument


def snapshot_instrument(source_config_path: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    source_config = load_config(source_config_path)
    validate_config(source_config)
    state_dir = pathlib.Path(source_config["state_dir"]).resolve()
    script_source = pathlib.Path(__file__).resolve()
    publisher_source = pathlib.Path(source_config["publisher"]["path"]).resolve()
    usage_policy_source = pathlib.Path(source_config["usage_policy"]["path"]).resolve()
    instrument = state_dir / "closure-instrument"
    script_target = instrument / "terminal_closure.py"
    publisher_target = instrument / "seed-fleet-brainwaves-sb70.mjs"
    usage_policy_target = instrument / "usage_impairment.py"
    config_target = instrument / "config.json"
    frozen_config = json.loads(json.dumps(source_config))
    frozen_config["publisher"]["path"] = str(publisher_target.resolve())
    frozen_config["usage_policy"]["path"] = str(usage_policy_target.resolve())
    source_files = {
        script_target.name: {
            "path": str(script_source),
            "sha256": source_config["controller_sha256"],
            "mode": 0o500,
        },
        publisher_target.name: {
            "path": str(publisher_source),
            "sha256": source_config["publisher"]["sha256"],
            "mode": 0o500,
        },
        usage_policy_target.name: {
            "path": str(usage_policy_source),
            "sha256": source_config["usage_policy"]["sha256"],
            "mode": 0o400,
        },
    }
    materialize_closure_instrument_snapshot(state_dir, source_files, frozen_config)
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
        child_environment = safe_environment()
        if child_environment.get("PYTHONDONTWRITEBYTECODE") != "1":
            raise ClosureError("detached closure must disable Python bytecode writes")
        with os.fdopen(descriptor, "ab", buffering=0) as log:
            process = subprocess.Popen(
                [sys.executable, "-B", "-u", str(script), "run", "--config", str(frozen_config)],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                env=child_environment,
                start_new_session=True,
            )
        receipt = stable_process_receipt(process.pid)
        if receipt is None:
            raise ClosureError("closure supervisor exited during detached launch")
        try:
            snapshot_instrument(config_path)
        except BaseException:
            terminate_process_group(process.pid)
            raise
        atomic_json(pid_path, receipt)
        print(f"closure started pid={process.pid} state={state_dir}")
        return 0
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)


def status(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    validate_config(config)
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
    validate_config(config)
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
    validate_config(config)
    result_path = pathlib.Path(config["state_dir"]) / "result.json"
    if not result_path.is_file():
        print("closure result is not available")
        return 1
    print(json.dumps(read_json(result_path), indent=2, sort_keys=True))
    return 0


def stop(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    validate_config(config)
    stop_path = pathlib.Path(config["state_dir"]) / "STOP"
    atomic_write(stop_path, (utc_now() + "\n").encode())
    print("closure stop requested; the live benchmark run will not be signalled")
    return 0


def preflight(config_path: pathlib.Path) -> int:
    config = load_config(config_path)
    validate_config(config)
    audit = object.__new__(TerminalClosure)
    audit.config = config
    audit.live_root = pathlib.Path(config["live_root"]).resolve()
    audit.run_dir = pathlib.Path(config["run_dir"]).resolve()
    launch, manifest = TerminalClosure.validate_frozen_inputs(audit)
    playwright = preflight_playwright_runtime(
        config["runtime"]["playwright"],
        pathlib.Path(config["publisher"]["node"]),
        config["publisher"]["node_sha256"],
        audit.live_root
        / "instrument/evals/swarm-bench/bench/product_probe_v3.mjs",
    )
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
                "playwright": {
                    "version": playwright["version"],
                    "browser": playwright["browser_name"],
                    "revision": playwright["browser_revision"],
                    "module_tree_sha256": playwright["module_tree_sha256"],
                    "module_package": playwright["resolution_receipt"][
                        "modulePackage"
                    ],
                    "module_resolution_exact": playwright["resolution_receipt"][
                        "pinnedModule"
                    ],
                    "browser_tree_sha256": playwright["browser_tree_sha256"],
                    "executable_sha256": playwright["executable_sha256"],
                    "empty_home_smoke": True,
                },
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
    binder = commands.add_parser("bind-v21")
    binder.add_argument("--template", type=pathlib.Path, required=True)
    worker = commands.add_parser("score-worker")
    worker.add_argument("--job", type=pathlib.Path, required=True)
    return root


def main(argv: Sequence[str] | None = None) -> int:
    os.umask(0o077)
    args = parser().parse_args(argv)
    if args.command == "bind-v21":
        path, created = bind_v19(args.template)
        config = load_config(path)
        print(
            json.dumps(
                {
                    "armed_config": str(path),
                    "created": created,
                    "fixture_seed": config["expected"]["fixture_seed"],
                    "run_id": config["expected"]["run_id"],
                    "sha256": sha256_file(path),
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 0
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
