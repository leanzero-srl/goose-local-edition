import contextlib
import io
import json
import os
from pathlib import Path
import signal
import sys
import subprocess
import tempfile
import time
import unittest
from types import SimpleNamespace
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


def _listing(*entries):
    return contextlib.closing(io.BytesIO(json.dumps({'data': list(entries)}).encode()))


ASTRA = {'id': 'openai/gpt-6-astra', 'context_length': 1050000,
         'top_provider': {'context_length': 1050000, 'max_completion_tokens': 128000}}


class ProviderModelLimitsTests(unittest.TestCase):
    def setUp(self):
        patcher = patch.dict(os.environ, {}, clear=True)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_openrouter_takes_the_exact_id_and_records_its_source(self):
        decoy = {**ASTRA, 'id': 'openai/gpt-6-astra:thinking', 'context_length': 7,
                 'top_provider': {'max_completion_tokens': 3}}
        with patch.object(run_build.urllib.request, 'urlopen', return_value=_listing(decoy, ASTRA)) as call:
            limits = run_build.provider_model_limits('openrouter', 'openai/gpt-6-astra',
                                                     {'OPENROUTER_API_KEY': 'test-or',
                                                      'GOOSE_CONTEXT_LIMIT': '128000'})
        request = call.call_args.args[0]
        self.assertEqual(request.full_url, 'https://openrouter.ai/api/v1/models')
        self.assertEqual(request.get_header('Authorization'), 'Bearer test-or')
        self.assertEqual(limits['GOOSE_CONTEXT_LIMIT'], '1050000')
        # The listed output cap is recorded, never sent: it narrows the backend pool (run 9e652114).
        self.assertNotIn('GOOSE_MAX_TOKENS', limits)
        self.assertIsNone(limits['provenance']['max_tokens_sent'])
        provenance = limits['provenance']
        self.assertEqual(provenance['source'], 'openrouter-model-metadata')
        self.assertEqual(provenance['fields'], {'context_length': 1050000,
                                                'top_provider.max_completion_tokens': 128000})
        self.assertEqual(provenance['replaced_goose_config'], {'GOOSE_CONTEXT_LIMIT': '128000'})
        self.assertNotIn('test-or', json.dumps(limits))

    def test_openrouter_queries_the_configured_host(self):
        with patch.object(run_build.urllib.request, 'urlopen', return_value=_listing(ASTRA)) as call:
            run_build.provider_model_limits('openrouter', 'openai/gpt-6-astra',
                                            {'OPENROUTER_API_KEY': 'k', 'OPENROUTER_HOST': 'https://or.example/'})
        self.assertEqual(call.call_args.args[0].full_url, 'https://or.example/api/v1/models')

    def test_openrouter_missing_model_is_refused(self):
        with patch.object(run_build.urllib.request, 'urlopen', return_value=_listing(ASTRA)):
            with self.assertRaisesRegex(RuntimeError, r"REFUSED: OpenRouter lists 0 models with id exactly 'openai/gpt-6'"):
                run_build.provider_model_limits('openrouter', 'openai/gpt-6', {'OPENROUTER_API_KEY': 'k'})

    def test_openrouter_non_positive_or_null_limits_are_refused(self):
        for context, output in [(0, 128000), (1050000, None), (1050000, -1), ('1050000', 128000), (True, 128000)]:
            entry = {**ASTRA, 'context_length': context, 'top_provider': {'max_completion_tokens': output}}
            with self.subTest(context=context, output=output), \
                    patch.object(run_build.urllib.request, 'urlopen', return_value=_listing(entry)):
                with self.assertRaisesRegex(RuntimeError, 'REFUSED: OpenRouter .* not a positive integer'):
                    run_build.provider_model_limits('openrouter', 'openai/gpt-6-astra', {'OPENROUTER_API_KEY': 'k'})

    def test_openrouter_without_a_key_is_refused_before_any_request(self):
        with patch.object(run_build.urllib.request, 'urlopen') as call:
            with self.assertRaisesRegex(RuntimeError, 'OPENROUTER_API_KEY is absent'):
                run_build.provider_model_limits('openrouter', 'openai/gpt-6-astra', {})
        call.assert_not_called()

    def test_bedrock_without_explicit_limits_is_refused(self):
        with patch.object(run_build.urllib.request, 'urlopen') as call:
            with self.assertRaisesRegex(RuntimeError, 'REFUSED: no source for aws_bedrock/us.anthropic.claude-fable-5-1'):
                run_build.provider_model_limits('aws_bedrock', 'us.anthropic.claude-fable-5-1', {})
        call.assert_not_called()

    def test_bedrock_takes_explicit_operator_limits(self):
        with patch.dict(os.environ, {'BENCH_CONTEXT_LIMIT': '1000000', 'BENCH_MAX_TOKENS': '128000'}):
            limits = run_build.provider_model_limits('aws_bedrock', 'us.anthropic.claude-fable-5-1', {})
        self.assertEqual((limits['GOOSE_CONTEXT_LIMIT'], limits['GOOSE_MAX_TOKENS']), ('1000000', '128000'))
        self.assertEqual(limits['provenance']['source'], 'operator-env')

    def test_explicit_limits_must_be_complete_and_positive(self):
        for env, message in [({'BENCH_CONTEXT_LIMIT': '1000000'}, 'BENCH_MAX_TOKENS unset'),
                             ({'BENCH_CONTEXT_LIMIT': '0', 'BENCH_MAX_TOKENS': '4096'}, 'BENCH_CONTEXT_LIMIT is 0'),
                             ({'BENCH_CONTEXT_LIMIT': '1M', 'BENCH_MAX_TOKENS': '4096'}, "BENCH_CONTEXT_LIMIT is '1M'"),
                             ({'BENCH_CONTEXT_LIMIT': '1000000', 'BENCH_MAX_TOKENS': str(2**31)}, 'i32')]:
            with self.subTest(env=env), patch.dict(os.environ, env):
                with self.assertRaisesRegex(RuntimeError, message):
                    run_build.provider_model_limits('aws_bedrock', 'us.anthropic.claude-fable-5-1', {})

    def test_google_path_is_unchanged(self):
        with patch.object(run_build.urllib.request, 'urlopen', return_value=contextlib.closing(
                io.BytesIO(b'{"inputTokenLimit":1048576,"outputTokenLimit":65536}'))) as call:
            limits = run_build.provider_model_limits('google', 'gemini-3.8-flash', {'GOOGLE_API_KEY': 'test-only'})
        self.assertEqual(call.call_args.args[0].get_header('X-goog-api-key'), 'test-only')
        self.assertEqual((limits['GOOSE_CONTEXT_LIMIT'], limits['GOOSE_MAX_TOKENS']), ('1048576', '65536'))
        self.assertEqual(limits['provenance']['fields'], {'inputTokenLimit': 1048576, 'outputTokenLimit': 65536})
        with patch.object(run_build.urllib.request, 'urlopen', return_value=contextlib.closing(io.BytesIO(b'{}'))):
            with self.assertRaises(ValueError):
                run_build.provider_model_limits('google', 'gemini-3.8-flash', {'GOOGLE_API_KEY': 'test-only'})

    def test_providers_without_a_metadata_path_proceed_with_a_named_absence(self):
        with patch.object(run_build.urllib.request, 'urlopen') as call:
            for provider in ('omlx', 'ollama', 'anthropic', 'openai'):
                with self.subTest(provider=provider):
                    record = run_build.provider_model_limits(provider, 'some-model', {})
                    self.assertEqual(record['status'], 'provider_default_unverified')
                    self.assertIn(f'no limits metadata path for provider {provider}', record['reason'])
                    self.assertIn('DEFAULT_CONTEXT_LIMIT 128000', record['reason'])
                    self.assertNotIn('GOOSE_CONTEXT_LIMIT', record)
                    self.assertNotIn('goose_config', record)
            configured = run_build.provider_model_limits('omlx', 'm', {'GOOSE_CONTEXT_LIMIT': '262144'})
            self.assertEqual(configured['goose_config'], {'GOOSE_CONTEXT_LIMIT': '262144'})
            self.assertIn("goose's own config sets GOOSE_CONTEXT_LIMIT", configured['reason'])
        call.assert_not_called()

    def test_operator_limits_win_for_a_provider_without_metadata(self):
        with patch.dict(os.environ, {'BENCH_CONTEXT_LIMIT': '262144', 'BENCH_MAX_TOKENS': '32768'}):
            limits = run_build.provider_model_limits('omlx', 'local-27b', {})
        self.assertEqual((limits['GOOSE_CONTEXT_LIMIT'], limits['GOOSE_MAX_TOKENS']), ('262144', '32768'))
        self.assertEqual(limits['provenance']['source'], 'operator-env')

    def test_run_proceeds_for_a_custom_local_provider_and_records_the_absence(self):
        seen = {}
        def invoke(entrant, workdir, port, env, *args):
            seen['env'] = env
            seen['limits_file'] = json.loads((workdir / 'model-limits.json').read_text())
            raise RuntimeError('controlled stop after dispatch')
        scorer = SimpleNamespace(_port_holder=lambda port: None)
        vendor = SimpleNamespace(serve=lambda port, trace: trace.write_text('') or
                                 SimpleNamespace(shutdown=lambda: None, server_close=lambda: None),
                                 mark_phase=lambda *a: None)
        snapshot = {'version': 1, 'providers': ['omlx'], 'config': {}, 'secrets': {'OMLX_KEY': 'k'},
                    'custom_providers': {}}
        stdout = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp, \
                patch.object(run_build, '_regime', return_value=(scorer, vendor, None)), \
                patch.object(run_build, 'entrant_config', return_value=snapshot), \
                patch.object(run_build, 'invoke', side_effect=invoke), \
                contextlib.redirect_stdout(stdout):
            with self.assertRaisesRegex(RuntimeError, 'controlled stop after dispatch'):
                run_build.run('omlx-fixture', 0, Path(tmp), 0, 8850, 'omlx', 'local-27b')
        self.assertEqual(seen['limits_file']['status'], 'provider_default_unverified')
        self.assertNotIn('GOOSE_CONTEXT_LIMIT', seen['env'])
        self.assertIn('MODEL LIMITS UNVERIFIED: omlx/local-27b', stdout.getvalue())

    def test_local_engines_are_unaffected(self):
        with patch.object(run_build.urllib.request, 'urlopen') as call:
            for provider in ('lmstudio', 'swarm'):
                self.assertIsNone(run_build.provider_model_limits(provider, 'local-model', {}))
        call.assert_not_called()

    def test_run_refuses_a_bedrock_entrant_before_its_tree_or_model_exists(self):
        with tempfile.TemporaryDirectory() as tmp, \
                patch.object(run_build, 'MODELS', {'fable-5.1': 'us.anthropic.claude-fable-5-1'}), \
                patch.object(run_build, 'load_env', return_value={}), \
                patch.object(run_build, 'invoke') as invoke:
            with self.assertRaisesRegex(RuntimeError, 'REFUSED: no source for aws_bedrock'):
                run_build.run('fable-5.1', 0, Path(tmp), 0, 8850)
            self.assertFalse((Path(tmp) / 'fable-5.1-r0').exists())
        invoke.assert_not_called()


SLEEPER = 'import time; time.sleep(600)'


def _wait_for(path):
    for _ in range(200):
        if path.is_file() and path.read_text().strip():
            return [int(value) for value in path.read_text().split()]
        time.sleep(0.05)
    raise AssertionError(f'{path} never written')


def _alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def _launch_entrant(script, ready):
    """A stand-in entrant: a new session (as invoke() spawns it) that leaves orphans and exits."""
    entrant = subprocess.Popen([sys.executable, '-c', script], start_new_session=True)
    entrant.wait()
    return entrant.pid, _wait_for(ready)


@unittest.skipUnless(sys.platform == 'darwin', 'macOS lsof/ps teardown')
class EntrantTeardownTests(unittest.TestCase):
    children = ()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.workdir = Path(self.tmp.name) / 'tree'
        self.outside = Path(self.tmp.name) / 'outside'
        self.workdir.mkdir()
        self.outside.mkdir()
        self.strays = []
        self.addCleanup(self._kill_strays)

    def _kill_strays(self):
        for pid in self.strays:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for child in self.children:
            child.wait()

    def test_an_orphaned_test_instance_in_the_tree_is_reaped_and_outsiders_are_untouched(self):
        ready = self.outside / 'pids'
        script = f'''
import subprocess, sys
subprocess.Popen([sys.executable, '-c', """
import os, subprocess, sys, time, pathlib
child = subprocess.Popen([sys.executable, '-c', {SLEEPER!r}], cwd={str(self.outside)!r})
pathlib.Path({str(ready)!r}).write_text(f'{{os.getpid()}} {{child.pid}}')
time.sleep(600)
"""], cwd={str(self.workdir)!r}, process_group=0)
'''
        _, (app, app_child) = _launch_entrant(script, ready)
        self.strays += [app, app_child]
        operator_shell = subprocess.Popen([sys.executable, '-c', SLEEPER], cwd=self.workdir)
        unrelated = subprocess.Popen([sys.executable, '-c', SLEEPER], cwd=self.outside, start_new_session=True)
        self.children = (operator_shell, unrelated)
        self.strays += [operator_shell.pid, unrelated.pid]
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            record = run_build.reap_entrant_survivors(self.workdir, None, ('secret-never-shown',))
        reaped = {row['pid']: row for row in record['reaped_processes']}
        self.assertEqual(set(reaped), {app, app_child})
        self.assertEqual(reaped[app]['matched_by'], ['cwd'])
        self.assertEqual(reaped[app]['cwd'], os.path.realpath(self.workdir))
        self.assertEqual(reaped[app_child]['matched_by'], ['descendant'])
        self.assertTrue(all(row['signal'] == 'SIGTERM' and 'time.sleep' in row['args']
                            for row in reaped.values()))
        for _ in range(100):
            if not (_alive(app) or _alive(app_child)):
                break
            time.sleep(0.05)
        self.assertFalse(_alive(app) or _alive(app_child))
        self.assertIsNone(operator_shell.poll())
        self.assertIsNone(unrelated.poll())
        self.assertEqual([row['pid'] for row in record['skipped_processes']], [operator_shell.pid])
        self.assertIn(f'REAPED entrant survivor pid {app} ', output.getvalue())
        self.assertIn(f'NOT REAPED pid {operator_shell.pid} ', output.getvalue())

    def test_a_session_descendant_outside_the_tree_is_reaped_by_session(self):
        ready = self.outside / 'pid'
        script = f'''
import subprocess, sys, pathlib
child = subprocess.Popen([sys.executable, '-c', {SLEEPER!r}], cwd={str(self.outside)!r}, process_group=0)
pathlib.Path({str(ready)!r}).write_text(str(child.pid))
'''
        leader, (orphan,) = _launch_entrant(script, ready)
        self.strays.append(orphan)
        unrelated = subprocess.Popen([sys.executable, '-c', SLEEPER], cwd=self.outside, start_new_session=True)
        self.children = (unrelated,)
        self.strays.append(unrelated.pid)
        with contextlib.redirect_stdout(io.StringIO()):
            record = run_build.reap_entrant_survivors(self.workdir, leader)
        self.assertEqual([(row['pid'], row['matched_by']) for row in record['reaped_processes']],
                         [(orphan, ['session'])])
        self.assertIsNone(unrelated.poll())
        self.assertNotIn('skipped_processes', record)

    def test_a_term_ignoring_survivor_is_killed_per_pid(self):
        ready = self.outside / 'pid'
        stubborn_code = ('import os, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); '
                         f'open({str(ready)!r}, "w").write(str(os.getpid())); time.sleep(600)')
        script = f'''
import subprocess, sys
subprocess.Popen([sys.executable, '-c', {stubborn_code!r}], cwd={str(self.workdir)!r})
'''
        _, (stubborn,) = _launch_entrant(script, ready)
        self.strays.append(stubborn)
        with contextlib.redirect_stdout(io.StringIO()), patch.object(run_build.os, 'killpg') as killpg:
            record = run_build.reap_entrant_survivors(self.workdir, None, grace_seconds=0.5)
        killpg.assert_not_called()
        self.assertEqual([(row['pid'], row['signal']) for row in record['reaped_processes']],
                         [(stubborn, 'SIGKILL')])
        self.assertFalse(_alive(stubborn))
