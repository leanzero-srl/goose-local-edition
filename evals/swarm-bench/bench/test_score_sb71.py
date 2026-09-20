import copy
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path
import tempfile
import json
import subprocess
import sys
import score_sb71 as score


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.raw = {'score': 1.0, 'checks': [
            {'check': name, 'tier': 'X', 'score': 1, 'detail': 'observed'}
            for name in (*score.BACKEND_EXCELLENCE, *score.STRUCTURE_CHECKS, *score.QUALITY_CHECKS)], 'tiers': {},
            'harness_missing': [], 'sched_unreached': []}
        self.rows = [{'check': name, 'tier': tier, 'score': 1, 'detail': 'observed pixels'}
                     for name, tier in score.VISUAL_CHECKS.items()]

    def fail(self, name, value=0):
        rows = copy.deepcopy(self.rows)
        next(row for row in rows if row['check'] == name)['score'] = value
        return score.admit(self.raw, rows)

    def test_each_required_leg_independently_prevents_next_band(self):
        for name, ceiling in [('s_visible_surface', .599), ('s_tower_geometry', .699),
                              ('s_currency_collar', .699), ('q_payment_context', .799),
                              ('q_legible_presentation', .799), ('m_committed_event_replay', .899)]:
            with self.subTest(name=name):
                result = self.fail(name)
                self.assertEqual(result['score'], ceiling)
                self.assertLess(round(result['score'] * 100, 1), round((ceiling + .001) * 100, 1))

    def test_backend_and_visual_excellence_both_required(self):
        for name in score.BACKEND_EXCELLENCE:
            raw = copy.deepcopy(self.raw)
            next(r for r in raw['checks'] if r['check'] == name)['score'] = .99
            with self.subTest(name=name):
                self.assertEqual(score.admit(raw, self.rows)['score'], .899)
        self.assertEqual(self.fail('m_committed_event_replay')['score'], .899)
        self.assertEqual(score.admit(self.raw, self.rows)['score'], 1.0)

    def test_pretty_inspector_cannot_hide_broken_full_payment_field(self):
        for names, ceiling in [(score.STRUCTURE_CHECKS, .699), (score.QUALITY_CHECKS, .799)]:
            for name in names:
                raw = copy.deepcopy(self.raw)
                next(r for r in raw['checks'] if r['check'] == name)['score'] = 0
                with self.subTest(name=name):
                    self.assertEqual(score.admit(raw, self.rows)['score'], ceiling)

    def test_gates_never_award_credit_or_mutate_input(self):
        self.raw['score'] = .22
        before = copy.deepcopy(self.raw)
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .22)
        self.assertEqual(self.raw, before)

    def test_unavailable_probe_refuses_official_score(self):
        self.raw['probe_unavailable'] = ['t_pick_real_pass']
        with tempfile.TemporaryDirectory() as tmp, patch.object(score.base, 'evaluate', return_value=self.raw):
            with self.assertRaisesRegex(RuntimeError, 'required benchmark observations unavailable'):
                score.evaluate(SimpleNamespace(probes={}, root=Path(tmp), fixture_seed='123456789abcdef0'))
            diagnostic = json.loads((Path(tmp) / 'scoring-unavailable.json').read_text())
            self.assertNotIn('score', diagnostic)
            self.assertEqual(diagnostic['partial_checks'], self.raw['checks'])

    def test_visual_mutation_avoids_actual_backend_schedule_targets(self):
        import fixtures_v3
        from dataclasses import asdict
        fixture = fixtures_v3.build('5a05d7631d9276e3')
        ids = score.reserved_payment_ids(asdict(fixture.schedule), set(fixture.index()))
        self.assertIn(fixture.schedule.ooo_pair['payment_id'], ids)
        self.assertIn(fixture.schedule.forged_event['payment_id'], ids)
        self.assertLess(len(ids), len(fixture.payments))

    def test_missing_partial_and_vacuous_evidence_cannot_admit(self):
        self.assertEqual(score.admit(self.raw, [])['score'], .599)
        self.assertEqual(self.fail('s_tower_geometry', .999)['score'], .699)
        self.raw['checks'][0]['parts'] = {'vacuous_root': 'sync_completeness'}
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .899)
        self.raw['checks'][0].pop('parts')
        self.raw['sched_unreached'] = ['outbox_kill']
        self.assertEqual(score.admit(self.raw, self.rows)['score'], .899)


class ScorerRuntimeTests(unittest.TestCase):
    def test_only_observed_app_absence_replaces_unavailability(self):
        absent = lambda _: score.base.unavail('observation not reached')
        ctx = SimpleNamespace(workflow={}, api_lat={}, stream_head={}, probes={}, sb71_endpoint_absences=[])
        names = ['r_workflow_durability', 'r_notification_multiset', 'p_api_latency',
                 'p_stream_apply', 't_stream_diff', 'e_stream_apply_latency', 't_vs7dbg_truth']
        for name in names:
            self.assertTrue(score.observed_absence_result(name, absent, ctx)['unavailable'])
        ctx.workflow = {'create_status': 501}
        ctx.api_lat = {'payments': {'n_ok': 0, 'errors': ['status 501']}}
        ctx.stream_head = {'status': 501}
        ctx.sb71_endpoint_absences = [{'url': 'http://localhost:123/notify/notifications?limit=200',
                                      'status': 501, 'body': {'error': {'code': 'not_implemented'}}}]
        ctx.probes = {'viz': {'debugSurfaceObservations': [
            {'present': False, 'evaluationSucceeded': True},
            {'present': False, 'evaluationSucceeded': True}]}}
        for name in names:
            result = score.observed_absence_result(name, absent, ctx)
            self.assertEqual(result['score'], 0)
            self.assertIn('absent_surface', result['parts'])
            self.assertNotIn('unavailable', result)
        for malformed in [[], 'error', {'error': 'not_implemented'}]:
            ctx.sb71_endpoint_absences[0]['body'] = malformed
            self.assertTrue(score.observed_absence_result('r_notification_multiset', absent, ctx)['unavailable'])
        ctx.probes['viz']['debugSurfaceObservations'][0] = {'evaluationSucceeded': False}
        self.assertTrue(score.observed_absence_result('t_vs7dbg_truth', absent, ctx)['unavailable'])
        ctx.api_lat['payments']['errors'] = ['TimeoutError']
        ctx.stream_head = {'status': None, 'error': 'HTTPError'}
        ctx.probes = {'viz': {'timedOut': True}}
        ctx.sb71_endpoint_absences[0]['url'] = 'http://localhost:123/wrong'
        for name in names[1:]:
            self.assertTrue(score.observed_absence_result(name, absent, ctx)['unavailable'])

    def test_real_stream_http_status_is_preserved(self):
        import threading
        from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
        class Missing(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(501)
                self.end_headers()
                self.wfile.write(b'not implemented')
            def log_message(self, *_):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Missing)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            with score.probe_runtime():
                observed = score.base._sse_head('http://127.0.0.1:' + str(server.server_port))
            self.assertEqual(observed['status'], 501)
            self.assertEqual(observed['head'], 'not implemented')
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_stream_handshake_fires_only_after_ready_and_never_twice(self):
        import threading
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'ready.json'
            fired = threading.Event()
            calls = []
            def fire():
                calls.append('actual mutation')
                fired.set()
                return {'version': 2}
            handshake = score.StreamHandshake(path, fire)
            handshake.start()
            self.assertFalse(fired.is_set())
            staged = path.with_suffix('.pending')
            staged.write_text(json.dumps({'state': 'armed'}))
            staged.replace(path)
            self.assertTrue(fired.wait(2))
            self.assertEqual(handshake.result(), {'version': 2})
            self.assertEqual(handshake.result(), {'version': 2})
            handshake.finish()
            self.assertEqual(calls, ['actual mutation'])
            handshake.finish()

    def test_unstarted_stream_handshake_cleanup_is_safe(self):
        handshake = score.StreamHandshake(Path('/unused'), lambda: None)
        handshake.finish()
        handshake.finish()

    def test_stream_handshake_missing_or_bad_witness_is_loud(self):
        for state in [None, 'witness_unavailable']:
            with tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / 'ready.json'
                if state:
                    path.write_text(json.dumps({'state': state}))
                handshake = score.StreamHandshake(path, lambda: self.fail('must not mutate'))
                handshake.start()
                if state:
                    self.assertTrue(handshake.done.wait(2))
                handshake.finish()
                with self.assertRaisesRegex(RuntimeError, 'SB7.1 stream'):
                    handshake.result()

    def test_zero_label_offset_is_not_missing_and_missing_stays_missing(self):
        for dy, expected in [(0, 1), (None, .9), (3, .9), (float('nan'), .9)]:
            ctx = SimpleNamespace(probes={'viz': {'labels': {'perLabel': [{'dx': 0, 'dy': dy}]}}})
            original = lambda _: {'score': .9, 'parts': {'offset': 0}, 'detail': 'observed'}
            with self.subTest(dy=dy):
                self.assertEqual(score.label_offset_result(original, ctx)['score'], expected)

    def test_chatty_app_completes_and_full_output_is_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            with score.probe_runtime():
                child = score.base.subprocess.Popen(
                    [sys.executable, '-c', "import sys;sys.stdout.write('x'*(2*1024*1024));sys.stdout.flush()"],
                    cwd=tmp, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                    text=True, start_new_session=True)
                try:
                    self.assertEqual(child.wait(timeout=5), 0)
                    self.assertEqual(child.stdout.read(), 'x' * (2 * 1024 * 1024))
                    probe = score.base.subprocess.run([sys.executable, '-c', "print('probe capture')"],
                                                     capture_output=True, text=True)
                    self.assertEqual(probe.stdout, 'probe capture\n')
                finally:
                    score._kill_owned(child)
            self.assertIs(score.base.subprocess, subprocess)

    def test_only_explicit_not_implemented_short_circuits_sync_wait(self):
        with patch.object(score.base, '_wait_total', return_value=(True, 12, 100)) as original:
            with score.probe_runtime():
                with patch.object(score.base, '_get', return_value=(501, {}, b'', {})):
                    self.assertEqual(score.base._wait_total('http://app', 100, 420), (False, None, None))
                    original.assert_not_called()
                with patch.object(score.base, '_get', return_value=(503, {}, b'', {})):
                    self.assertEqual(score.base._wait_total('http://app', 100, 420), (True, 12, 100))
                    original.assert_called_once_with('http://app', 100, 420)


if __name__ == '__main__':
    unittest.main()
