#!/usr/bin/env python3
"""Local TLS/WSS transport fixture. Uses only disposable, repository test keys."""
import base64
import hashlib
import http.server
import json
import signal
import ssl
import struct
import sys
import threading
from pathlib import Path

FIXTURES = Path(__file__).resolve().parents[1] / "GooseRemoteTests" / "Fixtures"


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer disposable-test-token":
            self.send_response(401)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.path == "/redirect":
            self.send_response(302)
            self.send_header("Location", "https://127.0.0.1:42385/should-not-follow")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        payload = json.dumps({"proof": "native-owner-tls", "owner_node_id": "11111111-1111-4111-8111-111111111111"}).encode()
        if self.headers.get("Upgrade", "").lower() == "websocket":
            key = self.headers.get("Sec-WebSocket-Key", "")
            accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()
            if self.path.startswith("/close/"):
                payload = struct.pack("!H", int(self.path.rsplit("/", 1)[1]))
                self.wfile.write(b"\x88" + bytes([len(payload)]) + payload)
                self.wfile.flush()
                self.connection.recv(4096)
                return
            self.wfile.write(b"\x81" + bytes([len(payload)]) + payload)
            self.wfile.flush()
            # Keep the socket alive until the client cancels; no remote work exists.
            self.connection.recv(4096)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_):
        pass


servers = []
for index, name in enumerate(["valid", "expired", "future", "client-only", "wrong-owner", "renewed"]):
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 42385 + index), Handler)
    server.daemon_threads = True
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(str(FIXTURES / (name + ".pem")), str(FIXTURES / "disposable-key.pem"))
    server.socket = context.wrap_socket(server.socket, server_side=True)
    servers.append(server)
    threading.Thread(target=server.serve_forever, daemon=True).start()


def finish(*_):
    for server in servers:
        server.shutdown()
        server.server_close()
    sys.exit(0)


signal.signal(signal.SIGTERM, finish)
signal.signal(signal.SIGINT, finish)
print("READY", flush=True)
signal.pause()
