import copy
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
import score_sb71 as score

class PrematureResyncTests(unittest.TestCase):
    def setUp(self):
        saved = json.loads((Path(__file__).parent / 'fixtures/sb71-premature-resync.json').read_text())
        self.ctx = SimpleNamespace(**{k: saved[k] for k in ('trace', 'sb71_resync', 'read_stream', 'payments', 'b3_result')}, schedule=SimpleNamespace(sigkill_after_list=saved['target']))
        self.original = next(fn for name, _, fn in score.base.SB7_CHECKS if name == 'r_b3_sigkill_resync')
    def result(self, ctx):
        return score.observed_absence_result('r_b3_sigkill_resync', self.original, ctx)
    def test_actual_archive(self):
        self.assertTrue(self.original(self.ctx)['unavailable'])
        result = self.result(self.ctx)
        self.assertFalse(result.get('unavailable', False))
        self.assertEqual(result['score'], 0)
        self.assertNotIn('vacuous_root', result['parts'])
        evidence = result['parts']['premature_resync']
        self.assertEqual(evidence['last_payment_response']['seq'], 303)
        self.assertEqual(evidence['reversals_response']['seq'], 304)
        self.assertFalse(evidence['kill_fired'])
        self.assertEqual(self.ctx.payments['total'], 12291)
        self.assertEqual(self.ctx.sb71_resync['phase'], 'combined')
        self.assertFalse(self.ctx.sb71_resync['armed'])
    def test_absent_or_contradictory_witness(self):
        mutations = {
            'no_gate': lambda c: setattr(c, 'sb71_resync', {}),
            'target_mismatch': lambda c: c.sb71_resync.update(target=3),
            'held_without_kill': lambda c: c.sb71_resync.update(held=[{}]),
            'failed_transport': lambda c: c.trace[3].update(status=500),
            'unauthenticated': lambda c: c.trace[3].update(authorized=False),
            'no_cached_page': lambda c: c.trace.pop(0),
            'terminal_page': lambda c: c.trace[0].update(last=True),
            'different_generation': lambda c: c.trace[0].update(generation=-1),
            'missing_reversals': lambda c: c.trace.pop(4),
            'failed_reversals': lambda c: c.trace[4].update(status=500),
            'no_phase_end': lambda c: c.trace.pop(),
            'missing_summary': lambda c: setattr(c, 'read_stream', []),
            'no_completion': lambda c: c.read_stream[1]['summary'].update(last_sync=c.read_stream[0]['summary']['last_sync']),
            'completed_after_phase': lambda c: c.read_stream[1].update(t=c.trace[-1]['t']+1),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                ctx = copy.deepcopy(self.ctx)
                mutate(ctx)
                self.assertTrue(self.result(ctx)['unavailable'])
    def test_executed_kill_keeps_original(self):
        self.ctx.b3_result.update(kill_fired=True, restarted=True, converged=False, no_duplicates=True)
        self.assertEqual(self.result(self.ctx), self.original(self.ctx))
    def test_missing_sync_root_keeps_original(self):
        self.ctx.payments['total'] = 0
        self.assertEqual(self.result(self.ctx), self.original(self.ctx))
if __name__ == '__main__':
    unittest.main()
