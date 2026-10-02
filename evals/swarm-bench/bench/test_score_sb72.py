"""SB7.2 scorer: weights, 3D-pure bands, the presentation split, public-number parity and wiring."""
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


if __name__ == '__main__':
    unittest.main()
