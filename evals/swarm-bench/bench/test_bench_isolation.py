import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

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
            program = '''import pathlib,sys
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


if __name__ == '__main__':
    unittest.main()
