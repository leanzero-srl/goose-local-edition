"""proxy.py <listen> <upstream> <log.jsonl> — forward HTTP to upstream, stream the response back
(close-delimited), and append every POST body to the log."""
import http.client
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

listen, up, log = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
lock = threading.Lock()


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def _fwd(self, body=None):
        if body is not None:
            with lock, open(log, "a") as f:
                f.write(json.dumps({"path": self.path, "body": json.loads(body)}) + "\n")
        c = http.client.HTTPConnection("127.0.0.1", up, timeout=3600)
        hdrs = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection", "accept-encoding")}
        c.request(self.command, self.path, body=body, headers=hdrs)
        r = c.getresponse()
        self.send_response(r.status)
        for k, v in r.getheaders():
            if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                self.send_header(k, v)
        self.send_header("Connection", "close")
        self.end_headers()
        while True:
            chunk = r.read1(65536)
            if not chunk:
                break
            self.wfile.write(chunk)
            self.wfile.flush()

    def do_GET(self):
        self._fwd()

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        self._fwd(self.rfile.read(n))

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("127.0.0.1", listen), H).serve_forever()
