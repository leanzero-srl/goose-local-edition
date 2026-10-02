"""Meridian API v3 client — stdlib only, every request timed out at 10 s.

Implements the documented behaviours that defeat a client that did not read: server-fixed
64-per-page cursor walk (no limit param, ever), dropped-connection resume with the SAME
cursor, one Retry-After'd retry on a 500, 410 cursor restart, per-page (ETag, generation)
validator pairs with the generation rule (a 304 whose X-Collection-Generation disagrees
with the generation stored WITH that validator is a cache miss: drop it and refetch
unconditionally exactly once), If-Match note writes with one refetch+retry on 412, and
idempotent creates whose retry MUST reuse the stored key.
"""

from __future__ import annotations

import http.client
import json
import socket
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Dict, Optional

REQUEST_TIMEOUT = 10
TIMEOUT_RETRIES = 3


class VendorUnavailable(Exception):
    pass


class VendorConflict(Exception):
    pass


class VendorError(Exception):
    pass


class MeridianClient:
    def __init__(self, base_url: str, api_key: str):
        self.base = base_url.rstrip("/")
        self.key = api_key

    def _request(self, method: str, path: str, body: Optional[Dict] = None,
                 headers: Optional[Dict[str, str]] = None,
                 retry_dropped: bool = True) -> tuple:
        """One exchange. Timeouts and dropped connections are documented vendor behaviour:
        a timed-out or severed idempotent request is retried (same request, same cursor) a
        bounded number of times. Returns (status, json_or_None, headers)."""
        url = self.base + path
        payload = json.dumps(body).encode() if body is not None else None
        attempts = 0
        while True:
            req = urllib.request.Request(url, data=payload, method=method)
            req.add_header("Authorization", f"Bearer {self.key}")
            if payload is not None:
                req.add_header("Content-Type", "application/json")
            for k, v in (headers or {}).items():
                req.add_header(k, v)
            try:
                with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT) as resp:
                    raw = resp.read()
                    return resp.status, (json.loads(raw) if raw else None), dict(resp.headers)
            except urllib.error.HTTPError as err:
                raw = err.read()
                try:
                    data = json.loads(raw) if raw else None
                except json.JSONDecodeError:
                    data = None
                return err.code, data, dict(err.headers)
            except (socket.timeout, TimeoutError,
                    http.client.RemoteDisconnected, ConnectionResetError,
                    ConnectionRefusedError, BrokenPipeError,
                    http.client.BadStatusLine) as err:
                attempts += 1
                refused = isinstance(err, ConnectionRefusedError)
                if refused or not retry_dropped or attempts > TIMEOUT_RETRIES:
                    raise VendorUnavailable(f"{type(err).__name__}") from err
                time.sleep(0.2)
                continue
            except urllib.error.URLError as err:
                reason = getattr(err, "reason", None)
                if isinstance(reason, (socket.timeout, TimeoutError,
                                       ConnectionResetError)) and retry_dropped:
                    attempts += 1
                    if attempts > TIMEOUT_RETRIES:
                        raise VendorUnavailable("stalled") from err
                    time.sleep(0.2)
                    continue
                raise VendorUnavailable(f"{reason}") from err

    # ── reads ────────────────────────────────────────────────────────────────────────────────

    def list_page(self, cursor: Optional[str],
                  validator: Optional[Dict] = None) -> Dict:
        """One page of the walk. NEVER passes a limit — the page size is server-fixed.

        Returns one of:
          {"kind": "page", "data": [...], "next_cursor", "total", "etag", "gen"}
          {"kind": "not_modified", "gen": ...}            honest 304, generation agrees
          {"kind": "cache_miss"}                          304 with disagreeing generation
          {"kind": "expired"}                             410 — restart the walk
          {"kind": "retry_after", "secs": float}          500 — wait, retry once, continue
        """
        path = "/v3/payments"
        if cursor:
            path += "?" + urllib.parse.urlencode({"cursor": cursor})
        cond = {"If-None-Match": validator["etag"]} if validator else {}
        status, data, hdrs = self._request("GET", path, headers=cond)
        gen = hdrs.get("X-Collection-Generation")
        if status == 304:
            if validator and gen is not None and gen != validator["gen"]:
                return {"kind": "cache_miss"}
            return {"kind": "not_modified", "gen": gen}
        if status == 410:
            return {"kind": "expired"}
        if status == 500:
            try:
                secs = float(hdrs.get("Retry-After") or 1)
            except ValueError:
                secs = 1.0
            return {"kind": "retry_after", "secs": secs}
        if status != 200 or not isinstance(data, dict):
            raise VendorError(f"list: HTTP {status}")
        return {"kind": "page", "data": data.get("data") or [],
                "next_cursor": data.get("next_cursor"), "total": data.get("total"),
                "etag": hdrs.get("ETag"), "gen": gen}

    def list_page_unconditional(self, cursor: Optional[str]) -> Dict:
        return self.list_page(cursor, validator=None)

    def reversals(self) -> Dict:
        status, data, _h = self._request("GET", "/v3/reversals")
        if status != 200 or not isinstance(data, dict):
            raise VendorError(f"reversals: HTTP {status}")
        return data

    def payment(self, pid: str) -> Optional[Dict]:
        status, data, _h = self._request("GET", f"/v3/payments/{pid}")
        if status == 404:
            return None
        if status != 200 or not isinstance(data, dict):
            raise VendorError(f"payment {pid}: HTTP {status}")
        return data

    # ── writes ───────────────────────────────────────────────────────────────────────────────

    def patch_note(self, pid: str, note: str, version: int) -> Dict:
        """The documented If-Match dance: 412 -> refetch, retry ONCE with the fresh
        version; a second 412 raises VendorConflict. Never writes without If-Match."""
        status, data, _h = self._request("PATCH", f"/v3/payments/{pid}", {"note": note},
                                         headers={"If-Match": str(version)})
        if status == 412:
            fresh = self.payment(pid)
            if fresh is None:
                raise VendorError("payment vanished during the note dance")
            status, data, _h = self._request(
                "PATCH", f"/v3/payments/{pid}", {"note": note},
                headers={"If-Match": str(fresh["version"])})
            if status == 412:
                raise VendorConflict("second 412 — someone keeps winning the race")
        if status != 200 or not isinstance(data, dict):
            raise VendorError(f"note write: HTTP {status}")
        return data

    def create_payment(self, body: Dict, idempotency_key: str) -> Dict:
        """201 first time; any later use of the SAME key returns the SAME payment (200).
        A crashed or timed-out send retries into exactly this replay."""
        status, data, _h = self._request("POST", "/v3/payments", body,
                                         headers={"Idempotency-Key": idempotency_key})
        if status in (200, 201) and isinstance(data, dict) and data.get("id"):
            return data
        raise VendorError(f"create: HTTP {status}")

    def register_webhook(self, url: str) -> Dict:
        status, data, _h = self._request("POST", "/v3/webhooks", {"url": url})
        if status == 200 and isinstance(data, dict) and data.get("secret"):
            return data
        raise VendorError(f"register_webhook: HTTP {status}")
