"""run_build grades against a FRESH vendor, never the one the entrant used while building.

Run 43896353 (2026-10-02) scored 0.699 in the app and 0.799 hermetically on the same tree and seed:
the in-app scorer graded the build vendor's trace and state, so the entrant's own session walks,
fault hits, sends and a dead webhook registration were graded as the app's behavior.
"""
import json
import os
import socket
import threading
import tempfile
import unittest
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import run_build
import score_sb7
import vendor_service_v3

SEED = '0123456789abcdef'


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def request(method, url, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={
        'Authorization': f'Bearer {vendor_service_v3.API_KEY}', 'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=10) as response:
        return response.status


class ChallengeReceiver(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):  # noqa: N802
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        payload = json.dumps({'challenge': body.get('challenge')}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def rows(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


class ScoringVendorIsFresh(unittest.TestCase):
    def test_build_session_traffic_and_webhook_never_reach_the_graded_vendor(self):
        port = free_port()
        seen = {}

        def build_session(entrant, workdir, vendor_port, *args):
            base = f'http://127.0.0.1:{vendor_port}'
            self.assertEqual(request('GET', base + vendor_service_v3.LIST_PATH), 200)
            receiver = ThreadingHTTPServer(('127.0.0.1', 0), ChallengeReceiver)
            threading.Thread(target=receiver.serve_forever, daemon=True).start()
            hook = f'http://127.0.0.1:{receiver.server_port}/api/webhooks/meridian'
            self.assertEqual(request('POST', base + '/v3/webhooks', {'url': hook}), 200)
            receiver.shutdown()
            receiver.server_close()
            seen['hook'] = hook
            return {'exit': 0, 'timed_out': False, 'secs': 1.0, 'tail': 'done', 'reaped_processes': []}

        def gather(root, vendor_port, db, trace_path, mark_phase=None, seed=None):
            graded = rows(Path(trace_path))
            seen['at_gather'] = graded
            seen['webhook_at_gather'] = vendor_service_v3.STATE.webhook
            seen['seed_at_gather'] = vendor_service_v3.STATE.seed
            mark_phase('boot')
            self.assertEqual(request('GET', f'http://127.0.0.1:{vendor_port}' + vendor_service_v3.LIST_PATH), 200)
            return SimpleNamespace()

        scorer = SimpleNamespace(_port_holder=score_sb7._port_holder, _probe_preflight=lambda: None,
                                 _draw_seed=lambda: SEED, gather=gather, evaluate=lambda ctx: {'score': 0.0})
        with tempfile.TemporaryDirectory() as tmp, \
                patch.dict(os.environ, {'BENCH_SB7': '1'}, clear=True), \
                patch.object(run_build, '_regime', return_value=(scorer, vendor_service_v3, None)), \
                patch.object(run_build, 'load_env', return_value={}), \
                patch.object(run_build, 'invoke', side_effect=build_session):
            out = Path(tmp)
            run_build.run('controlled', 0, out, 0, port)
            self.assertIsNone(seen['webhook_at_gather'], 'the build session webhook reached the graded vendor')
            self.assertEqual([r.get('fixture_seed') for r in seen['at_gather']], [SEED],
                             'the graded trace carried build-session rows before boot')
            workdir = out / 'controlled-r0'
            build_rows = rows(workdir / run_build.BUILD_VENDOR_TRACE)
            graded_rows = rows(workdir / 'trace.jsonl')
            shared = (out / 'trace-controlled-r0.jsonl').read_bytes()
            graded_bytes = (workdir / 'trace.jsonl').read_bytes()
            port_after = score_sb7._port_holder(port)

        self.assertEqual(seen['seed_at_gather'], SEED)
        self.assertTrue(any(r.get('webhook_url') == seen['hook'] for r in build_rows), build_rows)
        self.assertTrue(any(r.get('path') == vendor_service_v3.LIST_PATH for r in build_rows))
        self.assertEqual(graded_rows[0].get('fixture_seed'), SEED)
        self.assertFalse(any('webhook_url' in r for r in graded_rows), graded_rows)
        self.assertEqual([r.get('__phase__') for r in graded_rows if '__phase__' in r], ['boot'])
        before_boot = graded_rows[1:next(i for i, r in enumerate(graded_rows) if '__phase__' in r)]
        self.assertEqual(before_boot, [])
        self.assertTrue(any(r.get('path') == vendor_service_v3.LIST_PATH for r in graded_rows))
        self.assertEqual(shared, graded_bytes)
        self.assertIsNone(port_after)

    def test_a_held_port_refuses_the_scoring_vendor_loudly(self):
        port = free_port()
        asked = []

        def holder(candidate):
            asked.append(candidate)
            return None if len(asked) == 1 else f'port {candidate} is held by pid 1 (controlled)'

        scorer = SimpleNamespace(_port_holder=holder, _probe_preflight=lambda: None, _draw_seed=lambda: SEED,
                                 gather=lambda *a, **k: self.fail('graded against no fresh vendor'))
        agent = {'exit': 0, 'timed_out': False, 'secs': 1.0, 'tail': 'done', 'reaped_processes': []}
        with tempfile.TemporaryDirectory() as tmp, \
                patch.dict(os.environ, {'BENCH_SB7': '1'}, clear=True), \
                patch.object(run_build, '_regime', return_value=(scorer, vendor_service_v3, None)), \
                patch.object(run_build, 'load_env', return_value={}), \
                patch.object(run_build, 'invoke', return_value=agent):
            with self.assertRaisesRegex(RuntimeError, 'REFUSED: the scoring vendor cannot bind: port'):
                run_build.run('controlled', 0, Path(tmp), 0, port)
            workdir = Path(tmp) / 'controlled-r0'
            self.assertTrue((workdir / run_build.BUILD_VENDOR_TRACE).is_file())
            self.assertEqual(rows(workdir / 'trace.jsonl')[0].get('fixture_seed'), SEED)
        self.assertEqual(asked, [port, port])
        self.assertIsNone(score_sb7._port_holder(port))


if __name__ == '__main__':
    unittest.main()
