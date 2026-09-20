import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import run_build


class CloudLaunchTests(unittest.TestCase):
    def test_google_invocation_uses_rendered_sb8_and_redacts_before_logging(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / 'engine'
            engine.write_text('#!' + sys.executable + '''
import json, os, pathlib, sys
pathlib.Path('request.json').write_text(json.dumps(sys.argv[1:]))
print(os.environ['GOOGLE_API_KEY'])
print('revision 1, endpoint 127.0.0.1')
''')
            engine.chmod(0o700)
            work = root / 'tree'
            work.mkdir()
            stdout = io.StringIO()
            with patch.object(run_build, 'GOOSE', engine), patch.dict(os.environ, {'BENCH_SB8': '1'}, clear=True), contextlib.redirect_stdout(stdout):
                result = run_build.invoke('google-fixture', work, 8850, {'GOOGLE_API_KEY': 'test-secret-only'}, 0, 'google', 'gemini-3.8-flash')
            args = json.loads((work / 'request.json').read_text())
            self.assertEqual(args[:5], ['run', '--provider', 'google', '--model', 'gemini-3.8-flash'])
            self.assertIn('--no-profile', args)
            self.assertIn('developer', args)
            self.assertIn('http://127.0.0.1:8850', args[-1])
            self.assertNotIn('{DOCS_URL}', args[-1])
            self.assertEqual(result['exit'], 0)
            for text in [stdout.getvalue(), (work / 'engine-console.log').read_text(), result['tail']]:
                self.assertNotIn('test-secret-only', text)
                self.assertIn('[REDACTED]', text)
                self.assertIn('revision 1, endpoint 127.0.0.1', text)

    def test_existing_tree_is_preserved_before_any_vendor_or_model_start(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tree = root / 'google-fixture-r0'
            tree.mkdir()
            source = tree / 'app.py'
            source.write_text('expensive output')
            with self.assertRaises(FileExistsError):
                run_build.run('google-fixture', 0, root, 0, 8850, 'google', 'gemini-3.8-flash')
            self.assertEqual(source.read_text(), 'expensive output')

    def test_google_loader_only_returns_the_active_credential(self):
        with patch.dict(os.environ, {'GOOGLE_API_KEY': 'test-google', 'DEEPSEEK_API_KEY': 'not-for-google'}, clear=True):
            self.assertEqual(run_build.cloud_env('google'), {'GOOGLE_API_KEY': 'test-google'})

    def test_google_limits_are_provider_metadata_not_generic_defaults(self):
        with patch.object(run_build.urllib.request,'urlopen',return_value=contextlib.closing(io.BytesIO(b'{"inputTokenLimit":1048576,"outputTokenLimit":65536}'))) as call:
            self.assertEqual(run_build.google_model_limits('gemini-3.8-flash',{'GOOGLE_API_KEY':'test-only'}),{'inputTokenLimit':1048576,'outputTokenLimit':65536})
            self.assertEqual(call.call_args.args[0].get_header('X-goog-api-key'),'test-only')
        with patch.object(run_build.urllib.request,'urlopen',return_value=contextlib.closing(io.BytesIO(b'{}'))):
            with self.assertRaises(ValueError):run_build.google_model_limits('gemini-3.8-flash',{'GOOGLE_API_KEY':'test-only'})
