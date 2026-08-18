"""The vspro HTTP backend — threaded so reads keep answering while a sync is in flight.

One structured error envelope everywhere, frozen field-error codes, webhook signature
verification against the RAW request body, and process-lifetime webhook counters (the health
quad counts events received by THIS process since it started — red-team F3; the registration
challenge increments nothing — F13).
"""

from __future__ import annotations

import hashlib
import hmac
import json
import re
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from urllib.parse import parse_qs, urlparse

from . import CURRENCIES, STATUSES
from .meridian import (MeridianClient, MeridianConflict, MeridianError, MeridianUnavailable,
                       parse_instant)
from .store import Store

WEB_DIR = Path(__file__).resolve().parent / "web"
STATIC = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/index.html": ("index.html", "text/html; charset=utf-8"),
    "/styles.css": ("styles.css", "text/css; charset=utf-8"),
    "/app.js": ("app.js", "application/javascript; charset=utf-8"),
    "/viz.js": ("viz.js", "application/javascript; charset=utf-8"),
}

SORTS = ("created_at", "-created_at", "amount_minor", "-amount_minor")
PAYMENT_PATH = re.compile(r"^/api/payments/([^/]+)$")
NOTE_PATH = re.compile(r"^/api/payments/([^/]+)/note$")
RFC3339 = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$")
COUNTRY = re.compile(r"^[A-Z]{2}$")


class WebhookLedger:
    """The four live counters plus the shared secret. In-memory on purpose: the spec pins the
    counters to events received by this process since it started."""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.registered = False
        self.secret: Optional[str] = None
        self.received = 0
        self.applied = 0
        self.ignored = 0
        self.rejected = 0

    def snapshot(self) -> dict:
        with self.lock:
            return {"registered": self.registered, "received": self.received,
                    "applied": self.applied, "ignored": self.ignored,
                    "rejected": self.rejected}

    def bump(self, counter: str) -> None:
        with self.lock:
            setattr(self, counter, getattr(self, counter) + 1)


class AppContext:
    def __init__(self, store: Store, client: MeridianClient) -> None:
        self.store = store
        self.client = client
        self.ledger = WebhookLedger()
        self.sync_lock = threading.Lock()


def field_error(path: str, code: str) -> dict:
    return {"path": path, "code": code}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    ctx: AppContext = None  # injected by serve()

    def log_message(self, *_args) -> None:
        return

    # ── plumbing ──────────────────────────────────────────────────────────────────────────────

    def _json(self, code: int, payload: dict) -> None:
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _error(self, code: int, err_code: str, message: str,
               field_errors: Optional[List[dict]] = None) -> None:
        err = {"code": err_code, "message": message}
        if field_errors:
            err["field_errors"] = field_errors
        self._json(code, {"error": err})

    def _static(self, path: str) -> None:
        name, ctype = STATIC[path]
        body = (WEB_DIR / name).read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    # ── routing ───────────────────────────────────────────────────────────────────────────────

    def do_GET(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        route = parsed.path
        if route in STATIC:
            self._static(route)
            return
        if route == "/api/health":
            self._health()
            return
        if route == "/api/payments":
            self._payments(parse_qs(parsed.query))
            return
        match = PAYMENT_PATH.match(route)
        if match:
            self._payment_one(match.group(1))
            return
        if route == "/api/summary":
            self._json(200, self.ctx.store.summary())
            return
        if route == "/api/buckets":
            self._buckets()
            return
        self._error(404, "not_found", f"no such path: {route}")

    def do_POST(self) -> None:  # noqa: N802
        route = urlparse(self.path).path
        raw = self._read_body()
        if route == "/api/webhooks/meridian":
            self._webhook(raw)
            return
        try:
            body = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            self._error(400, "bad_request", "request body is not valid JSON")
            return
        if route == "/api/sync":
            self._sync()
            return
        match = NOTE_PATH.match(route)
        if match:
            self._note(match.group(1), body)
            return
        if route == "/api/payments/batch":
            self._batch(body)
            return
        self._error(404, "not_found", f"no such path: {route}")

    # ── GET handlers ──────────────────────────────────────────────────────────────────────────

    def _health(self) -> None:
        self._json(200, {"status": "ok", "payments": self.ctx.store.count(),
                         "last_sync": self.ctx.store.last_sync(),
                         "webhook": self.ctx.ledger.snapshot()})

    @staticmethod
    def _int_param(params: Dict[str, list], name: str, default: int,
                   errors: List[dict]) -> int:
        raw = (params.get(name) or [None])[0]
        if raw is None:
            return default
        try:
            value = int(raw)
        except ValueError:
            errors.append(field_error(name, "not_an_integer"))
            return default
        if value < 0:
            errors.append(field_error(name, "not_positive"))
            return default
        return value

    def _payments(self, params: Dict[str, list]) -> None:
        errors: List[dict] = []
        limit = self._int_param(params, "limit", 50, errors)
        offset = self._int_param(params, "offset", 0, errors)
        status = (params.get("status") or [None])[0]
        currency = (params.get("currency") or [None])[0]
        sort = (params.get("sort") or ["created_at"])[0]
        if status is not None and status not in STATUSES:
            errors.append(field_error("status", "unsupported"))
        if currency is not None and currency not in CURRENCIES:
            errors.append(field_error("currency", "unsupported"))
        if sort not in SORTS:
            errors.append(field_error("sort", "unsupported"))
        if errors:
            self._error(400, "bad_request", "invalid query parameters", errors)
            return
        limit = min(limit, 200)
        rows, total = self.ctx.store.query(limit=limit, offset=offset, status=status,
                                           currency=currency, sort=sort)
        self._json(200, {"data": rows, "total": total, "limit": limit, "offset": offset})

    def _payment_one(self, payment_id: str) -> None:
        row = self.ctx.store.get(payment_id)
        if row is None:
            self._error(404, "not_found", f"no payment {payment_id}")
            return
        self._json(200, row)

    def _buckets(self) -> None:
        cells = self.ctx.store.buckets()
        days: List[str] = []
        for cell in cells:
            if not days or days[-1] != cell["day"]:
                days.append(cell["day"])
        self._json(200, {"timezone": "Europe/Berlin", "days": days,
                         "statuses": list(STATUSES), "cells": cells})

    # ── POST handlers ─────────────────────────────────────────────────────────────────────────

    def _sync(self) -> None:
        with self.ctx.sync_lock:
            try:
                payments = self.ctx.client.fetch_all_payments()
            except MeridianUnavailable:
                self._error(503, "vendor_unavailable", "the Meridian API is unreachable")
                return
            except MeridianError as err:
                self._error(502, "vendor_unavailable", f"vendor sync failed: {err}")
                return
            inserted, updated = self.ctx.store.upsert_many(payments)
            self.ctx.store.set_last_sync(
                datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
        self._json(200, {"fetched": len(payments), "inserted": inserted,
                         "updated": updated, "total": self.ctx.store.count()})

    def _note(self, payment_id: str, body: dict) -> None:
        note = body.get("note")
        errors: List[dict] = []
        if note is None:
            errors.append(field_error("note", "required"))
        elif not isinstance(note, str) or not note:
            errors.append(field_error("note", "required"))
        elif len(note) > 280:
            errors.append(field_error("note", "too_long"))
        if errors:
            self._error(400, "bad_request", "invalid note", errors)
            return
        row = self.ctx.store.get(payment_id)
        if row is None:
            self._error(404, "not_found", f"no payment {payment_id}")
            return
        try:
            fresh = self.ctx.client.update_payment(payment_id, {"note": note},
                                                   version=row["version"])
        except MeridianConflict:
            # Second 412: surface the conflict, local row untouched.
            self._error(409, "conflict",
                        "the payment was modified concurrently and the edit could not be applied")
            return
        except MeridianUnavailable:
            self._error(503, "vendor_unavailable", "the Meridian API is unreachable")
            return
        except MeridianError as err:
            self._error(502, "vendor_unavailable", f"vendor update failed: {err}")
            return
        self.ctx.store.upsert_one(fresh)
        self._json(200, {"id": payment_id, "note": fresh.get("note", note),
                         "version": int(fresh["version"])})

    def _batch(self, body: dict) -> None:
        items = body.get("items")
        errors: List[dict] = []
        if not isinstance(items, list) or not items:
            errors.append(field_error("items", "required"))
        elif len(items) > 20:
            errors.append(field_error("items", "too_long"))
        else:
            for i, item in enumerate(items):
                errors.extend(self._validate_item(i, item if isinstance(item, dict) else {}))
        if errors:
            self._error(400, "bad_request", "invalid batch payload", errors)
            return
        vendor_items = [{"amount": it["amount"], "counterparty": it["counterparty"],
                         "occurred_at": it["occurred_at"],
                         "idempotency_key": it["idempotency_key"]} for it in items]
        try:
            outcomes = self.ctx.client.create_batch(vendor_items)
        except MeridianUnavailable:
            self._error(503, "vendor_unavailable", "the Meridian API is unreachable")
            return
        except MeridianError as err:
            self._error(502, "vendor_unavailable", f"vendor batch failed: {err}")
            return
        results, succeeded, failed = [], 0, 0
        for i, outcome in enumerate(outcomes):
            if outcome.get("status") == "created" or outcome.get("id"):
                succeeded += 1
                results.append({"index": i, "status": "created", "id": outcome.get("id")})
            else:
                failed += 1
                err = outcome.get("error") or {}
                results.append({"index": i, "status": "error",
                                "error": {"code": err.get("code", "bad_request"),
                                          "message": err.get("message", "item failed")}})
        self._json(200, {"results": results, "succeeded": succeeded, "failed": failed})

    @staticmethod
    def _validate_item(i: int, item: dict) -> List[dict]:
        errors: List[dict] = []
        prefix = f"items[{i}]"
        amount = item.get("amount")
        if not isinstance(amount, dict):
            errors.append(field_error(f"{prefix}.amount", "required"))
        else:
            value = amount.get("value_minor")
            if value is None:
                errors.append(field_error(f"{prefix}.amount.value_minor", "required"))
            elif isinstance(value, bool) or not isinstance(value, int):
                errors.append(field_error(f"{prefix}.amount.value_minor", "not_an_integer"))
            elif value <= 0:
                errors.append(field_error(f"{prefix}.amount.value_minor", "not_positive"))
            currency = amount.get("currency")
            if not currency:
                errors.append(field_error(f"{prefix}.amount.currency", "required"))
            elif currency not in CURRENCIES:
                errors.append(field_error(f"{prefix}.amount.currency", "unsupported"))
        counterparty = item.get("counterparty")
        if not isinstance(counterparty, dict):
            errors.append(field_error(f"{prefix}.counterparty", "required"))
        else:
            name = counterparty.get("name")
            if not name or not isinstance(name, str):
                errors.append(field_error(f"{prefix}.counterparty.name", "required"))
            elif len(name) > 80:
                errors.append(field_error(f"{prefix}.counterparty.name", "too_long"))
            country = counterparty.get("country")
            if not country or not isinstance(country, str):
                errors.append(field_error(f"{prefix}.counterparty.country", "required"))
            elif not COUNTRY.match(country):
                errors.append(field_error(f"{prefix}.counterparty.country", "bad_format"))
        occurred = item.get("occurred_at")
        if not occurred or not isinstance(occurred, str):
            errors.append(field_error(f"{prefix}.occurred_at", "required"))
        elif not RFC3339.match(occurred):
            errors.append(field_error(f"{prefix}.occurred_at", "bad_format"))
        else:
            try:
                parse_instant(occurred)
            except ValueError:
                errors.append(field_error(f"{prefix}.occurred_at", "bad_format"))
        key = item.get("idempotency_key")
        if not key or not isinstance(key, str):
            errors.append(field_error(f"{prefix}.idempotency_key", "required"))
        return errors

    # ── webhooks ──────────────────────────────────────────────────────────────────────────────

    def _webhook(self, raw: bytes) -> None:
        try:
            body = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            body = None
        if isinstance(body, dict) and body.get("type") == "webhook.verify" \
                and "challenge" in body:
            # Registration plumbing, not an event delivery: increments no counter.
            self._json(200, {"challenge": body["challenge"]})
            return
        self.ctx.ledger.bump("received")
        secret = self.ctx.ledger.secret
        if not self._signature_ok(self.headers.get("Meridian-Signature"), secret, raw):
            self.ctx.ledger.bump("rejected")
            self._error(401, "bad_signature", "webhook signature missing or invalid")
            return
        if not isinstance(body, dict) or "id" not in body or "data" not in body:
            self.ctx.ledger.bump("rejected")
            self._error(400, "bad_request", "malformed event payload")
            return
        outcome = self.ctx.store.apply_event(body)
        if outcome == "applied":
            self.ctx.ledger.bump("applied")
        else:
            self.ctx.ledger.bump("ignored")
        self._json(200, {"received": True})

    @staticmethod
    def _signature_ok(header: Optional[str], secret: Optional[str], raw: bytes) -> bool:
        if not header or not secret:
            return False
        parts = dict(part.split("=", 1) for part in header.split(",") if "=" in part)
        stamp, signature = parts.get("t"), parts.get("v1")
        if not stamp or not signature:
            return False
        expected = hmac.new(secret.encode(), f"{stamp}.".encode() + raw,
                            hashlib.sha256).hexdigest()
        return hmac.compare_digest(expected, signature)


def register_webhook(ctx: AppContext, port: int, attempts: int = 5, delay: float = 1.0) -> bool:
    """Register with the vendor AFTER the server is listening; tolerate a briefly unreachable
    vendor at boot (the app serves local data regardless)."""
    url = f"http://127.0.0.1:{port}/api/webhooks/meridian"
    for i in range(attempts):
        try:
            result = ctx.client.register_webhook(url)
            with ctx.ledger.lock:
                ctx.ledger.secret = result["secret"]
                ctx.ledger.registered = True
            return True
        except MeridianError:
            if i < attempts - 1:
                threading.Event().wait(delay)
    return False


def serve(port: int, store: Store, client: MeridianClient) -> Tuple[ThreadingHTTPServer, AppContext]:
    ctx = AppContext(store, client)
    handler = type("BoundHandler", (Handler,), {"ctx": ctx})
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, ctx
