"""Forge 2.0 through the harness (forge2/SPEC.md §3 P11): FORGE20 and --forge2, the per-tier call budget, the
entrant's workspace, the forge2 kit source and the per-era release payloads.

    cd evals/swarm-bench/bench && python3 -m unittest test_forge2_harness -v

Forge20DeliveryTests read what P1/P6/P7/P8/P9 deliver (the public files, scorer, checks, probe, site and the kit
lock). They fail until those packages are merged, and each failure names what is missing.
"""
import contextlib
import dataclasses
import fcntl
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import patch

import bench_budget
import forge2_kit
import isolated_tiers
import release_manifest
import run_build
from test_bench_budget import IsolatedInvoke

ROOT = Path(run_build.ROOT)
FORGE10, FORGE20 = isolated_tiers.FORGE10, isolated_tiers.FORGE20
SEED = '0123456789abcdef'


class _Stop(Exception):
    pass


def _fake_site():
    site = types.ModuleType('forge2_site')
    site.DOCS_PATH = site.API_KEY = None
    return site


class TierTests(unittest.TestCase):
    def test_the_forge2_flag_selects_forge20_and_never_beside_another_tier(self):
        seen = []

        def fake_run(*_args, **_kwargs):
            seen.append(isolated_tiers.active())
            raise _Stop
        with patch.dict(os.environ, {}, clear=True), patch.object(sys, 'argv', ['run_build.py', '--forge2']), \
                patch.object(run_build, 'run', fake_run), self.assertRaises(_Stop):
            run_build.main()
        self.assertEqual(seen, [FORGE20])
        with patch.dict(os.environ, {}, clear=True), patch.object(sys, 'argv', ['run_build.py', '--forge', '--forge2']), \
                self.assertRaisesRegex(RuntimeError, 'more than one isolated tier'):
            run_build.main()

    def test_the_regime_is_forge2s_own_scorer_site_and_prompt(self):
        scorer, site = types.ModuleType('score_forge2'), _fake_site()
        with patch.dict(os.environ, {'BENCH_FORGE20': '1'}, clear=True), \
                patch.dict(sys.modules, {'score_forge2': scorer, 'forge2_site': site}):
            self.assertEqual(run_build._regime(), (scorer, site, 'forge2/public/spec-build-forge2.md'))

    def test_forge20_is_a_forge_era_of_its_own(self):
        self.assertIs(isolated_tiers.BY_VERSION['forge-2.0'], FORGE20)
        self.assertEqual((FORGE20.family, FORGE20.vendor, FORGE20.network, FORGE20.kit, FORGE20.reasoning_effort,
                          FORGE20.own_scoring_site, FORGE20.starter),
                         ('forge', 'forge2_site', 'fenced', True, 'medium', True, 'forge2/starter'))
        self.assertEqual(dict(FORGE20.public), {'FORGE2-CONTRACT.md': 'forge2/public/FORGE2-CONTRACT.md',
                                                'STARTER.md': 'forge2/public/STARTER.md',
                                                'RATE-MODEL.json': 'forge2/public/RATE-MODEL.json',
                                                'BROWSER-TESTING.md': 'forge2/public/BROWSER-TESTING.md'})
        self.assertEqual([p for p in FORGE20.contracts if not p.startswith('forge2/public/')], [])
        self.assertEqual([f for f in FORGE20.scorer_files if 'forge' in f and 'forge2' not in f], [])
        # forge-1.0 is untouched: a 1.0 receipt still replays the 1.0 scorer, prompt and budget.
        self.assertEqual((FORGE10.spec, FORGE10.starter, FORGE10.scorer, FORGE10.vendor, FORGE10.call_budget),
                         ('forge/public/spec-build-forge.md', 'forge/starter', 'score_forge', 'forge_site', 150))

    def test_forge20_publishes_300_calls_and_every_other_tier_keeps_150(self):
        self.assertEqual(bench_budget.CALL_BUDGET, 150)
        self.assertEqual({tier.version: tier.call_budget for tier in isolated_tiers.TIERS},
                         {'sb-7.1': 150, 'sb-7.2': 150, 'forge-1.0': 150, 'forge-2.0': 300})
        flag, calls = bench_budget.call_budget_args(FORGE20)
        self.assertEqual(calls, '300')
        self.assertEqual(bench_budget.call_budget_args(FORGE10), [flag, '150'])


class Forge20Invoke(IsolatedInvoke):
    def test_a_forge2_entrant_gets_and_records_its_tiers_300_calls(self):
        spec = self.root / 'spec.md'
        spec.write_text('You have a budget of 300 model calls.\n')
        with patch.dict(sys.modules, {'score_forge2': types.ModuleType('score_forge2'), 'forge2_site': _fake_site()}):
            result, out, _ = self.invoke('finished', environ={'BENCH_SB72': '', 'BENCH_FORGE20': '1', 'BENCH_SPEC': str(spec)},
                                         env={'GOOGLE_API_KEY': 'k', 'FORGE_KIT': str(self.root)})
        args = self.args()
        flag, calls = bench_budget.call_budget_args(FORGE20)
        self.assertEqual(args[args.index(flag) + 1], calls)
        self.assertEqual((result['budget']['max_calls'], result['budget']['stopped_by']), (300, 'model_finished'))
        self.assertIn('"max_calls": 300', out)


class Forge20Workspace(unittest.TestCase):
    """run() under a FORGE20 whose files sit in a temp dir, with the model, the dev site and the scorer replaced."""

    def test_the_workspace_is_the_starter_the_kit_clone_and_the_tiers_own_public_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            starter = tmp / 'starter'
            (starter / 'src').mkdir(parents=True)
            (starter / 'manifest.yml').write_text('app: {}\n')
            (starter / 'src' / 'index.js').write_text('export const v1 = true;\n')
            (starter / 'node_modules' / 'stray').mkdir(parents=True)   # a local install in the checkout
            (starter / '.forge-dev').mkdir()
            (starter / '.forge-dev' / 'state.json').write_text('{}')
            public = tmp / 'public'
            public.mkdir()
            sources = []
            for name, _source in FORGE20.public:
                (public / name).write_text(f'{name} talks to {{BASE_URL}}\n')
                sources.append((name, str(public / name)))
            spec = public / 'spec.md'
            spec.write_text('Upgrade v1 at {BASE_URL}. You have a budget of 300 model calls.\n')
            tier = dataclasses.replace(FORGE20, spec=str(spec), starter=str(starter), public=tuple(sources))
            modules = tmp / 'kit' / 'app-modules' / 'node_modules'
            (modules / '@forge' / 'react').mkdir(parents=True)
            (modules / '@forge' / 'react' / 'package.json').write_text('{"version": "12.3.0"}')
            kit = {'dir': str(tmp / 'kit'), 'lock_sha256': 'k' * 64, 'wrapper_sha256': 'w' * 64, 'app_modules': str(modules)}
            seen = {}

            class DevSite:
                url = 'http://admin:t@127.0.0.1:1'

                def shutdown(self):
                    seen['dev_site_stopped'] = True

                def server_close(self):
                    pass

            def serve(port, trace, seed=None):
                Path(trace).write_text(json.dumps({'trace_header': True, 'tier': 'forge-2.0', 'fixture_seed': seed}) + '\n')
                return DevSite()

            def gather(root, port, db, trace_path, mark_phase=None, seed=None):
                seen['gather'] = (seed, seen.get('dev_site_stopped'))
                return {}

            def build(entrant, workdir, port, env, *args):
                seen['env'] = env
                return {'exit': 0, 'timed_out': False, 'secs': 1.0, 'tail': 'done', 'reaped_processes': []}
            site = types.SimpleNamespace(DOCS_PATH=None, API_KEY=None, serve=serve, mark_phase=lambda name: None)
            scorer = types.SimpleNamespace(_kit=lambda: (kit, None), _port_holder=lambda port: None,
                                           _probe_preflight=lambda: None, _draw_seed=lambda: SEED, gather=gather,
                                           evaluate=lambda ctx: {'score': 0.1, 'scorerVersion': 'forge-2.0'})
            snapshot = {'version': 1, 'providers': [], 'config': {}, 'secrets': {}, 'custom_providers': {}}
            with patch.dict(os.environ, {'BENCH_FORGE20': '1', 'BENCH_SPEC': str(spec)}, clear=True), \
                    patch.object(isolated_tiers, 'active', return_value=tier), \
                    patch.object(run_build, '_regime', return_value=(scorer, site, tier.spec)), \
                    patch.object(run_build, 'entrant_config', return_value=snapshot), \
                    patch.object(run_build, 'invoke', side_effect=build), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                verdict = run_build.run('fixture', 0, tmp / 'runs', 0, 8851)
            work = tmp / 'runs' / 'fixture-r0'
            self.assertTrue((work / 'manifest.yml').is_file() and (work / 'src' / 'index.js').is_file())
            self.assertTrue((work / 'node_modules' / '@forge' / 'react' / 'package.json').is_file())
            self.assertFalse((work / 'node_modules' / 'stray').exists(), 'the checkout\'s local install reached the entrant')
            self.assertFalse((work / '.forge-dev').exists())
            for name, _source in FORGE20.public:
                self.assertEqual((work / name).read_text(), f'{name} talks to http://127.0.0.1:8851\n', name)
            self.assertTrue((work / 'benchmark-prompt.md').read_text().startswith(
                'Upgrade v1 at http://127.0.0.1:8851. You have a budget of 300 model calls.\n'))
            manifest = json.loads((work / 'input-manifest.json').read_text())
            self.assertEqual(manifest['node_modules'], 'kit:' + 'k' * 64)
            self.assertEqual([p for p in manifest if p.startswith(('node_modules/', '.forge-dev'))], [])
            self.assertEqual((seen['env']['FORGE_KIT'], seen['env']['FORGE_SITE_URL'], seen['env']['GOOSE_THINKING_EFFORT']),
                             (kit['dir'], DevSite.url, 'medium'))
            self.assertEqual(seen['gather'], (SEED, True), 'the scorer grades on its own site, after the dev site stopped')
            self.assertTrue((work / run_build.BUILD_VENDOR_TRACE).is_file())
            self.assertEqual(verdict['kit'], {'lock_sha256': 'k' * 64, 'wrapper_sha256': 'w' * 64, 'dir': kit['dir']})
            self.assertEqual(verdict['reasoning_effort']['source'], 'forge-2.0 pin')


class ScoreLockTests(unittest.TestCase):
    """run_build scores forge-2.0 in-process through score_forge2.gather(), and so does the score_forge2.py CLI: every
    probe runs under the one per-host score lock, at a path no caller's $TMPDIR moves."""

    def test_gather_runs_every_probe_under_the_score_lock_and_releases_it(self):
        import score_forge2
        held = []

        def probe_one(_root, seed, *_rest):
            with open(score_forge2.LOCK_FILE, 'w') as other:
                try:
                    fcntl.flock(other, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    held.append(seed)
            return {}, {}
        with tempfile.TemporaryDirectory() as tmp, \
                patch.object(score_forge2, 'LOCK_FILE', Path(tmp) / 'score.lock'), \
                patch.object(score_forge2, 'FORGE1_LOCK_FILE', Path(tmp) / 'forge1' / 'score.lock'), \
                patch.object(score_forge2, '_kit', return_value=({'dir': tmp}, None)), \
                patch.object(score_forge2, '_probe_one', side_effect=probe_one), \
                patch.object(score_forge2, 'Ctx', side_effect=lambda *_a, **_k: types.SimpleNamespace()):
            score_forge2.gather(Path(tmp), None, None, seed=SEED)
            for path in (score_forge2.LOCK_FILE, score_forge2.FORGE1_LOCK_FILE):
                with open(path, 'w') as after:
                    fcntl.flock(after, fcntl.LOCK_EX | fcntl.LOCK_NB)   # released: the next scoring proceeds
        self.assertEqual(len(held), score_forge2.SCORING_SEEDS)

    def test_a_holder_of_either_lock_blocks_a_forge2_scoring(self):
        """The frozen Forge 1.0 scorer locks <temp dir>/goose-forge-score.lock, the 2.0 scorer /tmp/goose-forge-score.lock
        too: a 2.0 scoring takes both, so a process holding EITHER (a 1.0 re-score, another 2.0 scoring) makes it wait,
        and it releases both when it leaves. Two real processes, the locks at scratch paths."""
        import score_forge2
        hold = ('import fcntl, sys, time\nf = open(sys.argv[1], "w")\nfcntl.flock(f, fcntl.LOCK_EX)\n'
                'open(sys.argv[2], "w").close()\nwhile not __import__("os").path.exists(sys.argv[3]): time.sleep(0.05)\n')
        score = ('import os, sys, time\nfrom pathlib import Path\nsys.path.insert(0, sys.argv[1])\nimport score_forge2 as s\n'
                 's.LOCK_FILE, s.FORGE1_LOCK_FILE = Path(sys.argv[2]), Path(sys.argv[3])\nwith s.score_lock():\n'
                 '    open(sys.argv[4], "w").close()\n    while not os.path.exists(sys.argv[5]): time.sleep(0.05)\n')

        def wait_for(path, seconds):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline and not path.exists():
                time.sleep(0.05)
            return path.exists()

        def free(path):
            with open(path, 'w') as f:
                try:
                    fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    return True
                except BlockingIOError:
                    return False
        for held in ('fixed', 'forge1'):
            with tempfile.TemporaryDirectory() as tmp:
                tmp = Path(tmp)
                fixed, forge1 = tmp / 'fixed.lock', tmp / 'tmpdir' / 'goose-forge-score.lock'
                forge1.parent.mkdir()
                procs = []
                try:
                    procs.append(subprocess.Popen([sys.executable, '-c', hold, str(fixed if held == 'fixed' else forge1),
                                                   str(tmp / 'holding'), str(tmp / 'release-holder')]))
                    self.assertTrue(wait_for(tmp / 'holding', 20), 'the holder never took its lock')
                    procs.append(subprocess.Popen([sys.executable, '-B', '-c', score, str(run_build.HERE), str(fixed),
                                                   str(forge1), str(tmp / 'scoring'), str(tmp / 'release-scorer')],
                                                  stderr=subprocess.PIPE, text=True))
                    self.assertFalse(wait_for(tmp / 'scoring', 3), f'a 2.0 scoring ran while the {held} lock was held')
                    (tmp / 'release-holder').touch()
                    self.assertTrue(wait_for(tmp / 'scoring', 30), f'the 2.0 scoring never ran after the {held} lock freed')
                    self.assertEqual([free(fixed), free(forge1)], [False, False], 'a 2.0 scoring holds both locks')
                    (tmp / 'release-scorer').touch()
                    self.assertEqual(procs[1].wait(timeout=30), 0)
                    self.assertIn(f'waiting for {fixed if held == "fixed" else forge1}', procs[1].stderr.read())
                    self.assertEqual([free(fixed), free(forge1)], [True, True], 'both locks released')
                finally:
                    for p in procs:   # the pid only (gate 4): these two are this test's own children
                        if p.poll() is None:
                            p.kill()
                            p.wait()
                        if p.stderr:
                            p.stderr.close()

    def test_one_lock_file_under_both_names_is_locked_once(self):
        import score_forge2
        with tempfile.TemporaryDirectory() as tmp, \
                patch.object(score_forge2, 'LOCK_FILE', Path(tmp) / 'score.lock'), \
                patch.object(score_forge2, 'FORGE1_LOCK_FILE', Path(tmp) / '.' / 'score.lock'):
            self.assertEqual(score_forge2.score_lock_files(), [score_forge2.LOCK_FILE])
            with score_forge2.score_lock():   # a second flock of the same file in one process would wait on itself
                pass

    def test_callers_with_different_tmpdirs_lock_the_same_file(self):
        import score_forge2
        with tempfile.TemporaryDirectory() as tmp:
            out = subprocess.run([sys.executable, '-B', '-c', 'import score_forge2; print(score_forge2.LOCK_FILE)'],
                                 cwd=run_build.HERE, env={**os.environ, 'TMPDIR': tmp}, capture_output=True, text=True,
                                 check=True)
        self.assertEqual(out.stdout.strip(), str(score_forge2.LOCK_FILE))


class KitTests(unittest.TestCase):
    def test_the_forge2_kit_is_built_from_forge2_kit(self):
        self.assertEqual(forge2_kit.KIT_SRC, ROOT / 'forge2' / 'kit')
        with tempfile.TemporaryDirectory() as tmp:
            got = forge2_kit.status(Path(tmp))
        self.assertEqual(got['missing'], ['app-modules', 'lint-modules', 'wrapper/wrapper.js', 'wrapper/loader.js',
                                          'schema', 'kit'])
        self.assertEqual(got['kit_lock_sha256'], forge2_kit.lock_sha256(ROOT / 'forge2' / 'kit'))

    def test_the_lint_pack_is_kit_code_once_the_recipe_has_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp)
            for part in ('bin', 'lib', 'openapi'):
                (src / part).mkdir()
                (src / part / 'x.cjs').write_text(part)
            (src / 'runtime-pin.json').write_text('{}')
            without = forge2_kit.code_sha256(src)
            self.assertEqual(forge2_kit.code_parts(src), ['bin', 'lib', 'openapi', 'runtime-pin.json'])
            (src / 'lint-pack').mkdir()
            (src / 'lint-pack' / 'rules.cjs').write_text('module.exports = [];\n')
            self.assertEqual(forge2_kit.code_parts(src), ['bin', 'lib', 'openapi', 'lint-pack', 'runtime-pin.json'])
            self.assertNotEqual(forge2_kit.code_sha256(src), without)


class PayloadTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.payments = set(release_manifest.payload())
        cls.forge10 = set(release_manifest.payload(family='forge', version='forge-1.0'))
        cls.forge20 = set(release_manifest.payload(family='forge', version='forge-2.0'))

    def test_no_forge_file_of_any_era_enters_the_sb7_payload(self):
        self.assertEqual(sorted(f for f in self.payments if 'forge' in f), [])
        for name in ('score_forge2.py', 'forge2_site.py', 'forge2_kit.py', 'forge2_checks.py', 'forge2_probe.mjs',
                     'forge2-thresholds.json', 'forge_probe_v2.mjs', 'forge_oracle2.py', 'test_forge2_harness.py'):
            self.assertTrue(release_manifest.FORGE_BENCH.search(name), name)
        for name in ('score_sb72.py', 'product_probe_sb72.mjs', 'sb72-thresholds.json', 'run_build.py'):
            self.assertFalse(release_manifest.FORGE_BENCH.search(name), name)

    def test_the_forge10_payload_holds_nothing_of_forge20_and_stays_the_default_era(self):
        self.assertEqual(sorted(f for f in self.forge10 if 'forge2' in f), [])
        self.assertEqual(self.forge10, set(release_manifest.payload(family='forge')))

    def test_the_forge20_payload_is_its_own_trees_and_harness(self):
        self.assertEqual(sorted(f for f in self.forge20 if f.startswith('forge/')), [])
        self.assertLessEqual({'forge2/starter/manifest.yml', 'forge2/kit/bin/lint.cjs', 'forge2/site/site.cjs',
                              'bench/run_build.py', 'bench/isolated_tiers.py', 'bench/bench_budget.py',
                              'bench/forge2_kit.py', 'bench/forge2_site.py', 'bench/score_forge2.py',
                              'bench/forge2_controls.py'}, self.forge20)
        self.assertEqual(sorted(f for f in self.forge20 if 'node_modules' in f or f.startswith('forge2/research')), [])
        self.assertFalse({'bench/score_sb72.py', 'spec-build-sb72.md'} & self.forge20)

    def test_a_manifest_pins_the_payload_of_its_own_era(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'release-manifest.json'

            def check(manifest):
                path.write_text(json.dumps(manifest))
                with contextlib.redirect_stdout(io.StringIO()):
                    return release_manifest.main([str(path), '--check'])
            two = release_manifest.pins(ROOT, 'forge', 'forge-2.0')
            self.assertEqual(check({'scorerVersion': 'forge-2.0', 'family': 'forge', 'files': two}), 0)
            self.assertEqual(check({'scorerVersion': 'forge-2.0', 'family': 'forge',
                                    'files': release_manifest.pins(ROOT, 'forge', 'forge-1.0')}), 1)
            with self.assertRaisesRegex(SystemExit, "REFUSED: .* names family 'payments', but forge-2.0 is 'forge'"):
                check({'scorerVersion': 'forge-2.0', 'files': two})


class Forge20DeliveryTests(unittest.TestCase):
    """What the other packages hand the harness. Fails until P1/P6/P7/P8/P9 are merged."""

    def test_every_file_the_tier_names_exists(self):
        named = [*FORGE20.contracts, FORGE20.starter, *(f'bench/{name}' for name in FORGE20.scorer_files)]
        self.assertEqual([name for name in named if not (ROOT / name).exists()], [])

    def test_the_prompt_states_the_tiers_budget(self):
        self.assertEqual(bench_budget.stated_budgets((ROOT / FORGE20.spec).read_text()), [FORGE20.call_budget])

    def test_the_kit_lock_pins_forge_react_12_3_0(self):
        kit = ROOT / 'forge2' / 'kit'
        self.assertEqual(json.loads((kit / 'package.json').read_text())['dependencies'].get('@forge/react'), '12.3.0')
        packages = json.loads((kit / 'package-lock.json').read_text())['packages']
        self.assertEqual(packages.get('node_modules/@forge/react', {}).get('version'), '12.3.0')

    def test_the_scorer_runs_on_the_forge2_kit_and_names_forge20(self):
        info = {'kit_dir': '/kit', 'modules_dir': '/modules', 'kit_lock_sha256': 'l' * 64, 'kit_code_sha256': 'c' * 64,
                'wrapper_sha256': 'w' * 64}
        two = types.ModuleType('forge2_kit')
        two.ensure = lambda: info
        one = types.ModuleType('forge_kit')

        def wrong_kit():
            raise AssertionError('the forge-2.0 scorer materialised the forge-1.0 kit')
        one.ensure = wrong_kit
        import score_forge2
        with patch.dict(sys.modules, {'forge2_kit': two, 'forge_kit': one}):
            kit, why = score_forge2._kit()
        self.assertIsNone(why)
        self.assertEqual((kit['dir'], kit['lock_sha256'], kit['app_modules']),
                         ('/kit', 'l' * 64, '/modules/app-modules/node_modules'))
        self.assertEqual(score_forge2.VERSION.removesuffix('-rc'), 'forge-2.0')
        used = set()
        for name in ('run_build.py', 'bench_rescore.py'):
            used |= set(re.findall(r'\bscorer\.([A-Za-z_][A-Za-z0-9_]*)', (run_build.HERE / name).read_text()))
        self.assertEqual(sorted(a for a in used if not hasattr(score_forge2, a)), [])

    def test_the_dev_site_serves_no_docs_and_no_key(self):
        import forge2_site
        self.assertEqual((forge2_site.DOCS_PATH, forge2_site.API_KEY), (None, None))
        self.assertTrue(callable(forge2_site.serve) and callable(forge2_site.mark_phase))

    def test_the_forge20_payload_holds_no_forge10_bench_module(self):
        forge20 = release_manifest.payload(family='forge', version='forge-2.0')
        self.assertEqual([f for f in forge20 if f.startswith('bench/') and 'forge' in f and 'forge2' not in f], [])


if __name__ == '__main__':
    unittest.main()
