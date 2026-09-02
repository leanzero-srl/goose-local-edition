"""notifierd — idempotent event consumer (boot + walking-skeleton server).

Boot contract: ``python3 -m app.notifierd --db-dir P --port M`` binds
127.0.0.1:<port> and serves immediately, before any other-service call.

Contract for the real implementation (``app/notifierd/impl.py``, owned by
the notifierd task): expose a blocking ``run(db_dir, port)`` that binds
first and serves forever. Until that module exists (or imports cleanly),
this package serves the walking skeleton: every advertised route answers
501 with the JSON error envelope, unknown paths answer 404 with the
``not_found`` envelope.
"""

import json
import os
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

HOST = "127.0.0.1"

# Advertised notifierd routes (method, regex). Each answers 501 in the
# skeleton until its owning module fills it in; none may ever answer 404.
ROUTES = (
    ("POST", r"^/notify/events$"),
    ("GET", r"^/health$"),
    ("GET", r"^/notify/processed$"),
    ("GET", r"^/notify/notifications$"),
)


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

    def _send_json(self, status, obj):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    @staticmethod
    def _envelope(code, message):
        return {"error": {"code": code, "message": message}}

    # -- dispatch --------------------------------------------------------
    def _handle(self):
        self._drain_body()
        path = urlsplit(self.path).path
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


def run(db_dir, port):
    """Bind 127.0.0.1:<port> and serve; blocks until the process exits."""
    os.makedirs(db_dir, exist_ok=True)
    try:
        from . import impl  # real service lands here (notifierd task)
    except ImportError as exc:
        print(
            f"[notifierd] impl unavailable ({exc}); serving walking skeleton",
            file=sys.stderr,
        )
        impl = None
    if impl is not None and hasattr(impl, "run"):
        return impl.run(db_dir=db_dir, port=port)
    server = SkeletonServer((HOST, int(port)), SkeletonHandler)
    print(f"[notifierd] skeleton listening on http://{HOST}:{port}", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
