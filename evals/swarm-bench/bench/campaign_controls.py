#!/usr/bin/env python3
"""Bind a causal lever arm to the exact engine control manifest that will execute it."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple


class ControlManifestError(ValueError):
    pass


CONTROL_NAME = re.compile(r"^[a-z][a-z0-9_]*$")


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def sha256_value(value: Any) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_dict(value: Any, label: str) -> Dict[str, Any]:
    if not isinstance(value, dict):
        raise ControlManifestError(f"{label} must be an object")
    return value


def _require_list(value: Any, label: str) -> List[Any]:
    if not isinstance(value, list):
        raise ControlManifestError(f"{label} must be an array")
    return value


@dataclass(frozen=True)
class ControlCatalog:
    export: Mapping[str, Any]
    config: Mapping[str, Mapping[str, Any]]
    environment_only: Mapping[str, Mapping[str, Any]]
    spellings: Mapping[str, str]

    @property
    def registry_sha256(self) -> str:
        return str(self.export["registry_sha256"])

    def canonicalize(self, spelling: str) -> str:
        normalized = spelling.strip()
        if not normalized:
            raise ControlManifestError("control name is empty")
        direct = self.spellings.get(normalized)
        if direct is not None:
            return direct
        lowered = normalized.lower()
        direct = self.spellings.get(lowered)
        if direct is not None:
            return direct
        if normalized.upper().startswith("GOOSE_SWARM_"):
            suffix = normalized[len("GOOSE_SWARM_") :].lower()
            direct = self.spellings.get(suffix)
            if direct is not None:
                return direct
        raise ControlManifestError(f"unknown engine control: {spelling}")

    def causal_config(self, spelling: str) -> Mapping[str, Any]:
        canonical = self.canonicalize(spelling)
        if canonical in self.environment_only:
            raise ControlManifestError(
                f"{canonical} is environment-only; the persisted-config campaign cannot arm it"
            )
        row = self.config[canonical]
        role = row["campaign_role"]
        if role != "behavior":
            raise ControlManifestError(
                f"{canonical} is {role}, not a causal behavior lever"
            )
        return row


def _register_spelling(
    spellings: Dict[str, str], spelling: str, canonical: str, label: str
) -> None:
    previous = spellings.get(spelling)
    if previous is not None and previous != canonical:
        raise ControlManifestError(
            f"{label} {spelling} collides: {previous} versus {canonical}"
        )
    spellings[spelling] = canonical


def _environment_sha256(registry: Mapping[str, Any]) -> str:
    readers = _require_list(
        registry.get("environment_readers"), "control_registry.environment_readers"
    )
    inputs = {}
    for raw in readers:
        environment = str(
            _require_dict(raw, "environment reader").get("environment")
        )
        inputs[environment] = os.environ.get(environment)
    return sha256_value(inputs)


def validate_engine_export(payload: Any) -> ControlCatalog:
    envelope = _require_dict(payload, "engine control export")
    if envelope.get("schema_version") != 1:
        raise ControlManifestError("unsupported engine control export schema")
    engine = _require_dict(envelope.get("engine"), "engine")
    for key in ("version", "build_sha", "crate_version"):
        if not isinstance(engine.get(key), str) or not engine[key]:
            raise ControlManifestError(f"engine.{key} must be a non-empty string")
    registry = _require_dict(envelope.get("control_registry"), "control_registry")
    if registry.get("schema_version") != 2:
        raise ControlManifestError("unsupported control registry schema")
    expected_digest = envelope.get("registry_sha256")
    if not isinstance(expected_digest, str) or not re.fullmatch(
        r"[0-9a-f]{64}", expected_digest
    ):
        raise ControlManifestError("registry_sha256 must be a SHA-256 hex digest")
    if sha256_value(registry) != expected_digest:
        raise ControlManifestError("control registry digest does not match its payload")
    environment_digest = envelope.get("control_environment_sha256")
    if not isinstance(environment_digest, str) or not re.fullmatch(
        r"[0-9a-f]{64}", environment_digest
    ):
        raise ControlManifestError(
            "control_environment_sha256 must be a SHA-256 hex digest"
        )

    allowed_dispositions = {
        "retain_enabled",
        "retain_disabled",
        "modify",
        "remove_merge",
        "runtime_profile",
    }
    allowed_roles = {"behavior", "runtime_profile", "removal", "telemetry"}
    allowed_types = {"boolean", "integer", "number", "string", "array", "object"}
    config: Dict[str, Mapping[str, Any]] = {}
    environment_only: Dict[str, Mapping[str, Any]] = {}
    spellings: Dict[str, str] = {}

    for raw in _require_list(registry.get("config"), "control_registry.config"):
        row = _require_dict(raw, "config control")
        canonical = row.get("canonical")
        if not isinstance(canonical, str) or not CONTROL_NAME.fullmatch(canonical):
            raise ControlManifestError("config control canonical name is missing")
        if canonical in config or canonical in environment_only:
            raise ControlManifestError(f"duplicate canonical control: {canonical}")
        if row.get("source") != "config":
            raise ControlManifestError(f"{canonical} has the wrong source")
        if row.get("disposition") not in allowed_dispositions:
            raise ControlManifestError(f"{canonical} has an unknown disposition")
        if row.get("campaign_role") not in allowed_roles:
            raise ControlManifestError(f"{canonical} has an unknown campaign role")
        if row.get("value_type") not in allowed_types:
            raise ControlManifestError(f"{canonical} has an unknown value type")
        if "default" not in row or row.get("effective_echo") is not True:
            raise ControlManifestError(f"{canonical} lacks a default or effective echo")
        config[canonical] = row
        _register_spelling(spellings, canonical, canonical, "canonical control")

    for raw in _require_list(
        registry.get("environment_only"), "control_registry.environment_only"
    ):
        row = _require_dict(raw, "environment-only control")
        canonical = row.get("canonical")
        environment = row.get("environment")
        if not isinstance(canonical, str) or not CONTROL_NAME.fullmatch(canonical):
            raise ControlManifestError("environment-only canonical name is missing")
        if canonical in config or canonical in environment_only:
            raise ControlManifestError(f"duplicate canonical control: {canonical}")
        if not isinstance(environment, str) or not environment.startswith("GOOSE_SWARM_"):
            raise ControlManifestError(f"{canonical} has an invalid environment reader")
        if row.get("source") != "environment":
            raise ControlManifestError(f"{canonical} has the wrong source")
        if row.get("disposition") not in allowed_dispositions:
            raise ControlManifestError(f"{canonical} has an unknown disposition")
        if row.get("campaign_role") not in allowed_roles:
            raise ControlManifestError(f"{canonical} has an unknown campaign role")
        if not isinstance(row.get("effective_echo"), bool):
            raise ControlManifestError(f"{canonical} lacks effective-echo metadata")
        environment_only[canonical] = row
        _register_spelling(spellings, canonical, canonical, "canonical control")
        _register_spelling(spellings, environment, canonical, "environment spelling")
        _register_spelling(
            spellings,
            environment[len("GOOSE_SWARM_") :].lower(),
            canonical,
            "environment suffix",
        )

    all_controls = set(config) | set(environment_only)
    seen_aliases = set()
    for raw in _require_list(registry.get("aliases"), "control_registry.aliases"):
        row = _require_dict(raw, "control alias")
        alias = row.get("alias")
        canonical = row.get("canonical")
        if (
            not isinstance(alias, str)
            or not CONTROL_NAME.fullmatch(alias)
            or alias in seen_aliases
        ):
            raise ControlManifestError(f"duplicate or invalid alias: {alias}")
        if canonical not in all_controls:
            raise ControlManifestError(f"alias {alias} points at missing {canonical}")
        seen_aliases.add(alias)
        _register_spelling(spellings, alias, canonical, "alias")

    seen_readers = set()
    for raw in _require_list(
        registry.get("environment_readers"), "control_registry.environment_readers"
    ):
        row = _require_dict(raw, "environment reader")
        environment = row.get("environment")
        canonical = row.get("canonical")
        if (
            not isinstance(environment, str)
            or not environment.startswith("GOOSE_SWARM_")
            or environment in seen_readers
        ):
            raise ControlManifestError(f"duplicate or invalid reader: {environment}")
        if canonical not in all_controls:
            raise ControlManifestError(
                f"reader {environment} points at missing {canonical}"
            )
        seen_readers.add(environment)
        _register_spelling(spellings, environment, canonical, "environment reader")
        _register_spelling(
            spellings,
            environment[len("GOOSE_SWARM_") :].lower(),
            canonical,
            "environment reader suffix",
        )

    return ControlCatalog(envelope, config, environment_only, spellings)


def export_from_engine(engine_path: Path) -> ControlCatalog:
    engine = engine_path.expanduser().resolve()
    if not engine.is_file():
        raise ControlManifestError(f"engine binary does not exist: {engine}")
    result = subprocess.run(
        [str(engine), "swarm", "controls"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise ControlManifestError(
            f"engine control export failed with exit {result.returncode}: {detail}"
        )
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ControlManifestError("engine control export was not one JSON document") from exc
    catalog = validate_engine_export(payload)
    if catalog.export["control_environment_sha256"] != _environment_sha256(
        catalog.export["control_registry"]
    ):
        raise ControlManifestError(
            "engine control export does not match the current process environment"
        )
    return catalog


def _atomic_json(path: Path, payload: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as handle:
        staged = Path(handle.name)
        handle.write(encoded)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(staged, path)


def _atomic_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as handle:
        staged = Path(handle.name)
        handle.write(text)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(staged, path)


def _read_json(path: Path, label: str) -> Dict[str, Any]:
    try:
        return _require_dict(json.loads(path.read_text()), label)
    except (OSError, json.JSONDecodeError) as exc:
        raise ControlManifestError(f"cannot read {label}: {path}") from exc


def create_or_resume_lock(
    engine_path: Path,
    lock_path: Path,
    expected_build_sha: Optional[str] = None,
) -> Dict[str, Any]:
    engine = engine_path.expanduser().resolve()
    if lock_path.exists():
        catalog, receipt = verify_lock(engine, lock_path)
        if (
            expected_build_sha is not None
            and catalog.export["engine"]["build_sha"] != expected_build_sha
        ):
            raise ControlManifestError("sealed engine does not match the expected build SHA")
        return receipt
    binary_before_export = sha256_file(engine)
    catalog = export_from_engine(engine)
    binary_after_export = sha256_file(engine)
    if binary_before_export != binary_after_export:
        raise ControlManifestError("engine binary changed while its controls were exported")
    if (
        expected_build_sha is not None
        and catalog.export["engine"]["build_sha"] != expected_build_sha
    ):
        raise ControlManifestError("engine does not match the expected build SHA")
    receipt = {
        "schema_version": 1,
        "engine_path": str(engine),
        "engine_binary_sha256": binary_after_export,
        "engine_control_export": catalog.export,
    }
    _atomic_json(lock_path, receipt)
    return receipt


def verify_lock(
    engine_path: Path, lock_path: Path
) -> Tuple[ControlCatalog, Dict[str, Any]]:
    engine = engine_path.expanduser().resolve()
    receipt = _read_json(lock_path, "campaign control lock")
    if receipt.get("schema_version") != 1:
        raise ControlManifestError("unsupported campaign control lock schema")
    if receipt.get("engine_path") != str(engine):
        raise ControlManifestError("campaign engine path differs from the sealed path")
    expected_binary = receipt.get("engine_binary_sha256")
    binary_before_export = sha256_file(engine)
    if expected_binary != binary_before_export:
        raise ControlManifestError("stale or replaced engine binary; binary digest changed")
    locked_catalog = validate_engine_export(receipt.get("engine_control_export"))
    current_catalog = export_from_engine(engine)
    if binary_before_export != sha256_file(engine):
        raise ControlManifestError("engine binary changed while its controls were exported")
    if current_catalog.registry_sha256 != locked_catalog.registry_sha256:
        raise ControlManifestError("engine control registry differs from the campaign lock")
    if current_catalog.export["engine"] != locked_catalog.export["engine"]:
        raise ControlManifestError("engine build identity differs from the campaign lock")
    if (
        current_catalog.export["control_environment_sha256"]
        != locked_catalog.export["control_environment_sha256"]
    ):
        raise ControlManifestError("swarm control environment differs from the campaign lock")
    return current_catalog, receipt


def _parse_value(raw: str, value_type_name: str) -> Any:
    text = raw.strip()
    if text.lower() == "null":
        raise ControlManifestError("null is not an attributable control value")
    if value_type_name == "boolean":
        lowered = text.lower()
        if lowered in {"1", "true", "on", "yes"}:
            return True
        if lowered in {"0", "false", "off", "no"}:
            return False
        raise ControlManifestError(f"invalid boolean value: {raw}")
    if value_type_name == "integer":
        try:
            return int(text, 10)
        except ValueError as exc:
            raise ControlManifestError(f"invalid integer value: {raw}") from exc
    if value_type_name == "number":
        try:
            value = float(text)
        except ValueError as exc:
            raise ControlManifestError(f"invalid numeric value: {raw}") from exc
        if not math.isfinite(value):
            raise ControlManifestError(f"numeric value must be finite: {raw}")
        return value
    if value_type_name == "string":
        return text
    if value_type_name in {"array", "object"}:
        try:
            value = json.loads(text)
        except json.JSONDecodeError as exc:
            raise ControlManifestError(
                f"invalid JSON {value_type_name} value: {raw}"
            ) from exc
        expected = list if value_type_name == "array" else dict
        if not isinstance(value, expected):
            raise ControlManifestError(f"value is not a JSON {value_type_name}: {raw}")
        return value
    raise ControlManifestError(f"unsupported control value type: {value_type_name}")


def parse_behavior_profile(
    catalog: ControlCatalog, tokens: Iterable[str]
) -> Dict[str, Any]:
    profile: Dict[str, Any] = {}
    for token in tokens:
        if not token or token.startswith("#"):
            continue
        if "=" not in token:
            raise ControlManifestError(f"control assignment lacks '=': {token}")
        spelling, raw = token.split("=", 1)
        row = catalog.causal_config(spelling)
        canonical = str(row["canonical"])
        if canonical in profile:
            raise ControlManifestError(
                f"control assigned twice after alias resolution: {canonical}"
            )
        profile[canonical] = _parse_value(raw, str(row["value_type"]))
    return profile


def read_profile(path: Path) -> List[str]:
    tokens: List[str] = []
    for raw_line in path.read_text().splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if line:
            tokens.extend(line.split())
    return tokens


def effective_behavior_profile(
    catalog: ControlCatalog, explicit: Mapping[str, Any]
) -> Dict[str, Any]:
    profile = {
        canonical: row["default"]
        for canonical, row in catalog.config.items()
        if row["campaign_role"] == "behavior"
    }
    profile.update(explicit)
    return profile


def _yaml_scalar(value: Any) -> str:
    if value is True:
        return "true"
    if value is False:
        return "false"
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def render_staged_config(
    catalog: ControlCatalog,
    runtime_baseline: Path,
    explicit_profile: Mapping[str, Any],
    staged_config: Path,
) -> str:
    lines = runtime_baseline.read_text().splitlines(keepends=True)
    swarm_match = next(
        (
            (index, match)
            for index, line in enumerate(lines)
            if (
                match := re.match(
                    r"^(?P<indent> *)swarm:\s*(?P<empty>\{\})?\s*(?:#.*)?(?:\r?\n)?$",
                    line,
                )
            )
        ),
        None,
    )
    if swarm_match is None:
        raise ControlManifestError("runtime baseline has no editable swarm mapping")
    start, match = swarm_match
    base_indent = len(match.group("indent"))
    if match.group("empty"):
        newline = "\r\n" if lines[start].endswith("\r\n") else "\n"
        lines[start] = f"{' ' * base_indent}swarm:{newline}"
    end = len(lines)
    for index in range(start + 1, len(lines)):
        stripped = lines[index].strip()
        if not stripped or lines[index].lstrip().startswith("#"):
            continue
        indentation = len(lines[index]) - len(lines[index].lstrip(" "))
        if indentation <= base_indent:
            end = index
            break
    child_indents = [
        len(line) - len(line.lstrip(" "))
        for line in lines[start + 1 : end]
        if line.strip() and not line.lstrip().startswith("#")
    ]
    child_indent = min(child_indents, default=base_indent + 2)
    if child_indent <= base_indent:
        raise ControlManifestError("runtime baseline has an invalid swarm mapping indent")
    behavior_names = {
        canonical
        for canonical, row in catalog.config.items()
        if row["campaign_role"] == "behavior"
    }
    kept = []
    skip_behavior_value = False
    for index, line in enumerate(lines):
        if start < index < end:
            indentation = len(line) - len(line.lstrip(" "))
            if skip_behavior_value:
                if not line.strip() or line.lstrip().startswith("#"):
                    continue
                if indentation > child_indent:
                    continue
                skip_behavior_value = False
            key_match = re.match(
                rf"^ {{{child_indent}}}([a-z_][a-z0-9_]*):", line
            )
            if key_match and key_match.group(1) in behavior_names:
                skip_behavior_value = True
                continue
        kept.append(line)
    insertion = start + 1
    rendered = [
        f"{' ' * child_indent}{canonical}: {_yaml_scalar(value)}\n"
        for canonical, value in sorted(explicit_profile.items())
    ]
    kept[insertion:insertion] = rendered
    text = "".join(kept)
    _atomic_text(staged_config, text)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def prepare_arm_receipt(
    catalog: ControlCatalog,
    lock_receipt: Mapping[str, Any],
    arm_name: str,
    reference_tokens: Sequence[str],
    candidate_tokens: Sequence[str],
    runtime_baseline: Path,
    staged_config: Optional[Path] = None,
    replicate: bool = False,
) -> Dict[str, Any]:
    reference = parse_behavior_profile(catalog, reference_tokens)
    candidate = parse_behavior_profile(catalog, candidate_tokens)
    reference_effective = effective_behavior_profile(catalog, reference)
    candidate_effective = effective_behavior_profile(catalog, candidate)
    delta = sorted(
        canonical
        for canonical in reference_effective
        if reference_effective[canonical] != candidate_effective[canonical]
    )
    if replicate:
        if delta:
            raise ControlManifestError("a replicate must match its reference behavior profile")
    elif len(delta) != 1:
        raise ControlManifestError(
            f"a causal arm must change exactly one behavior control; changed {delta}"
        )
    elif delta[0] not in candidate:
        raise ControlManifestError(
            f"the changed control {delta[0]} must have an explicit candidate value"
        )
    if not runtime_baseline.is_file():
        raise ControlManifestError(f"runtime baseline does not exist: {runtime_baseline}")
    receipt = {
        "schema_version": 1,
        "arm": arm_name,
        "engine_binary_sha256": lock_receipt["engine_binary_sha256"],
        "engine": catalog.export["engine"],
        "registry_sha256": catalog.registry_sha256,
        "control_environment_sha256": catalog.export[
            "control_environment_sha256"
        ],
        "runtime_baseline": str(runtime_baseline.expanduser().resolve()),
        "runtime_baseline_sha256": sha256_file(runtime_baseline),
        "reference_profile_sha256": sha256_value(reference_effective),
        "candidate_profile_sha256": sha256_value(candidate_effective),
        "reference_explicit": reference,
        "candidate_explicit": candidate,
        "reference_effective": reference_effective,
        "candidate_effective": candidate_effective,
        "delta": delta,
        "replicate": replicate,
    }
    if staged_config is not None:
        receipt["staged_config"] = str(staged_config.expanduser().resolve())
        receipt["staged_config_sha256"] = render_staged_config(
            catalog, runtime_baseline, candidate, staged_config
        )
    return receipt


def create_or_resume_arm_receipt(path: Path, receipt: Mapping[str, Any]) -> None:
    if path.exists():
        existing = _read_json(path, "arm receipt")
        if existing != receipt:
            raise ControlManifestError(
                f"arm receipt already exists with different evidence: {path}"
            )
        return
    _atomic_json(path, receipt)


def validate_arm_receipt(
    catalog: ControlCatalog,
    lock_receipt: Mapping[str, Any],
    receipt: Mapping[str, Any],
) -> None:
    if receipt.get("schema_version") != 1:
        raise ControlManifestError("unsupported arm receipt schema")
    if receipt.get("engine_binary_sha256") != lock_receipt.get(
        "engine_binary_sha256"
    ):
        raise ControlManifestError("arm receipt used a different engine binary")
    if receipt.get("engine") != catalog.export["engine"]:
        raise ControlManifestError("arm receipt used a different engine build")
    if receipt.get("registry_sha256") != catalog.registry_sha256:
        raise ControlManifestError("arm receipt belongs to a different control registry")
    if (
        receipt.get("control_environment_sha256")
        != catalog.export["control_environment_sha256"]
    ):
        raise ControlManifestError("arm receipt used a different control environment")

    reference_explicit = _require_dict(
        receipt.get("reference_explicit"), "arm reference_explicit"
    )
    candidate_explicit = _require_dict(
        receipt.get("candidate_explicit"), "arm candidate_explicit"
    )
    for label, explicit in (
        ("reference", reference_explicit),
        ("candidate", candidate_explicit),
    ):
        for canonical in explicit:
            row = catalog.causal_config(canonical)
            if row["canonical"] != canonical:
                raise ControlManifestError(
                    f"arm {label} profile contains a non-canonical spelling: {canonical}"
                )
    reference_effective = effective_behavior_profile(catalog, reference_explicit)
    candidate_effective = effective_behavior_profile(catalog, candidate_explicit)
    if receipt.get("reference_effective") != reference_effective:
        raise ControlManifestError("arm reference behavior profile is invalid")
    if receipt.get("candidate_effective") != candidate_effective:
        raise ControlManifestError("arm candidate behavior profile is invalid")
    if receipt.get("reference_profile_sha256") != sha256_value(reference_effective):
        raise ControlManifestError("arm reference behavior profile digest is invalid")
    if receipt.get("candidate_profile_sha256") != sha256_value(candidate_effective):
        raise ControlManifestError("arm candidate behavior profile digest is invalid")
    delta = sorted(
        canonical
        for canonical in reference_effective
        if reference_effective[canonical] != candidate_effective[canonical]
    )
    if receipt.get("delta") != delta:
        raise ControlManifestError("arm declared delta does not match its profiles")
    if receipt.get("replicate"):
        if delta:
            raise ControlManifestError("arm marked as a replicate has a behavior delta")
    elif len(delta) != 1 or delta[0] not in candidate_explicit:
        raise ControlManifestError("arm is not an explicit one-control causal delta")

    for path_key, digest_key, label in (
        ("runtime_baseline", "runtime_baseline_sha256", "runtime baseline"),
        ("staged_config", "staged_config_sha256", "staged config"),
    ):
        path_value = receipt.get(path_key)
        digest = receipt.get(digest_key)
        if path_key == "staged_config" and path_value is None and digest is None:
            continue
        if not isinstance(path_value, str) or not isinstance(digest, str):
            raise ControlManifestError(f"arm {label} evidence is missing")
        path = Path(path_value)
        if not path.is_file() or sha256_file(path) != digest:
            raise ControlManifestError(f"arm {label} changed after preparation")


def _event_registry_digest(event: Mapping[str, Any]) -> str:
    registry = _require_dict(event.get("control_registry"), "event control_registry")
    return sha256_value(registry)


def validate_run_event(
    catalog: ControlCatalog,
    event: Mapping[str, Any],
    arm_receipt: Optional[Mapping[str, Any]] = None,
) -> Dict[str, Any]:
    if event.get("event") != "levers_resolved":
        raise ControlManifestError("run evidence is not a levers_resolved event")
    engine = catalog.export["engine"]
    for event_key, engine_key in (
        ("version", "version"),
        ("build_sha", "build_sha"),
        ("crate_version", "crate_version"),
    ):
        if event.get(event_key) != engine[engine_key]:
            raise ControlManifestError(f"run event {event_key} differs from the campaign lock")
    if event.get("control_registry_sha256") != catalog.registry_sha256:
        raise ControlManifestError("run event registry digest differs from the campaign lock")
    if (
        event.get("control_environment_sha256")
        != catalog.export["control_environment_sha256"]
    ):
        raise ControlManifestError(
            "run event control environment differs from the campaign lock"
        )
    if _event_registry_digest(event) != catalog.registry_sha256:
        raise ControlManifestError("run event control registry differs from the campaign lock")
    levers = _require_dict(event.get("levers"), "levers_resolved.levers")
    expected = {
        canonical
        for canonical, row in catalog.config.items()
        if row["effective_echo"]
    }
    expected.update(
        canonical
        for canonical, row in catalog.environment_only.items()
        if row["effective_echo"]
    )
    missing = sorted(expected - set(levers))
    if missing:
        raise ControlManifestError(f"run event is missing effective controls: {missing}")
    if arm_receipt is not None:
        if arm_receipt.get("registry_sha256") != catalog.registry_sha256:
            raise ControlManifestError("arm receipt belongs to a different control registry")
        explicit = _require_dict(
            arm_receipt.get("candidate_explicit"), "arm candidate_explicit"
        )
        mismatched = {
            canonical: {"requested": value, "executed": levers.get(canonical)}
            for canonical, value in explicit.items()
            if levers.get(canonical) != value
        }
        if mismatched:
            raise ControlManifestError(
                f"run did not execute the requested behavior profile: {mismatched}"
            )
    return {canonical: levers[canonical] for canonical in sorted(expected)}


def prepare_verification_receipt(
    catalog: ControlCatalog,
    lock_receipt: Mapping[str, Any],
    arm_receipt: Mapping[str, Any],
    event: Mapping[str, Any],
    event_log: Path,
    reference_verification: Optional[Mapping[str, Any]] = None,
) -> Dict[str, Any]:
    validate_arm_receipt(catalog, lock_receipt, arm_receipt)
    executed = validate_run_event(catalog, event, arm_receipt)
    receipt = {
        "schema_version": 1,
        "arm": arm_receipt.get("arm"),
        "arm_receipt_sha256": sha256_value(arm_receipt),
        "engine": catalog.export["engine"],
        "registry_sha256": catalog.registry_sha256,
        "control_environment_sha256": catalog.export[
            "control_environment_sha256"
        ],
        "runtime_baseline_sha256": arm_receipt.get("runtime_baseline_sha256"),
        "event_log": str(event_log.expanduser().resolve()),
        "event_log_sha256": sha256_file(event_log),
        "executed_controls": executed,
        "executed_controls_sha256": sha256_value(executed),
        "delta": arm_receipt.get("delta"),
        "replicate": arm_receipt.get("replicate"),
    }
    if reference_verification is None:
        if not arm_receipt.get("replicate"):
            raise ControlManifestError(
                "a causal arm requires a verified executed reference profile"
            )
        receipt["executed_delta_from_reference"] = []
        return receipt

    if reference_verification.get("schema_version") != 1:
        raise ControlManifestError("unsupported reference verification schema")
    if reference_verification.get("engine") != catalog.export["engine"]:
        raise ControlManifestError("reference verification used a different engine build")
    if reference_verification.get("registry_sha256") != catalog.registry_sha256:
        raise ControlManifestError("reference verification used a different registry")
    if (
        reference_verification.get("control_environment_sha256")
        != catalog.export["control_environment_sha256"]
    ):
        raise ControlManifestError("reference verification used a different environment")
    if (
        reference_verification.get("runtime_baseline_sha256")
        != arm_receipt.get("runtime_baseline_sha256")
    ):
        raise ControlManifestError("reference verification used a different runtime baseline")
    reference_executed = _require_dict(
        reference_verification.get("executed_controls"),
        "reference executed_controls",
    )
    if reference_verification.get("executed_controls_sha256") != sha256_value(
        reference_executed
    ):
        raise ControlManifestError("reference executed-control digest is invalid")
    if set(reference_executed) != set(executed):
        raise ControlManifestError("reference and candidate expose different controls")
    executed_delta = sorted(
        canonical
        for canonical in executed
        if executed[canonical] != reference_executed[canonical]
    )
    expected_delta = [] if arm_receipt.get("replicate") else arm_receipt.get("delta")
    if executed_delta != expected_delta:
        raise ControlManifestError(
            "executed arm does not have its declared one-control delta: "
            f"expected {expected_delta}, observed {executed_delta}"
        )
    receipt["reference_verification_sha256"] = sha256_value(reference_verification)
    receipt["executed_delta_from_reference"] = executed_delta
    return receipt


def read_levers_event(path: Path) -> Mapping[str, Any]:
    events = []
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict) and event.get("event") == "levers_resolved":
                events.append(event)
    if len(events) != 1:
        raise ControlManifestError(
            f"expected exactly one levers_resolved event, found {len(events)}"
        )
    return events[0]


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    lock = subparsers.add_parser("lock")
    lock.add_argument("--engine", type=Path, required=True)
    lock.add_argument("--lock", type=Path, required=True)
    lock.add_argument("--expected-build-sha")

    prepare = subparsers.add_parser("prepare-arm")
    prepare.add_argument("--engine", type=Path, required=True)
    prepare.add_argument("--lock", type=Path, required=True)
    prepare.add_argument("--arm", required=True)
    prepare.add_argument("--reference", type=Path, required=True)
    prepare.add_argument("--candidate", type=Path, required=True)
    prepare.add_argument("--runtime-baseline", type=Path, required=True)
    prepare.add_argument("--staged-config", type=Path, required=True)
    prepare.add_argument("--receipt", type=Path, required=True)
    prepare.add_argument("--replicate", action="store_true")

    verify = subparsers.add_parser("verify-event")
    verify.add_argument("--engine", type=Path, required=True)
    verify.add_argument("--lock", type=Path, required=True)
    verify.add_argument("--receipt", type=Path, required=True)
    verify.add_argument("--event-log", type=Path, required=True)
    verify.add_argument("--verification", type=Path, required=True)
    verify.add_argument("--reference-verification", type=Path)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _build_parser().parse_args(argv)
    try:
        if args.command == "lock":
            receipt = create_or_resume_lock(
                args.engine, args.lock, args.expected_build_sha
            )
            print(json.dumps(receipt, sort_keys=True))
            return 0
        catalog, lock_receipt = verify_lock(args.engine, args.lock)
        if args.command == "prepare-arm":
            receipt = prepare_arm_receipt(
                catalog,
                lock_receipt,
                args.arm,
                read_profile(args.reference),
                read_profile(args.candidate),
                args.runtime_baseline,
                args.staged_config,
                args.replicate,
            )
            create_or_resume_arm_receipt(args.receipt, receipt)
            print(json.dumps(receipt, sort_keys=True))
            return 0
        arm_receipt = _read_json(args.receipt, "arm receipt")
        reference = (
            _read_json(args.reference_verification, "reference verification")
            if args.reference_verification
            else None
        )
        verification = prepare_verification_receipt(
            catalog,
            lock_receipt,
            arm_receipt,
            read_levers_event(args.event_log),
            args.event_log,
            reference,
        )
        create_or_resume_arm_receipt(args.verification, verification)
        print(json.dumps(verification, sort_keys=True))
        return 0
    except ControlManifestError as exc:
        print(f"error: {exc}", file=os.sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
