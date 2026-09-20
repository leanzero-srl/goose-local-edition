import contextlib
import io
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import run_build


class CloudLaunchTests(unittest.TestCase):
    def test_known_setup_errors_are_actionable_without_exposing_engine_stderr(self):
        for marker, message in [
            ('Benchmark isolation cannot attach the managed MLX swarm engine', 'Single model'),
            ('Benchmarks require a Bedrock API key or explicit AWS credentials', 'AWS profile/SSO'),
        ]:
            with self.subTest(marker=marker), patch.object(run_build.subprocess, 'run', return_value=
                    subprocess.CompletedProcess([], 1, '', marker + ': secret-should-not-escape')):
                with self.assertRaises(RuntimeError) as error:
                    run_build.entrant_config(None)
                self.assertIn(message, str(error.exception))
                self.assertNotIn('secret-should-not-escape', str(error.exception))

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox integration')
    def test_sb71_actual_dispatch_is_isolated_and_omits_other_credentials(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / 'engine'
            private = root / 'private-reference.txt'
            private.write_text('private answer')
            engine.write_text('#!' + sys.executable + '''
import json, os, pathlib, subprocess, sys
assert 'UNRELATED_SECRET' not in os.environ
assert os.environ['GOOGLE_API_KEY'] == 'test-google-only'
try:
 pathlib.Path(sys.argv[0]).with_name('private-reference.txt').read_text()
except PermissionError:
 pass
else:
 raise SystemExit('private reference readable')
subprocess.run(['/bin/sh', '-c', 'printf child-shell-works > shell-proof.txt'], check=True)
pathlib.Path('request.json').write_text(json.dumps(sys.argv[1:]))
print('isolated dispatch passed')
''')
            engine.chmod(0o700)
            work = root / 'candidate'
            work.mkdir()
            with patch.object(run_build, 'GOOSE', engine), patch.dict(os.environ, {
                'BENCH_SB71': '1', 'UNRELATED_SECRET': 'must-not-inherit',
            }, clear=True), contextlib.redirect_stdout(io.StringIO()):
                result = run_build.invoke('google-fixture', work, 8850,
                                          {'GOOGLE_API_KEY': 'test-google-only'}, 0,
                                          'google', 'gemini-3.8-flash')
            self.assertEqual(result['exit'], 0, result['tail'])
            self.assertEqual((work / 'shell-proof.txt').read_text(), 'child-shell-works')
            prompt = json.loads((work / 'request.json').read_text())[-1]
            self.assertIn('VISUAL-CONTRACT.md', prompt)
            self.assertEqual(result['usage']['status'], 'unavailable')
            self.assertFalse(list(root.glob('isolation-control-*')))

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

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox integration')
    def test_swarm_console_redacts_metadata_secrets_without_suffix_heuristics(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / 'engine'
            engine.write_text('#!' + sys.executable + '\n' +
                              "import os,json,pathlib; print(os.environ['AWS_BEARER_TOKEN_BEDROCK'])\n"
                              "definition=json.loads((pathlib.Path(os.environ['GOOSE_PATH_ROOT'])/'config/custom_providers/example.json').read_text())\n"
                              "print(definition['headers']['X-Entrant-Authorization'])\n"
                              "assert 'X-Entrant-Authorization' not in os.environ\n")
            engine.chmod(0o700)
            work = root / 'candidate'
            work.mkdir()
            snapshot = {'config': {}, 'secrets': {'AWS_BEARER_TOKEN_BEDROCK': 'private-bedrock-token'},
                        'custom_providers': {'example': {'headers': {'X-Entrant-Authorization': 'private-custom-header'}}}}
            output = io.StringIO()
            with patch.object(run_build, 'GOOSE', engine), patch.dict(os.environ, {'BENCH_SB71': '1'}, clear=True), contextlib.redirect_stdout(output):
                result = run_build.invoke('swarm-3node', work, 8850, snapshot['secrets'], 0, snapshot=snapshot)
            self.assertEqual(result['exit'], 0)
            for content in [output.getvalue(), (work / 'engine-console.log').read_text(), result['tail']]:
                self.assertNotIn('private-bedrock-token', content)
                self.assertNotIn('private-custom-header', content)
                self.assertIn('[REDACTED]', content)

    def test_google_loader_only_returns_the_active_credential(self):
        with patch.dict(os.environ, {'GOOGLE_API_KEY': 'test-google', 'DEEPSEEK_API_KEY': 'not-for-google'}, clear=True):
            snapshot = {'providers': ['google'], 'config': {}, 'secrets': {'GOOGLE_API_KEY': 'saved-google'}}
            self.assertEqual(run_build.cloud_env('google', snapshot), {'GOOGLE_API_KEY': 'saved-google'})
            with self.assertRaises(ValueError):
                run_build.cloud_env('deepseek', snapshot)

    def test_local_discovery_and_provider_share_the_selected_endpoint(self):
        snapshot = {'config': {'LMSTUDIO_HOST': 'http://127.0.0.1:4321', 'swarm': {'devices': []}},
                    'secrets': {'LMSTUDIO_API_KEY': 'local-key'}}
        self.assertEqual(run_build.snapshot_environment(snapshot),
                         {'LMSTUDIO_HOST': 'http://127.0.0.1:4321', 'LMSTUDIO_API_KEY': 'local-key'})

    def test_selected_config_uses_engine_store_and_removes_private_transfer_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine = root / 'engine'
            marker = root / 'transfer-path'
            engine.write_text('#!' + sys.executable + '\n' + '''
import json,pathlib,sys
assert sys.argv[1]=='benchmark-config'
assert sys.argv[-2:]==['--provider','custom_provider']
output=pathlib.Path(sys.argv[3])
pathlib.Path(sys.argv[0]).with_name('transfer-path').write_text(str(output))
output.write_text(json.dumps({'version':1,'providers':['custom_provider'],'config':{'HOST':'https://custom.invalid'},'secrets':{'CUSTOM_KEY':'saved'},'custom_providers':{}}))
''')
            engine.chmod(0o700)
            with patch.object(run_build, 'GOOSE', engine):
                snapshot = run_build.entrant_config('custom_provider')
            self.assertEqual(snapshot['secrets'], {'CUSTOM_KEY': 'saved'})
            self.assertFalse(Path(marker.read_text()).exists())

    def test_google_limits_are_provider_metadata_not_generic_defaults(self):
        with patch.object(run_build.urllib.request,'urlopen',return_value=contextlib.closing(io.BytesIO(b'{"inputTokenLimit":1048576,"outputTokenLimit":65536}'))) as call:
            self.assertEqual(run_build.google_model_limits('gemini-3.8-flash',{'GOOGLE_API_KEY':'test-only'}),{'inputTokenLimit':1048576,'outputTokenLimit':65536})
            self.assertEqual(call.call_args.args[0].get_header('X-goog-api-key'),'test-only')
        with patch.object(run_build.urllib.request,'urlopen',return_value=contextlib.closing(io.BytesIO(b'{}'))):
            with self.assertRaises(ValueError):run_build.google_model_limits('gemini-3.8-flash',{'GOOGLE_API_KEY':'test-only'})
