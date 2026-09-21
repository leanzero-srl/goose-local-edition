import copy
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
import tempfile
import subprocess
from unittest.mock import patch
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bench'))
import score_sb71 as score


class ReportingTests(unittest.TestCase):
    def verdict(self):
        return json.loads((Path(__file__).resolve().parents[1] / 'runs/sb71-validation/20260921/installed-302-acceptance/verdict.json').read_text())

    def test_actual_run_keeps_score_and_explains_both_phases(self):
        result = self.verdict()
        before = copy.deepcopy(result)
        score.explain_initial_state(result, SimpleNamespace(payments={'total': 8388}, expected_total_at_load=12289))
        rows = {r['check']: r for r in result['checks']}
        self.assertIn('12291', rows['sync_completeness']['detail'])
        self.assertIn('8388', rows['sync_completeness']['detail'])
        self.assertIn('incomplete initial dataset', rows['b_buckets_dst']['detail'])
        self.assertIn('does not establish a timezone', rows['b_buckets_dst']['consequence'])
        self.assertIn('8388/12289', result['critical']['rows'][0]['why'])
        self.assertEqual(result['score'], .4992)
        self.assertEqual([r['score'] for r in result['checks']], [r['score'] for r in before['checks']])
        self.assertEqual([r.get('parts') for r in result['checks']], [r.get('parts') for r in before['checks']])
        self.assertEqual(result['critical']['multiplier'], before['critical']['multiplier'])
        behavioral = [r for r in result['checks'] if r['check'] not in score.VISUAL_CHECKS]
        self.assertEqual(score.base.compose_from_rows(behavioral, 1)['score'], .4992)

    def test_complete_initial_dataset_does_not_get_incomplete_label(self):
        result = self.verdict()
        original = copy.deepcopy(result['critical'])
        score.explain_initial_state(result, SimpleNamespace(payments={'total': 12289}, expected_total_at_load=12289))
        self.assertEqual(result['critical'], original)
        self.assertNotIn('recovery', next(r['detail'] for r in result['checks'] if r['check'] == 'sync_completeness'))

    def test_snapshot_retains_actual_values_and_expected_cells(self):
        ctx = SimpleNamespace(payments={'total': 8388}, summary={'count': 8388},
            buckets={'cells': [{'day': '2027-03-28', 'status': 'pending', 'count': 7}]},
            viz_records={'count': 8388}, buckets_expected={('2027-03-28', 'pending'): 12},
            expected_total_at_load=12289, sync1_done=False, sync1_wall_ms=420000, fixture_seed='8714b2874ad94ed7')
        saved = score.initial_api_evidence(ctx)
        self.assertEqual(saved['missing_fields'], [])
        self.assertEqual(saved['snapshot']['buckets'], ctx.buckets)
        self.assertEqual(saved['snapshot']['buckets_expected'], [{'day': '2027-03-28', 'status': 'pending', 'count': 12}])
        ctx.buckets['cells'][0]['count'] = 99
        self.assertEqual(saved['snapshot']['buckets']['cells'][0]['count'], 7)
        self.assertEqual(json.loads(json.dumps(saved)), saved)

    def test_absent_snapshot_is_named_not_substituted(self):
        evidence = score.initial_api_evidence(SimpleNamespace())
        self.assertIn('buckets', evidence['missing_fields'])
        self.assertNotIn('buckets', evidence['snapshot'])

    def test_encoding_failure_preserves_completed_grading_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'bench-media').mkdir()
            media = {'manifest': 'bench-media/media-manifest.json', 'videos': [{'file': 'raw.webm'}], 'errors': []}
            manifest = root / media['manifest']
            manifest.write_text(json.dumps(media))
            ctx = SimpleNamespace(probes={'viz': {'cameraMath': {'score': 1}, 'sb71': {'checks': [{'score': 1}], 'media': media}}})
            before = copy.deepcopy(ctx.probes)
            with patch.object(score.subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'node')):
                score.encode_recorded_media(ctx, root)
            self.assertEqual(ctx.probes['viz']['cameraMath'], before['viz']['cameraMath'])
            self.assertEqual(ctx.probes['viz']['sb71']['checks'], before['viz']['sb71']['checks'])
            self.assertEqual(media['videos'], before['viz']['sb71']['media']['videos'])
            self.assertIn('graded observations retained', json.loads(manifest.read_text())['errors'][0])

    def test_stable_identity_does_not_claim_empirical_calibration(self):
        policy = score.threshold_policy()
        self.assertFalse(policy['empirically_calibrated'])
        self.assertEqual(len(policy['thresholds_sha256']), 64)
        result = self.verdict()
        result['calibration'] = 'Fixed specification budgets; empirical calibration not performed'
        report = score.format_report(result)
        self.assertNotIn('rc-grade', report)
        self.assertIn('empirical calibration not performed', report)
        self.assertIn('sb-7.1', report)


if __name__ == '__main__':
    unittest.main()
