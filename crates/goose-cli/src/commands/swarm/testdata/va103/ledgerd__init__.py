"""ledgerd — Meridian payments ledger service (boot + walking-skeleton server).

Boot contract: ``python3 -m app.ledgerd --db-dir P --port N --notifier URL
--vendor URL --tokens-file T`` binds 127.0.0.1:<port> and serves immediately,
before any vendor or cross-service call.

Contract for the real implementation (``app/ledgerd/impl.py``, owned by the
ledgerd-core task): expose a blocking ``run(db_dir, port, notifier_url,
vendor_url, tokens_file)`` that binds first and serves forever. Until that
module exists (or imports cleanly), this package serves the walking skeleton:
every advertised API route answers 501 with the JSON error envelope, unknown
paths answer 404 with the ``not_found`` envelope, and ``GET /`` + static paths
serve the frontend files from ``web/`` (an inline shell page stands in for
``index.html`` until the console-page task lands).
"""

import json
import os
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

HOST = "127.0.0.1"
WEB_DIR = str(Path(__file__).resolve().parents[2] / "web")

# Advertised ledgerd API routes (method, regex). Each answers 501 in the
# skeleton until its owning module fills it in; none may ever answer 404.
ROUTES = (
    ("GET", r"^/api/health$"),
    ("GET", r"^/api/payments$"),
    ("GET", r"^/api/payments/[^/]+$"),
    ("GET", r"^/api/summary$"),
    ("GET", r"^/api/buckets$"),
    ("POST", r"^/api/sync$"),
    ("POST", r"^/api/payments/[^/]+/note$"),
    ("POST", r"^/api/webhooks/meridian$"),
    ("GET", r"^/api/events$"),
    ("GET", r"^/api/outbox/status$"),
    ("GET", r"^/api/notifications$"),
    ("GET", r"^/api/viz/records$"),
    ("GET", r"^/api/stream$"),
    ("POST", r"^/api/drafts$"),
    ("GET", r"^/api/drafts$"),
    ("POST", r"^/api/drafts/[^/]+/(submit|approve|reject)$"),
)

_CONTENT_TYPES = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "application/javascript; charset=utf-8",
}

_SHELL_PAGE = b"""<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Meridian Payments Console</title></head>
<body>
<header id="app-header"><h1>Meridian Payments Console</h1></header>
<main><p id="notice" role="status">Console shell \u2014 frontend assets pending.</p></main>
</body>
</html>
"""


def _static_response(path):
    """Return (status, content_type, body) for a static GET, or None."""
    rel = "index.html" if path == "/" else path.lstrip("/")
    fp = os.path.normpath(os.path.join(WEB_DIR, rel))
    if os.path.commonpath([WEB_DIR, fp]) != WEB_DIR:
        return None  # traversal attempt outside web/
    if os.path.isfile(fp):
        ctype = _CONTENT_TYPES.get(
            os.path.splitext(fp)[1].lower(), "application/octet-stream"
        )
        with open(fp, "rb") as fh:
            return 200, ctype, fh.read()
    if path == "/":
        return 200, "text/html; charset=utf-8", _SHELL_PAGE
    return None


class SkeletonHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # keep boot output quiet
        pass

    # -- helpers ---------------------------------------------------------
    def _drain_body(self):
        try:
            remaining = int(self.headers.get("Content-Length") or 0)
        except ValueError:
            remaining = 0
        while remaining > 0:
            chunk = self.rfile.read(min(remaining, 65536))
            if not chunk:
                break
            remaining -= len(chunk)

    def _send(self, status, ctype, body):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _send_json(self, status, obj):
        self._send(status, "application/json", json.dumps(obj).encode("utf-8"))

    @staticmethod
    def _envelope(code, message):
        return {"error": {"code": code, "message": message}}

    # -- dispatch --------------------------------------------------------
    def _handle(self):
        self._drain_body()
        path = urlsplit(self.path).path
        if self.command in ("GET", "HEAD") and not path.startswith("/api/"):
            static = _static_response(path)
            if static is not None:
                return self._send(*static)
        for method, pattern in ROUTES:
            if method == self.command and re.match(pattern, path):
                return self._send_json(
                    501,
                    self._envelope(
                        "not_implemented",
                        f"{self.command} {path} is not implemented yet",
                    ),
                )
        return self._send_json(
            404, self._envelope("not_found", f"no such route: {self.command} {path}")
        )

    def do_GET(self):
        self._handle()

    def do_HEAD(self):
        self._handle()

    def do_POST(self):
        self._handle()


class SkeletonServer(ThreadingHTTPServer):
    daemon_threads = True


def run(db_dir, port, notifier_url=None, vendor_url=None, tokens_file=None):
    """Bind 127.0.0.1:<port> and serve; blocks until the process exits."""
    os.makedirs(db_dir, exist_ok=True)
    try:
        from . import impl  # real service lands here (ledgerd-core task)
    except ImportError as exc:
        print(
            f"[ledgerd] impl unavailable ({exc}); serving walking skeleton",
            file=sys.stderr,
        )
        impl = None
    if impl is not None and hasattr(impl, "run"):
        return impl.run(
            db_dir=db_dir,
            port=port,
            notifier_url=notifier_url,
            vendor_url=vendor_url,
            tokens_file=tokens_file,
        )
    server = SkeletonServer((HOST, int(port)), SkeletonHandler)
    print(f"[ledgerd] skeleton listening on http://{HOST}:{port}", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
