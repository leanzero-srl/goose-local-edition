"""Meridian API v2 client — stdlib only.

Vendor endpoint paths are collected in ENDPOINTS below because the sb-6 vendor mock
(vendor_service_v2) is built by a different workstream: if its paths differ, this one block is
the only place to change. The behaviours themselves are the spec's frozen contract: cursor
pagination with per-page ETag/If-None-Match, Retry-After in both RFC 7231 forms, 410
cursor_expired restart, If-Match optimistic concurrency with exactly one refetch+retry on 412,
idempotent creates (409 duplicate is the success path for a retry), independent batch items,
idempotent-by-URL webhook registration, and a hard request timeout (vendor stalls are documented
behaviour a client must survive — red-team F14).
"""

from __future__ import annotations

import json
import socket
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from typing import Dict, List, Optional

ENDPOINTS = {
    "list": "/v2/payments",
    "payment": "/v2/payments/{id}",
    "create": "/v2/payments",
    "batch": "/v2/payments/batch",
    "webhooks": "/v2/webhooks",
}

PAGE_LIMIT = 100          # the documented max — fewer requests, same trap chain
REQUEST_TIMEOUT = 10      # seconds; the docs require a client-side timeout (stall trap)
TIMEOUT_RETRIES = 3
MAX_RETRY_AFTER_LOOPS = 8


class MeridianError(Exception):
    """Base for vendor-side failures surfaced to callers."""


class MeridianConflict(MeridianError):
    """A second 412 after the documented refetch+retry — someone keeps winning the race."""


class MeridianUnavailable(MeridianError):
    """The vendor cannot be reached at all (connection refused, DNS, repeated stalls)."""


def parse_instant(value: str) -> datetime:
    if value.endswith("Z"):
        value = value[:-1] + "+00:00"
    return datetime.fromisoformat(value)


def to_utc_rfc3339(value: str) -> str:
    return parse_instant(value).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self._page_cache: Dict[str, Dict] = {}
        self._cache_lock = threading.Lock()

    # ── low-level ─────────────────────────────────────────────────────────────────────────────

    def _request(self, method: str, path: str, body: Optional[dict] = None,
                 headers: Optional[Dict[str, str]] = None) -> tuple:
        """One HTTP exchange with the vendor. Handles 429 Retry-After (both forms) and request
        timeouts internally; every other status is returned to the caller.

        Returns (status, parsed_json_or_None, response_headers)."""
        url = self.base_url + path
        payload = json.dumps(body).encode() if body is not None else None
        throttled = 0
        timeouts = 0
        while True:
            req = urllib.request.Request(url, data=payload, method=method)
            req.add_header("Authorization", f"Bearer {self.api_key}")
            req.add_header("User-Agent", "vspro-reference/1.0")
            if payload is not None:
                req.add_header("Content-Type", "application/json")
            for key, val in (headers or {}).items():
                req.add_header(key, val)
            try:
                with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT) as resp:
                    raw = resp.read()
                    data = json.loads(raw) if raw else None
                    return resp.status, data, dict(resp.headers)
            except urllib.error.HTTPError as err:
                raw = err.read()
                try:
                    data = json.loads(raw) if raw else None
                except json.JSONDecodeError:
                    data = None
                if err.code == 429:
                    throttled += 1
                    if throttled > MAX_RETRY_AFTER_LOOPS:
                        raise MeridianUnavailable("vendor keeps rate-limiting") from err
                    self._wait_retry_after(err.headers.get("Retry-After"))
                    continue
                return err.code, data, dict(err.headers)
            except (socket.timeout, TimeoutError) as err:
                # Documented stall behaviour: time out and retry the same request.
                timeouts += 1
                if timeouts > TIMEOUT_RETRIES:
                    raise MeridianUnavailable("vendor stalled repeatedly") from err
                continue
            except urllib.error.URLError as err:
                if isinstance(err.reason, (socket.timeout, TimeoutError)):
                    timeouts += 1
                    if timeouts > TIMEOUT_RETRIES:
                        raise MeridianUnavailable("vendor stalled repeatedly") from err
                    continue
                raise MeridianUnavailable(f"vendor unreachable: {err.reason}") from err

    @staticmethod
    def _wait_retry_after(value: Optional[str]) -> None:
        delay = 1.0
        if value:
            try:
                delay = float(value)
            except ValueError:
                try:
                    when = parsedate_to_datetime(value)
                    delay = (when - datetime.now(timezone.utc)).total_seconds()
                except (ValueError, TypeError):
                    delay = 1.0
        time.sleep(max(0.0, min(delay, 30.0)))

    # ── reads ─────────────────────────────────────────────────────────────────────────────────

    def _list_page(self, cursor: Optional[str]) -> Dict:
        """One list page, conditional when we hold an ETag for it. A 304 is served from the
        cache (the vendor sends no body); a fresh 200 refreshes the cache. Raises a 410 marker
        via KeyError-free contract: returns {"_expired": True} so the caller restarts."""
        params = {"limit": str(PAGE_LIMIT)}
        if cursor:
            params["cursor"] = cursor
        path = ENDPOINTS["list"] + "?" + urllib.parse.urlencode(params)
        cache_key = cursor or ""
        with self._cache_lock:
            cached = self._page_cache.get(cache_key)
        cond = {"If-None-Match": cached["etag"]} if cached else {}
        status, data, resp_headers = self._request("GET", path, headers=cond)
        if status == 304 and cached:
            return cached["payload"]
        if status == 410:
            return {"_expired": True}
        if status != 200 or not isinstance(data, dict):
            raise MeridianError(f"list failed: HTTP {status}")
        etag = resp_headers.get("ETag")
        if etag:
            with self._cache_lock:
                self._page_cache[cache_key] = {"etag": etag, "payload": data}
        return data

    def fetch_all_payments(self) -> List[dict]:
        rows: Dict[str, dict] = {}
        cursor: Optional[str] = None
        restarts = 0
        while True:
            page = self._list_page(cursor)
            if page.get("_expired"):
                restarts += 1
                if restarts > 3:
                    raise MeridianError("cursor kept expiring after restarts")
                rows.clear()
                cursor = None
                continue
            for payment in page.get("data") or []:
                rows[payment["id"]] = payment
            cursor = page.get("next_cursor")
            if cursor is None:
                break
        out = list(rows.values())
        out.sort(key=lambda p: (parse_instant(p["created_at"]), p["id"]))
        return out

    def get_payment(self, payment_id: str) -> dict:
        status, data, _ = self._request("GET", ENDPOINTS["payment"].format(id=payment_id))
        if status != 200 or not isinstance(data, dict):
            raise MeridianError(f"get_payment {payment_id}: HTTP {status}")
        return data

    def total_count(self) -> int:
        params = urllib.parse.urlencode({"limit": "1"})
        status, data, _ = self._request("GET", ENDPOINTS["list"] + "?" + params)
        if status != 200 or not isinstance(data, dict):
            raise MeridianError(f"total_count: HTTP {status}")
        return int(data["total"])

    # ── writes ────────────────────────────────────────────────────────────────────────────────

    def create_payment(self, value_minor: int, currency: str, counterparty: dict,
                       occurred_at: str, idempotency_key: str) -> str:
        body = {"amount": {"value_minor": value_minor, "currency": currency},
                "counterparty": counterparty, "occurred_at": occurred_at}
        status, data, _ = self._request("POST", ENDPOINTS["create"], body=body,
                                        headers={"Idempotency-Key": idempotency_key})
        if status in (200, 201) and isinstance(data, dict) and data.get("id"):
            return data["id"]
        if status == 409 and isinstance(data, dict) and data.get("payment_id"):
            # Documented: the success path for a retried create. Never resubmit with a fresh key.
            return data["payment_id"]
        raise MeridianError(f"create_payment: HTTP {status}")

    def create_batch(self, items: List[dict]) -> List[dict]:
        if not 1 <= len(items) <= 20:
            raise MeridianError("batch size must be 1-20")
        status, data, _ = self._request("POST", ENDPOINTS["batch"], body={"items": items})
        if status != 200 or not isinstance(data, dict) or "results" not in data:
            raise MeridianError(f"create_batch: HTTP {status}")
        # Per-item outcomes, input order kept; failed items are the caller's news to report —
        # never retried, never resubmitted here.
        return data["results"]

    def update_payment(self, payment_id: str, fields: dict, version: int) -> dict:
        result = self._patch_once(payment_id, fields, version)
        if not result.get("_conflict"):
            return result
        fresh = self.get_payment(payment_id)
        result = self._patch_once(payment_id, fields, int(fresh["version"]))
        if result.get("_conflict"):
            raise MeridianConflict(f"payment {payment_id} kept conflicting after one retry")
        return result

    def _patch_once(self, payment_id: str, fields: dict, version: int) -> dict:
        status, data, _ = self._request("PATCH", ENDPOINTS["payment"].format(id=payment_id),
                                        body=fields, headers={"If-Match": str(version)})
        if status == 412:
            return {"_conflict": True}
        if status == 428:
            # The vendor never sees a write without If-Match from this client; a 428 means the
            # header was lost in transit or the contract drifted — a bug, not a retry case.
            raise MeridianError("428 Precondition Required despite If-Match being sent")
        if status != 200 or not isinstance(data, dict):
            raise MeridianError(f"update_payment {payment_id}: HTTP {status}")
        return data

    def register_webhook(self, url: str) -> dict:
        status, data, _ = self._request("POST", ENDPOINTS["webhooks"], body={"url": url})
        if status in (200, 201) and isinstance(data, dict) and data.get("secret"):
            return data
        raise MeridianError(f"register_webhook: HTTP {status}")
