"""HTTP plumbing only. No database, vendor, workflow, or consistency implementation."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


class UnimplementedApplication:
    def handle(self, method, target, headers, raw_body):
        raise NotImplementedError('Implement the SB7 application behavior')


def serve(port, application, web_root=None):
    assets = {'/': ('index.html', 'text/html'),
              '/web/index.html': ('index.html', 'text/html'),
              '/web/styles.css': ('styles.css', 'text/css'),
              '/web/app.js': ('app.js', 'text/javascript'),
              '/web/viz.js': ('viz.js', 'text/javascript')}

    class Handler(BaseHTTPRequestHandler):
        def respond(self, status, value, content_type='application/json'):
            body = json.dumps(value).encode() if content_type == 'application/json' else value
            self.send_response(status)
            self.send_header('Content-Type', content_type)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def dispatch(self):
            path = urlsplit(self.path).path
            if self.command == 'GET' and web_root is not None and path in assets:
                name, content_type = assets[path]
                self.respond(200, (Path(web_root) / name).read_bytes(), content_type)
                return
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if length < 0:
                    raise ValueError('Negative Content-Length')
            except ValueError:
                self.respond(400, {'error': {'code': 'bad_request', 'message': 'Invalid Content-Length'}})
                return
            raw_body = self.rfile.read(length)
            try:
                status, value = application.handle(self.command, self.path, self.headers, raw_body)
            except NotImplementedError as error:
                self.respond(501, {'error': {'code': 'not_implemented', 'message': str(error)}})
                return
            self.respond(status, value)

        do_GET = dispatch
        do_POST = dispatch
        do_PATCH = dispatch
        do_DELETE = dispatch

    ThreadingHTTPServer(('127.0.0.1', port), Handler).serve_forever()
