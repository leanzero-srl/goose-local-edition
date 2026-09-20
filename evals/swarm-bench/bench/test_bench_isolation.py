import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import bench_isolation


@unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox integration')
class IsolationTests(unittest.TestCase):
    def test_real_reads_writes_reference_and_operator_log_boundaries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work, private = root / 'candidate', root / 'private'
            work.mkdir()
            private.mkdir()
            reference = private / 'reference.py'
            reference.write_text('private answer')
            console = work / 'engine-console.log'
            console.write_text('operator log')
            prefix, env = bench_isolation.prepare(work, Path(sys.executable), private)
            program = '''import pathlib,sys,zoneinfo,datetime
berlin=zoneinfo.ZoneInfo('Europe/Berlin')
assert datetime.datetime(2026,1,1,tzinfo=berlin).utcoffset()==datetime.timedelta(hours=1)
assert datetime.datetime(2026,7,1,tzinfo=berlin).utcoffset()==datetime.timedelta(hours=2)
p=pathlib.Path('app.txt');p.write_text('candidate');assert p.read_text()=='candidate'
for name in sys.argv[1:]:
 try:pathlib.Path(name).read_text()
 except PermissionError:pass
 else:raise SystemExit('private read permitted: '+name)
print('real isolation passed')
'''
            process = subprocess.run(prefix + [sys.executable, '-c', program, str(reference), str(console)],
                                     cwd=work, env={**os.environ, **env}, capture_output=True, text=True)
            self.assertEqual(process.returncode, 0, process.stderr + process.stdout)
            self.assertIn('real isolation passed', process.stdout)

    def test_packaged_node_cannot_expose_packaged_private_scorer(self):
        wrapper = Path('/Applications/Goose.app/Contents/Resources/bin/node')
        scorer = wrapper.parent.parent / 'swarm-bench/bench/score_sb7.py'
        if not wrapper.is_file() or not scorer.is_file():
            self.skipTest('Installed packaged benchmark required for this integration control')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / 'candidate'
            work.mkdir()
            with patch.object(bench_isolation.shutil, 'which', return_value=str(wrapper)):
                prefix, env = bench_isolation.prepare(work, Path(sys.executable), root)
            program = """import pathlib,subprocess,sys
try:pathlib.Path(sys.argv[1]).read_bytes()
except PermissionError:pass
else:raise SystemExit('actual packaged scorer readable')
assert subprocess.check_output(['node','-p','1+1'],text=True).strip() == '2'
print('actual Node works; packaged scorer denied')
"""
            result = subprocess.run(prefix + [sys.executable, '-c', program, str(scorer)],
                                    cwd=work, env={**os.environ, **env}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertIn('packaged scorer denied', result.stdout)


if __name__ == '__main__':
    unittest.main()
