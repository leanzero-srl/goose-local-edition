"""Real approval response, committed identity and note-state evidence; no model calls."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

import fixtures_v3
import score_sb71
import vendor_service_v3
from test_sb71_visual import free_port, fetch


@unittest.skipUnless(os.environ.get('SB71_BROWSER_TESTS') == '1', 'opt-in browser journey')
class FlowJourney(unittest.TestCase):
    def test_actual_approval_and_payment_identity(self):
        bench = Path(__file__).resolve().parent
        root = Path(tempfile.mkdtemp(prefix='sb71-flow-proof-'))
        tree = root / 'candidate'
        original = Path(os.environ.get('SB71_CANDIDATE_TREE', str(bench / 'golden-sb71')))
        tree.mkdir()
        for directory in ('app', 'web'):
            shutil.copytree(original / directory, tree / directory, ignore=shutil.ignore_patterns('__pycache__'))
        seed = '4aaad9cc95c55bd7'
        fixture = fixtures_v3.build(seed)
        (root / 'tokens.json').write_text(json.dumps(fixture.tokens))
        pack = {'approval': {'drafts': fixture.schedule.approval_fixture}, 'tokens': fixture.tokens}
        (root / 'expect.json').write_text(json.dumps(pack))
        vp, lp, np = free_port(), free_port(), free_port()
        vendor = vendor_service_v3.serve(vp, root / 'trace.jsonl', seed)
        children = []
        try:
            with (root / 'apps.log').open('w') as log:
                for module, flags in [('notifierd', ['--port', str(np)]), ('ledgerd', [
                    '--port', str(lp), '--notifier', f'http://127.0.0.1:{np}',
                    '--vendor', f'http://127.0.0.1:{vp}', '--tokens-file', str(root / 'tokens.json')])]:
                    children.append(subprocess.Popen([sys.executable, '-m', 'app.' + module,
                        '--db-dir', str(root / 'db'), *flags], cwd=tree,
                        stdout=log, stderr=log, start_new_session=True))
                base = f'http://127.0.0.1:{lp}'
                deadline = time.monotonic() + 90
                while time.monotonic() < deadline:
                    try:
                        if fetch(base + '/api/payments?limit=1').get('total', 0) >= 12288: break
                    except OSError: pass
                    time.sleep(.2)
                else: self.fail('Application did not synchronize')
                with score_sb71.committed_payment_channel(root) as channel:
                    result = subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'),
                        str(bench / 'product_probe_sb71.mjs'), 'flow', base],
                        env={**os.environ, 'BENCH_SHOTS_DIR': str(root / 'shots'),
                             'SB7_EXPECT_FILE': str(root / 'expect.json'),
                             'BENCH_SB71_COMMITTED_PAYMENTS': str(channel)},
                        capture_output=True, text=True, timeout=180)
                (root / 'probe.log').write_text(result.stderr)
                data = json.loads(result.stdout)
                (root / 'result.json').write_text(json.dumps(data, indent=2))
                print('Flow evidence:', root, flush=True)
                self.assertTrue(data['approveCausal']['reachedApproved'], data.get('approveCausal'))
                self.assertTrue(data['paymentWitness']['ok'], data.get('paymentWitness'))
                self.assertTrue(data['optimistic']['paintedWhileHeld'], data.get('optimistic'))
        finally:
            for child in children: score_sb71._kill_owned(child)
            vendor.shutdown(); vendor.server_close()


if __name__ == '__main__': unittest.main()
