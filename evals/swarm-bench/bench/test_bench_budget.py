"""The benchmark budget: the published call budget and the operator's wallet guard (bench_budget.py)."""
import contextlib
from decimal import Decimal
import io
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import bench_budget
import bench_cost
import bench_rescore
import isolated_tiers
import run_build

GOOSE_AGENT_RS = Path(run_build.ROOT).parents[1] / 'crates' / 'goose' / 'src' / 'agents' / 'agent.rs'
MODEL = 'vendor/entrant-model'
TITLE_MODEL = 'google/gemini-2.5-flash'

# A fake `goose run`: records its argv, writes one telemetry line per "call" (the entrant model, plus
# the session-title call goose makes on the fast model) and ends the way its `mode` file says.
FAKE_ENGINE = '''
import json, os, pathlib, sys, time
pathlib.Path('request.json').write_text(json.dumps(sys.argv[1:]))
sink = os.environ['GOOSE_SWARM_TELEMETRY_FILE']
def call(model, n):
    with open(sink, 'a') as fh:
        fh.write(json.dumps({'model': model, 'response_id': 'gen-1790948725-' + 'c%%04d' %% n,
                             'ended': 'completed'}) + '\\n')
call(%(title)r, 0)
mode = pathlib.Path(sys.argv[0]).with_name('mode').read_text()
if mode == 'budget':
    for n in range(1, 4):
        call(%(model)r, n)
    print('The model returned an empty response. Please resend your message to continue.')
    print('Wrote web/app.js.')
    print(%(closing)r)
elif mode == 'finished':
    call(%(model)r, 1)
    print('Done. The app serves on http://127.0.0.1:8080.')
elif mode == 'spend':
    n = 0
    while True:
        n += 1
        call(%(model)r, n)
        print('working on call', n, flush=True)
        time.sleep(0.05)
'''


def _engine(root: Path) -> Path:
    engine = root / 'engine'
    engine.write_text('#!' + sys.executable + '\n' + FAKE_ENGINE % {
        'title': TITLE_MODEL, 'model': MODEL, 'closing': bench_budget.MAX_TURNS_MESSAGE})
    engine.chmod(0o700)
    return engine


def _billing(per_call_usd: str, seen: list):
    """OpenRouter's /api/v1/generation, answering every id with the same billed usage."""
    class Response(io.BytesIO):
        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

    def urlopen(request, timeout=None):
        seen.append(request.full_url)
        return Response(json.dumps({'data': {'usage': float(per_call_usd)}}).encode())
    return urlopen


class IsolatedInvoke(unittest.TestCase):
    """invoke() under an isolated tier, with the sandbox replaced by a plain runtime directory."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.engine = _engine(self.root)
        self.work = self.root / 'candidate'
        self.work.mkdir()
        self.runtime = self.root / 'runtime'
        self.runtime.mkdir()

    def invoke(self, mode, provider='google', env=None, environ=None, entrant='fixture', model=MODEL):
        prepared = ([], {'BENCH_SB71_RUNTIME': str(self.runtime), 'PATH': os.environ.get('PATH', '')})
        (self.root / 'mode').write_text(mode)
        with patch.object(run_build, 'GOOSE', self.engine), \
                patch.dict(os.environ, {'BENCH_SB72': '1', **(environ or {})}, clear=True), \
                patch('bench_isolation.prepare', return_value=prepared), \
                patch('bench_isolation.usage', return_value={'status': 'unavailable', 'reason': 'test'}), \
                patch.object(run_build, 'reap_entrant_survivors', return_value={'reaped_processes': []}), \
                contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(io.StringIO()) as err:
            result = run_build.invoke(entrant, self.work, 8850, env or {'GOOGLE_API_KEY': 'k'}, 0,
                                      provider, model,
                                      {'version': 1, 'providers': [provider], 'config': {}, 'secrets': {},
                                       'custom_providers': {}})
        return result, out.getvalue(), err.getvalue()

    def args(self):
        return json.loads((self.work / 'request.json').read_text())


class CallBudgetTests(IsolatedInvoke):
    def test_an_isolated_single_model_entrant_gets_the_published_budget_before_its_prompt(self):
        result, out, _ = self.invoke('finished')
        args = self.args()
        at = args.index('--max-turns')
        self.assertEqual(args[at + 1], str(bench_budget.CALL_BUDGET))
        self.assertEqual(args[at + 2], '-t')
        self.assertEqual(result['budget'], {
            'max_calls': bench_budget.CALL_BUDGET, 'calls_used': 1,
            'calls_source': 'telemetry (provider calls on the entrant model)',
            'calls_other_models': {TITLE_MODEL: 1}, 'stopped_by': 'model_finished'})
        self.assertIn('BENCH_BUDGET ', out)
        self.assertNotIn('wallet', result['budget'])

    def test_a_session_ended_by_the_budget_is_recorded_as_a_budget_stop(self):
        result, _, _ = self.invoke('budget')
        self.assertEqual(result['exit'], 0)
        self.assertEqual(result['budget']['stopped_by'], 'call_budget')
        self.assertEqual(result['budget']['calls_used'], 3)
        cost = json.loads((self.work / 'model-cost.json').read_text())
        self.assertEqual(cost['budget'], result['budget'])

    def test_a_swarm_entrant_is_not_budgeted(self):
        result, _, _ = self.invoke('finished', provider=None, env={}, entrant='swarm-1node', model=None)
        self.assertNotIn('--max-turns', self.args())
        self.assertEqual(self.args()[0], 'swarm')
        self.assertNotIn('budget', result)

    def test_the_budget_is_one_named_policy(self):
        self.assertEqual(bench_budget.call_budget_args(isolated_tiers.SB72), ['--max-turns', str(bench_budget.CALL_BUDGET)])
        sources = [path for path in Path(run_build.HERE).glob('*.py')
                   if path.name != 'test_bench_budget.py'
                   and '--max-turns' in path.read_text()]
        self.assertEqual([path.name for path in sources], ['bench_budget.py'])

    def test_every_budget_a_public_contract_states_is_the_policy(self):
        self.assertEqual(bench_budget.stated_budgets('You have a budget of 1,500 model calls.'), [1500])
        root = Path(run_build.ROOT)
        for tier in isolated_tiers.TIERS:
            for contract in tier.contracts:
                with self.subTest(contract=contract):
                    for stated in bench_budget.stated_budgets((root / contract).read_text()):
                        self.assertEqual(stated, tier.call_budget)

    @unittest.skipUnless(GOOSE_AGENT_RS.is_file(), 'the goose sources ship only with the checkout')
    def test_the_budget_closing_is_the_sentence_goose_writes(self):
        self.assertIn(f'MAX_TURNS_MESSAGE: &str = "{bench_budget.MAX_TURNS_MESSAGE}"',
                      GOOSE_AGENT_RS.read_text())


class WalletGuardTests(IsolatedInvoke):
    OPENROUTER = {'OPENROUTER_API_KEY': 'or-secret', 'OPENROUTER_HOST': 'https://router.test'}

    def test_the_guard_stops_the_entrant_pid_at_the_limit(self):
        seen = []
        with patch.object(bench_budget, 'WALLET_POLL_SECONDS', 0.05), \
                patch.object(bench_cost.urllib.request, 'urlopen', _billing('0.4', seen)), \
                patch.object(run_build.os, 'killpg', side_effect=AssertionError('gate 4: no group signal')):
            result, out, err = self.invoke('spend', provider='openrouter', env=self.OPENROUTER,
                                           environ={'BENCH_MAX_USD': '1.00'})
        budget = result['budget']
        self.assertEqual(budget['stopped_by'], 'wallet_guard')
        self.assertEqual(result['exit'], -signal.SIGTERM)
        wallet = budget['wallet']
        self.assertEqual(wallet['status'], 'tripped')
        self.assertEqual(wallet['max_usd'], 1.0)
        self.assertGreaterEqual(wallet['tripped']['spent_usd'], 1.0)
        self.assertEqual(wallet['tripped']['signal'], 'SIGTERM')
        self.assertIn('WALLET GUARD: OpenRouter billed', err)
        self.assertTrue(all(url.startswith('https://router.test/api/v1/generation?id=gen-') for url in seen))
        self.assertNotIn('or-secret', out + err)

    def test_an_entrant_that_finishes_under_the_limit_is_not_stopped(self):
        with patch.object(bench_budget, 'WALLET_POLL_SECONDS', 0.05), \
                patch.object(bench_cost.urllib.request, 'urlopen', _billing('0.01', [])):
            result, _, _ = self.invoke('finished', provider='openrouter', env=self.OPENROUTER,
                                       environ={'BENCH_MAX_USD': '5'})
        self.assertEqual(result['budget']['stopped_by'], 'model_finished')
        self.assertEqual(result['budget']['wallet']['status'], 'armed')
        self.assertEqual(result['budget']['wallet']['calls_billed'], 2)

    def test_a_provider_without_a_per_call_bill_is_a_loud_unguarded_record(self):
        result, _, err = self.invoke('finished', environ={'BENCH_MAX_USD': '5'})
        wallet = result['budget']['wallet']
        self.assertEqual(wallet['status'], 'unavailable')
        self.assertIn('Google exposes no per-call bill', wallet['reason'])
        self.assertIn('WALLET GUARD UNAVAILABLE (google)', err)

    def test_the_last_read_after_exit_never_counts_as_a_stop(self):
        process = SimpleNamespace(pid=os.getpid(), poll=lambda: 0)
        with tempfile.TemporaryDirectory() as tmp:
            sink = Path(tmp) / 'telemetry.jsonl'
            sink.write_text(json.dumps({'response_id': 'gen-1-aaaa'}) + '\n')
            guard = bench_budget.WalletGuard(process, sink, Decimal('0.5'), 'k', 'https://router.test',
                                             urlopen=_billing('2', []), poll_seconds=60)
            guard.start()
            guard.stop()
        self.assertEqual(guard.record()['status'], 'armed')
        self.assertEqual(guard.record()['spent_usd_seen'], 2.0)

    def test_a_malformed_limit_is_refused_and_an_unset_one_is_off(self):
        self.assertIsNone(bench_budget.wallet_limit({}))
        self.assertEqual(bench_budget.wallet_limit({'BENCH_MAX_USD': '$2.50'}), Decimal('2.50'))
        for raw in ('abc', '0', '-1', 'nan', 'inf'):
            with self.subTest(raw=raw), self.assertRaisesRegex(RuntimeError, 'REFUSED: BENCH_MAX_USD'):
                bench_budget.wallet_limit({'BENCH_MAX_USD': raw})


class BudgetStopIsScoredTests(unittest.TestCase):
    """run(): a budget stop is the run's scored end; the same console without one is still refused."""

    def run_with(self, agent):
        scorer = SimpleNamespace(_port_holder=lambda port: None, gather=lambda *a, **k: 'ctx',
                                 evaluate=lambda ctx: {'score': 0.25})
        vendor = SimpleNamespace(serve=lambda port, trace: trace.write_text('') or
                                 SimpleNamespace(shutdown=lambda: None, server_close=lambda: None),
                                 mark_phase=lambda *a: None)
        snapshot = {'version': 1, 'providers': ['omlx'], 'config': {}, 'secrets': {'OMLX_KEY': 'k'},
                    'custom_providers': {}}
        regime_flags = ('BENCH_SB7', 'BENCH_SB71', 'BENCH_SB72', 'BENCH_SB8', 'BENCH_COMPLETION_RECEIPT',
                        'BENCH_MAX_USD')
        environ = {name: value for name, value in os.environ.items() if name not in regime_flags}
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        with patch.dict(os.environ, environ, clear=True), \
                patch.object(run_build, '_regime', return_value=(scorer, vendor, None)), \
                patch.object(run_build, 'entrant_config', return_value=snapshot), \
                patch.object(run_build, 'invoke', return_value=agent), \
                contextlib.redirect_stdout(io.StringIO()):
            verdict = run_build.run('omlx-fixture', 0, Path(tmp.name), 0, 8850, 'omlx', 'local-27b')
        return verdict, Path(tmp.name) / 'omlx-fixture-r0'

    def test_a_call_budget_stop_after_an_empty_reply_is_scored(self):
        tail = ('The model returned an empty response. Please resend your message to continue.\n'
                + bench_budget.MAX_TURNS_MESSAGE + '\n')
        budget = {'max_calls': bench_budget.CALL_BUDGET, 'calls_used': 150, 'stopped_by': 'call_budget'}
        verdict, tree = self.run_with({'exit': 0, 'secs': 60.0, 'tail': tail, 'timed_out': False,
                                       'budget': budget})
        self.assertEqual(verdict['score'], 0.25)
        self.assertEqual(verdict['budget'], budget)
        self.assertEqual(json.loads((tree / 'verdict.json').read_text())['agent']['budget'], budget)

    def test_a_wallet_stop_on_a_provider_closer_is_scored(self):
        tail = 'Network error: connection reset.\n\nPlease resend your message to try again.\n'
        budget = {'max_calls': bench_budget.CALL_BUDGET, 'calls_used': 40, 'stopped_by': 'wallet_guard',
                  'wallet': {'status': 'tripped', 'max_usd': 1.0}}
        verdict, _ = self.run_with({'exit': -15, 'secs': 60.0, 'tail': tail, 'timed_out': False,
                                    'budget': budget})
        self.assertEqual(verdict['budget']['stopped_by'], 'wallet_guard')

    def test_without_a_budget_stop_the_refusals_still_hold(self):
        tail = 'The model returned an empty response. Please resend your message to continue.\n'
        for budget in (None, {'max_calls': 150, 'calls_used': 9, 'stopped_by': 'model_finished'}):
            agent = {'exit': 0, 'secs': 60.0, 'tail': tail, 'timed_out': False}
            if budget:
                agent['budget'] = budget
            with self.subTest(budget=budget), self.assertRaisesRegex(RuntimeError, 'REFUSED: provider ended'):
                self.run_with(agent)


class CompletionReceiptTests(unittest.TestCase):
    def test_a_wallet_stop_is_completion_evidence_and_a_bare_signal_is_not(self):
        wallet_stop = {'exit': -15, 'timed_out': False, 'secs': 9.0,
                       'budget': {'stopped_by': 'wallet_guard', 'wallet': {'status': 'tripped'}}}
        self.assertEqual(bench_rescore.completion_evidence(wallet_stop),
                         'runner stopped the engine at the operator wallet limit')
        self.assertEqual(bench_rescore.completion_evidence({'exit': 0, 'timed_out': False}),
                         'runner observed engine exit 0')
        for agent in ({'exit': -15, 'timed_out': False},
                      {**wallet_stop, 'budget': {'stopped_by': 'wallet_guard', 'wallet': {'status': 'armed'}}},
                      {**wallet_stop, 'exit': 1}, {**wallet_stop, 'timed_out': True}):
            with self.subTest(agent=agent):
                self.assertIsNone(bench_rescore.completion_evidence(agent))


if __name__ == '__main__':
    unittest.main()
