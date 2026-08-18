from __future__ import annotations

import datetime as dt
import hashlib
import hmac
import json
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

from .meridian import MeridianClient, MeridianConflict, MeridianError
from .store import STATUSES, Store

CURRENCIES = ("EUR", "USD", "JPY", "KWD")
SORTS = ("created_at", "-created_at", "amount_minor", "-amount_minor")
WEB_ROOT = Path(__file__).with_name("web")
MAX_BODY = 1_048_576
RFC3339 = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$")


def now_utc() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def valid_rfc3339(value: object) -> bool:
    if not isinstance(value, str) or not RFC3339.fullmatch(value):
        return False
    try:
        return dt.datetime.fromisoformat(value.replace("Z", "+00:00")).tzinfo is not None
    except ValueError:
        return False


def serve(port: int, store: Store, client: MeridianClient):
    state = {"secret": None, "registered": False, "received": 0, "applied": 0, "ignored": 0, "rejected": 0}
    state_lock = threading.Lock()
    sync_lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, _format, *args):
            pass

        def send_json(self, code: int, body: dict) -> None:
            raw = json.dumps(body, separators=(",", ":"), allow_nan=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(raw)

        def error(self, code: str, message: str, fields: list[dict] | None = None) -> None:
            detail = {"code": code, "message": message}
            if fields:
                detail["field_errors"] = fields
            status = 400 if fields else {"not_found": 404, "conflict": 409, "bad_signature": 401, "vendor_unavailable": 503}.get(code, 400)
            self.send_json(status, {"error": detail})

        def read_raw(self) -> bytes | None:
            try:
                value = self.headers.get("Content-Length")
                if value is None:
                    return b""
                length = int(value)
                if length < 0 or length > MAX_BODY:
                    return None
                return self.rfile.read(length)
            except ValueError:
                return None

        @staticmethod
        def parse_json(raw: bytes | None):
            if raw is None:
                return None
            try:
                return json.loads(raw)
            except (UnicodeDecodeError, json.JSONDecodeError):
                return None

        def do_GET(self):
            parsed = urlparse(self.path)
            if parsed.path in ("/", "/index.html"):
                return self.static("index.html")
            if parsed.path in ("/styles.css", "/app.js", "/viz.js"):
                return self.static(parsed.path[1:])
            query = parse_qs(parsed.query, keep_blank_values=True)
            if parsed.path == "/api/health":
                with state_lock:
                    webhook = {key: state[key] for key in ("registered", "received", "applied", "ignored", "rejected")}
                return self.send_json(200, {"status": "ok", "payments": store.count(), "last_sync": store.last_sync(), "webhook": webhook})
            if parsed.path == "/api/summary":
                return self.send_json(200, store.summary())
            if parsed.path == "/api/buckets":
                cells = store.buckets()
                days = list(dict.fromkeys(cell["day"] for cell in cells))
                return self.send_json(200, {"timezone": "Europe/Berlin", "days": days, "statuses": list(STATUSES), "cells": cells})
            if parsed.path == "/api/payments":
                return self.payments(query)
            parts = parsed.path.split("/")
            if len(parts) == 4 and parts[:3] == ["", "api", "payments"] and parts[3]:
                payment = store.get(parts[3])
                return self.send_json(200, payment) if payment else self.error("not_found", "Payment was not found")
            self.error("not_found", "Path was not found")

        def static(self, filename: str):
            path = WEB_ROOT / filename
            if not path.is_file():
                return self.error("not_found", "Asset was not found")
            raw = path.read_bytes()
            content_types = {"index.html": "text/html; charset=utf-8", "styles.css": "text/css; charset=utf-8", "app.js": "application/javascript; charset=utf-8", "viz.js": "application/javascript; charset=utf-8"}
            self.send_response(200)
            self.send_header("Content-Type", content_types[filename])
            self.send_header("Content-Length", str(len(raw)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(raw)

        def payments(self, query):
            fields = []

            def number(name: str, default: int) -> int:
                raw = query.get(name, [str(default)])[0]
                try:
                    result = int(raw)
                except (TypeError, ValueError):
                    fields.append({"path": name, "code": "not_an_integer"})
                    return default
                if result < 0:
                    fields.append({"path": name, "code": "not_positive"})
                    return default
                return result

            limit, offset = number("limit", 50), number("offset", 0)
            limit = min(limit, 200)
            status = query.get("status", [None])[0]
            currency = query.get("currency", [None])[0]
            sort = query.get("sort", ["created_at"])[0]
            if status is not None and status not in STATUSES:
                fields.append({"path": "status", "code": "unsupported"})
            if currency is not None and currency not in CURRENCIES:
                fields.append({"path": "currency", "code": "unsupported"})
            if sort not in SORTS:
                fields.append({"path": "sort", "code": "unsupported"})
            if fields:
                return self.error("bad_request", "One or more parameters are invalid", fields)
            rows, total = store.query(limit, offset, status, currency, sort)
            self.send_json(200, {"data": rows, "total": total, "limit": limit, "offset": offset})

        def method_not_allowed(self):
            self.error("bad_request", "HTTP method is not supported")

        def do_PUT(self):
            self.method_not_allowed()

        def do_DELETE(self):
            self.method_not_allowed()

        def do_PATCH(self):
            self.method_not_allowed()

        def do_HEAD(self):
            self.method_not_allowed()

        def do_OPTIONS(self):
            self.method_not_allowed()

        def do_POST(self):
            parsed = urlparse(self.path)
            if parsed.path == "/api/webhooks/meridian":
                return self.webhook()
            if parsed.path == "/api/sync":
                # Consume a possible body so an HTTP/1.1 keep-alive connection stays aligned.
                if self.read_raw() is None:
                    return self.error("bad_request", "Request body is too large", [{"path": "body", "code": "bad_format"}])
                return self.sync()
            raw = self.read_raw()
            body = self.parse_json(raw)
            if body is None:
                return self.error("bad_request", "Request body must be JSON", [{"path": "body", "code": "bad_format"}])
            if parsed.path == "/api/payments/batch":
                return self.batch(body)
            parts = parsed.path.split("/")
            if len(parts) == 5 and parts[:3] == ["", "api", "payments"] and parts[3] and parts[4] == "note":
                return self.note(parts[3], body)
            self.error("not_found", "Path was not found")

        def sync(self):
            with sync_lock:
                try:
                    payments = client.fetch_all_payments()
                    inserted, updated = store.upsert_many(payments)
                    store.set_last_sync(now_utc())
                    self.send_json(200, {"fetched": len(payments), "inserted": inserted, "updated": updated, "total": store.count()})
                except MeridianError:
                    self.error("vendor_unavailable", "Meridian is temporarily unavailable")
                except Exception:
                    self.error("vendor_unavailable", "Meridian returned invalid data")

        def note(self, payment_id: str, body):
            note = body.get("note") if isinstance(body, dict) else None
            fields = []
            if not isinstance(note, str):
                fields.append({"path": "note", "code": "required"})
            elif not note:
                fields.append({"path": "note", "code": "required"})
            elif not note.strip():
                fields.append({"path": "note", "code": "bad_format"})
            elif len(note) > 280:
                fields.append({"path": "note", "code": "too_long"})
            if fields:
                return self.error("bad_request", "Note is invalid", fields)
            old = store.get(payment_id)
            if not old:
                return self.error("not_found", "Payment was not found")
            try:
                updated = client.update_payment(payment_id, {"note": note}, old["version"])
            except MeridianConflict:
                return self.error("conflict", "Payment changed concurrently")
            except MeridianError:
                return self.error("vendor_unavailable", "Meridian is temporarily unavailable")
            store.upsert_many([updated])
            persisted = store.get(payment_id)
            if not persisted or persisted["version"] > updated["version"]:
                return self.error("conflict", "Payment changed concurrently")
            self.send_json(200, {"id": updated["id"], "note": updated["note"], "version": updated["version"]})

        def batch(self, body):
            items = body.get("items") if isinstance(body, dict) else None
            if items is None:
                return self.error("bad_request", "Items are required", [{"path": "items", "code": "required"}])
            if not isinstance(items, list):
                return self.error("bad_request", "Items must be an array", [{"path": "items", "code": "bad_format"}])
            if not items:
                return self.error("bad_request", "Items are required", [{"path": "items", "code": "required"}])
            if len(items) > 20:
                return self.error("bad_request", "Too many items", [{"path": "items", "code": "too_long"}])
            fields = []
            for index, item in enumerate(items):
                prefix = f"items[{index}]"
                if not isinstance(item, dict):
                    fields.append({"path": prefix, "code": "bad_format"})
                    continue
                amount = item.get("amount")
                if not isinstance(amount, dict):
                    fields.append({"path": prefix + ".amount", "code": "required" if amount is None else "bad_format"})
                else:
                    value, currency = amount.get("value_minor"), amount.get("currency")
                    if value is None:
                        fields.append({"path": prefix + ".amount.value_minor", "code": "required"})
                    elif isinstance(value, bool) or not isinstance(value, int):
                        fields.append({"path": prefix + ".amount.value_minor", "code": "not_an_integer"})
                    elif value <= 0:
                        fields.append({"path": prefix + ".amount.value_minor", "code": "not_positive"})
                    if currency is None:
                        fields.append({"path": prefix + ".amount.currency", "code": "required"})
                    elif not isinstance(currency, str) or currency not in CURRENCIES:
                        fields.append({"path": prefix + ".amount.currency", "code": "unsupported"})
                counterparty = item.get("counterparty")
                if not isinstance(counterparty, dict):
                    fields.append({"path": prefix + ".counterparty", "code": "required" if counterparty is None else "bad_format"})
                else:
                    name, country = counterparty.get("name"), counterparty.get("country")
                    if name is None or not isinstance(name, str) or not name:
                        fields.append({"path": prefix + ".counterparty.name", "code": "required"})
                    elif len(name) > 80:
                        fields.append({"path": prefix + ".counterparty.name", "code": "too_long"})
                    if country is None:
                        fields.append({"path": prefix + ".counterparty.country", "code": "required"})
                    elif not isinstance(country, str) or not re.fullmatch(r"[A-Z]{2}", country):
                        fields.append({"path": prefix + ".counterparty.country", "code": "bad_format"})
                occurred_at = item.get("occurred_at")
                if occurred_at is None:
                    fields.append({"path": prefix + ".occurred_at", "code": "required"})
                elif not valid_rfc3339(occurred_at):
                    fields.append({"path": prefix + ".occurred_at", "code": "bad_format"})
                key = item.get("idempotency_key")
                if not isinstance(key, str) or not key:
                    fields.append({"path": prefix + ".idempotency_key", "code": "required"})
            if fields:
                return self.error("bad_request", "One or more items are invalid", fields)
            try:
                outcomes = client.create_batch(items)
            except MeridianError:
                return self.error("vendor_unavailable", "Meridian is temporarily unavailable")
            if len(outcomes) != len(items):
                return self.error("vendor_unavailable", "Meridian returned an invalid batch result")
            results = []
            for index, outcome in enumerate(outcomes):
                if not isinstance(outcome, dict):
                    return self.error("vendor_unavailable", "Meridian returned an invalid batch result")
                if outcome.get("status") == "created" or isinstance(outcome.get("id"), str):
                    payment_id = outcome.get("id")
                    if not isinstance(payment_id, str):
                        return self.error("vendor_unavailable", "Meridian returned an invalid batch result")
                    results.append({"index": index, "status": "created", "id": payment_id})
                else:
                    item_error = outcome.get("error")
                    if not isinstance(item_error, dict) or not isinstance(item_error.get("code"), str) or not isinstance(item_error.get("message"), str):
                        return self.error("vendor_unavailable", "Meridian returned an invalid batch result")
                    results.append({"index": index, "status": "error", "error": {"code": item_error["code"], "message": item_error["message"]}})
            succeeded = sum(result["status"] == "created" for result in results)
            self.send_json(200, {"results": results, "succeeded": succeeded, "failed": len(results) - succeeded})

        def webhook(self):
            raw = self.read_raw()
            event = self.parse_json(raw)
            if isinstance(event, dict) and event.get("type") == "webhook.verify" and isinstance(event.get("challenge"), str):
                return self.send_json(200, {"challenge": event["challenge"]})
            with state_lock:
                state["received"] += 1
                secret = state["secret"]
            signature = self.headers.get("Meridian-Signature", "")
            parts = dict(part.strip().split("=", 1) for part in signature.split(",") if "=" in part)
            expected = hmac.new((secret or "").encode(), (parts.get("t", "") + ".").encode() + (raw or b""), hashlib.sha256).hexdigest()
            if not secret or not parts.get("t") or not parts.get("v1") or not hmac.compare_digest(expected, parts["v1"]):
                with state_lock:
                    state["rejected"] += 1
                return self.error("bad_signature", "Webhook signature is invalid")
            fields = []
            if not isinstance(event, dict):
                fields.append({"path": "body", "code": "bad_format"})
            else:
                if event.get("type") != "payment.updated":
                    fields.append({"path": "type", "code": "unsupported"})
                if not isinstance(event.get("id"), str) or not event["id"]:
                    fields.append({"path": "id", "code": "required"})
                data = event.get("data")
                if not isinstance(data, dict):
                    fields.append({"path": "data", "code": "required" if data is None else "bad_format"})
                else:
                    if not isinstance(data.get("id"), str) or not data["id"]:
                        fields.append({"path": "data.id", "code": "required"})
                    version = data.get("version")
                    if version is None:
                        fields.append({"path": "data.version", "code": "required"})
                    elif isinstance(version, bool) or not isinstance(version, int):
                        fields.append({"path": "data.version", "code": "not_an_integer"})
                    elif version <= 0:
                        fields.append({"path": "data.version", "code": "not_positive"})
            if fields:
                with state_lock:
                    state["rejected"] += 1
                return self.error("bad_request", "Webhook event is invalid", fields)
            try:
                result = store.apply_event(event)
            except (KeyError, TypeError, ValueError):
                with state_lock:
                    state["rejected"] += 1
                return self.error("bad_request", "Webhook event is invalid", [{"path": "data", "code": "bad_format"}])
            with state_lock:
                state["applied" if result == "applied" else "ignored"] += 1
            self.send_json(200, {"received": True})

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True, name="vspro-http").start()
    def register() -> None:
        url = f"http://127.0.0.1:{server.server_port}/api/webhooks/meridian"
        for _ in range(5):
            try:
                registration = client.register_webhook(url)
                with state_lock:
                    state["secret"] = registration["secret"]
                    state["registered"] = True
                return
            except MeridianError:
                threading.Event().wait(1)

    threading.Thread(target=register, daemon=True, name="vspro-webhook-registration").start()
    return server
