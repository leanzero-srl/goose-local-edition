"""Real child-process economic stops must not become Forge capability results."""
from pathlib import Path
import signal
import tempfile
import unittest
from unittest.mock import patch

import bench_budget
import bench_cost
import bench_rescore
import isolated_tiers
from test_bench_budget import IsolatedInvoke

OPENROUTER = {'OPENROUTER_API_KEY': 'or-secret', 'OPENROUTER_HOST': 'https://router.test'}


class BillingFailureStops(IsolatedInvoke):
    def test_accounting_exception_stops_actual_entrant_without_exposing_key(self):
        def unavailable(*args, **kwargs):
            raise RuntimeError('accounting failed or-secret')
        with patch.object(bench_budget, 'WALLET_POLL_SECONDS', 0.01), \
                patch.object(bench_cost.urllib.request, 'urlopen', unavailable):
            result, out, err = self.invoke('spend', provider='openrouter', env=OPENROUTER,
                                          environ={'BENCH_MAX_USD': '1'})
        self.assertEqual(result['exit'], -signal.SIGTERM)
        self.assertEqual(result['budget']['stopped_by'], 'wallet_accounting_failure')
        self.assertEqual(result['budget']['wallet']['status'], 'failed')
        self.assertNotIn('or-secret', out + err)
        self.assertIn('[REDACTED]', err)
        self.assertIsNotNone(bench_budget.publication_abort(result, isolated_tiers.FORGE20))

    def test_missing_billing_records_cannot_run_unbounded(self):
        with patch.object(bench_budget, 'WALLET_POLL_SECONDS', 0.01), \
                patch.object(bench_cost, 'LATE_RECORD_BACKOFF', (0.001,) * 5), \
                patch.object(bench_cost, '_fetch', return_value=(None, 'HTTP 404')):
            result, _, _ = self.invoke('spend', provider='openrouter', env=OPENROUTER,
                                       environ={'BENCH_MAX_USD': '1'})
        self.assertEqual(result['exit'], -signal.SIGTERM)
        self.assertEqual(result['budget']['wallet']['status'], 'failed')
        self.assertIn('late-record retry policy', result['budget']['wallet']['reason'])


class InterruptedCapabilityEvidence(unittest.TestCase):
    def test_wallet_stop_is_unscored_for_forge2_but_call_budget_is_a_submission(self):
        wallet_stop = {'exit': -15, 'timed_out': False,
                       'budget': {'stopped_by': 'wallet_guard', 'wallet': {'status': 'tripped'}}}
        self.assertIsNotNone(bench_budget.publication_abort(wallet_stop, isolated_tiers.FORGE20))
        self.assertIsNone(bench_budget.publication_abort(wallet_stop, isolated_tiers.FORGE10))
        self.assertIsNone(bench_budget.publication_abort({'exit': 0, 'budget': {'stopped_by': 'call_budget'}},
                                                        isolated_tiers.FORGE20))
        with tempfile.TemporaryDirectory() as tmp, self.assertRaisesRegex(ValueError, 'No Forge 2.0 completion receipt'):
            bench_rescore.write_completion(Path(tmp)/'candidate', Path(tmp)/'receipt.json', wallet_stop,
                                            run_id='pilot', started_at=1, seed='0123456789abcdef', port=8850,
                                            provider='openrouter', model='xiaomi/mimo-v2.6-flash',
                                            tier=isolated_tiers.FORGE20)

    def test_unknown_billing_is_not_zero_cost_even_after_exit_zero(self):
        for wallet in ({'status': 'failed'}, {'status': 'unavailable'},
                       {'status': 'armed', 'unbilled': [{'id': 'gen-1-example', 'reason': 'HTTP 404'}]}):
            with self.subTest(wallet=wallet):
                agent = {'exit': 0, 'budget': {'stopped_by': 'model_finished', 'wallet': wallet}}
                self.assertIsNotNone(bench_budget.publication_abort(agent, isolated_tiers.FORGE20))


if __name__ == '__main__':
    unittest.main()
