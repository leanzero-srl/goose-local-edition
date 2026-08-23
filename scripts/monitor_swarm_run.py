#!/usr/bin/env python3
"""Durable, evidence-gated monitor for an unattended local Goose swarm run.

The monitor consumes the engine's full-stream recurrence meter from each atomic
activity digest. It never infers recurrence from the truncated `last_thinking`
tail. A process is stopped only after an incident bundle is durably captured and
the exact process identity still matches the launch identity.
"""

import argparse
import dataclasses
import datetime as dt
import hashlib
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import time
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple


F924_MEASURED_REPEAT_SHARE = 0.4033
DEFAULT_RECURRENCE_SHARE = 0.30
DEFAULT_REPEATED_WINDOWS = 1024


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def atomic_write(path: pathlib.Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("wb") as handle:
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    directory_fd = os.open(str(path.parent), os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def write_json(path: pathlib.Path, value: Any) -> None:
    atomic_write(path, (json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run_capture(command: Sequence[str], timeout: float = 8.0) -> Dict[str, Any]:
    try:
        completed = subprocess.run(
            list(command),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
        return {
            "command": list(command),
            "exit_code": completed.returncode,
            "stdout": completed.stdout.decode("utf-8", "replace"),
            "stderr": completed.stderr.decode("utf-8", "replace"),
        }
    except Exception as error:  # diagnostic capture must not prevent the primary evidence bundle
        return {"command": list(command), "error": repr(error)}


def process_identity(pid: int) -> Optional[Dict[str, str]]:
    result = run_capture(
        ["ps", "-p", str(pid), "-o", "pid=", "-o", "lstart=", "-o", "command="],
        timeout=3.0,
    )
    output = result.get("stdout", "").strip()
    if result.get("exit_code") != 0 or not output:
        return None
    return {"pid": str(pid), "ps_line": output}


def parse_goose_processes(ps_output: str) -> List[Dict[str, Any]]:
    processes: List[Dict[str, Any]] = []
    for line in ps_output.splitlines():
        columns = line.strip().split(None, 2)
        if len(columns) < 3 or not columns[0].isdigit():
            continue
        pid = int(columns[0])
        executable = pathlib.Path(columns[1]).name
        arguments = columns[2]
        first_argument = arguments.split(None, 1)[0] if arguments else ""
        argument_executable = pathlib.Path(first_argument).name
        if executable == "goose" or argument_executable == "goose":
            processes.append(
                {"pid": pid, "comm": columns[1], "arguments": arguments}
            )
    return processes


def local_goose_processes() -> List[Dict[str, Any]]:
    result = run_capture(["ps", "-axo", "pid=,comm=,args="], timeout=5.0)
    return parse_goose_processes(result.get("stdout", ""))


def lms_snapshot() -> Dict[str, Any]:
    result = run_capture(["lms", "ps", "--json"], timeout=10.0)
    if result.get("exit_code") != 0:
        return {"ok": False, "capture": result}
    try:
        raw_models = json.loads(result.get("stdout", ""))
    except json.JSONDecodeError as error:
        return {"ok": False, "capture": result, "parse_error": str(error)}
    models = [
        {
            "identifier": model.get("identifier"),
            "device_identifier": model.get("deviceIdentifier"),
            "status": model.get("status"),
            "queued": model.get("queued"),
            "parallel": model.get("parallel"),
            "last_used_time": model.get("lastUsedTime"),
        }
        for model in raw_models
    ]
    return {"ok": True, "models": models}


def preflight_report(
    forbidden_pids: Iterable[int],
    binary: Optional[pathlib.Path],
    expected_sha256: Optional[str],
    expected_nodes: int,
) -> Tuple[bool, Dict[str, Any]]:
    failures: List[str] = []
    forbidden = []
    for pid in forbidden_pids:
        identity = process_identity(pid)
        if identity is not None:
            forbidden.append(identity)
            failures.append("forbidden pid {} is still alive".format(pid))

    goose_processes = [
        process for process in local_goose_processes() if process["pid"] != os.getpid()
    ]
    if goose_processes:
        failures.append("prior Goose process is still alive")

    binary_proof: Optional[Dict[str, Any]] = None
    if binary is not None:
        if not binary.is_file():
            failures.append("binary does not exist: {}".format(binary))
        else:
            actual = sha256_file(binary)
            binary_proof = {
                "path": str(binary.resolve()),
                "sha256": actual,
                "expected_sha256": expected_sha256,
            }
            if expected_sha256 and actual.lower() != expected_sha256.lower():
                failures.append("binary SHA-256 does not match expected SHA-256")

    lms = lms_snapshot()
    if not lms.get("ok"):
        failures.append("LM Studio process inventory is unavailable")
    else:
        models = lms.get("models", [])
        if len(models) != expected_nodes:
            failures.append(
                "LM Studio has {} models, expected {}".format(len(models), expected_nodes)
            )
        for model in models:
            if str(model.get("status", "")).lower() != "idle" or int(
                model.get("queued", 0) or 0
            ) != 0:
                failures.append(
                    "LM Studio model {} is not idle/unqueued".format(
                        model.get("identifier", "unknown")
                    )
                )

    report = {
        "checked_at": utc_now(),
        "ok": not failures,
        "failures": failures,
        "forbidden_processes": forbidden,
        "goose_processes": goose_processes,
        "binary": binary_proof,
        "lms": lms,
    }
    return not failures, report


@dataclasses.dataclass
class ActivitySample:
    path: str
    model: str
    phase: str
    thinking_chars: int
    tool_calls: int
    malformed: int
    observed_windows: int
    repeated_windows: int
    repeat_share: float
    recurrence_window_chars: int
    earlier_reasoning: str
    provider_revision: int
    provider_bytes: int
    structured_output_bytes: int
    structured_output_chunks: int
    structured_output_active: bool
    raw_sha256: str

    @classmethod
    def from_bytes(cls, path: pathlib.Path, raw: bytes) -> "ActivitySample":
        value = json.loads(raw)
        recurrence = value.get("reasoning_recurrence") or {}
        provider = value.get("provider_stream") or {}
        return cls(
            path=str(path),
            model=str(value.get("model", "")),
            phase=str(value.get("phase", "")),
            thinking_chars=int(value.get("thinking_chars", 0) or 0),
            tool_calls=int(value.get("tool_calls", 0) or 0),
            malformed=int(value.get("malformed", 0) or 0),
            observed_windows=int(recurrence.get("observed_windows", 0) or 0),
            repeated_windows=int(recurrence.get("repeated_windows", 0) or 0),
            repeat_share=float(recurrence.get("repeat_share", 0.0) or 0.0),
            recurrence_window_chars=int(recurrence.get("window_chars", 0) or 0),
            earlier_reasoning=str(recurrence.get("earlier_reasoning", "")),
            provider_revision=int(provider.get("revision", 0) or 0),
            provider_bytes=int(provider.get("bytes", 0) or 0),
            structured_output_bytes=int(
                provider.get("structured_output_bytes", 0) or 0
            ),
            structured_output_chunks=int(
                provider.get("structured_output_chunks", 0) or 0
            ),
            structured_output_active=bool(
                provider.get("structured_output_active", False)
            ),
            raw_sha256=hashlib.sha256(raw).hexdigest(),
        )


@dataclasses.dataclass
class RecurrenceDecision:
    incident: bool
    reason: str
    evidence: Dict[str, Any]


class RecurrenceGate:
    """Corroborate recurrence using the engine's full-stream hash counts."""

    def __init__(
        self,
        repeat_share: float = DEFAULT_RECURRENCE_SHARE,
        repeated_windows: int = DEFAULT_REPEATED_WINDOWS,
        confirmations: int = 2,
    ) -> None:
        self.repeat_share = repeat_share
        self.repeated_windows = repeated_windows
        self.confirmations = confirmations
        self.previous: Dict[str, ActivitySample] = {}
        self.streaks: Dict[str, int] = {}

    def observe(self, sample: ActivitySample) -> RecurrenceDecision:
        previous = self.previous.get(sample.path)
        self.previous[sample.path] = sample
        if previous is None or sample.phase == "done":
            self.streaks[sample.path] = 0
            return RecurrenceDecision(False, "baseline", {})

        thinking_growth = sample.thinking_chars - previous.thinking_chars
        repeated_growth = sample.repeated_windows - previous.repeated_windows
        structured_growth = (
            sample.structured_output_bytes - previous.structured_output_bytes
        )
        full_stream_consistent = (
            sample.recurrence_window_chars > 0
            and 0 <= sample.repeated_windows <= sample.observed_windows
            and abs(
                sample.repeat_share
                - (
                    sample.repeated_windows / sample.observed_windows
                    if sample.observed_windows
                    else 0.0
                )
            )
            < 1e-6
        )
        recurrence_measured = (
            full_stream_consistent
            and sample.repeat_share >= self.repeat_share
            and sample.repeated_windows >= self.repeated_windows
            and thinking_growth > 0
        )
        structured_progress = structured_growth > 0 or (
            sample.structured_output_active and not previous.structured_output_active
        )

        unchanged = sample.raw_sha256 == previous.raw_sha256
        if unchanged:
            return RecurrenceDecision(False, "unchanged atomic activity digest", {})
        if recurrence_measured and not structured_progress:
            self.streaks[sample.path] = self.streaks.get(sample.path, 0) + 1
        elif structured_progress or thinking_growth < 0 or thinking_growth > 0:
            self.streaks[sample.path] = 0

        evidence = {
            "activity_path": sample.path,
            "model": sample.model,
            "source": "full-stream-reasoning-recurrence-meter",
            "tail_reasoning_used": False,
            "window_chars": sample.recurrence_window_chars,
            "observed_windows": sample.observed_windows,
            "repeated_windows": sample.repeated_windows,
            "repeat_share": sample.repeat_share,
            "repeat_share_gate": self.repeat_share,
            "measured_f924_repeat_share": F924_MEASURED_REPEAT_SHARE,
            "thinking_chars": sample.thinking_chars,
            "thinking_growth": thinking_growth,
            "repeated_window_growth": repeated_growth,
            "provider_revision": sample.provider_revision,
            "provider_bytes": sample.provider_bytes,
            "structured_output_bytes": sample.structured_output_bytes,
            "structured_output_growth": structured_growth,
            "structured_output_active": sample.structured_output_active,
            "corroboration_streak": self.streaks[sample.path],
            "required_corroborations": self.confirmations,
            "activity_sha256": sample.raw_sha256,
            "earlier_reasoning": sample.earlier_reasoning,
        }
        incident = self.streaks[sample.path] >= self.confirmations
        return RecurrenceDecision(
            incident,
            "measured full-stream recurrence grew while structured output did not progress",
            evidence,
        )


@dataclasses.dataclass
class EventRecord:
    value: Dict[str, Any]
    start_offset: int
    end_offset: int
    raw_sha256: str


class JsonlCursor:
    def __init__(self, path: pathlib.Path) -> None:
        self.path = path
        self.offset = 0
        self.pending = b""

    def read(self) -> Tuple[List[EventRecord], Optional[Dict[str, Any]]]:
        if not self.path.exists():
            return [], None
        size = self.path.stat().st_size
        if size < self.offset:
            return [], {
                "reason": "event log was truncated during a live run",
                "path": str(self.path),
                "previous_offset": self.offset,
                "current_size": size,
            }
        with self.path.open("rb") as handle:
            handle.seek(self.offset)
            chunk = handle.read()
        base = self.offset - len(self.pending)
        data = self.pending + chunk
        records: List[EventRecord] = []
        consumed = 0
        for line in data.splitlines(keepends=True):
            if not line.endswith((b"\n", b"\r")):
                break
            start = base + consumed
            consumed += len(line)
            try:
                value = json.loads(line)
            except json.JSONDecodeError as error:
                return records, {
                    "reason": "event log contains a complete malformed JSON line",
                    "path": str(self.path),
                    "start_offset": start,
                    "end_offset": start + len(line),
                    "line_sha256": hashlib.sha256(line).hexdigest(),
                    "parse_error": str(error),
                }
            records.append(
                EventRecord(
                    value=value,
                    start_offset=start,
                    end_offset=start + len(line),
                    raw_sha256=hashlib.sha256(line).hexdigest(),
                )
            )
        self.pending = data[consumed:]
        self.offset += len(chunk)
        return records, None


class EventGate:
    def __init__(
        self, expected_nodes: int, require_physical: bool, require_judge: bool = False
    ) -> None:
        self.expected_nodes = expected_nodes
        self.require_physical = require_physical
        self.require_judge = require_judge
        self.physical_snapshot_ready = False
        self.physical_control_active = False
        self.seed_roles: Optional[int] = None
        self.seed_merged = False
        self.seed_concurrency_observed = False
        self.initial_seed_models: List[str] = []
        self.run_finished = False

    def observe_lms(self, snapshot: Dict[str, Any]) -> None:
        if self.seed_roles is None or self.seed_merged or not snapshot.get("ok"):
            return
        models = snapshot.get("models", [])
        if len(models) != self.expected_nodes:
            return
        active = {
            "processing",
            "generating",
            "loading",
        }
        if all(str(model.get("status", "")).lower() in active for model in models):
            self.seed_concurrency_observed = True

    def observe(self, record: EventRecord) -> Optional[Dict[str, Any]]:
        value = record.value
        event = value.get("event")
        evidence = {
            "event": event,
            "seq": value.get("seq"),
            "event_log_start_offset": record.start_offset,
            "event_log_end_offset": record.end_offset,
            "event_line_sha256": record.raw_sha256,
            "value": value,
        }

        if event == "physical_fleet_snapshot_observed":
            lanes = (value.get("snapshot") or {}).get("lanes") or []
            hosts = [str(lane.get("host_id", "")) for lane in lanes]
            instances = [str(lane.get("model_instance_id", "")) for lane in lanes]
            ready = (
                value.get("enforcement") == "provider-boundary-ready"
                and value.get("provider_lifecycle_available") is True
                and len(lanes) == self.expected_nodes
                and all(hosts)
                and len(set(hosts)) == self.expected_nodes
                and all(instances)
                and len(set(instances)) == self.expected_nodes
                and all(
                    int(lane.get("advertised_instance_capacity", 0) or 0) > 0
                    for lane in lanes
                )
            )
            self.physical_snapshot_ready = ready
            if self.require_physical and not ready:
                return {
                    "reason": "physical broker was requested but the observed snapshot is not provider-boundary ready",
                    "evidence": evidence,
                }

        if event == "physical_fleet_snapshot_unavailable" and self.require_physical:
            return {
                "reason": "physical fleet snapshot is unavailable",
                "evidence": evidence,
            }

        if event == "research_seed_roles_assigned":
            roles = value.get("roles") or []
            initial = value.get("initial_node_roles") or []
            self.seed_roles = len(roles)
            initial_models = [str(item.get("model", "")) for item in initial]
            valid = (
                int(value.get("available_nodes", 0) or 0) == self.expected_nodes
                and int(value.get("assigned_nodes", 0) or 0) == self.expected_nodes
                and value.get("all_nodes_assigned_before_first_model_call") is True
                and value.get("coordinator_calls_started") == 0
                and len(initial_models) == self.expected_nodes
                and all(initial_models)
                and len(set(initial_models)) == self.expected_nodes
                and len(roles) >= self.expected_nodes
            )
            if not valid:
                return {
                    "reason": "research seed fan did not assign every distinct node before model work",
                    "evidence": evidence,
                }

        if (
            event == "research_pod_started"
            and self.require_physical
            and not self.physical_snapshot_ready
        ):
            return {
                "reason": "research started before a provider-boundary-ready physical snapshot was observed",
                "evidence": evidence,
            }

        if event == "research_seed_packet_attempt_started" and int(
            value.get("attempt", 0) or 0
        ) == 1:
            if len(self.initial_seed_models) < self.expected_nodes:
                self.initial_seed_models.append(str(value.get("model", "")))
                if len(self.initial_seed_models) == self.expected_nodes and len(
                    set(self.initial_seed_models)
                ) != self.expected_nodes:
                    return {
                        "reason": "initial research seed admissions reused a roster device before all nodes were active",
                        "evidence": evidence,
                    }

        if event in (
            "research_seed_packet_reassigned",
            "research_evidence_packet_reassigned",
            "planning_pod_audit_reassigned",
        ):
            prior = value.get("prior_failed_nodes") or value.get(
                "prior_failed_devices"
            ) or []
            if int(value.get("attempt", 0) or 0) <= 1 or value.get("model") in prior:
                return {
                    "reason": "retry reassignment did not move authority to a distinct roster device",
                    "evidence": evidence,
                }

        if event == "research_pod_role_started":
            role = value.get("role")
            if role == "evidence-saturation-coordinator" and not self.seed_merged:
                return {
                    "reason": "research coordinator started before every seed packet compiled and merged",
                    "evidence": evidence,
                }

        if event == "research_pod_role_completed" and value.get("role") == (
            "seed-requirement-evidence-mapper"
        ):
            if int(value.get("tool_calls", 0) or 0) != 1:
                return {
                    "reason": "ResponseOnly research seed did not complete with exactly one final_output call",
                    "evidence": evidence,
                }

        if event == "research_seed_merged":
            self.seed_merged = True
            if self.seed_roles is not None and int(
                value.get("completed_node_roles", 0) or 0
            ) != self.seed_roles:
                return {
                    "reason": "research seed merge did not contain every assigned semantic packet",
                    "evidence": evidence,
                }
            if not self.seed_concurrency_observed:
                return {
                    "reason": "seed fan merged without an LM Studio sample showing all nodes active concurrently",
                    "evidence": evidence,
                }

        if event == "physical_semantic_control_active":
            self.physical_control_active = True
            semantic_ready = (
                int(value.get("verified_route_count", 0) or 0) == self.expected_nodes
                and value.get("semantic_nudge_delivery") is True
                and (not self.require_judge or value.get("legacy_judge_substituted") is True)
            )
            if self.require_physical and not semantic_ready:
                return {
                    "reason": "physical semantic control started without every verified route and requested judge substitution",
                    "evidence": evidence,
                }

        if event == "task_dispatched" and self.require_physical and not self.physical_control_active:
            return {
                "reason": "build task dispatched before physical semantic control became active",
                "evidence": evidence,
            }

        if event == "run_finished":
            self.run_finished = True
        return None


def write_capture(path: pathlib.Path, payload: bytes) -> Dict[str, Any]:
    atomic_write(path, payload)
    return {
        "path": str(path),
        "bytes": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def capture_segment(source: pathlib.Path, destination: pathlib.Path) -> Dict[str, Any]:
    if not source.exists():
        return {"source": str(source), "missing": True}
    end = source.stat().st_size
    start = max(0, end - 2 * 1024 * 1024)
    with source.open("rb") as handle:
        handle.seek(start)
        payload = handle.read(end - start)
    proof = write_capture(destination, payload)
    proof.update({"source": str(source), "start_offset": start, "end_offset": end})
    return proof


def capture_incident(
    run_dir: pathlib.Path,
    monitor_dir: pathlib.Path,
    pid: int,
    launch_identity: Dict[str, str],
    binary: pathlib.Path,
    binary_sha256: str,
    reason: str,
    evidence: Dict[str, Any],
    event_log: pathlib.Path,
    engine_console: pathlib.Path,
) -> pathlib.Path:
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    safe_reason = "".join(character if character.isalnum() else "-" for character in reason)
    incident_dir = monitor_dir / "incidents" / (stamp + "-" + safe_reason[:64])
    incident_dir.mkdir(parents=True, exist_ok=False)

    activity_proofs = []
    activity_dir = run_dir / ".swarm" / "activity"
    if activity_dir.is_dir():
        for source in sorted(activity_dir.glob("*.json")):
            raw = source.read_bytes()
            destination = incident_dir / "activity" / source.name
            proof = write_capture(destination, raw)
            proof["source"] = str(source)
            activity_proofs.append(proof)

    telemetry = run_dir / ".swarm" / "telemetry.jsonl"
    segments = [
        capture_segment(event_log, incident_dir / "run-jsonl.segment"),
        capture_segment(engine_console, incident_dir / "engine-console.segment"),
        capture_segment(telemetry, incident_dir / "telemetry.segment"),
    ]
    heartbeat_candidates = [run_dir / "heartbeat", run_dir / ".swarm" / "heartbeat"]
    heartbeat_proofs = []
    for heartbeat in heartbeat_candidates:
        if heartbeat.exists():
            raw = heartbeat.read_bytes()
            proof = write_capture(
                incident_dir / ("heartbeat-" + heartbeat.parent.name), raw
            )
            proof["source"] = str(heartbeat)
            heartbeat_proofs.append(proof)

    diagnostics = {
        "process": process_identity(pid),
        "goose_processes": local_goose_processes(),
        "lms": lms_snapshot(),
        "lsof": run_capture(["lsof", "-nP", "-a", "-p", str(pid), "-iTCP"], 5.0),
        "nettop": run_capture(
            ["nettop", "-n", "-m", "tcp", "-p", str(pid), "-d", "-x", "-L", "1"],
            5.0,
        ),
    }
    write_json(incident_dir / "diagnostics.json", diagnostics)

    incident = {
        "captured_at": utc_now(),
        "reason": reason,
        "evidence": evidence,
        "run_dir": str(run_dir),
        "pid": pid,
        "launch_identity": launch_identity,
        "current_identity": process_identity(pid),
        "binary": str(binary),
        "binary_sha256": binary_sha256,
        "event_log_offset": event_log.stat().st_size if event_log.exists() else None,
        "engine_console_offset": engine_console.stat().st_size
        if engine_console.exists()
        else None,
        "segments": segments,
        "activity_files": activity_proofs,
        "heartbeats": heartbeat_proofs,
    }
    write_json(incident_dir / "incident.json", incident)

    manifest_lines = []
    for path in sorted(incident_dir.rglob("*")):
        if path.is_file() and path.name not in ("manifest.sha256", "CAPTURE_COMPLETE"):
            manifest_lines.append(
                "{}  {}".format(sha256_file(path), path.relative_to(incident_dir))
            )
    write_capture(
        incident_dir / "manifest.sha256", ("\n".join(manifest_lines) + "\n").encode()
    )
    write_capture(
        incident_dir / "CAPTURE_COMPLETE",
        (json.dumps({"captured_at": utc_now(), "pid": pid}) + "\n").encode(),
    )
    directory_fd = os.open(str(incident_dir), os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    return incident_dir


class DurableLog:
    def __init__(self, path: pathlib.Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.handle = path.open("a", encoding="utf-8", buffering=1)

    def emit(self, event: str, **fields: Any) -> None:
        row = {"at": utc_now(), "event": event}
        row.update(fields)
        self.handle.write(json.dumps(row, sort_keys=True) + "\n")
        self.handle.flush()
        os.fsync(self.handle.fileno())


def acquire_monitor_lock(path: pathlib.Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        lock_fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError:
        text = path.read_text(encoding="utf-8").strip()
        if text.isdigit() and process_identity(int(text)) is not None:
            raise RuntimeError("live monitor already owns lock: {}".format(path))
        path.unlink()
        lock_fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.write(lock_fd, (str(os.getpid()) + "\n").encode())
    os.fsync(lock_fd)
    os.close(lock_fd)


def read_pid_file(path: pathlib.Path, wait_secs: float) -> int:
    deadline = time.monotonic() + wait_secs
    while time.monotonic() < deadline:
        if path.exists():
            text = path.read_text(encoding="utf-8").strip()
            if text.isdigit() and int(text) > 1:
                return int(text)
        time.sleep(0.25)
    raise RuntimeError("pid file did not contain a valid pid: {}".format(path))


def stop_after_capture(
    pid: int,
    launch_identity: Dict[str, str],
    incident_dir: pathlib.Path,
    expected_stop_path: Optional[pathlib.Path] = None,
    initiator: str = "monitor",
) -> bool:
    current = process_identity(pid)
    if current != launch_identity:
        write_json(
            incident_dir / "STOP_REFUSED.json",
            {
                "at": utc_now(),
                "reason": "process identity changed; refusing to signal a reused pid",
                "launch_identity": launch_identity,
                "current_identity": current,
            },
        )
        return False
    if not (incident_dir / "CAPTURE_COMPLETE").is_file():
        write_json(
            incident_dir / "STOP_REFUSED.json",
            {
                "at": utc_now(),
                "reason": "incident capture is not durably complete",
                "launch_identity": launch_identity,
            },
        )
        return False
    marker_path = expected_stop_path or incident_dir.parent.parent / "expected-stop.json"
    write_json(
        marker_path,
        {
            "armed_at": utc_now(),
            "pid": pid,
            "launch_identity": launch_identity,
            "incident_dir": str(incident_dir),
            "initiator": initiator,
            "signal": "SIGTERM",
        },
    )
    os.kill(pid, signal.SIGTERM)
    write_json(
        incident_dir / "SIGNAL_SENT.json",
        {"at": utc_now(), "pid": pid, "signal": "SIGTERM"},
    )
    return True


def matching_expected_stop(
    path: pathlib.Path,
    pid: int,
    launch_identity: Dict[str, str],
) -> Optional[Dict[str, Any]]:
    try:
        marker = json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, OSError, json.JSONDecodeError):
        return None
    if (
        marker.get("pid") != pid
        or marker.get("launch_identity") != launch_identity
        or marker.get("signal") != "SIGTERM"
    ):
        return None
    return marker


def watch(args: argparse.Namespace) -> int:
    run_dir = pathlib.Path(args.run_dir).resolve()
    binary = pathlib.Path(args.binary).resolve()
    if not binary.is_file():
        raise RuntimeError("binary does not exist: {}".format(binary))
    actual_sha256 = sha256_file(binary)
    if actual_sha256.lower() != args.sha256.lower():
        raise RuntimeError(
            "binary SHA-256 mismatch: expected {}, got {}".format(
                args.sha256, actual_sha256
            )
        )
    pid = read_pid_file(pathlib.Path(args.pid_file), args.pid_wait_secs)
    launch_identity = process_identity(pid)
    if launch_identity is None:
        raise RuntimeError("Goose pid {} is not alive".format(pid))
    if str(binary) not in launch_identity["ps_line"]:
        raise RuntimeError(
            "pid {} command does not name expected binary {}: {}".format(
                pid, binary, launch_identity["ps_line"]
            )
        )

    monitor_dir = run_dir / ".swarm-monitor"
    monitor_dir.mkdir(parents=True, exist_ok=True)
    lock_path = monitor_dir / "monitor.pid"
    expected_stop_path = monitor_dir / "expected-stop.json"
    acquire_monitor_lock(lock_path)

    durable = DurableLog(monitor_dir / "watch.jsonl")
    event_log = pathlib.Path(args.event_log or (run_dir / "run.jsonl"))
    engine_console = pathlib.Path(
        args.engine_console or (run_dir / "engine-console.log")
    )
    cursor = JsonlCursor(event_log)
    event_gate = EventGate(
        args.expected_nodes, args.require_physical, args.require_judge
    )
    recurrence_gate = RecurrenceGate(
        args.recurrence_share, args.repeated_windows, args.confirmations
    )
    durable.emit(
        "monitor_started",
        pid=pid,
        launch_identity=launch_identity,
        binary=str(binary),
        binary_sha256=actual_sha256,
        event_log=str(event_log),
        recurrence_source="full-stream-reasoning-recurrence-meter",
        recurrence_share=args.recurrence_share,
        repeated_windows=args.repeated_windows,
        confirmations=args.confirmations,
        stop_on_incident=args.stop_on_incident,
    )

    last_lms_at = 0.0
    try:
        while True:
            incident: Optional[Dict[str, Any]] = None
            records, cursor_error = cursor.read()
            if cursor_error is not None:
                incident = cursor_error
            for record in records:
                event = record.value.get("event")
                if event in (
                    "research_pod_started",
                    "research_seed_roles_assigned",
                    "research_seed_merged",
                    "research_seed_tail_started",
                    "research_evidence_tail_started",
                    "research_saturation_checked",
                    "planning_pod_started",
                    "planning_pod_audits_drained",
                    "physical_semantic_control_active",
                    "task_dispatched",
                    "repair_started",
                    "run_finished",
                ):
                    durable.emit(
                        "engine_transition",
                        engine_event=event,
                        seq=record.value.get("seq"),
                        start_offset=record.start_offset,
                        end_offset=record.end_offset,
                    )
                gate_incident = event_gate.observe(record)
                if gate_incident is not None:
                    incident = gate_incident
                    break

            activity_dir = run_dir / ".swarm" / "activity"
            if incident is None and activity_dir.is_dir():
                for activity in sorted(activity_dir.glob("*.json")):
                    try:
                        raw = activity.read_bytes()
                        sample = ActivitySample.from_bytes(activity, raw)
                    except (OSError, ValueError, json.JSONDecodeError) as error:
                        incident = {
                            "reason": "atomic activity digest is unreadable",
                            "evidence": {
                                "path": str(activity),
                                "error": repr(error),
                            },
                        }
                        break
                    decision = recurrence_gate.observe(sample)
                    if decision.incident:
                        incident = {
                            "reason": decision.reason,
                            "evidence": decision.evidence,
                        }
                        break

            if incident is not None:
                reason = str(incident.get("reason", "monitor incident"))
                evidence = incident.get("evidence", incident)
                durable.emit("incident_detected", reason=reason, evidence=evidence)
                incident_dir = capture_incident(
                    run_dir,
                    monitor_dir,
                    pid,
                    launch_identity,
                    binary,
                    actual_sha256,
                    reason,
                    evidence,
                    event_log,
                    engine_console,
                )
                durable.emit("incident_captured", incident_dir=str(incident_dir))
                if args.stop_on_incident:
                    signalled = stop_after_capture(pid, launch_identity, incident_dir)
                    durable.emit(
                        "stop_after_capture", incident_dir=str(incident_dir), signalled=signalled
                    )
                return 20

            identity = process_identity(pid)
            if identity is None:
                if event_gate.run_finished:
                    durable.emit("monitor_completed", outcome="run_finished")
                    return 0
                expected_stop = matching_expected_stop(
                    expected_stop_path, pid, launch_identity
                )
                if expected_stop is not None:
                    durable.emit(
                        "monitor_completed",
                        outcome="expected_stop",
                        expected_stop=expected_stop,
                    )
                    return 0
                reason = "Goose process exited without a run_finished event"
                incident_dir = capture_incident(
                    run_dir,
                    monitor_dir,
                    pid,
                    launch_identity,
                    binary,
                    actual_sha256,
                    reason,
                    {"last_event_log_offset": cursor.offset},
                    event_log,
                    engine_console,
                )
                durable.emit("incident_captured", reason=reason, incident_dir=str(incident_dir))
                return 21
            if identity != launch_identity:
                reason = "Goose pid identity changed during the run"
                incident_dir = capture_incident(
                    run_dir,
                    monitor_dir,
                    pid,
                    launch_identity,
                    binary,
                    actual_sha256,
                    reason,
                    {"current_identity": identity},
                    event_log,
                    engine_console,
                )
                durable.emit("incident_captured", reason=reason, incident_dir=str(incident_dir))
                return 22

            now = time.monotonic()
            if now - last_lms_at >= args.lms_poll_secs:
                lms = lms_snapshot()
                event_gate.observe_lms(lms)
                durable.emit("lms_snapshot", snapshot=lms)
                last_lms_at = now
            write_json(
                monitor_dir / "status.json",
                {
                    "at": utc_now(),
                    "pid": pid,
                    "event_log_offset": cursor.offset,
                    "event_log_pending_bytes": len(cursor.pending),
                    "seed_merged": event_gate.seed_merged,
                    "seed_concurrency_observed": event_gate.seed_concurrency_observed,
                    "physical_snapshot_ready": event_gate.physical_snapshot_ready,
                    "physical_control_active": event_gate.physical_control_active,
                },
            )
            atomic_write(monitor_dir / "heartbeat", (utc_now() + "\n").encode())
            time.sleep(args.poll_secs)
    finally:
        try:
            lock_path.unlink()
        except FileNotFoundError:
            pass


def preflight(args: argparse.Namespace) -> int:
    binary = pathlib.Path(args.binary).resolve() if args.binary else None
    ok, report = preflight_report(
        args.forbid_pid, binary, args.sha256, args.expected_nodes
    )
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if args.output:
        write_json(pathlib.Path(args.output), report)
    return 0 if ok else 10


def evaluate_activity(args: argparse.Namespace) -> int:
    gate = RecurrenceGate(
        args.recurrence_share, args.repeated_windows, args.confirmations
    )
    decision: Optional[RecurrenceDecision] = None
    for path_text in args.activity:
        path = pathlib.Path(path_text)
        decision = gate.observe(ActivitySample.from_bytes(path, path.read_bytes()))
        print(json.dumps(dataclasses.asdict(decision), sort_keys=True))
    return 20 if decision is not None and decision.incident else 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    subparsers = root.add_subparsers(dest="command", required=True)

    pre = subparsers.add_parser("preflight")
    pre.add_argument("--forbid-pid", type=int, action="append", default=[])
    pre.add_argument("--binary")
    pre.add_argument("--sha256")
    pre.add_argument("--expected-nodes", type=int, default=3)
    pre.add_argument("--output")
    pre.set_defaults(handler=preflight)

    monitor = subparsers.add_parser("watch")
    monitor.add_argument("--run-dir", required=True)
    monitor.add_argument("--pid-file", required=True)
    monitor.add_argument("--binary", required=True)
    monitor.add_argument("--sha256", required=True)
    monitor.add_argument("--event-log")
    monitor.add_argument("--engine-console")
    monitor.add_argument("--expected-nodes", type=int, default=3)
    monitor.add_argument("--require-physical", action="store_true")
    monitor.add_argument("--require-judge", action="store_true")
    monitor.add_argument("--stop-on-incident", action="store_true")
    monitor.add_argument("--poll-secs", type=float, default=5.0)
    monitor.add_argument("--lms-poll-secs", type=float, default=15.0)
    monitor.add_argument("--pid-wait-secs", type=float, default=300.0)
    monitor.add_argument(
        "--recurrence-share", type=float, default=DEFAULT_RECURRENCE_SHARE
    )
    monitor.add_argument(
        "--repeated-windows", type=int, default=DEFAULT_REPEATED_WINDOWS
    )
    monitor.add_argument("--confirmations", type=int, default=2)
    monitor.set_defaults(handler=watch)

    evaluate = subparsers.add_parser("evaluate-activity")
    evaluate.add_argument("activity", nargs="+")
    evaluate.add_argument(
        "--recurrence-share", type=float, default=DEFAULT_RECURRENCE_SHARE
    )
    evaluate.add_argument(
        "--repeated-windows", type=int, default=DEFAULT_REPEATED_WINDOWS
    )
    evaluate.add_argument("--confirmations", type=int, default=2)
    evaluate.set_defaults(handler=evaluate_activity)
    return root


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parser().parse_args(argv)
    try:
        return int(args.handler(args))
    except KeyboardInterrupt:
        return 130
    except Exception as error:
        print("monitor error: {}".format(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
