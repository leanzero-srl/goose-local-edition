"""Actual reference sync with unchanged readable values; no model calls."""
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


@unittest.skipUnless(os.environ.get('SB71_BROWSER_TESTS') == '1', 'opt-in real-browser journey')
class SyncJourney(unittest.TestCase):
    def test_noop_sync_is_verified_against_backend_truth(self):
        bench = Path(__file__).resolve().parent
        root = Path(tempfile.mkdtemp(prefix='sb71-sync-proof-'))
        tree = root / 'candidate'
        shutil.copytree(bench / 'golden-sb71', tree, ignore=shutil.ignore_patterns('__pycache__'))
        app = tree / 'web/app.js'
        app.write_text(app.read_text().replace("'Last sync ' + fmtDate(s.last_sync)", "'Last sync just now'"))
        seed = '5a05d7631d9276e3'
        (root / 'tokens.json').write_text(json.dumps(fixtures_v3.build(seed).tokens))
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
                        if fetch(base + '/api/health').get('payments', 0) >= 12288:
                            break
                    except OSError:
                        pass
                    time.sleep(.2)
                else:
                    self.fail('Reference did not synchronize')
                result = subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'),
                    str(bench / 'product_probe_sb71.mjs'), 'sync', base],
                    env={**os.environ, 'BENCH_SHOTS_DIR': str(root / 'shots')},
                    capture_output=True, text=True, timeout=100)
                (root / 'probe.log').write_text(result.stderr)
                data = json.loads(result.stdout)
                (root / 'result.json').write_text(json.dumps(data, indent=2))
                print('Sync evidence:', root)
                causal = data['syncCausal']
                self.assertTrue(causal['completed'], data)
                self.assertTrue(causal['viewRefreshed'], data)
                self.assertTrue(causal['refreshedTruth']['ok'], data)
                self.assertNotEqual(causal['lastSyncBefore'], causal['lastSyncAfter'])
        finally:
            for child in children:
                score_sb71._kill_owned(child)
            vendor.shutdown()
            vendor.server_close()


if __name__ == '__main__':
    unittest.main()
