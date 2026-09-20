"""Actual browser readiness regression; requires the supplied benchmark browser environment."""
import unittest
import os


class BrowserReadinessTests(unittest.TestCase):
    @unittest.skipUnless(all(os.environ.get(key) for key in (
        'BENCH_BROWSER_MODULE', 'BENCH_BROWSER_EXECUTABLE', 'GOOSE_SWARM_RENDER_NODE')),
        'explicit benchmark browser paths required')
    def test_open_event_stream_does_not_block_screenshot(self):
        import http.server, threading, tempfile, subprocess, json, os
        from pathlib import Path
        connected=threading.Event()
        closed=threading.Event()
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                if self.path=='/events':
                    self.send_header('Content-Type','text/event-stream'); self.end_headers()
                    self.wfile.write(b'data: ready\n\n'); self.wfile.flush()
                    connected.set(); closed.wait(30)
                else:
                    self.send_header('Content-Type','text/html'); self.end_headers()
                    self.wfile.write(b'<title>Live payments</title><table><tbody><tr><td>p-1</td><td>EUR</td></tr></tbody></table><script>window.stream=new EventSource("/events")</script>')
            def log_message(self,*args): pass
        server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                screenshot=Path(tmp)/'result.png'
                p=subprocess.run([os.environ['GOOSE_SWARM_RENDER_NODE'],str(Path(__file__).with_name('browser-self-test.mjs')),f'http://127.0.0.1:{server.server_port}',str(screenshot)],env=os.environ,capture_output=True,text=True,timeout=20)
                assert p.returncode==0,p.stderr
                data=json.loads(p.stdout)
                assert connected.is_set(),'SSE connection was not exercised'
                assert data['renderedRowCount']==1 and data['readinessError'] is None,data
                assert screenshot.stat().st_size>1000
        finally:
            closed.set(); server.shutdown(); server.server_close()
