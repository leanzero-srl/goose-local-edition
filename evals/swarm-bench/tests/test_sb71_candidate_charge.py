"""Every unavailability the SB7.1/SB7.2 gather traces to the CANDIDATE is charged, never refused.

The law (goose-benchmark-iteration): an entrant's defect scores low; only a harness fault refuses.
One test per converted path, each with the harness-side control that must keep refusing.
"""
import copy
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))
import fixtures_v3  # noqa: E402
import score_sb71 as score  # noqa: E402
import score_sb72  # noqa: E402

base = score.base
SEED = 'feede0d7f2950157'


def check(name):
    return next(fn for n, _t, fn in base.SB7_CHECKS if n == name)


def result(name, ctx, original=None):
    return score.observed_absence_result(name, original or check(name), ctx)


def unavailable(_ctx):
    return base.unavail('observation not reached')


class UnboundLedgerdTests(unittest.TestCase):
    """gather() returns right after boot when ledgerd never binds: every later phase is unreached
    because the candidate never served."""

    def ctx(self, tmp):
        ctx = base.Ctx(Path(tmp))
        fx = fixtures_v3.build(SEED)
        ctx.pack, ctx.schedule, ctx.fixture_seed, ctx.db_dir = fx, fx.schedule, SEED, Path(tmp)
        ctx.boot_error = 'SyntaxError: invalid syntax'
        return ctx

    def test_a_ledgerd_that_never_binds_gets_a_low_verdict(self):
        with tempfile.TemporaryDirectory() as tmp:
            ctx = self.ctx(tmp)
            inherited = {name: score.inherited_observation_result(name, fn, ctx) for name, _t, fn in base.SB7_CHECKS}
            self.assertTrue([n for n, row in inherited.items() if row.get('unavailable')],
                            'the control: without the charge these rows refuse the run')
            verdict = score_sb72.evaluate(ctx)
        self.assertEqual(verdict['probe_unavailable'], [])
        self.assertLess(verdict['score'], .05)
        charged = [r for r in verdict['checks'] if (r.get('parts') or {}).get('vacuous_root') == 'server_runs']
        self.assertIn('r_b3_sigkill_resync', {r['check'] for r in charged})
        self.assertIn('a_combined_entrypoint', {r['check'] for r in charged})
        self.assertEqual([r['check'] for r in verdict['critical']['rows'] if not r.get('suppressed')], ['server_runs'])

    def test_a_bound_ledgerd_keeps_unreached_phases_refusing(self):
        with tempfile.TemporaryDirectory() as tmp:
            ctx = self.ctx(tmp)
            ctx.port_bound = True
            with self.assertRaises(score.UnavailableEvidence):
                score_sb72.evaluate(ctx)


class B3ShortWalkTests(unittest.TestCase):
    """xiaomi/mimo-v2.6-flash, SB7.2 (2026-10-02): the restarted ledgerd fetched page 1 (200, last:false),
    saw the generation it already stored, stopped its walk and slept 45 s; 90 s later the gate had two
    answered list requests of the three the kill waits for. REFUSED before; charged now."""

    def setUp(self):
        saved = json.loads((Path(__file__).parent / 'fixtures/sb72-mimo-b3-short-walk.json').read_text())
        self.ctx = SimpleNamespace(trace=saved['trace'], sb71_resync=saved['sb71_resync'], read_stream=[],
                                   payments=saved['payments'], b3_result=saved['b3_result'],
                                   schedule=SimpleNamespace(sigkill_after_list=saved['target']))

    def test_the_mimo_receipt_is_charged(self):
        self.assertTrue(check('r_b3_sigkill_resync')(self.ctx)['unavailable'])
        row = result('r_b3_sigkill_resync', self.ctx)
        self.assertNotIn('unavailable', row)
        self.assertEqual(row['score'], 0)
        self.assertEqual([r['status'] for r in row['parts']['b3_walk_short']['completed']], [200, 304])
        self.assertEqual([r['last'] for r in row['parts']['b3_walk_short']['sync2_list_requests']], [False, None])
        self.assertIn('2 of the 3 list requests', row['detail'])

    def test_harness_side_receipts_still_refuse(self):
        mutations = {
            'gate_never_armed': lambda c: c.sb71_resync.pop('target'),
            'schedule_disagrees': lambda c: setattr(c.schedule, 'sigkill_after_list', 4),
            'response_held': lambda c: c.sb71_resync.update(held=[{'status': 200}]),
            'kill_recorded': lambda c: c.sb71_resync.update(kill={'pid': 1}),
            'vendor_error_in_receipt': lambda c: c.sb71_resync['completed'][0].update(status=500),
            'trace_disagrees': lambda c: next(e for e in c.trace if e.get('status') == 304).update(status=200),
            'no_sync2_marker': lambda c: c.trace.pop(0),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                ctx = copy.deepcopy(self.ctx)
                mutate(ctx)
                self.assertTrue(result('r_b3_sigkill_resync', ctx)['unavailable'])


def list_get(sync, status, conditional):
    return {'method': 'GET', 'path': '/v3/payments', 'sync': sync, 'status': status,
            **({'if_none_match': '"v"'} if conditional else {})}


class B5UnpresentedTests(unittest.TestCase):
    def ctx(self, trace):
        return SimpleNamespace(trace=trace, b5_result={'armed': False}, payments={'total': 12288},
                               schedule=SimpleNamespace(stale_304_sync=4))

    def test_a_candidate_that_never_revalidates_is_charged(self):
        ctx = self.ctx([list_get(1, 200, False), list_get(2, 200, False), list_get(3, 200, False)])
        walk = result('c_b5_generation_304', ctx)
        self.assertEqual((walk['score'], walk.get('unavailable')), (0, None))
        self.assertEqual(walk['parts']['b5_unpresented']['stale_presenting_syncs'], [])
        truth = result('r_cache_truth', ctx)
        self.assertEqual(truth['score'], 0)
        self.assertIn('absent_surface', truth['parts'])

    def test_enough_stale_validators_without_the_lie_still_refuses(self):
        ctx = self.ctx([list_get(2, 200, True), list_get(3, 200, True)])
        for name in ('c_b5_generation_304', 'r_cache_truth'):
            self.assertTrue(result(name, ctx)['unavailable'])


class PartitionStatusTests(unittest.TestCase):
    def ctx(self, samples):
        return SimpleNamespace(sb71_partition=score.partition_status_evidence(samples),
                               b7_result={'status_down': False})

    def test_a_status_endpoint_that_never_answers_an_object_is_charged(self):
        samples = [{'started': i, 'completed': i + .1, 'status': status, 'body': '<html>'}
                   for i, status in enumerate([404, 404, None, 404])]
        row = result('r_b7_partition', self.ctx(samples))
        self.assertEqual((row['score'], row.get('unavailable')), (0, None))
        self.assertIn('statuses: 404, None', row['detail'])

    def test_a_short_successful_window_still_refuses(self):
        samples = [{'started': 0, 'completed': .1, 'status': 200, 'body': {'notifier': 'up', 'pending': 0}},
                   {'started': .2, 'completed': .3, 'status': None, 'body': None}]
        self.assertTrue(result('r_b7_partition', self.ctx(samples))['unavailable'])


class WorkflowDurabilityTests(unittest.TestCase):
    def test_a_failed_create_is_an_absent_surface(self):
        ctx = SimpleNamespace(workflow={'create_status': 500, 'create_ms': 4.0}, draft_ids={})
        row = result('r_workflow_durability', ctx)
        self.assertEqual(row['score'], 0)
        self.assertIn('HTTP 500', row['parts']['absent_surface'])

    def test_a_draft_never_listed_after_either_restart_is_charged(self):
        ctx = SimpleNamespace(workflow={'create_status': 201, 'submit_status': 200, 'a1_state_after_restart': None,
                                        'a2_state_after_restart': None}, draft_ids={'F3': 'draft_7'})
        row = result('r_workflow_durability', ctx)
        self.assertEqual((row['score'], row.get('unavailable')), (0, None))
        self.assertEqual(row['parts']['draft_id'], 'draft_7')

    def test_an_unexercised_workflow_still_refuses(self):
        self.assertTrue(result('r_workflow_durability', SimpleNamespace(workflow={}, draft_ids={}))['unavailable'])


class NotifierNotificationsTests(unittest.TestCase):
    def ctx(self, reads):
        return SimpleNamespace(notifier_notifications={}, events=[], pack=None, sb71_endpoint_absences=[],
                               sb71_notifier_reads=reads)

    def test_reads_without_a_data_list_are_an_absent_surface(self):
        reads = [{'url': 'http://127.0.0.1:1/notify/notifications?limit=200', 'status': 404, 'data_list': False},
                 {'url': 'http://127.0.0.1:1/notify/notifications?limit=200', 'status': None, 'data_list': False}]
        row = result('r_notification_multiset', self.ctx(reads), unavailable)
        self.assertEqual(row['score'], 0)
        self.assertIn('404, None', row['parts']['absent_surface'])

    def test_unread_endpoint_still_refuses(self):
        self.assertTrue(result('r_notification_multiset', self.ctx([]), unavailable)['unavailable'])

    def test_the_gather_records_every_notifications_read(self):
        reads = []
        with patch.object(base, '_get', return_value=(404, None, b'', {})):
            with score.probe_runtime(reads):
                base._get('http://127.0.0.1:9/notify/notifications?limit=200', timeout=8)
                base._get('http://127.0.0.1:9/notify/processed?after=0', timeout=8)
        self.assertEqual(reads, [{'url': 'http://127.0.0.1:9/notify/notifications?limit=200',
                                  'status': 404, 'data_list': False}])


class ApiLatencyTests(unittest.TestCase):
    def test_http_error_answers_are_charged(self):
        ctx = SimpleNamespace(api_lat={'payments': {'n_ok': 3, 'errors': ['status 500'] * 17},
                                       'summary': {'n_ok': 0, 'errors': ['status 404'] * 20}})
        row = result('p_api_latency', ctx)
        self.assertEqual((row['score'], row.get('unavailable')), (0, None))
        self.assertIn('status 404, status 500', row['detail'])

    def test_timeouts_still_refuse(self):
        ctx = SimpleNamespace(api_lat={'payments': {'n_ok': 0, 'errors': ['status 500'] * 10 + ['TimeoutError'] * 10}})
        self.assertTrue(result('p_api_latency', ctx)['unavailable'])


class ReadStreamTests(unittest.TestCase):
    def ctx(self, **overrides):
        values = dict(read_stream=[], pack=object(), port_bound=True, read_targets=['pay_1'],
                      payments={'total': 12288}, probes={})
        values.update(overrides)
        return SimpleNamespace(**values)

    def test_a_summary_that_never_answers_is_charged(self):
        for name in ('x_l3_monotonic_reads', 'x_l5_group_atomicity', 'x_m2_pair_conservation'):
            with self.subTest(name=name):
                row = result(name, self.ctx())
                self.assertEqual((row['score'], row.get('unavailable')), (0, None))

    def test_a_reader_that_never_started_still_refuses(self):
        for name in ('x_l3_monotonic_reads', 'x_l5_group_atomicity', 'x_m2_pair_conservation'):
            with self.subTest(name=name):
                self.assertTrue(result(name, self.ctx(read_targets=[]))['unavailable'])


class DuplicatedVendorPaymentTests(unittest.TestCase):
    def ctx(self, why):
        flow = {'roleTokenAccepted': {'found': True}, 'draftCreated': {'draftId': 'd1'},
                'submitCausal': {'reached': True}, 'approveCausal': {'reachedApproved': True},
                'draftListStates': {'found': True}, 'notificationSeen': True, 'paymentInTable': False,
                'paymentWitness': {'ok': False, 'unavailable': why}}
        return SimpleNamespace(probes={'flow': flow})

    def test_two_vendor_payments_for_one_approval_are_charged_not_refused(self):
        row = result('j_workflow_journey', self.ctx('ambiguous vendor-created F1 identity: 2'))
        self.assertNotIn('unavailable', row)
        self.assertLess(row['score'], 1)
        self.assertFalse(row['parts']['paymentInTable'])
        self.assertIn('more than once at the vendor', row['detail'])

    def test_a_missing_receipt_channel_still_refuses(self):
        self.assertTrue(result('j_workflow_journey', self.ctx('run-bound committed payment receipt is missing'))['unavailable'])


if __name__ == '__main__':
    unittest.main()


class BurstRejectedTests(unittest.TestCase):
    """qwen/qwen3.8-omni-flash, SB7.2 (2026-10-03): its ledgerd strips 'whsec_' from the HMAC key, so all
    24 burst deliveries were answered 401 within 16 ms; the readers' overlap with that instant burst was
    0.367 and the run was REFUSED as 'load unproven'. The stream load never existed: charged at any overlap."""

    def setUp(self):
        saved = json.loads((Path(__file__).parent / 'fixtures/sb72-omni-flash-burst-rejected.json').read_text())
        self.trace, self.under = saved['trace'], saved['under_stream']

    def ctx(self, trace=None, under=None):
        return SimpleNamespace(trace=copy.deepcopy(self.trace if trace is None else trace),
                               under_stream=dict(self.under if under is None else under))

    def test_the_omni_flash_receipt_is_charged(self):
        for name in ('p_under_stream', 'e_under_load_latency'):
            self.assertTrue(check(name)(self.ctx()).get('unavailable'), 'the control: it refused before')
            row = result(name, self.ctx())
            self.assertEqual(row['score'], 0)
            self.assertNotIn('unavailable', row)
            self.assertIn('HTTP 401 x24 of 24 burst deliveries', row['detail'])
            self.assertEqual(row['parts']['burst_rejected']['burst_ms'], 15.9)

    def test_a_rejected_burst_is_charged_even_when_reads_overlapped_it(self):
        ctx = self.ctx(under={'overlap_frac': 0.9, 'p95_ms': 2.0, 'n': 30, 'correct': True})
        self.assertEqual(check('p_under_stream')(ctx)['score'], 1.0, 'the control: a fast idle read earned 1.0')
        self.assertEqual(result('p_under_stream', ctx)['score'], 0)

    def test_a_never_registered_webhook_is_charged(self):
        trace = [e for e in self.trace if not e.get('webhook_delivery') and 'sched-partial' not in e]
        trace += [{'sched-partial': 'burst', 'event': f'evt_burst_{i:04d}', 'reason': 'no_webhook_registered',
                   't': e['t']} for i, e in enumerate(trace)]
        row = result('p_under_stream', self.ctx(trace=trace))
        self.assertEqual(row['score'], 0)
        self.assertIn('no webhook registered x24', row['detail'])

    def test_harness_side_bursts_still_refuse(self):
        def accepted(trace):
            next(e for e in trace if e.get('webhook_delivery'))['delivery_status'] = 200
        def timed_out(trace):
            first = next(e for e in trace if e.get('webhook_delivery'))
            first.update(delivery_status=None, delivery_error='TimeoutError')
        def lost(trace):
            trace.remove(next(e for e in trace if e.get('webhook_delivery')))
        for name, edit in [('one delivery acked', accepted), ('a transport timeout', timed_out),
                           ('a delivery missing from the trace', lost),
                           ('no burst fired', lambda trace: trace.clear())]:
            trace = copy.deepcopy(self.trace)
            edit(trace)
            with self.subTest(name):
                self.assertTrue(result('p_under_stream', self.ctx(trace=trace))['unavailable'])
        never = SimpleNamespace(trace=copy.deepcopy(self.trace), under_stream={})
        self.assertTrue(result('e_under_load_latency', never)['unavailable'])


class D1TargetAbsentTests(unittest.TestCase):
    """openai/gpt-6-luna-pro, SB7.2 (2026-10-03): its sync() raised UnboundLocalError before any list
    request, so it held 30 of 12,288 payments and drew 6; no pose could draw the D1 target, the probe
    signalled witness_unavailable, the D1 mutation never fired and the run was REFUSED as
    'fire_d1_mutation:failed'. Its own GET /api/payments/<target> answers 404: charged now."""

    def setUp(self):
        saved = json.loads((Path(__file__).parent / 'fixtures/sb72-luna-pro-d1-target-absent.json').read_text())
        self.viz = saved['viz']
        self.decisions = {'score': 0.5, 'detail': 'D1=0.5, D2=0.5, D3=0.5', 'parts': {
            'D1': {'documented': True, 'doc_stance': None, 'observed': None, 'score': 0.5},
            'D2': {'documented': True, 'doc_stance': None, 'observed': None, 'score': 0.5},
            'D3': {'documented': True, 'doc_stance': None, 'observed': None, 'score': 0.5}}}

    def test_the_handshake_fires_the_mutation_on_the_target_absent_signal(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'ready.json'
            path.write_text(json.dumps(self.viz['sb71StreamHandshake']['signal']))
            calls = []
            handshake = score.StreamHandshake(path, lambda: calls.append('fired') or {'version': 2})
            handshake.start()
            self.assertTrue(handshake.done.wait(2))
            handshake.finish()
            self.assertEqual(handshake.result(), {'version': 2})
            self.assertEqual(calls, ['fired'])

    def test_the_luna_pro_receipt_is_charged(self):
        witness = score.stream_witness(self.viz)
        self.assertEqual(witness['status'], 'target_absent')
        self.assertFalse(witness['stream_measured'])
        self.assertEqual(witness['evidence']['payment']['status'], 404)
        ctx = SimpleNamespace(probes={'viz': copy.deepcopy(self.viz)})
        charged = result('d_decisions_doc', ctx, lambda _: copy.deepcopy(self.decisions))
        self.assertEqual(charged['parts']['D1']['score'], 0.0)
        self.assertAlmostEqual(charged['score'], 1 / 3)
        unmeasured = {'score': 0, 'detail': 'no stream batch applied', 'consequence': 'the live stream never landed'}
        row = result('p_stream_apply', ctx, lambda _: dict(unmeasured))
        self.assertEqual(row['parts'], {'stream_witness': 'target_absent'})
        timed = {'score': 1.0, 'detail': 'batch visible in 0.4 ms', 'parts': {'ms': 0.4}}
        self.assertEqual(result('p_stream_apply', ctx, lambda _: timed), timed)

    def test_contradicted_absence_still_refuses(self):
        def edits(viz):
            signal = viz['sb71StreamHandshake']['signal']
            return [
                ('candidate holds the record', lambda: signal['targetAbsent']['payment'].update(status=200)),
                ('observation disagrees', lambda: viz['streamPixelArm']['targetAbsent'].update(sceneCount=12289)),
                ('another target', lambda: signal.update(id='pay_00001')),
                ('early arm found the row', lambda: viz['d1Arm'].update(via='table-click', brushed=True)),
                ('pixel search never ran', lambda: viz['streamPixelArm'].update(rejected={'vs7dbg.brush unavailable': 1})),
            ]
        for index in range(5):
            viz = copy.deepcopy(self.viz)
            name, apply = edits(viz)[index]
            apply()
            with self.subTest(name):
                self.assertIn('contradictory SB7.1 D1 stream witness', score.stream_witness(viz)['refuse'])

    def test_a_witness_unavailable_signal_still_refuses(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'ready.json'
            path.write_text(json.dumps({'state': 'witness_unavailable'}))
            handshake = score.StreamHandshake(path, lambda: self.fail('must not mutate'))
            handshake.start()
            self.assertTrue(handshake.done.wait(2))
            handshake.finish()
            with self.assertRaisesRegex(RuntimeError, 'SB7.1 stream witness unavailable'):
                handshake.result()
