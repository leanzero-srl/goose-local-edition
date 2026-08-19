"""Shared helpers: instants, Berlin days, the error envelope, HTTP plumbing."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Dict, List, Optional
from zoneinfo import ZoneInfo

BERLIN = ZoneInfo("Europe/Berlin")
STATUS_ORDER = ["settled", "pending", "refunded", "failed"]
CURRENCY_EXPONENTS = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}


def parse_instant(value: str) -> datetime:
    """RFC3339 with any offset -> aware datetime. Compare instants, never strings."""
    if value.endswith("Z"):
        value = value[:-1] + "+00:00"
    return datetime.fromisoformat(value)


def utc_rfc3339(value: str) -> str:
    return parse_instant(value).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def now_rfc3339() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def berlin_day(value: str) -> str:
    """The Europe/Berlin calendar date of the INSTANT — the backend owns DST."""
    return parse_instant(value).astimezone(BERLIN).date().isoformat()


def epoch_of(value: str) -> float:
    return parse_instant(value).timestamp()


def envelope(code: str, message: str,
             field_errors: Optional[List[Dict]] = None) -> Dict:
    err: Dict = {"code": code, "message": message}
    if field_errors:
        err["field_errors"] = field_errors
    return {"error": err}


def fe(path: str, code: str) -> Dict:
    return {"path": path, "code": code}


def parse_int_param(raw: Optional[str], name: str, default: int) -> tuple:
    """(value, field_error_or_None): non-numeric -> not_an_integer, negative -> not_positive."""
    if raw is None:
        return default, None
    try:
        v = int(raw)
    except (TypeError, ValueError):
        return None, fe(name, "not_an_integer")
    if v < 0:
        return None, fe(name, "not_positive")
    return v, None


def json_bytes(payload) -> bytes:
    return json.dumps(payload, separators=(",", ":")).encode()
