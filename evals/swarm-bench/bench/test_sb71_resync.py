import json
from http.server import ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import score_sb71 as score


class ResyncInterruptionTests(unittest.TestCase):
    def test_real_fast_conditional_walk_is_held_until_process_dies(self):
        self.exercise(304)

    def test_real_uncached_walk_uses_the_same_causal_interruption(self):
        self.exercise(200)

    def exercise(self, status):
        class Handler(score.vendor.Handler):
            def do_GET(self):
                self._send(status, b'{}' if status == 200 else None)

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.daemon_threads = True
        worker = threading.Thread(target=server.serve_forever)
        worker.start()
        process = None
        try:
            with tempfile.TemporaryDirectory() as directory:
                receipt = Path(directory) / 'responses.jsonl'
                script = '''import http.client,json,sys
c=http.client.HTTPConnection('127.0.0.1',int(sys.argv[1]))
with open(sys.argv[2],'w') as f:
 for i in range(192):
  c.request('GET','/v3/payments?cursor='+str(i),headers={'Authorization':'Bearer '+sys.argv[3]})
  r=c.getresponse();r.read();f.write(json.dumps({'status':r.status})+'\\n');f.flush()
'''
                def spawn():
                    return subprocess.Popen([sys.executable, '-c', script,
                        str(server.server_port), str(receipt), score.vendor.API_KEY],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)

                def kill(proc):
                    proc.kill()
                    proc.wait(timeout=5)

                original_send = score.vendor.Handler._send
                with patch.object(score.base, '_spawn_ledgerd', spawn), \
                     patch.object(score.base, '_kill', kill), \
                     patch.object(score.vendor, 'STATE', SimpleNamespace(sch=SimpleNamespace(sigkill_after_list=3))):
                    with score.resync_runtime(lambda name: None) as (mark, evidence):
                        mark('sync2')
                        process = score.base._spawn_ledgerd()
                        reached, seen = score.base._TraceTail(Path(directory) / 'unused').wait_for(
                            lambda row: row.get('status') == 200, 3, timeout=5)
                        self.assertTrue(reached)
                        self.assertEqual(seen, 3)
                        self.assertEqual([row['status'] for row in evidence['completed']], [status]*3)
                        self.assertIsNone(process.poll())
                        self.assertEqual(len(receipt.read_text().splitlines()), 3)
                        self.assertEqual(evidence['held'][0]['path'], '/v3/payments?cursor=3')
                        score.base._kill(process)
                        self.assertIsNotNone(evidence['kill']['returncode'])
                        self.assertTrue(evidence['kill']['response_still_held'])
                        self.assertGreater(evidence['kill']['terminated_at'], evidence['held'][0]['held_before_send_at'])
                        mark('sync3')
                    self.assertIs(score.vendor.Handler._send, original_send)
                self.assertEqual([json.loads(line)['status'] for line in receipt.read_text().splitlines()], [status]*3)
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            server.shutdown()
            server.server_close()
            worker.join()


if __name__ == '__main__':
    unittest.main()
