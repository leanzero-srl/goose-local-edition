"""SB7.2 scorer: weights, 3D-pure bands, the presentation split, public-number parity and wiring."""
import contextlib
import copy
import json
import os
import re
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import bench_rescore
import isolated_tiers
import score_sb7 as base
import score_sb71
import score_sb72 as score

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def perfect_rows():
    rows = [{'check': name, 'tier': tier, 'score': 1.0, 'detail': 'synthetic'}
            for name, tier, _fn in base.SB7_CHECKS]
    rows += [{'check': name, 'tier': tier, 'score': 1.0, 'detail': 'synthetic'}
             for name, tier in score.VISUAL_CHECKS.items()]
    return rows


def compose(overrides=None):
    rows = [{**row, 'score': (overrides or {}).get(row['check'], row['score'])} for row in perfect_rows()]
    with score.tier_runtime():
        raw = base.compose_from_rows(rows, 0)
    return score.admit(raw)


class WeightTests(unittest.TestCase):
    def test_three_d_carries_046_and_every_visual_row_earns_points(self):
        result = compose()
        self.assertEqual(result['score'], 1.0)
        for tier in ('S', 'Q', 'M'):
            self.assertIsInstance(result['tiers'][tier]['weight'], float)
            self.assertGreater(result['tiers'][tier]['weight'], 0)
            self.assertNotIn('admission_only', result['tiers'][tier])
        self.assertAlmostEqual(sum(score.TIER_WEIGHT_SB72[t] for t in score.THREE_D_TIERS), .46)
        for name in score.VISUAL_CHECKS:
            with self.subTest(name=name):
                self.assertLess(compose({name: 0.0})['rawScore'], 1.0)

    def test_sb7_weights_and_selftest_are_restored_after_sb72_composition(self):
        before = dict(base.TIER_WEIGHT_SB7), dict(base.ROOT_BLOCKS), copy.deepcopy(base.TH['e_stream_apply_ms_rungs'])
        self.assertEqual(score.severity_selftest(), [])
        compose()
        self.assertEqual((dict(base.TIER_WEIGHT_SB7), dict(base.ROOT_BLOCKS), base.TH['e_stream_apply_ms_rungs']), before)
        self.assertEqual(base.severity_selftest(), [])
        self.assertEqual(score_sb71.PROBE_NAME, 'product_probe_sb71.mjs')
        self.assertEqual(score_sb71.VERSION, 'sb-7.1')

    def test_runtime_restores_on_error(self):
        with self.assertRaises(RuntimeError), score.tier_runtime():
            raise RuntimeError('boom')
        self.assertIsNot(base.TIER_WEIGHT_SB7, score.TIER_WEIGHT_SB72)
        self.assertEqual(score_sb71.PROBE_NAME, 'product_probe_sb71.mjs')


class BandTests(unittest.TestCase):
    def test_each_3d_leg_holds_its_band(self):
        for name, ceiling in [('s_visible_surface', .599), ('s_tower_geometry', .699), ('s_currency_collar', .699),
                              ('t_vs7dbg_truth', .699), ('q_overview_legibility', .799), ('q_inspector_framing', .799),
                              ('t_brush_link', .799), ('t_stream_diff', .799), ('m_committed_event_replay', .899),
                              ('x_no_lost_write', .899)]:
            with self.subTest(name=name):
                result = compose({name: .5})
                self.assertEqual(result['admission']['ceiling'], ceiling)
                self.assertLessEqual(result['score'], ceiling)

    def test_text_contrast_and_inspector_values_cost_points_never_bands(self):
        for name in ('v_presentation_text', 'q_payment_context'):
            with self.subTest(name=name):
                result = compose({name: .75})
                self.assertEqual(result['admission']['ceiling'], 1.0)
                self.assertLess(result['score'], 1.0)
                self.assertGreater(result['score'], .99)

    def test_admission_never_awards_or_mutates(self):
        rows = perfect_rows()
        with score.tier_runtime():
            raw = base.compose_from_rows(rows, 0)
        raw['score'] = .22
        frozen = copy.deepcopy(raw)
        self.assertEqual(score.admit(raw)['score'], .22)
        self.assertEqual(raw, frozen)


class PresentationSplitTests(unittest.TestCase):
    def test_low_contrast_control_moves_to_v_and_framing_stays_3d(self):
        legible = {'check': 'q_legible_presentation', 'tier': 'Q', 'score': .75, 'parts': {
            'framing': [{'ok': True}] * 8,
            'elements': [{'id': g, 'ok': g != 'inspector-controls'} for g in
                         ('tower-legend', 'inspect-details', 'inspector-controls', 'tower-annotations')],
            'unreadable': [{'group': 'inspector-controls', 'text': 'Replay payment update',
                            'failures': ['effective contrast 3.37 below 4.5']}]}}
        framing, text = score.split_presentation(legible)
        self.assertEqual((framing['check'], framing['score']), ('q_inspector_framing', 1.0))
        self.assertEqual((text['check'], text['tier'], text['score']), ('v_presentation_text', 'V', .75))
        self.assertIn('3.37', text['detail'])

    def test_unreached_observations_are_zero_rows(self):
        rows = {row['check']: row for row in score.visual_rows([])}
        self.assertEqual(set(rows), set(score.VISUAL_CHECKS))
        self.assertTrue(all(row['score'] == 0 for row in rows.values()))


class PublicContractTests(unittest.TestCase):
    contract = (ROOT / 'sb7.2/VISUAL-CONTRACT.md').read_text()
    probe = (HERE / 'product_probe_sb71.mjs').read_text()

    def test_every_legibility_number_and_the_camera_are_published(self):
        block = re.search(r'const SB72_LEGIBILITY = \{(.*?)\};', self.probe, re.S).group(1)
        values = dict(re.findall(r'(\w+): ([\d.]+)', block))
        published = {'spanMin': '60%', 'edgePx': '4 CSS px', 'litContrastMin': '2.0:1', 'capBlockPx': '2 × 2',
                     'capsMin': 'at least 64', 'capsSample': 'up to 96', 'statusDistance': 'RGB distance 40',
                     'accuracyMin': 'at least 90%'}
        self.assertEqual(set(values), set(published))
        for key, text in published.items():
            with self.subTest(key=key):
                self.assertIn(text, self.contract)
        camera = re.search(r"Object\.assign\(V7, \{ yaw0: (\d+), pitch0: (\d+), dist0: (\d+)", self.probe).groups()
        self.assertIn('**yaw {}, pitch {}, distance {}**'.format(*camera), self.contract)
        golden = (HERE / 'golden-sb72/web/viz.js').read_text()
        self.assertIn('var DEFAULTS = {{ yaw: {}, pitch: {}, distance: {} }};'.format(*camera), golden)

    def test_excellence_rungs_are_published(self):
        rungs = score.thresholds()['e_stream_apply_ms_rungs']
        self.assertIn('stream visible ≤' + '/'.join(str(int(ms)) for _s, ms in rungs) + ' ms', self.contract)
        for key, label in (('e_drag_frames_rungs', 'drag frames over 40 moves ≥'),
                           ('underload_p95_rungs', 'burst p95 ≤')):
            self.assertIn(label + '/'.join(str(int(v)) for _s, v in base.TH[key]), self.contract)

    def test_public_contract_shrinks_and_method_section_is_behaviour(self):
        def size(*names):
            return sum(len((ROOT / name).read_bytes()) for name in names)
        self.assertLess(size('spec-build-sb72.md', 'sb7.2/VISUAL-CONTRACT.md'),
                        size('spec-build-sb71.md', 'sb7.1/VISUAL-CONTRACT.md'))
        evidence = self.contract[self.contract.index('## Evidence and grading'):]
        self.assertLessEqual(len(evidence.encode()), 1100)

    def test_sb72_hands_out_its_own_trimmed_contract_and_sb71_inputs_stay_frozen(self):
        # SB7.1 receipts hash exactly these three files; its entrant still receives spec-build-sb7.md.
        self.assertEqual(isolated_tiers.SB71.contracts,
                         ('spec-build-sb71.md', 'spec-build-sb7.md', 'sb7.1/VISUAL-CONTRACT.md'))
        public = dict(isolated_tiers.SB72.public)
        self.assertEqual(public, {'SB7-CONTRACT.md': 'sb7.2/SB7-CONTRACT.md',
                                  'VISUAL-CONTRACT.md': 'sb7.2/VISUAL-CONTRACT.md',
                                  'STARTER.md': 'sb7.2/STARTER.md'})
        import release_manifest
        self.assertLessEqual(set(public.values()), set(release_manifest.payload()))

        def size(*names):
            return sum(len((ROOT / name).read_bytes()) for name in names)
        before = size('spec-build-sb71.md', 'spec-build-sb7.md', 'sb7.1/VISUAL-CONTRACT.md', 'sb7.1/starter/STARTER.md')
        after = size(*isolated_tiers.SB72.contracts)
        self.assertLessEqual(after, 0.6 * before)

    def test_prompt_carries_the_definition_of_done_and_the_call_budget(self):
        prompt = (ROOT / 'spec-build-sb72.md').read_text()
        self.assertIn('## Definition of done', prompt)
        self.assertIn('You have a budget of 150 model calls.', prompt)
        self.assertIn('polishing beyond the\nscored behaviours earns nothing', prompt)


class WiringTests(unittest.TestCase):
    def test_sb72_flag_selects_its_scorer_spec_and_contract(self):
        import run_build
        with patch.dict(os.environ, {'BENCH_SB72': '1'}, clear=True):
            scorer, vendor, spec = run_build._regime()
            self.assertIs(scorer, score)
            self.assertEqual(spec, 'spec-build-sb72.md')
            self.assertEqual(isolated_tiers.active().visual_contract, 'sb7.2/VISUAL-CONTRACT.md')
        with patch.dict(os.environ, {'BENCH_SB71': '1', 'BENCH_SB72': '1'}, clear=True), \
                self.assertRaisesRegex(RuntimeError, 'more than one isolated tier'):
            isolated_tiers.active()
        with patch.dict(os.environ, {'BENCH_SB71': '1'}, clear=True):
            self.assertIs(run_build._regime()[0], score_sb71)

    def test_sb72_receipt_replays_its_own_scorer_and_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            tree = Path(tmp) / 'tree'
            tree.mkdir()
            (tree / 'app.py').write_text('print(1)\n')
            (tree / 'trace.jsonl').write_text(json.dumps({'fixture_seed': '0123456789abcdef'}) + '\n')
            agent = {'exit': 0, 'secs': 12.5, 'timed_out': False, 'tail': ''}
            receipt = bench_rescore.write_completion(tree, Path(tmp) / 'receipt.json', agent, run_id='r', started_at='t',
                                                     seed='0123456789abcdef', port=8850, provider='p', model='m',
                                                     tier=isolated_tiers.SB72)
            self.assertEqual(receipt['scorerVersion'], 'sb-7.2')
            self.assertIn('sb7.2/VISUAL-CONTRACT.md', receipt['contracts'])
            self.assertIn('score_sb72.py', receipt['scorerFiles'])
            self.assertIs(bench_rescore.validate_receipt(receipt, tree, 'r'), receipt)
            self.assertIs(bench_rescore.receipt_tier(receipt), isolated_tiers.SB72)
            with self.assertRaises(ValueError):
                bench_rescore.validate_receipt({**receipt, 'scorerVersion': 'sb-9'}, tree, 'r')

    def test_evaluate_reports_sb72_identity_and_weights(self):
        rows = [r for r in perfect_rows() if r['check'] not in score.VISUAL_CHECKS]
        observed = [{'check': n, 'tier': t, 'score': 1, 'detail': 'observed'} for n, t in score.VISUAL_CHECKS.items()
                    if n not in ('q_inspector_framing', 'v_presentation_text')]
        observed.append({'check': 'q_legible_presentation', 'tier': 'Q', 'score': 1, 'parts': {
            'framing': [{'ok': True}] * 8, 'elements': [{'ok': True}] * 4, 'unreadable': []}})
        inherited = {'checks': rows + [{'check': 'q_legible_presentation', 'tier': 'Q', 'score': 1}],
                     'critical': {'rows': []}, 'initial_api': {}, 'threshold_policy': {}}
        ctx = SimpleNamespace(probes={'viz': {'sb71': {'checks': observed}}}, root=Path('.'), fixture_seed='0' * 16)
        with patch.object(score_sb71, 'evaluate', return_value=inherited), \
                patch.object(base, '_nominal_console_errors', return_value=0):
            result = score.evaluate(ctx)
        self.assertEqual((result['scorerVersion'], result['score']), ('sb-7.2', 1.0))
        self.assertEqual(result['weights']['three_d_share'], .46)
        self.assertIn('sb7.2/VISUAL-CONTRACT.md', result['contract_sha256'])


def check_fn(name):
    return next(fn for n, _t, fn in base.SB7_CHECKS if n == name)


class NotifierFeedTests(unittest.TestCase):
    """r_notification_multiset read one ?limit=200 page: 7 SB7.2 runs returned exactly 200 rows
    (194 reversal.created + 6 drafts) against 1,438-5,646 committed reversals."""

    def feed(self, rows, *, honour_offset=True, total=None, totals=None):
        calls = []

        def get(url, timeout=None):
            calls.append(url)
            q = dict(part.split('=') for part in url.split('?')[1].split('&'))
            limit, offset = min(int(q.get('limit', 50)), 200), int(q.get('offset', 0)) if honour_offset else 0
            t = totals.pop(0) if totals else (len(rows) if total is None else total)
            return 200, {'data': rows[offset:offset + limit], 'total': t}, b'', {}
        return get, calls

    def test_sb72_pages_the_whole_feed_and_sb71_keeps_its_single_read(self):
        rows = [{'id': i, 'event_seq': i, 'kind': 'reversal.created'} for i in range(2537)]
        get, calls = self.feed(rows)
        with patch.object(base, '_get', get):
            frozen = base._notifications_feed('http://n')
            self.assertEqual((len(frozen['data']), calls), (200, ['http://n/notify/notifications?limit=200']))
            calls.clear()
            with score.tier_runtime():
                paged = base._notifications_feed('http://n')
        self.assertEqual(len(paged['data']), 2537)
        self.assertEqual(paged['paging']['pages'], 13)
        self.assertEqual(paged['paging']['stop'], 'short page')
        self.assertFalse(base.NOTIFY_PAGED)

    def test_ignored_offset_stops_after_one_page_and_a_moving_total_is_walked_again(self):
        rows = [{'id': i, 'kind': 'draft.submitted'} for i in range(450)]
        get, _calls = self.feed(rows, honour_offset=False)
        with patch.object(base, '_get', get), score.tier_runtime():
            once = base._notifications_feed('http://n')
        self.assertEqual(len(once['data']), 200)
        self.assertIn('offset not honoured', once['paging']['stop'])
        get, calls = self.feed(rows, totals=[449, 450, 450, 450, 450, 450])
        with patch.object(base, '_get', get), score.tier_runtime():
            walked = base._notifications_feed('http://n')
        self.assertEqual((walked['paging']['walks'], len(walked['data']), walked['total']), (2, 450, 450))

    def test_multiset_counts_every_page(self):
        ctx = SimpleNamespace(notifier_notifications={
            'data': [{'id': i, 'kind': 'reversal.created'} for i in range(2537)],
            'paging': {'pages': 13, 'rows_read': 2537, 'reported_total': 2537, 'stop': 'short page'}},
            events=[{'type': 'reversal.created'}] * 2537, pack=None)
        row = check_fn('r_notification_multiset')(ctx)
        self.assertEqual(row['score'], 1.0)
        self.assertIn('13 pages', row['detail'])


def viz_ctx(**sections):
    return SimpleNamespace(probes={'viz': sections}, payments={'total': base.N_FROZEN})


class AimedPressTests(unittest.TestCase):
    """07caff2d/1b72fc45/648336b4/82b40634/b36032ee: the first 3D click brushed nothing and the
    toggle-off click brushed the target IN; the coast flick never moved the camera on 6 runs
    while the same session's 40-move drag landed on expectedYaw 23.8."""

    archived_miss = {'door3d': {'targetId': 'pay_05951', 'inBrush': False, 'rowFound': False},
                     'toggleOff': {'targetId': 'pay_05951', 'removed': False}}

    def test_archived_evidence_scores_exactly_as_before(self):
        ctx = viz_ctx(brush=self.archived_miss, brushClear={'emptied': True})
        self.assertAlmostEqual(check_fn('t_click_semantics')(ctx)['score'], 1 / 3)

    def test_a_press_not_delivered_to_the_canvas_is_neither_credit_nor_charge(self):
        miss = 'the point was not delivered to #viz3d after re-centring the canvas twice'
        brush = {'door3d': {**self.archived_miss['door3d'], 'probeMiss': miss},
                 'toggleOff': {**self.archived_miss['toggleOff'], 'probeMiss': miss},
                 'doorTable': {'inBrush': True}}
        ctx = viz_ctx(brush=brush, brushClear={'emptied': True, 'fullHexOk': True},
                      brushHighlight={'memberPx': {'ok': True}, 'nonMemberPx': {'ok': True}},
                      brushCount={'present': True})
        click = check_fn('t_click_semantics')(ctx)
        self.assertEqual((click['score'], click['parts']['click_toggles_on']), (1.0, None))
        self.assertIn('probe miss', click['detail'])
        link = check_fn('t_brush_link')(ctx)
        self.assertEqual(link['parts']['instance_click_toggles'], None)
        self.assertLess(link['score'], 1.0)          # brush_count needs a counts text: charged
        self.assertNotIn('unavailable', link)

    def coast(self, flick, slow):
        return viz_ctx(coast={'flick': flick, 'slowRelease': slow})

    still = {'v0Reported': 0, 'releaseYaw': 70, 'restYaw': 70, 'coastDeg': 0, 'directionOk': True,
             'settleMs': 1.5, 'settleBudgetMs': 700, 'movedPixelCount': 0, 'movedPixelTotal': 5,
             'residuals': [], 'samples': [{'t': 0, 'yaw': 70}], 'restPixel': {'ok': True}}

    def test_archived_still_flick_keeps_its_old_partial_credit(self):
        ctx = self.coast(self.still, {'watched': True, 'driftDeg': 0, 'ok': True})
        self.assertEqual(check_fn('t_coast_identity')(ctx)['score'], .5)
        self.assertAlmostEqual(check_fn('t_coast_reality')(ctx)['score'], .2667, places=3)

    def test_sb72_still_flick_after_an_orbiting_drag_is_unmeasured_not_credited(self):
        flick = {**self.still, 'moved': False, 'dragOrbitDeg': 46.2,
                 'attempts': [{'moved': False}, {'moved': False}]}
        ctx = self.coast(flick, {'watched': True, 'driftDeg': 0, 'ok': True, 'dragMoved': False, 'dragDeg': 0})
        self.assertTrue(check_fn('t_coast_identity')(ctx)['unavailable'])
        self.assertTrue(check_fn('t_coast_reality')(ctx)['unavailable'])

    def test_sb72_app_that_never_orbits_is_charged_without_vacuous_credit(self):
        flick = {**self.still, 'moved': False, 'dragOrbitDeg': 0, 'attempts': [{'moved': False}, {'moved': False}]}
        ctx = self.coast(flick, {'watched': True, 'driftDeg': 0, 'ok': True, 'dragMoved': False, 'dragDeg': 0})
        self.assertEqual(check_fn('t_coast_identity')(ctx)['score'], 0.0)
        self.assertEqual(check_fn('t_coast_reality')(ctx)['score'], 0.0)

    def test_sb72_moving_flick_grades_like_before(self):
        flick = {'v0Reported': 11.9, 'releaseYaw': 12.3, 'restYaw': 8.1, 'coastDeg': 4.2, 'directionOk': True,
                 'settleMs': 1130, 'settleBudgetMs': 1413, 'movedPixelCount': 5, 'movedPixelTotal': 5,
                 'residuals': [{'residualDeg': .1, 'tolDeg': 1}], 'samples': [{}], 'moved': True, 'dragOrbitDeg': 46.2}
        ctx = self.coast(flick, {'watched': True, 'driftDeg': 0, 'ok': True, 'dragMoved': True, 'dragDeg': 15})
        self.assertEqual(check_fn('t_coast_identity')(ctx)['score'], 1.0)
        self.assertEqual(check_fn('t_coast_reality')(ctx)['score'], 1.0)


class StringencyTests(unittest.TestCase):
    """G1 rest after every interaction kind, G2 pick refresh after a batch and mid-coast, G4 named paths."""

    rest = [{'defaultDraws': 0, 'batchLanded': False}] * 2

    def test_idle_flatness_grades_every_rest_context_and_old_evidence_stays_binary(self):
        fn = check_fn('p_idle_flatness')
        self.assertEqual(fn(viz_ctx(idle={'windows': self.rest}))['score'], 1.0)
        self.assertEqual(fn(viz_ctx(idle={'windows': [{'defaultDraws': 3}]}))['score'], 0.0)
        ctx = viz_ctx(idle={'windows': self.rest}, idleAfterWheel=self.rest, idleAfterStream=self.rest,
                      idleAfterAnimation=[{'defaultDraws': 30, 'batchLanded': False}],
                      idleAfterCoast=[{'defaultDraws': 4, 'batchLanded': True}])
        row = fn(ctx)
        self.assertEqual(row['score'], .75)          # load, wheel, stream flat; animation draws; coast void
        self.assertIn('animation 30', row['detail'])
        self.assertNotIn('coast', row['parts']['contexts'])

    def test_pick_refresh_after_batch_and_mid_coast(self):
        fn = check_fn('t_pick_real_pass')
        base_pc = {'pickCounters': {'sinceInvalidation': {'offDraws': 1, 'offReads': 1},
                                    'duringPickCalls': {'defDraws': 0}, 'pickCalls': 9}}
        self.assertEqual(fn(viz_ctx(**base_pc))['score'], 1.0)
        good = {'offDrawsSinceBatch': 1, 'offReadsSinceBatch': 1, 'pickDefaultDraws': 0, 'agrees': True}
        coast = {'cameraMovedSinceRelease': True, 'offDrawsSinceRelease': 3, 'offReadsSinceRelease': 1,
                 'pickDefaultDraws': 0, 'agrees': True}
        self.assertEqual(fn(viz_ctx(**base_pc, pickAfterBatch=good, pickDuringCoast=coast))['score'], 1.0)
        stale = {**good, 'offReadsSinceBatch': 0}
        self.assertEqual(fn(viz_ctx(**base_pc, pickAfterBatch=stale, pickDuringCoast=coast))['score'], 0.0)
        still = {**coast, 'cameraMovedSinceRelease': False, 'offReadsSinceRelease': 0}
        self.assertEqual(fn(viz_ctx(**base_pc, pickAfterBatch=good, pickDuringCoast=still))['score'], 1.0)

    def test_error_paths_must_name_the_offending_field(self):
        fn = check_fn('b_error_envelope')
        cases = [{'path': '/api/payments?limit=-1', 'envelope_ok': True, 'expects_field_errors': True,
                  'field_paths_ok': True, 'expected_path': 'limit', 'field_path_named': False},
                 {'path': '/api/drafts[counterparty.name missing]', 'envelope_ok': True, 'expects_field_errors': True,
                  'field_paths_ok': True, 'expected_path': 'counterparty.name', 'field_path_named': True}]
        row = fn(SimpleNamespace(envelope_cases=cases))
        self.assertAlmostEqual(row['score'], .6 + .4 * .5)
        old = [{k: v for k, v in x.items() if k not in ('expected_path', 'field_path_named')} for x in cases]
        self.assertEqual(fn(SimpleNamespace(envelope_cases=old))['score'], 1.0)


class GroupAtomicityWindowTests(unittest.TestCase):
    """x_l5: a sync-#1 read whose reversals are not yet backfilled is incomplete, not torn (64 confirmed
    'half states' on 8 runs were all this); a torn read with the reversals loaded, or any after sync #1, counts."""
    boot, amt, T = 1000, 50, 100.0

    def sample(self, t, status, total):
        return {'t': t, 'summary': {'reversals': [{'currency': 'EUR', 'total_minor': total}]},
                'rows': {'pay_1': {'id': 'pay_1', 'status': status, 'version': 2 if status == 'refunded' else 1}}}

    def row(self, stream, window=True):
        conclusion, _hs = base._l5_conclusion(stream, 'pay_1', 'EUR', self.amt, self.boot,
                                              self.T if window else None)
        ctx = SimpleNamespace(payments={'total': base.N_FROZEN}, _m2_conclusion=conclusion)
        return check_fn('x_l5_group_atomicity')(ctx)

    def test_pre_sync_backfill_gap_is_not_counted(self):
        stream = [self.sample(t, 'settled', 400) for t in (90, 91, 92)] + \
                 [self.sample(t, 'refunded', self.boot + self.amt) for t in (101, 102)]
        self.assertEqual(self.row(stream)['score'], 1.0)
        self.assertEqual(self.row(stream, window=False)['score'], 0.0)      # SB7.1 reading unchanged

    def test_post_sync_half_state_is_charged(self):
        stream = [self.sample(t, 'refunded', self.boot) for t in (101, 102, 103)]
        self.assertEqual(self.row(stream)['score'], 0.0)

    def test_sync1_torn_group_with_reversals_loaded_is_charged(self):
        stream = [self.sample(t, 'refunded', self.boot) for t in (95, 96)]
        self.assertEqual(self.row(stream)['score'], 0.0)

    def test_no_gradable_read_is_vacuous_not_a_pass(self):
        row = self.row([self.sample(t, 'settled', 0) for t in (90, 91)])
        self.assertEqual((row['score'], row['parts']['vacuous_root']), (0.0, 'sync_completeness'))


class CompositionReworkTests(unittest.TestCase):
    def test_x_m2_is_report_only_and_x_l5_still_costs(self):
        self.assertEqual(compose({'x_m2_pair_conservation': 0.0})['score'], 1.0)
        both = compose({'x_m2_pair_conservation': 0.0, 'x_l5_group_atomicity': 0.0})
        one = compose({'x_l5_group_atomicity': 0.0})
        self.assertEqual(both['rawScore'], one['rawScore'])
        self.assertLess(one['rawScore'], 1.0)
        self.assertNotIn('x_m2_pair_conservation', score.BACKEND_EXCELLENCE)

    def test_the_0899_band_is_graded_and_the_lower_bands_stay_binary(self):
        failing = ['x_l5_group_atomicity', 'r_notification_multiset', 'r_b3_sigkill_resync', 'm_committed_event_replay',
                   'x_no_lost_write', 'r_b7_partition']
        for n, ceiling in [(1, .899), (2, .869), (3, .839), (4, .809), (5, .799), (6, .799)]:
            with self.subTest(n=n):
                result = compose({name: .5 for name in failing[:n]})
                self.assertEqual(result['admission']['ceiling'], ceiling)
        self.assertEqual(compose({'t_brush_link': .9, 'x_l5_group_atomicity': .5})['admission']['ceiling'], .799)

    def test_the_0799_and_0699_bands_step_by_their_own_failures_and_0599_stays_binary(self):
        interaction = ['t_click_semantics', 't_coast_identity', 't_coast_reality', 't_brush_link', 'q_inspector_framing']
        for n, ceiling in [(1, .799), (2, .769), (3, .739), (4, .709), (5, .699)]:
            with self.subTest(band=.799, n=n):
                self.assertEqual(compose({name: .5 for name in interaction[:n]})['admission']['ceiling'], ceiling)
        structure = ['s_tower_geometry', 's_currency_collar', 't_height_pixels', 't_vs7dbg_truth', 't_layout_basis']
        for n, ceiling in [(1, .699), (2, .669), (3, .639), (4, .609), (5, .599)]:
            with self.subTest(band=.699, n=n):
                self.assertEqual(compose({name: .5 for name in structure[:n]})['admission']['ceiling'], ceiling)
        dark = compose({'s_visible_surface': .5})
        self.assertEqual(dark['admission']['ceiling'], .599)
        # a failed lower band never counts toward a higher band's step
        self.assertEqual(compose({'s_visible_surface': .5, 's_tower_geometry': .5})['admission']['failedChecksByBand'][1]
                         ['graded_failures'], 1)

    def test_a_capped_final_sits_below_its_cap_monotone_and_continuous(self):
        for cap in (.899, .869, .799, .699, .599):
            finals = [score.capped(e / 1000, cap) for e in range(0, 1001)]
            self.assertTrue(all(b >= a for a, b in zip(finals, finals[1:])))
            self.assertTrue(all(abs(b - a) <= .0011 for a, b in zip(finals, finals[1:])))
            self.assertEqual(score.capped(1.0, cap), cap)
            self.assertTrue(all(f < cap for f in finals[:-1]))
            self.assertAlmostEqual(cap - score.capped(cap, cap), .05 * (1 - cap), delta=.0001)
        self.assertEqual(score.capped(.9084, 1.0), .9084)
        self.assertEqual(score.capped(.9084, .799), round(.799 - .05 * (1 - .9084), 4))
        self.assertEqual(score.capped(.7, .799), .7)
        self.assertEqual(score.capped(.99998, .899), .8989)
        self.assertEqual(score.capped(.9, .799), .794)
        self.assertEqual(score.capped(.7424, .799), .7424)

    def test_runtime_restores_diagnostic_and_paging(self):
        before = (set(base.DIAGNOSTIC), base.NOTIFY_PAGED, base.SB72_STRICT)
        with score.tier_runtime():
            self.assertIn('x_m2_pair_conservation', base.DIAGNOSTIC)
            self.assertTrue(base.NOTIFY_PAGED)
        self.assertEqual((set(base.DIAGNOSTIC), base.NOTIFY_PAGED, base.SB72_STRICT), before)

    def test_half_state_samples_ride_both_atomicity_rows(self):
        samples = [{'previous': {'t': 1}, 'confirming': {'t': 2}}]
        rows = score.report_only_rows([
            {'check': 'x_l5_group_atomicity', 'tier': 'X', 'score': 0, 'parts': {'confirmed_half_states': 1}},
            {'check': 'x_m2_pair_conservation', 'tier': 'X', 'score': 0, 'parts': {'confirmed_half_states': 1}}],
            samples)
        self.assertEqual([r['parts']['half_state_samples'] for r in rows], [samples, samples])
        self.assertTrue(rows[1]['report_only'])
        self.assertNotIn('report_only', rows[0])


# GLM-5.3 (openrouter-cloud-e7aa12a4, refused 2026-10-04): the viz probe emitted these and nothing past
# them; nine rows came back "lost to the probe's hard cap". The load figures are what the box measured.
GLM_CAP = {'timedOut': True, 'elapsedMs': 480017, 'hardMs': 480000,
           'sb71StreamHandshake': {'error': score_sb71.READINESS_ABSENT}}
GLM_LOAD = {'cpus': 16, 'start': [12.6, 12.0, 11.3], 'end': [12.1, 12.3, 11.4]}
CAP_LOST = {'t_stream_diff': 'stream', 'p_stream_apply': 'stream', 'e_stream_apply_latency': 'stream',
            't_brush_link': 'brush', 't_click_semantics': 'brush', 't_coast_identity': 'coast',
            't_coast_reality': 'coast', 't_height_pixels': 'heightPixels', 't_vs7dbg_truth': 'vs7dbgTruth'}


class VizCapTests(unittest.TestCase):
    """Owner 2026-10-04: "it needs to give a bad score and that it! without changing the benchmark"."""

    def stub(self, name):
        key = CAP_LOST.get(name)

        def fn(c):
            lost = base._viz_section(c, key)[1] if key else None
            return lost or base.g(1.0, 'synthetic')
        return fn

    def score(self, tier, viz, harness_missing=()):
        checks = [(name, t, self.stub(name)) for name, t, _fn in base.SB7_CHECKS]
        with tempfile.TemporaryDirectory() as tmp, patch.object(base, 'SB7_CHECKS', checks), \
                patch.object(base, '_nominal_console_errors', return_value=0):
            ctx = SimpleNamespace(probes={'viz': viz}, root=Path(tmp), fixture_seed='1bc7a0beb26de9ef',
                                  port_bound=True, harness_missing=list(harness_missing), sched_unreached=[],
                                  stream_head=None, b3_result=None)
            return tier.evaluate(ctx)

    def test_a_cap_hit_is_scored_with_every_unobserved_row_zero_and_the_load_named(self):
        # base.gather records fire_d1_mutation:failed when the capped probe never signalled its stream arm.
        result = self.score(score, {**GLM_CAP, 'harnessLoad': GLM_LOAD}, ['fire_d1_mutation:failed'])
        rows = {r['check']: r for r in result['checks']}
        self.assertEqual(result['harness_missing'], ['fire_d1_mutation:failed'])
        self.assertEqual(result['viz_cap']['harness_missing_from_cap'], ['fire_d1_mutation:failed'])
        self.assertEqual(result['probe_unavailable'], [])
        self.assertEqual([r['check'] for r in result['checks'] if 'PROBE ERROR' in r.get('detail', '')], [])
        for name in (*CAP_LOST, *score.VISUAL_CHECKS):
            with self.subTest(name=name):
                self.assertEqual(rows[name]['score'], 0.0)
                self.assertNotIn('unavailable', rows[name])
                self.assertIn("the app's 3D view did not finish the probe's visual observations within the 480 s "
                              "budget (the viz probe ran 480.0 s; 1-min load 12.6 at the viz probe's start and 12.1 "
                              'at its end, on 16 CPUs)', rows[name]['detail'])
        self.assertEqual(set(result['viz_cap']['charged']), set(CAP_LOST) | set(score.VISUAL_CHECKS))
        self.assertEqual((result['viz_cap']['elapsedMs'], result['viz_cap']['hardMs']), (480017, 480000))
        self.assertEqual(result['harness_load'], GLM_LOAD)
        # The bands are the ordinary ones: no visible surface holds the binary 0.599 band.
        self.assertEqual(result['admission']['ceiling'], .599)
        self.assertLess(result['score'], .599)
        self.assertEqual(result['critical']['multiplier'], 1.0)
        self.assertIn('VIZ CAP: ', score.format_report(result, 'glm'))

    def test_a_cap_hit_without_a_load_sample_names_the_absence(self):
        result = self.score(score, dict(GLM_CAP))
        detail = next(r['detail'] for r in result['checks'] if r['check'] == 't_stream_diff')
        self.assertIn('machine load was not recorded for this probe', detail)
        self.assertNotIn('harness_load', result)
        no_budget = score_sb71.viz_cap_note({'timedOut': True})
        self.assertIn("the probe's hard cap (the probe emitted no budget)", no_budget)
        self.assertIn('the probe emitted no elapsed time', no_budget)

    def test_no_cap_leaves_the_verdict_unchanged(self):
        result = self.score(score, {'timedOut': False, 'stream': {}, 'brush': {}, 'coast': {},
                                    'heightPixels': {}, 'vs7dbgTruth': {}})
        self.assertNotIn('viz_cap', result)
        self.assertNotIn('harness_load', result)
        rows = {r['check']: r for r in result['checks']}
        self.assertEqual(rows['s_visible_surface']['detail'], 'Required SB7.2 visual observation was not reached')
        self.assertEqual(rows['q_inspector_framing']['detail'], 'Required SB7.2 inspector observation was not reached')
        quiet = self.score(score, {'timedOut': False, 'harnessLoad': GLM_LOAD})
        self.assertEqual(quiet['harness_load'], GLM_LOAD)
        self.assertNotIn('viz_cap', quiet)

    def test_a_harness_fault_the_cap_did_not_cause_still_refuses(self):
        with self.assertRaisesRegex(RuntimeError, r'missing benchmark infrastructure: arm_hold_create:failed$'):
            self.score(score, dict(GLM_CAP), ['fire_d1_mutation:failed', 'arm_hold_create:failed'])
        # Without the cap a failed D1 fire is a harness fault, as before.
        with self.assertRaisesRegex(RuntimeError, 'missing benchmark infrastructure: fire_d1_mutation:failed'):
            self.score(score, {'timedOut': False, 'sb71StreamHandshake': {'error': score_sb71.READINESS_ABSENT}},
                       ['fire_d1_mutation:failed'])
        # A capped probe whose handshake failed for another reason does not excuse the fire.
        with self.assertRaisesRegex(RuntimeError, 'missing benchmark infrastructure: fire_d1_mutation:failed'):
            self.score(score, {**GLM_CAP, 'sb71StreamHandshake': {'error': 'SB7.1 stream witness unavailable: {}'}},
                       ['fire_d1_mutation:failed'])

    def test_sb71_keeps_refusing_and_records_nothing_new(self):
        self.assertFalse(score_sb71.CHARGE_VIZ_CAP)
        with self.assertRaisesRegex(RuntimeError, 'viz probe hit its hard cap before the SB7.1 visual observations'):
            self.score(score_sb71, dict(GLM_CAP), ['fire_d1_mutation:failed'])
        self.assertEqual(score_sb71.viz_cap_missing(dict(GLM_CAP)), ())
        lost = base.unavail("viz section 'stream' lost to the probe's hard cap (timedOut)")
        self.assertIsNone(score_sb71.viz_cap_charge(SimpleNamespace(probes={'viz': dict(GLM_CAP)}), lost))
        self.assertFalse(score_sb71.CHARGE_VIZ_CAP)

    def test_the_load_is_sampled_around_the_viz_probe_only_under_sb72(self):
        def fake_probe(scenario, url, *flags, env=None, timeout=None):
            return dict(GLM_CAP)
        samples = [(12.6, 12.0, 11.3), (12.1, 12.3, 11.4)]
        for tier, expected in ((score, GLM_LOAD), (None, None)):
            with self.subTest(tier=tier and tier.VERSION), patch.object(base, '_probe', fake_probe), \
                    patch.object(score_sb71.os, 'getloadavg', side_effect=list(samples)), \
                    patch.object(score_sb71.os, 'cpu_count', return_value=16):
                with (tier.tier_runtime() if tier else contextlib.nullcontext()), score_sb71.probe_runtime():
                    result = base._probe('viz', 'http://127.0.0.1:1')
                self.assertEqual(result.get('harnessLoad'), expected)


if __name__ == '__main__':
    unittest.main()
