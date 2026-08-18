"""Dependency-free Meridian v2 client."""
from __future__ import annotations

import datetime as dt
import email.utils
import json
import socket
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


class MeridianError(Exception):
    def __init__(self, status: int, body: dict | None = None):
        self.status = status
        self.body = body or {}
        super().__init__(str(self.body.get("error", f"Meridian returned {status}")))


class MeridianConflict(MeridianError):
    pass


class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self._collection_etag: str | None = None
        self._collection: list[dict] | None = None

    @staticmethod
    def _object(value: object) -> dict:
        if not isinstance(value, dict):
            raise MeridianError(502, {"error": "invalid_response"})
        return value

    @staticmethod
    def _instant(value: str) -> dt.datetime:
        return dt.datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(dt.timezone.utc)

    @staticmethod
    def _timed_out(error: BaseException) -> bool:
        return isinstance(error, (socket.timeout, TimeoutError)) or (
            isinstance(error, urllib.error.URLError)
            and isinstance(error.reason, (socket.timeout, TimeoutError))
        )

    @staticmethod
    def _retry_after(value: str | None) -> float:
        try:
            return max(0.0, float(value or ""))
        except ValueError:
            try:
                when = email.utils.parsedate_to_datetime(value or "")
                if when.tzinfo is None:
                    when = when.replace(tzinfo=dt.timezone.utc)
                return max(0.0, (when - dt.datetime.now(dt.timezone.utc)).total_seconds())
            except (TypeError, ValueError, IndexError):
                return 0.0

    def _request(
        self, method: str, path: str, body: Any = None, headers: dict[str, str] | None = None
    ) -> tuple[int, dict[str, str], dict | None]:
        payload = None if body is None else json.dumps(body, separators=(",", ":"), allow_nan=False).encode()
        request_headers = {"Authorization": f"Bearer {self.api_key}", "Accept": "application/json"}
        if payload is not None:
            request_headers["Content-Type"] = "application/json"
        request_headers.update(headers or {})
        retried_timeout = False
        while True:
            request = urllib.request.Request(self.base_url + path, data=payload, headers=request_headers, method=method)
            try:
                with urllib.request.urlopen(request, timeout=10) as response:
                    raw = response.read()
                    try:
                        parsed = json.loads(raw) if raw else None
                    except (UnicodeDecodeError, json.JSONDecodeError):
                        raise MeridianError(502, {"error": "invalid_response"}) from None
                    return response.status, dict(response.headers.items()), parsed
            except urllib.error.HTTPError as error:
                raw = error.read()
                try:
                    parsed = json.loads(raw) if raw else {}
                except (UnicodeDecodeError, json.JSONDecodeError):
                    parsed = {}
                if error.code == 429:
                    time.sleep(self._retry_after(error.headers.get("Retry-After")))
                    continue
                if error.code == 304:
                    return 304, dict(error.headers.items()), None
                raise MeridianError(error.code, parsed if isinstance(parsed, dict) else {}) from None
            except (socket.timeout, TimeoutError, urllib.error.URLError) as error:
                if self._timed_out(error) and not retried_timeout:
                    retried_timeout = True
                    continue
                raise MeridianError(503, {"error": "unavailable"}) from error

    def fetch_all_payments(self) -> list[dict]:
        cursor: str | None = None
        payments: list[dict] = []
        while True:
            params = {"limit": "100"}
            if cursor is not None:
                params["cursor"] = cursor
            conditional = {"If-None-Match": self._collection_etag} if cursor is None and self._collection_etag else {}
            try:
                status, response_headers, body = self._request(
                    "GET", "/v2/payments?" + urllib.parse.urlencode(params), headers=conditional
                )
            except MeridianError as error:
                if error.status == 410 and error.body.get("error") == "cursor_expired":
                    cursor, payments = None, []
                    continue
                raise
            if status == 304 and cursor is None and self._collection is not None:
                return list(self._collection)
            document = self._object(body)
            page = document.get("data")
            missing = object()
            next_cursor = document.get("next_cursor", missing)
            if not isinstance(page, list) or next_cursor is missing or (next_cursor is not None and not isinstance(next_cursor, str)):
                raise MeridianError(502, {"error": "invalid_collection"})
            if cursor is None:
                self._collection_etag = response_headers.get("ETag")
            payments.extend(page)
            cursor = next_cursor
            if cursor is None:
                try:
                    payments.sort(key=lambda payment: self._instant(payment["created_at"]))
                except (KeyError, TypeError, ValueError):
                    raise MeridianError(502, {"error": "invalid_payment"}) from None
                self._collection = list(payments)
                return payments

    def get_payment(self, payment_id: str) -> dict:
        _, _, body = self._request("GET", f"/v2/payments/{urllib.parse.quote(payment_id, safe='')}")
        return self._object(body)

    def total_count(self) -> int:
        _, _, body = self._request("GET", "/v2/payments?limit=1")
        try:
            return int(self._object(body)["total"])
        except (KeyError, TypeError, ValueError):
            raise MeridianError(502, {"error": "invalid_collection"}) from None

    def create_payment(self, value_minor: int, currency: str, counterparty: dict, occurred_at: str, idempotency_key: str) -> str:
        body = {"amount": {"value_minor": value_minor, "currency": currency}, "counterparty": counterparty, "occurred_at": occurred_at}
        try:
            _, _, result = self._request("POST", "/v2/payments", body, {"Idempotency-Key": idempotency_key})
            payment_id = self._object(result).get("id")
            if not isinstance(payment_id, str):
                raise MeridianError(502, {"error": "invalid_payment"})
            return payment_id
        except MeridianError as error:
            if error.status == 409 and error.body.get("error") == "duplicate" and isinstance(error.body.get("payment_id"), str):
                return error.body["payment_id"]
            raise

    def create_batch(self, items: list[dict]) -> list[dict]:
        _, _, body = self._request("POST", "/v2/payments/batch", {"items": items})
        results = self._object(body).get("results")
        if not isinstance(results, list):
            raise MeridianError(502, {"error": "invalid_batch"})
        return results

    def update_payment(self, payment_id: str, fields: dict, version: int) -> dict:
        path = f"/v2/payments/{urllib.parse.quote(payment_id, safe='')}"
        for attempt in range(2):
            try:
                _, _, body = self._request("PATCH", path, fields, {"If-Match": f'"{version}"'})
                return self._object(body)
            except MeridianError as error:
                if error.status != 412:
                    raise
                if attempt == 1:
                    raise MeridianConflict(error.status, error.body) from None
                fresh = self.get_payment(payment_id)
                try:
                    version = int(fresh["version"])
                except (KeyError, TypeError, ValueError):
                    raise MeridianError(502, {"error": "invalid_payment"}) from None
        raise MeridianConflict(412, {"error": "version_conflict"})

    def register_webhook(self, url: str) -> dict:
        _, _, body = self._request("POST", "/v2/webhooks", {"url": url})
        result = self._object(body)
        if not isinstance(result.get("id"), str) or not isinstance(result.get("secret"), str):
            raise MeridianError(502, {"error": "invalid_webhook"})
        return result
