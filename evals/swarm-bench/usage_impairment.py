from __future__ import annotations

import hashlib
import json
import math
import pathlib
import re
import statistics
from typing import Any, Iterable, Mapping, Sequence


class UsageEvidenceError(ValueError):
    pass


NUMERIC_FIELDS = (
    "calls",
    "prompt_tokens",
    "completion_tokens",
    "prefill_tok_s",
    "decode_tok_s",
)
COUNT_FIELDS = ("calls", "prompt_tokens", "completion_tokens")
PUBLIC_RECEIPT_FIELDS = (
    "usage_complete",
    "unmetered_unproven_requests",
    "unmetered_unproven_request_identity_sha256s",
)
DISPATCHER_UNPROVEN_REASON_RE = re.compile(
    r"^(?:provider dispatcher returned success|provider dispatcher failed \([^\r\n]{1,4096}\)) "
    r"without terminal proof: (?P<reason>outstanding provider request "
    r"`engine-provider-request:[0-9a-f]{32}` has no proven cancelled terminal)$"
)


def _canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _finite_number(value: Any, minimum: float | None = None) -> bool:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    number = float(value)
    return math.isfinite(number) and (minimum is None or number >= minimum)


def _required_string(row: Mapping[str, Any], name: str, identity: str) -> str:
    value = row.get(name)
    if not isinstance(value, str) or not value:
        raise UsageEvidenceError(f"{identity} lacks {name}")
    return value


def _node_models(
    expected_nodes: Sequence[str], expected_models: Sequence[str]
) -> dict[str, str]:
    nodes = list(expected_nodes)
    if len(nodes) != len(set(nodes)) or not nodes:
        raise UsageEvidenceError("expected telemetry nodes are empty or duplicated")
    models = list(expected_models)
    if len(models) != len(nodes) or len(models) != len(set(models)):
        raise UsageEvidenceError("expected model identities do not match the node roster")
    by_node: dict[str, str] = {}
    for model in models:
        if not isinstance(model, str) or "-" not in model:
            raise UsageEvidenceError("expected model identity is malformed")
        node = model.split("-", 1)[0]
        if node not in nodes or node in by_node:
            raise UsageEvidenceError("expected model does not map uniquely to a frozen node")
        by_node[node] = model
    if set(by_node) != set(nodes):
        raise UsageEvidenceError("expected model roster omits a frozen node")
    return by_node


def _lifecycle_identity(row: Mapping[str, Any], identity: str) -> dict[str, Any]:
    return {
        "run_id": _required_string(row, "run_id", identity),
        "admission_id": _required_string(row, "admission_id", identity),
        "provider_request_id": _required_string(row, "provider_request_id", identity),
        "physical_host_id": _required_string(row, "physical_host_id", identity),
        "model_instance_id": _required_string(row, "model_instance_id", identity),
        "ordinal": row.get("ordinal"),
    }


def _identity_key(identity: Mapping[str, Any]) -> tuple[Any, ...]:
    return tuple(
        identity[field]
        for field in (
            "run_id",
            "admission_id",
            "provider_request_id",
            "physical_host_id",
            "model_instance_id",
            "ordinal",
        )
    )


def _validate_lifecycle(
    rows: Sequence[Mapping[str, Any]], run_id: str
) -> dict[tuple[Any, ...], dict[str, Any]]:
    started: dict[tuple[Any, ...], dict[str, Any]] = {}
    terminal: set[tuple[Any, ...]] = set()
    previous_hash = "genesis"
    for index, row in enumerate(rows):
        identity = f"provider lifecycle row {index}"
        if not isinstance(row, Mapping):
            raise UsageEvidenceError(f"{identity} is not an object")
        if row.get("seq") != index:
            raise UsageEvidenceError("provider lifecycle sequence is not contiguous")
        entry_hash = row.get("entry_hash")
        if (
            row.get("prev_hash") != previous_hash
            or not isinstance(entry_hash, str)
            or len(entry_hash) != 71
            or not entry_hash.startswith("sha256:")
        ):
            raise UsageEvidenceError("provider lifecycle hash chain is malformed")
        try:
            int(entry_hash.removeprefix("sha256:"), 16)
        except ValueError as error:
            raise UsageEvidenceError("provider lifecycle entry hash is malformed") from error
        previous_hash = entry_hash
        item = _lifecycle_identity(row, identity)
        if item["run_id"] != run_id:
            raise UsageEvidenceError("provider lifecycle row belongs to another run")
        if not isinstance(item["ordinal"], int) or isinstance(item["ordinal"], bool):
            raise UsageEvidenceError(f"{identity} has a malformed ordinal")
        key = _identity_key(item)
        transition = row.get("transition")
        if transition == "started":
            if key in started:
                raise UsageEvidenceError("provider lifecycle repeats a started identity")
            started[key] = item
        elif transition == "terminal":
            if key not in started or key in terminal:
                raise UsageEvidenceError("provider lifecycle terminal lacks one exact start")
            if row.get("terminal_kind") not in {"finished", "failed", "cancelled"}:
                raise UsageEvidenceError("provider lifecycle terminal kind is malformed")
            terminal.add(key)
        else:
            raise UsageEvidenceError("provider lifecycle transition is malformed")
    return {key: item for key, item in started.items() if key not in terminal}


def _quarantine_identity(event: Mapping[str, Any], run_id: str) -> dict[str, Any]:
    receipt = event.get("receipt")
    if not isinstance(receipt, Mapping):
        raise UsageEvidenceError("quarantine event lacks a receipt")
    admission = receipt.get("admission")
    unresolved = receipt.get("unresolved")
    if not isinstance(admission, Mapping) or not isinstance(unresolved, Mapping):
        raise UsageEvidenceError("quarantine event lacks admission evidence")
    unresolved_admission = unresolved.get("admission")
    if not isinstance(unresolved_admission, Mapping):
        raise UsageEvidenceError("quarantine event lacks unresolved admission evidence")
    identity_fields = (
        "admission_id",
        "physical_host_id",
        "model_instance_id",
    )
    for field in identity_fields:
        if admission.get(field) != unresolved_admission.get(field):
            raise UsageEvidenceError("quarantine admission identities disagree")
    admission_id = _required_string(admission, "admission_id", "quarantine admission")
    physical_host_id = _required_string(
        admission, "physical_host_id", "quarantine admission"
    )
    model_instance_id = _required_string(
        admission, "model_instance_id", "quarantine admission"
    )
    reason = receipt.get("reason")
    if not isinstance(reason, str):
        raise UsageEvidenceError("quarantine event lacks an unproven-terminal reason")
    prefix = "outstanding provider request `"
    suffix = "` has no proven cancelled terminal"
    if not reason.startswith(prefix) or not reason.endswith(suffix):
        wrapped = DISPATCHER_UNPROVEN_REASON_RE.fullmatch(reason)
        if wrapped is None:
            raise UsageEvidenceError(
                "quarantine reason is not exact unproven-terminal evidence"
            )
        reason = wrapped.group("reason")
    if not reason.startswith(prefix) or not reason.endswith(suffix):
        raise UsageEvidenceError("quarantine reason is not exact unproven-terminal evidence")
    provider_request_id = reason[len(prefix) : -len(suffix)]
    if (
        not provider_request_id.startswith("engine-provider-request:")
        or len(provider_request_id.removeprefix("engine-provider-request:")) != 32
    ):
        raise UsageEvidenceError("quarantine provider request identity is malformed")
    try:
        int(provider_request_id.removeprefix("engine-provider-request:"), 16)
    except ValueError as error:
        raise UsageEvidenceError("quarantine provider request identity is malformed") from error
    provider_requests_started = unresolved.get("provider_requests_started")
    provider_requests_terminal = unresolved.get("provider_requests_terminal")
    if (
        isinstance(provider_requests_started, bool)
        or not isinstance(provider_requests_started, int)
        or isinstance(provider_requests_terminal, bool)
        or not isinstance(provider_requests_terminal, int)
        or provider_requests_terminal < 0
        or provider_requests_started != provider_requests_terminal + 1
    ):
        raise UsageEvidenceError(
            "quarantine unresolved counts do not prove exactly one outstanding request"
        )
    expected_unresolved = {
        "provider_request_pending": False,
        "provider_turn_permit_held": True,
        "provider_starts_closed": True,
        "local_completion": "error",
    }
    for field, expected in expected_unresolved.items():
        if unresolved.get(field) != expected:
            raise UsageEvidenceError(
                f"quarantine unresolved evidence changed at {field}"
            )
    if event.get("run_id") != run_id:
        raise UsageEvidenceError("quarantine event belongs to another run")
    return {
        "run_id": run_id,
        "admission_id": admission_id,
        "provider_request_id": provider_request_id,
        "physical_host_id": physical_host_id,
        "model_instance_id": model_instance_id,
    }


def summarize_known_usage(
    rows: Sequence[Mapping[str, Any]],
    *,
    expected_nodes: Sequence[str],
    expected_models: Sequence[str],
) -> dict[str, Any]:
    model_by_node = _node_models(expected_nodes, expected_models)
    per: dict[str, dict[str, Any]] = {}
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            raise UsageEvidenceError(f"telemetry row {index} is not an object")
        if row.get("usage") is not True:
            continue
        model = row.get("model")
        node = row.get("node")
        if not isinstance(model, str) or not isinstance(node, str):
            raise UsageEvidenceError("completed telemetry row lacks node/model identity")
        derived_node = model.split("-", 1)[0]
        if (
            node != derived_node
            or node not in model_by_node
            or model != model_by_node[node]
        ):
            raise UsageEvidenceError("completed telemetry row differs from the frozen fleet")
        prompt_tokens = row.get("prompt_tokens")
        completion_tokens = row.get("completion_tokens")
        if (
            isinstance(prompt_tokens, bool)
            or not isinstance(prompt_tokens, int)
            or prompt_tokens < 0
            or isinstance(completion_tokens, bool)
            or not isinstance(completion_tokens, int)
            or completion_tokens < 0
        ):
            raise UsageEvidenceError("completed telemetry row has malformed token usage")
        ttft_ms = row.get("ttft_ms")
        total_ms = row.get("total_ms")
        if not _finite_number(ttft_ms, 0) or not _finite_number(total_ms, 0):
            raise UsageEvidenceError("completed telemetry row has malformed timing")
        node_usage = per.setdefault(
            node,
            {
                "calls": 0,
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "prefill": [],
                "decode": [],
            },
        )
        node_usage["calls"] += 1
        node_usage["prompt_tokens"] += prompt_tokens
        node_usage["completion_tokens"] += completion_tokens
        if prompt_tokens > 0 and ttft_ms > 0:
            node_usage["prefill"].append(prompt_tokens / (ttft_ms / 1000.0))
        if completion_tokens > 0 and total_ms > ttft_ms:
            node_usage["decode"].append(
                completion_tokens / ((total_ms - ttft_ms) / 1000.0)
            )
    if not per:
        raise UsageEvidenceError("sealed telemetry contains no completed usage rows")
    summary: dict[str, Any] = {
        "nodes": {},
        "calls": 0,
        "prompt_tokens": 0,
        "completion_tokens": 0,
    }
    all_prefill: list[float] = []
    all_decode: list[float] = []
    for node, values in sorted(per.items()):
        summary["nodes"][node] = {
            "calls": values["calls"],
            "prompt_tokens": values["prompt_tokens"],
            "completion_tokens": values["completion_tokens"],
            "prefill_tok_s": round(statistics.median(values["prefill"]), 1)
            if values["prefill"]
            else None,
            "decode_tok_s": round(statistics.median(values["decode"]), 1)
            if values["decode"]
            else None,
        }
        for field in COUNT_FIELDS:
            summary[field] += values[field]
        all_prefill.extend(values["prefill"])
        all_decode.extend(values["decode"])
    summary["prefill_tok_s"] = (
        round(statistics.median(all_prefill), 1) if all_prefill else None
    )
    summary["decode_tok_s"] = (
        round(statistics.median(all_decode), 1) if all_decode else None
    )
    return summary


def derive_usage_contract(
    *,
    run_id: str,
    run_events: Sequence[Mapping[str, Any]],
    lifecycle_rows: Sequence[Mapping[str, Any]],
    telemetry_rows: Sequence[Mapping[str, Any]],
    expected_nodes: Sequence[str],
    expected_models: Sequence[str],
) -> dict[str, Any]:
    if not isinstance(run_id, str) or not run_id:
        raise UsageEvidenceError("run identity is malformed")
    model_by_node = _node_models(expected_nodes, expected_models)
    known_usage = summarize_known_usage(
        telemetry_rows,
        expected_nodes=expected_nodes,
        expected_models=expected_models,
    )
    unmatched = _validate_lifecycle(lifecycle_rows, run_id)
    quarantines = [
        event
        for event in run_events
        if isinstance(event, Mapping)
        and event.get("event") == "broker_admission_quarantined"
    ]
    if not unmatched and not quarantines:
        if set(known_usage["nodes"]) != set(expected_nodes):
            raise UsageEvidenceError(
                "complete lifecycle cannot explain a missing telemetry node"
            )
        return {
            "schema_version": 1,
            "run_id": run_id,
            "usage_complete": True,
            "known_usage": known_usage,
            "unmetered_unproven_requests": 0,
            "unmetered_unproven_request_identity_sha256s": [],
        }
    if not quarantines:
        raise UsageEvidenceError(
            "usage impairment lacks quarantine evidence"
        )
    matched: dict[tuple[Any, ...], dict[str, Any]] = {}
    for event in quarantines:
        quarantine = _quarantine_identity(event, run_id)
        candidates = [
            (key, item)
            for key, item in unmatched.items()
            if all(
                item[field] == quarantine[field]
                for field in (
                    "run_id",
                    "admission_id",
                    "provider_request_id",
                    "physical_host_id",
                    "model_instance_id",
                )
            )
        ]
        if len(candidates) != 1:
            raise UsageEvidenceError(
                "quarantine host/request does not match one exact unproven lifecycle start"
            )
        key, item = candidates[0]
        if key in matched:
            raise UsageEvidenceError("duplicate quarantine evidence targets one request")
        matched[key] = item
    if set(matched) != set(unmatched):
        raise UsageEvidenceError(
            "an active nonquarantined provider request remains in sealed evidence"
        )
    unproven_nodes: set[str] = set()
    identity_sha256s: list[str] = []
    for item in matched.values():
        matching_nodes = [
            node
            for node, model in model_by_node.items()
            if model == item["model_instance_id"]
        ]
        if len(matching_nodes) != 1:
            raise UsageEvidenceError("quarantined model is not one exact frozen node")
        unproven_nodes.add(matching_nodes[0])
        identity_sha256s.append(
            _sha256_bytes(
                _canonical_json(
                    {
                        "run_id": item["run_id"],
                        "admission_id": item["admission_id"],
                        "provider_request_id": item["provider_request_id"],
                        "physical_host_id": item["physical_host_id"],
                        "model_instance_id": item["model_instance_id"],
                        "ordinal": item["ordinal"],
                    }
                )
            )
        )
    missing_nodes = set(expected_nodes) - set(known_usage["nodes"])
    if not missing_nodes.issubset(unproven_nodes):
        raise UsageEvidenceError(
            "sealed telemetry omission is not explained by quarantined hosts"
        )
    identity_sha256s.sort()
    return {
        "schema_version": 1,
        "run_id": run_id,
        "usage_complete": False,
        "known_usage": known_usage,
        "unmetered_unproven_requests": len(identity_sha256s),
        "unmetered_unproven_request_identity_sha256s": identity_sha256s,
        "unmetered_unproven_nodes": sorted(unproven_nodes),
    }


def validate_score_telemetry(
    telemetry: Mapping[str, Any],
    contract: Mapping[str, Any],
    *,
    expected_nodes: Sequence[str],
) -> None:
    if not isinstance(telemetry, Mapping):
        raise UsageEvidenceError("score telemetry is missing")
    known_usage = contract.get("known_usage")
    if not isinstance(known_usage, Mapping):
        raise UsageEvidenceError("usage contract lacks known completed-call totals")
    unexpected_receipts = set(PUBLIC_RECEIPT_FIELDS) & set(telemetry)
    if unexpected_receipts:
        raise UsageEvidenceError(
            "raw scorer attempted to fabricate closure-owned usage fields"
        )
    for field in NUMERIC_FIELDS:
        if telemetry.get(field) != known_usage.get(field):
            raise UsageEvidenceError(
                f"score telemetry.{field} differs from sealed completed-call usage"
            )
    score_nodes = telemetry.get("nodes")
    known_nodes = known_usage.get("nodes")
    if not isinstance(score_nodes, Mapping) or not isinstance(known_nodes, Mapping):
        raise UsageEvidenceError("score or sealed telemetry nodes are malformed")
    if set(score_nodes) != set(known_nodes):
        raise UsageEvidenceError(
            "score telemetry node identities differ from sealed completed-call usage"
        )
    if not set(score_nodes).issubset(set(expected_nodes)):
        raise UsageEvidenceError("score telemetry contains a non-frozen node")
    for node, known in known_nodes.items():
        observed = score_nodes.get(node)
        if not isinstance(observed, Mapping) or not isinstance(known, Mapping):
            raise UsageEvidenceError("score or sealed telemetry node row is malformed")
        for field in NUMERIC_FIELDS:
            if observed.get(field) != known.get(field):
                raise UsageEvidenceError(
                    f"score telemetry node {node}.{field} differs from sealed usage"
                )
    usage_complete = contract.get("usage_complete")
    if usage_complete is True:
        if (
            contract.get("unmetered_unproven_requests") != 0
            or contract.get("unmetered_unproven_request_identity_sha256s") != []
            or set(score_nodes) != set(expected_nodes)
        ):
            raise UsageEvidenceError("complete usage contract is internally inconsistent")
        return
    if usage_complete is not False:
        raise UsageEvidenceError("usage contract completeness is malformed")
    identity_sha256s = contract.get("unmetered_unproven_request_identity_sha256s")
    count = contract.get("unmetered_unproven_requests")
    if (
        isinstance(count, bool)
        or not isinstance(count, int)
        or count < 1
        or not isinstance(identity_sha256s, list)
        or len(identity_sha256s) != count
        or identity_sha256s != sorted(identity_sha256s)
        or len(set(identity_sha256s)) != count
    ):
        raise UsageEvidenceError("impaired usage contract identities are malformed")
    for identity_sha256 in identity_sha256s:
        if not isinstance(identity_sha256, str) or len(identity_sha256) != 64:
            raise UsageEvidenceError("impaired usage contract identity is malformed")
        try:
            int(identity_sha256, 16)
        except ValueError as error:
            raise UsageEvidenceError("impaired usage contract identity is malformed") from error
    unproven_nodes = contract.get("unmetered_unproven_nodes")
    if (
        not isinstance(unproven_nodes, list)
        or not unproven_nodes
        or len(unproven_nodes) != len(set(unproven_nodes))
        or not set(unproven_nodes).issubset(set(expected_nodes))
    ):
        raise UsageEvidenceError("impaired usage contract nodes are malformed")
    missing_nodes = set(expected_nodes) - set(score_nodes)
    if not missing_nodes.issubset(set(unproven_nodes)):
        raise UsageEvidenceError(
            "score missing-node set differs from the sealed usage impairment"
        )


def public_usage_receipt(contract: Mapping[str, Any]) -> dict[str, Any]:
    receipt = {
        field: contract.get(field)
        for field in PUBLIC_RECEIPT_FIELDS
    }
    if receipt["usage_complete"] is True:
        if (
            receipt["unmetered_unproven_requests"] != 0
            or receipt["unmetered_unproven_request_identity_sha256s"] != []
        ):
            raise UsageEvidenceError("complete public usage receipt is inconsistent")
    elif receipt["usage_complete"] is False:
        if (
            not isinstance(receipt["unmetered_unproven_requests"], int)
            or receipt["unmetered_unproven_requests"] < 1
            or not isinstance(
                receipt["unmetered_unproven_request_identity_sha256s"], list
            )
            or len(receipt["unmetered_unproven_request_identity_sha256s"])
            != receipt["unmetered_unproven_requests"]
        ):
            raise UsageEvidenceError("impaired public usage receipt is inconsistent")
    else:
        raise UsageEvidenceError("public usage completeness is malformed")
    return receipt


def read_jsonl(path: pathlib.Path, *, maximum_bytes: int = 64 * 1024 * 1024) -> list[dict[str, Any]]:
    if path.is_symlink() or not path.is_file():
        raise UsageEvidenceError(f"sealed JSONL evidence is missing or linked: {path}")
    if path.stat().st_size > maximum_bytes:
        raise UsageEvidenceError(f"sealed JSONL evidence exceeds its bound: {path}")
    rows: list[dict[str, Any]] = []
    for index, line in enumerate(path.read_bytes().splitlines()):
        if not line:
            continue
        try:
            value = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise UsageEvidenceError(
                f"sealed JSONL evidence has malformed row {index}: {path}"
            ) from error
        if not isinstance(value, dict):
            raise UsageEvidenceError(
                f"sealed JSONL evidence has non-object row {index}: {path}"
            )
        rows.append(value)
    return rows


def usage_contract_from_run_dir(
    run_dir: pathlib.Path,
    *,
    run_id: str,
    expected_nodes: Sequence[str],
    expected_models: Sequence[str],
) -> dict[str, Any]:
    evidence_paths = {
        "run_events": run_dir / "run.jsonl",
        "provider_lifecycle": run_dir / ".swarm/provider-lifecycle-v1.jsonl",
        "telemetry": run_dir / ".swarm/telemetry.jsonl",
    }
    contract = derive_usage_contract(
        run_id=run_id,
        run_events=read_jsonl(evidence_paths["run_events"]),
        lifecycle_rows=read_jsonl(evidence_paths["provider_lifecycle"]),
        telemetry_rows=read_jsonl(evidence_paths["telemetry"]),
        expected_nodes=expected_nodes,
        expected_models=expected_models,
    )
    contract["evidence_sha256"] = {
        name: _sha256_bytes(path.read_bytes())
        for name, path in sorted(evidence_paths.items())
    }
    return contract
