import json
import os
from types import SimpleNamespace
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import bench_rescore as retry


class RetryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tree = self.root / 'candidate'
        self.tree.mkdir()
        (self.tree / 'app.py').write_text('print("candidate")\n')
        (self.tree / 'trace.jsonl').write_text(json.dumps({'fixture_seed': '0123456789abcdef'})+'\n')
        self.receipt_path = self.root / 'private' / 'receipt.json'
        self.agent = {'exit': 0, 'timed_out': False, 'secs': 498.2, 'tail': 'Completed build', 'usage': {'status': 'recorded'}}
        self.receipt = retry.write_completion(self.tree, self.receipt_path, self.agent,
            run_id='cloud-test', started_at='2026-09-20T20:00:00Z', seed='0123456789abcdef',
            port=8850, provider='google', model='controlled-test-model')

    def test_receipt_requires_actual_success_and_private_destination(self):
        self.assertEqual(retry.validate_receipt(self.receipt, self.tree, 'cloud-test'), self.receipt)
        for agent in [{**self.agent, 'exit': 1}, {**self.agent, 'timed_out': True}]:
            with self.assertRaisesRegex(ValueError, 'did not exit successfully'):
                retry.write_completion(self.tree, self.root / 'bad.json', agent, run_id='x', started_at='now', seed='0123456789abcdef', port=8850, provider=None, model=None)
        with self.assertRaisesRegex(ValueError, 'outside'):
            retry.write_completion(self.tree, self.tree / 'receipt.json', self.agent, run_id='x', started_at='now', seed='0123456789abcdef', port=8850, provider=None, model=None)

    def test_complete_inventory_detects_addition_deletion_change_and_symlink(self):
        for mutation in ['add', 'delete', 'change', 'symlink']:
            with self.subTest(mutation=mutation):
                source = self.tree / 'app.py'
                source.write_text('print("candidate")\n')
                extra = self.tree / 'extra.py'
                if mutation == 'add': extra.write_text('new code')
                if mutation == 'delete': source.unlink()
                if mutation == 'change': source.write_text('changed')
                if mutation == 'symlink': extra.symlink_to(source)
                with self.assertRaises(ValueError): retry.validate_receipt(self.receipt, self.tree, 'cloud-test')
                if extra.exists() or extra.is_symlink(): extra.unlink()
        source.write_text('print("candidate")\n')
        (self.tree / 'scoring-unavailable.json').write_text('{}')
        retry.validate_receipt(self.receipt, self.tree, 'cloud-test')

    def test_refuses_seed_identity_and_contract_mismatch(self):
        for field, value in [('runId', 'other'), ('fixture_seed', 'ffffffffffffffff'), ('contracts', {})]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                retry.validate_receipt({**self.receipt, field: value}, self.tree, 'cloud-test')

    def test_cli_only_invokes_scorer_and_preserves_original_candidate_and_duration(self):
        output = self.root / 'attempt'
        original = (self.tree / 'app.py').read_bytes()
        def score(command):
            self.assertEqual(Path(command[3]).name, 'score_sb71.py')
            self.assertNotIn('run_build.py', command)
            self.assertEqual(command[command.index('--seed')+1], '0123456789abcdef')
            self.assertEqual(command[command.index('--port')+1], '8850')
            report = Path(command[command.index('--json-out')+1])
            report.write_text(json.dumps({'score': 0.4, 'scorerVersion': 'sb-7.1', 'fixture_seed': '0123456789abcdef'}))
            return 0
        with patch.object(sys, 'argv', ['retry', '--receipt', str(self.receipt_path), '--tree', str(self.tree), '--run-id', 'cloud-test', '--out', str(output)]), patch.object(retry.subprocess, 'call', side_effect=score):
            self.assertEqual(retry.main(), 0)
        result = json.loads((output/'verdict.json').read_text())
        self.assertEqual(result['agent'], self.agent)
        self.assertEqual(result['model'], 'controlled-test-model')
        self.assertEqual((self.tree/'app.py').read_bytes(), original)
        self.assertFalse((self.tree/'verdict.json').exists())
        self.assertEqual(json.loads((output/'attempt.json').read_text())['status'], 'finished')

    def test_actual_scorer_subprocess_receives_exact_original_inputs(self):
        bundle = self.root / 'bundle'
        for name in retry.CONTRACTS:
            target = bundle / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((retry.ROOT / name).read_bytes())
        (bundle / 'bench').mkdir()
        (bundle / 'bench/score_sb71.py').write_text("""
import argparse,json
from pathlib import Path
p=argparse.ArgumentParser()
for key in ['tree','seed','port','json-out']:p.add_argument('--'+key,required=True)
a=p.parse_args()
assert a.seed=='0123456789abcdef' and a.port=='8850'
assert (Path(a.tree)/'app.py').read_text()=='print("candidate")\\n'
Path(a.json_out).write_text(json.dumps({'score':.4,'scorerVersion':'sb-7.1','fixture_seed':a.seed}))
""")
        output=self.root/'actual-process-attempt'
        with patch.object(retry, 'ROOT', bundle), patch.object(sys, 'argv', ['retry','--receipt',str(self.receipt_path),'--tree',str(self.tree),'--run-id','cloud-test','--out',str(output)]):
            self.assertEqual(retry.main(),0)
        self.assertEqual(json.loads((output/'verdict.json').read_text())['agent']['secs'],498.2)

    def test_runner_persists_completion_before_scorer_crashes(self):
        import run_build
        receipt = self.root / 'runner-private' / 'completion.json'
        def serve(port, trace, seed):
            trace.write_text(json.dumps({'fixture_seed': seed})+'\n')
            return SimpleNamespace(shutdown=lambda: None)
        def gather(*args, **kwargs):
            self.assertTrue(receipt.is_file())
            raise RuntimeError('controlled scoring crash')
        scorer = SimpleNamespace(_port_holder=lambda port: None,
            _probe_preflight=lambda: None, _draw_seed=lambda: '0123456789abcdef', gather=gather)
        vendor = SimpleNamespace(serve=serve, mark_phase=lambda *args: None)
        environment = {'BENCH_SB71': '1', 'BENCH_COMPLETION_RECEIPT': str(receipt),
            'BENCH_RUN_ID': 'completed-before-crash', 'BENCH_STARTED_AT': '2026-09-20T20:00:00Z'}
        with patch.dict(os.environ, environment, clear=True), \
             patch.object(run_build, '_regime', return_value=(scorer, vendor, None)), \
             patch.object(run_build, 'entrant_config', return_value={}), \
             patch.object(run_build, 'cloud_env', return_value={}), \
             patch.object(run_build, 'render_public_contract', side_effect=lambda text, *args: text), \
             patch.object(run_build, 'build_prompt', return_value='controlled prompt'), \
             patch.object(run_build, 'invoke', return_value=self.agent), \
             self.assertRaisesRegex(RuntimeError, 'controlled scoring crash'):
            run_build.run('controlled', 0, self.root / 'runs', 0, 8850,
                          provider='controlled-provider', model='controlled-model')
        actual = json.loads(receipt.read_text())
        self.assertEqual(actual['agent'], self.agent)
        self.assertEqual(actual['runId'], 'completed-before-crash')
        self.assertEqual(actual['fixture_seed'], '0123456789abcdef')
        retry.validate_receipt(actual, self.root / 'runs/controlled-r0', 'completed-before-crash')

    def test_failed_scorer_records_failure_without_installing_a_result(self):
        output = self.root / 'failed-attempt'
        with patch.object(sys, 'argv', ['retry', '--receipt', str(self.receipt_path), '--tree', str(self.tree), '--run-id', 'cloud-test', '--out', str(output)]), patch.object(retry.subprocess, 'call', return_value=2), self.assertRaisesRegex(RuntimeError, 'code 2'):
            retry.main()
        self.assertEqual(json.loads((output/'attempt.json').read_text())['status'], 'failed')
        self.assertFalse((self.tree/'verdict.json').exists())


if __name__ == '__main__':
    unittest.main()
