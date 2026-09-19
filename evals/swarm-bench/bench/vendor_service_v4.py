"""SB-8 immutable scene vendor. No business services or long-running fault choreography."""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from gantry_oracle import scene
API_KEY='not-required'
DOCS_PATH='/scene'
def mark_phase(*_args): pass

def serve(port,trace,seed=1):
    trace=Path(trace);trace.parent.mkdir(parents=True,exist_ok=True)
    trace.write_text(json.dumps({'fixture_seed':seed})+'\n')
    payload=json.dumps(scene(seed)).encode()
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args): pass
        def do_GET(self):
            if self.path=='/scene': data,kind=payload,'application/json'
            elif self.path=='/three.js':
                data=Path(__file__).with_name('sb8-three.module.js').read_bytes();kind='application/javascript'
            else: self.send_error(404);return
            self.send_response(200);self.send_header('Content-Type',kind);self.send_header('Access-Control-Allow-Origin','*');self.end_headers();self.wfile.write(data)
    server=ThreadingHTTPServer(('127.0.0.1',port),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    return server
