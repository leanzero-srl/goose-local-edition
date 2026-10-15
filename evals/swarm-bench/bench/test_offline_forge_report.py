"""Pure offline gates; no candidate, browser, probe or model is executed."""
import copy
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest

import offline_forge_report as offline


class OfflineReportTests(unittest.TestCase):
    def test_denial_blocks_process_socket_write_and_unpinned_reads(self):
        with tempfile.TemporaryDirectory(dir='/private/tmp') as tmp:
            source = Path(tmp) / 'known'; source.write_text('known')
            unknown = Path(tmp) / 'unknown'; unknown.write_text('unknown')
            with offline.denied_runtime({str(source.resolve())}) as proof:
                self.assertEqual(source.read_text(), 'known')
                for operation in (lambda: subprocess.run(['true']), lambda: socket.socket(),
                                  lambda: source.write_text('changed'), lambda: unknown.read_text()):
                    with self.assertRaises(offline.Refused):
                        operation()
            self.assertEqual(len(proof['attempts']), 4)
            self.assertEqual(source.read_text(), 'known')

    def test_numeric_gate_checks_each_seed_and_aggregate(self):
        seeds = ['0' * 16, '1' * 16, '2' * 16]
        old = {'checks': [{'check': 'a', 'score': .2, 'seeds': dict(zip(seeds, [.2, .4, .6]))}]}
        rows = [[{'check': 'a', 'score': n}] for n in [.2, .4, .6]]
        merged = [{'check': 'a', 'score': .2, 'seeds': old['checks'][0]['seeds']}]
        offline.check_numerics(old, rows, merged, seeds)
        changed = copy.deepcopy(rows); changed[1][0]['score'] = .5
        with self.assertRaisesRegex(offline.Refused, 'FIXTURE_NUMERIC_MISMATCH'):
            offline.check_numerics(old, changed, merged, seeds)
        changed = copy.deepcopy(merged); changed[0]['score'] = .1
        with self.assertRaisesRegex(offline.Refused, 'AGGREGATE_NUMERIC_MISMATCH'):
            offline.check_numerics(old, rows, changed, seeds)

    def test_missing_or_modified_original_bytes_refuse(self):
        with tempfile.TemporaryDirectory(dir='/private/tmp') as tmp:
            file = Path(tmp) / 'a'; file.write_bytes(b'original')
            pins = {'a': offline.sha(b'original')}
            offline.verify_files(tmp, pins)
            file.write_bytes(b'changed')
            with self.assertRaisesRegex(offline.Refused, 'INPUT_PIN_MISMATCH'):
                offline.verify_files(tmp, pins)
            file.unlink()
            with self.assertRaisesRegex(offline.Refused, 'NONREGULAR_INPUT'):
                offline.verify_files(tmp, pins)

    def test_public_projection_preserves_each_detail_and_omits_parts(self):
        row = {'check': 'economy', 'detail': 'mean ' + 'x' * 300, 'seed_evidence': [
            {'seed': 'a' * 16, 'score': .5, 'detail': 'Bearer abcdefghijk ' + 'q' * 2200, 'parts': {'private': 1}},
            {'seed': 'b' * 16, 'score': .8, 'detail': 'second fixture ratio=1.25'}]}
        public = offline.public_rows([row], [])[0]
        self.assertEqual(len(public['detail']), 220)
        self.assertEqual(len(public['fixtureEvidence'][0]['detail']), 2000)
        self.assertNotIn('abcdefghijk', public['fixtureEvidence'][0]['detail'])
        self.assertEqual(public['fixtureEvidence'][1]['detail'], 'second fixture ratio=1.25')
        self.assertEqual(set(public['fixtureEvidence'][0]), {'seed', 'score', 'detail'})

    def test_pure_fixture_loader_refuses_process_and_filesystem(self):
        helper = Path(__file__).with_name('offline_forge_pack.cjs')
        script = '''const {dataPack}=require(process.argv[1]);
        for(const body of ["require('child_process')", "require('fs').readFileSync('x')", "require('./site.cjs')"]){
          const sources=new Map([['fixtures.cjs',Buffer.from(body)]]);
          let refused=false;try{dataPack(sources,'aaaaaaaaaaaaaaaa')}catch(error){refused=/FORBIDDEN_FIXTURE/.test(error.message)}
          if(!refused)throw new Error('missing denial');
        }
        console.log('three forbidden fixture operations refused');'''
        result = subprocess.run(['node', '-e', script, str(helper)], text=True, capture_output=True, check=True)
        self.assertIn('three forbidden', result.stdout)


if __name__ == '__main__':
    unittest.main()
