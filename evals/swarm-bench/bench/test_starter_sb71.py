import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path
import score_sb71


class StarterTests(unittest.TestCase):
    def test_combined_launcher_serves_assets_but_never_fakes_implemented_backend(self):
        def port():
            with socket.socket() as listener:
                listener.bind(('127.0.0.1', 0))
                return listener.getsockname()[1]
        ledger, notifier = port(), port()
        starter = Path(__file__).resolve().parents[1] / 'sb7.1/starter'
        with tempfile.TemporaryDirectory() as tmp:
            child = subprocess.Popen([sys.executable, '-m', 'app', '--db-dir', tmp,
                                      '--ledger-port', str(ledger), '--notifier-port', str(notifier),
                                      '--vendor', 'http://127.0.0.1:1', '--tokens-file', tmp + '/tokens.json'],
                                     cwd=starter, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                     start_new_session=True)
            try:
                limit = time.monotonic() + 5
                while True:
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{ledger}/', timeout=1) as response:
                            html = response.read().decode()
                        break
                    except OSError:
                        if time.monotonic() >= limit:
                            raise
                        time.sleep(.05)
                self.assertIn('Application behavior and visualization are not implemented', html)
                for endpoint in [f'http://127.0.0.1:{ledger}/api/health', f'http://127.0.0.1:{notifier}/health']:
                    with self.assertRaises(urllib.error.HTTPError) as error:
                        urllib.request.urlopen(endpoint, timeout=1)
                    self.assertEqual(error.exception.code, 501)
                    error.exception.close()
                self.assertFalse((Path(tmp) / 'ledger.db').exists())
            finally:
                score_sb71._kill_owned(child)


if __name__ == '__main__':
    unittest.main()
