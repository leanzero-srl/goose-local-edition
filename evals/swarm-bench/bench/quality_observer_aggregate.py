"""Build a redacted morning summary from an append-only quality-observer tick log."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable
from zoneinfo import ZoneInfo


CLASSIFICATIONS = ("observation", "watch", "approaching", "proven")
CLASS_RANK = {name: rank for rank, name in enumerate(CLASSIFICATIONS)}
LIFECYCLE_KEYS = (
    "permits",
    "terminal",
    "released",
    "active_provider_requests",
    "unreleased_admissions",
)
PROGRESS_KEYS = (
    "jury_units_completed",
    "jury_pairs_completed",
    "adjudication_events",
    "citation_events",
    "corrections_started",
    "corrected_packets_completed",
    "material_gaps_in_completed_packets",
)
SAFE_IDENTIFIER_CHARACTERS = frozenset(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.:/@+-"
)
SENSITIVE_IDENTIFIER_PREFIXES = (
    "ghp_",
    "github_pat_",
    "sk-",
    "sk_",
    "xoxb-",
    "xoxp-",
)


def parse_timestamp(value: object) -> dt.datetime | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed if parsed.tzinfo is not None else None


def read_tick_prefix(path: Path) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    payload = path.read_bytes()
    complete_end = payload.rfind(b"\n") + 1
    complete = payload[:complete_end]
    pending = payload[complete_end:]
    ticks: list[dict[str, Any]] = []
    malformed = 0
    for raw in complete.splitlines():
        try:
            value = json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError):
            malformed += 1
            continue
        if isinstance(value, dict):
            ticks.append(value)
        else:
            malformed += 1
    return ticks, {
        "bytes": len(payload),
        "complete_bytes": len(complete),
        "pending_bytes": len(pending),
        "sha256": hashlib.sha256(payload).hexdigest(),
        "parsed_lines": len(ticks),
        "malformed_lines": malformed,
    }


def morning_bounds(
    local_day: dt.date,
    timezone: ZoneInfo,
    now: dt.datetime,
    end_hour: int,
) -> tuple[dt.datetime, dt.datetime]:
    if not 1 <= end_hour <= 23:
        raise ValueError("morning end hour must be between 1 and 23")
    if now.tzinfo is None:
        raise ValueError("now must carry a timezone")
    start = dt.datetime(
        local_day.year, local_day.month, local_day.day, tzinfo=timezone
    )
    scheduled_end = dt.datetime(
        local_day.year, local_day.month, local_day.day, end_hour, tzinfo=timezone
    )
    now_local = now.astimezone(timezone)
    if local_day < now_local.date():
        return start, scheduled_end
    if local_day > now_local.date():
        return start, start
    return start, min(now_local, scheduled_end)


def number(value: object) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value)


def median(values: Iterable[float]) -> float | None:
    materialized = list(values)
    return round(float(statistics.median(materialized)), 6) if materialized else None


def safe_identifier(value: object, fallback: str = "unknown") -> str:
    if not isinstance(value, str) or not value:
        return fallback
    safe_shape = len(value) <= 160 and all(
        character in SAFE_IDENTIFIER_CHARACTERS for character in value
    )
    if safe_shape and not value.casefold().startswith(SENSITIVE_IDENTIFIER_PREFIXES):
        return value
    return f"sha256:{hashlib.sha256(value.encode()).hexdigest()}"


def safe_source_metadata(source: dict[str, Any]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key in (
        "bytes",
        "complete_bytes",
        "pending_bytes",
        "parsed_lines",
        "malformed_lines",
    ):
        value = source.get(key)
        if isinstance(value, int) and not isinstance(value, bool):
            result[key] = value
    digest = source.get("sha256")
    if isinstance(digest, str) and len(digest) == 64 and all(
        character in "0123456789abcdef" for character in digest
    ):
        result["sha256"] = digest
    return result


def highest_classification(values: Iterable[str]) -> str:
    valid = [value for value in values if value in CLASS_RANK]
    return max(valid, key=CLASS_RANK.__getitem__) if valid else "observation"


def classify_activity(rows: list[dict[str, Any]]) -> tuple[str, list[str]]:
    classification = "observation"
    reasons: set[str] = set()
    recurrence_streak = 0
    for row in rows:
        explicit = row.get("classification")
        if explicit in CLASS_RANK:
            classification = highest_classification((classification, explicit))
            reasons.add(f"explicit_{explicit}")
        growth = row.get("growth") if isinstance(row.get("growth"), dict) else {}
        share = number(row.get("recurrence_share")) or 0.0
        repeated = int(number(row.get("recurrence_repeated_windows")) or 0)
        thinking_growth = number(growth.get("thinking_chars")) or 0.0
        repeated_growth = number(growth.get("recurrence_repeated_windows")) or 0.0
        structured_growth = number(growth.get("structured_output_bytes")) or 0.0
        structured_total = number(row.get("structured_output_bytes")) or 0.0
        structured_stagnation = (
            number(row.get("structured_stagnation_secs"))
            or number(row.get("stagnation_seconds"))
            or 0.0
        )
        recurrence_proof_sample = (
            share >= 0.30
            and repeated >= 1024
            and thinking_growth > 0
            and repeated_growth > 0
            and structured_total == 0
            and structured_growth == 0
        )
        recurrence_streak = recurrence_streak + 1 if recurrence_proof_sample else 0
        if recurrence_streak >= 2:
            classification = "proven"
            reasons.add("recurrence_gate_confirmed_twice")
        elif share >= 0.20 and repeated >= 512 and structured_total == 0:
            classification = highest_classification((classification, "approaching"))
            reasons.add("recurrence_near_gate")
        elif (
            thinking_growth > 0
            and repeated_growth > 0
            and structured_total == 0
            and structured_growth == 0
        ):
            classification = highest_classification((classification, "watch"))
            reasons.add("recurrence_growth_without_structured_growth")
        ratio = number(row.get("thinking_vs_completed_median"))
        if (
            ratio is not None
            and ratio >= 2.0
            and structured_total == 0
            and structured_stagnation >= 120.0
        ):
            classification = highest_classification((classification, "watch"))
            reasons.add("same_role_thinking_at_least_2x_median")
        if int(number(row.get("errors")) or 0) or int(number(row.get("malformed")) or 0):
            classification = highest_classification((classification, "watch"))
            reasons.add("provider_error_or_malformed")
        terminal = row.get("terminal_acceptance")
        if terminal in (False, "rejected", "failed"):
            classification = "proven"
            reasons.add("terminal_rejected")
    if any(
        "correction"
        in str(row.get("semantic_role") or row.get("role") or "")
        for row in rows
    ):
        classification = highest_classification((classification, "watch"))
        reasons.add("correction_role")
    return classification, sorted(reasons)


def safe_tool_names(rows: Iterable[dict[str, Any]]) -> list[str]:
    names: set[str] = set()
    for row in rows:
        candidates = row.get("tool_call_names") or row.get("tool_names") or []
        if not isinstance(candidates, list):
            continue
        for candidate in candidates:
            if not isinstance(candidate, str):
                continue
            if 1 <= len(candidate) <= 80:
                names.add(safe_identifier(candidate))
    return sorted(names)


def counter_rollup(
    ticks: list[dict[str, Any]], field: str, keys: tuple[str, ...]
) -> dict[str, Any]:
    snapshots = [
        tick.get(field)
        for tick in ticks
        if isinstance(tick.get(field), dict)
    ]
    if not snapshots:
        return {"first": {}, "last": {}, "delta": {}}
    first = {key: int(number(snapshots[0].get(key)) or 0) for key in keys}
    last = {key: int(number(snapshots[-1].get(key)) or 0) for key in keys}
    return {
        "first": first,
        "last": last,
        "delta": {key: last[key] - first[key] for key in keys},
    }


def role_rollups(ticks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    histories: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for tick in ticks:
        active = tick.get("active_calls") or []
        if not isinstance(active, list):
            continue
        for row in active:
            if not isinstance(row, dict):
                continue
            activity = safe_identifier(row.get("work_id") or row.get("activity"), "")
            role = safe_identifier(row.get("semantic_role") or row.get("role"))
            model = safe_identifier(row.get("model"))
            if activity:
                histories[(role, model, activity)].append(row)

    grouped: dict[
        tuple[str, str], list[tuple[str, list[dict[str, Any]]]]
    ] = defaultdict(list)
    for (role, model, activity), rows in histories.items():
        grouped[(role, model)].append((activity, rows))

    result: list[dict[str, Any]] = []
    for (role, model), activities in sorted(grouped.items()):
        all_rows = [row for _, rows in activities for row in rows]
        latest = [rows[-1] for _, rows in activities]
        classifications: list[str] = []
        reasons: set[str] = set()
        delta_thinking = 0
        delta_structured = 0
        delta_repeated = 0
        delta_samples = 0
        for _, rows in activities:
            classification, activity_reasons = classify_activity(rows)
            classifications.append(classification)
            reasons.update(activity_reasons)
            for row in rows[1:]:
                growth = row.get("growth") if isinstance(row.get("growth"), dict) else {}
                delta_thinking += max(0, int(number(growth.get("thinking_chars")) or 0))
                delta_structured += max(
                    0, int(number(growth.get("structured_output_bytes")) or 0)
                )
                delta_repeated += max(
                    0, int(number(growth.get("recurrence_repeated_windows")) or 0)
                )
                delta_samples += 1
        thinking_ratios = [
            value
            for value in (number(row.get("thinking_vs_completed_median")) for row in latest)
            if value is not None
        ]
        elapsed_ratios = [
            value
            for value in (number(row.get("elapsed_vs_completed_median")) for row in latest)
            if value is not None
        ]
        legacy_stagnation = [
            value
            for value in (number(row.get("stagnation_seconds")) for row in latest)
            if value is not None
        ]
        structured_stagnation = [
            value
            for value in (
                number(row.get("structured_stagnation_secs")) for row in latest
            )
            if value is not None
        ]
        thinking_stagnation = [
            value
            for value in (number(row.get("thinking_stagnation_secs")) for row in latest)
            if value is not None
        ]
        physical_hosts = sorted(
            {
                safe_identifier(row.get("physical_host_id"))
                for row in all_rows
                if row.get("physical_host_id") is not None
            }
        )
        broker_roles = sorted(
            {
                safe_identifier(row.get("broker_role"))
                for row in all_rows
                if row.get("broker_role") is not None
            }
        )
        provider_request_keys: set[str] = set()
        for row in all_rows:
            request_key = row.get("provider_request_key")
            if not isinstance(request_key, dict):
                continue
            ordinal = request_key.get("ordinal")
            request_id = request_key.get("provider_request_id")
            if isinstance(ordinal, int) and isinstance(request_id, str):
                provider_request_keys.add(f"{ordinal}:{safe_identifier(request_id)}")
        result.append(
            {
                "role": role,
                "model": model,
                "classification": highest_classification(classifications),
                "classification_basis": sorted(reasons),
                "distinct_activities": len(activities),
                "tick_observations": len(all_rows),
                "physical_hosts": physical_hosts,
                "broker_roles": broker_roles,
                "provider_request_keys": sorted(provider_request_keys),
                "completed_baseline_n_max": max(
                    (int(number(row.get("completed_baseline_n")) or 0) for row in all_rows),
                    default=0,
                ),
                "thinking_vs_completed_median": {
                    "median": median(thinking_ratios),
                    "max": round(max(thinking_ratios), 6) if thinking_ratios else None,
                },
                "elapsed_vs_completed_median": {
                    "median": median(elapsed_ratios),
                    "max": round(max(elapsed_ratios), 6) if elapsed_ratios else None,
                },
                "recurrence_share_max": round(
                    max(
                        (
                            number(row.get("recurrence_share")) or 0.0
                            for row in all_rows
                        ),
                        default=0.0,
                    ),
                    6,
                ),
                "stagnation_seconds_max": {
                    "structured": (
                        round(max(structured_stagnation), 3)
                        if structured_stagnation
                        else round(max(legacy_stagnation), 3)
                        if legacy_stagnation
                        else None
                    ),
                    "thinking": (
                        round(max(thinking_stagnation), 3)
                        if thinking_stagnation
                        else None
                    ),
                },
                "tool_calls_max": max(
                    (int(number(row.get("tool_calls")) or 0) for row in all_rows),
                    default=0,
                ),
                "tool_names": safe_tool_names(all_rows),
                "errors_max": max(
                    (int(number(row.get("errors")) or 0) for row in all_rows), default=0
                ),
                "malformed_max": max(
                    (int(number(row.get("malformed")) or 0) for row in all_rows), default=0
                ),
                "observed_growth": {
                    "delta_samples": delta_samples,
                    "thinking_chars": delta_thinking,
                    "structured_output_bytes": delta_structured,
                    "recurrence_repeated_windows": delta_repeated,
                },
            }
        )
    return result


def correction_events(ticks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    corrections: dict[tuple[object, ...], dict[str, Any]] = {}
    for tick in ticks:
        audit_rows = tick.get("correction_audits") or []
        recent_rows = tick.get("recent_corrections") or []
        rows = audit_rows if isinstance(audit_rows, list) and audit_rows else recent_rows
        if not isinstance(rows, list):
            rows = []
        for row in rows:
            if not isinstance(row, dict):
                continue
            started_seq = row.get("started_seq", row.get("seq"))
            if not isinstance(started_seq, int):
                continue
            outcome = row.get("outcome")
            if outcome not in {
                "active",
                "accepted",
                "repeated",
                "repeated_then_rescheduled",
            }:
                outcome = "observed"
            safe_row = {
                "started_seq": started_seq,
                "outcome_seq": row.get("outcome_seq")
                if isinstance(row.get("outcome_seq"), int)
                else None,
                "partition_id": safe_identifier(row.get("partition_id")),
                "pass": row.get("pass") if isinstance(row.get("pass"), int) else None,
                "physical_host_id": safe_identifier(row.get("physical_host_id")),
                "correction": (
                    row.get("correction")
                    if isinstance(row.get("correction"), int)
                    and not isinstance(row.get("correction"), bool)
                    else None
                ),
                "outcome": outcome,
                "correction_duration_secs": number(row.get("correction_duration_secs")),
                "total_packet_duration_secs": number(
                    row.get("total_packet_duration_secs")
                ),
                "material_gaps": (
                    row.get("material_gaps")
                    if isinstance(row.get("material_gaps"), int)
                    else None
                ),
                "ledger_corrections": (
                    row.get("ledger_corrections")
                    if isinstance(row.get("ledger_corrections"), int)
                    else None
                ),
            }
            digest = row.get("compiler_error_sha256")
            if isinstance(digest, str) and len(digest) == 64 and all(
                character in "0123456789abcdef" for character in digest
            ):
                safe_row["compiler_error_sha256"] = digest
            identity = (
                started_seq,
                safe_row["partition_id"],
                safe_row["pass"],
                safe_row["physical_host_id"],
            )
            corrections[identity] = safe_row
    return [corrections[key] for key in sorted(corrections, key=str)]


def build_morning_aggregate(
    ticks: list[dict[str, Any]],
    source: dict[str, Any],
    timezone_name: str,
    local_day: dt.date,
    now: dt.datetime,
    end_hour: int = 12,
) -> dict[str, Any]:
    timezone = ZoneInfo(timezone_name)
    start, end = morning_bounds(local_day, timezone, now, end_hour)
    selected: list[tuple[dt.datetime, dict[str, Any]]] = []
    invalid_timestamps = 0
    for tick in ticks:
        timestamp = parse_timestamp(tick.get("at"))
        if timestamp is None:
            invalid_timestamps += 1
            continue
        local_timestamp = timestamp.astimezone(timezone)
        if start <= local_timestamp <= end:
            selected.append((timestamp, tick))
    selected.sort(key=lambda item: item[0])
    if not selected:
        raise ValueError("no observer ticks fall inside the requested morning window")
    times = [timestamp for timestamp, _ in selected]
    morning_ticks = [tick for _, tick in selected]
    gaps = [
        (current - previous).total_seconds()
        for previous, current in zip(times, times[1:])
    ]
    role_summary = role_rollups(morning_ticks)
    corrections = correction_events(morning_ticks)
    quality_classification = highest_classification(
        [row["classification"] for row in role_summary]
        + [
            tick["classification"]
            for tick in morning_ticks
            if tick.get("classification") in CLASS_RANK
        ]
        + (["watch"] if corrections else [])
    )
    tick_errors = sum(tick.get("event") == "audit_tick_error" for tick in morning_ticks)
    return {
        "schema_version": 1,
        "kind": "quality-observer-morning-aggregate",
        "generated_at": now.astimezone(dt.timezone.utc).isoformat(),
        "timezone": timezone_name,
        "local_date": local_day.isoformat(),
        "window": {
            "start": start.isoformat(),
            "observed_through": end.isoformat(),
            "scheduled_end_hour": end_hour,
        },
        "source": safe_source_metadata(source),
        "ticks": {
            "count": len(morning_ticks),
            "invalid_timestamp_lines": invalid_timestamps,
            "tick_errors": tick_errors,
            "first_at": times[0].isoformat(),
            "last_at": times[-1].isoformat(),
            "median_gap_seconds": median(gaps),
            "max_gap_seconds": round(max(gaps), 3) if gaps else None,
        },
        "run": {
            "goose_pid": morning_ticks[-1].get("goose_pid"),
            "goose_alive": morning_ticks[-1].get("goose_alive"),
            "first_seq": morning_ticks[0].get("last_seq"),
            "last_seq": morning_ticks[-1].get("last_seq"),
            "seq_delta": (
                int(morning_ticks[-1]["last_seq"]) - int(morning_ticks[0]["last_seq"])
                if isinstance(morning_ticks[0].get("last_seq"), int)
                and isinstance(morning_ticks[-1].get("last_seq"), int)
                else None
            ),
        },
        "quality_classification": quality_classification,
        "instrument_classification": "proven" if tick_errors else "observation",
        "lifecycle": counter_rollup(morning_ticks, "lifecycle", LIFECYCLE_KEYS),
        "progress": counter_rollup(morning_ticks, "progress", PROGRESS_KEYS),
        "same_role_baselines": role_summary,
        "corrections": {
            "unique_seen": len(corrections),
            "outcome_counts": {
                outcome: sum(row["outcome"] == outcome for row in corrections)
                for outcome in sorted({str(row["outcome"]) for row in corrections})
            },
            "correction_duration_secs_total": round(
                sum(
                    float(row["correction_duration_secs"])
                    for row in corrections
                    if row["correction_duration_secs"] is not None
                ),
                3,
            ),
            "events": corrections,
        },
        "classification_thresholds": {
            "allowed": list(CLASSIFICATIONS),
            "recurrence_proven_share": 0.30,
            "recurrence_proven_repeated_windows": 1024,
            "recurrence_proven_confirmations": 2,
            "recurrence_approaching_share": 0.20,
            "recurrence_approaching_repeated_windows": 512,
            "same_role_thinking_watch_ratio": 2.0,
        },
    }


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    descriptor = os.open(temporary, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(value, handle, sort_keys=True, separators=(",", ":"))
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        os.chmod(path, 0o600)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary.exists():
            temporary.unlink()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ticks", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timezone", default="Europe/Bucharest")
    parser.add_argument("--date", type=dt.date.fromisoformat)
    parser.add_argument("--morning-end-hour", type=int, default=12)
    args = parser.parse_args()
    os.umask(0o077)
    now = dt.datetime.now(dt.timezone.utc)
    timezone = ZoneInfo(args.timezone)
    local_day = args.date or now.astimezone(timezone).date()
    ticks, source = read_tick_prefix(args.ticks)
    aggregate = build_morning_aggregate(
        ticks,
        source,
        args.timezone,
        local_day,
        now,
        args.morning_end_hour,
    )
    atomic_json(args.output, aggregate)
    print(
        json.dumps(
            {
                "output": str(args.output.resolve()),
                "ticks": aggregate["ticks"]["count"],
                "classification": aggregate["quality_classification"],
                "last_seq": aggregate["run"]["last_seq"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
