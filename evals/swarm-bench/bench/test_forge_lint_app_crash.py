"""A lint crash on the APP's own unparseable source is app evidence (l_deployable 0, observed); a lint crash the kit's
own parser cannot attribute to an app file stays a harness defect (unavailable). Both Forge scorers, through the real
probes' lint path and the real kits.

    cd evals/swarm-bench/bench && python3 -m unittest test_forge_lint_app_crash -v

Receipt (Gabee, 2026-10-15, Forge 2.0, JetBrains-blend Q8): src/index.js carried a parenthesised expression with a
trailing comma (line 324: `...(a ? [..] : b.map(..),\\n)`). @forge/lint crashed ("Expression expected. ... at line 324,
column 8", exit 2), the scorer wrote `UNAVAILABLE: the linter crashed (harness defect)`, the verdict went
unpublishable and the two-build series skipped build 2. The probe kept only the stderr's last 400 chars, so the
"Expression expected" line itself was lost.
"""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import forge2_kit
import forge2_oracle
import forge_kit
import forge_oracle
import score_forge
import score_forge2

HERE = Path(__file__).resolve().parent

MANIFEST = """modules:
  trigger:
    - key: noop-trigger
      function: noop
      events:
        - avi:jira:updated:issue
  function:
    - key: noop
      handler: index.run
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
permissions:
  scopes:
    - read:jira-work
"""

# The receipt's shape: a parenthesised conditional closed with a trailing comma. V8 and TypeScript refuse it; esbuild
# accepts it, so the bundle still builds (as it did on Gabee) and only lint can see it.
BROKEN = """export async function run(event) {
  const rows = [];
  const view = [
    ...(rows.length === 0
      ? ['none']
      : rows.map((row) => row),
    ),
  ];
  return view;
}
"""
CLEAN = """export async function run(event) {
  return [event?.issue?.id ?? null];
}
"""
HELPER = """export const twice = (n) => n * 2;
"""

PROBES = {'forge2': (forge2_kit, HERE / 'forge2_probe.mjs'), 'forge': (forge_kit, HERE / 'forge_probe.mjs')}


def _node() -> str:
    found = shutil.which('node')
    if not found:
        raise unittest.SkipTest('node is not on PATH: the probes need it')
    return found


def _kit(module):
    try:
        return module.ensure()
    except RuntimeError as error:   # npm ci refused (no network / registry) — named, never a silent pass
        raise unittest.SkipTest(f'{module.__name__}.ensure() could not materialise the kit: {error}')


def _app(root: Path, source: str) -> Path:
    (root / 'src').mkdir(parents=True)
    (root / 'manifest.yml').write_text(MANIFEST)
    (root / 'src' / 'index.js').write_text(source)
    (root / 'src' / 'numbers.js').write_text(HELPER)
    return root


def _lint_only(family: str, app: Path, kit_dir: str, env=None) -> list:
    _module, probe = PROBES[family]
    out = subprocess.run([_node(), str(probe), '--lint-only', '--app', str(app), '--kit', kit_dir],
                         capture_output=True, text=True, timeout=300, env=env)
    if out.returncode:
        raise AssertionError(f'{probe.name} --lint-only exited {out.returncode}: {out.stderr[-2000:]}')
    return json.loads(out.stdout)['runs']


def _score(family: str, runs: list):
    scorer, oracle = (score_forge2, forge2_oracle) if family == 'forge2' else (score_forge, forge_oracle)
    pack = (getattr(oracle, 'synthetic_pack_v2', None) or oracle.synthetic_pack)()
    obs = scorer.selftest_empty_observations(pack, one_function=True)
    obs['lint'] = {'runs': copy.deepcopy(runs)}
    ctx = scorer.Ctx(None, obs, pack, fixture_seed=pack['seed'])
    rows = scorer.evaluate_rows(ctx)
    verdict = scorer.compose_from_rows(rows, ctx)
    return {r['check']: r for r in rows}, verdict, scorer.deploy_readiness(ctx)


class ScorerClassification(unittest.TestCase):
    """Pure: stored lint runs -> rows, for both scorers."""
    PARSE_FAIL = {'ran': True, 'parsed': ['src/index.js', 'src/numbers.js'],
                  'failures': [{'file': 'src/index.js', 'line': 7, 'column': 4, 'message': 'Expression expected.'}]}

    def crashed(self, app_parse):
        run = {'crashed': True, 'exit': 2, 'stderr': 'LINT_CRASH Error: Expression expected.\n'}
        if app_parse is not None:
            run['appParse'] = app_parse
        return [copy.deepcopy(run), copy.deepcopy(run)]

    def test_app_parse_failure_is_an_observed_zero_never_unavailable(self):
        for family in PROBES:
            with self.subTest(family=family):
                rows, verdict, readiness = _score(family, self.crashed(self.PARSE_FAIL))
                row = rows['l_deployable']
                self.assertEqual(row['score'], 0.0, row)
                self.assertFalse(row.get('unavailable'), row)
                self.assertIn('src/index.js:7:4 Expression expected.', row['detail'])
                self.assertEqual(row['consequence'], 'deploy blocked')
                self.assertNotIn('l_deployable', verdict['probe_unavailable'])
                self.assertFalse(any('unavailable' in reason for reason in verdict['unpublishable_reasons']),
                                 verdict['unpublishable_reasons'])
                self.assertEqual(readiness['verdict'], 'would not deploy')

    def test_unattributed_crash_stays_a_harness_defect(self):
        controls = {
            'no parse check (an older observation)': None,
            'the parse check could not run (kit broken)': {'ran': False, 'exit': 1, 'stderr': 'REFUSED: no kit'},
            'every app file parses': {'ran': True, 'parsed': ['src/index.js'], 'failures': []},
        }
        for family in PROBES:
            for label, app_parse in controls.items():
                with self.subTest(family=family, control=label):
                    rows, verdict, _ = _score(family, self.crashed(app_parse))
                    row = rows['l_deployable']
                    self.assertTrue(row.get('unavailable'), row)
                    self.assertIn('harness defect', row['detail'])
                    self.assertIn('l_deployable', verdict['probe_unavailable'])
                    self.assertFalse(verdict['publishable'])

    def test_the_two_runs_must_agree_on_the_app_failure(self):
        runs = self.crashed(self.PARSE_FAIL)
        runs[1]['appParse'] = {**self.PARSE_FAIL, 'failures': [{**self.PARSE_FAIL['failures'][0], 'line': 8}]}
        for family in PROBES:
            with self.subTest(family=family):
                rows, _verdict, _ = _score(family, runs)
                self.assertTrue(rows['l_deployable'].get('unavailable'), rows['l_deployable'])


class ProbeLintPath(unittest.TestCase):
    """The real probe's lint path over a real kit: what a scoring run records, then what the scorer makes of it."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix='forge-lint-crash-'))

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_unbalanced_paren_app_is_scored_not_refused(self):
        for family, (module, _probe) in PROBES.items():
            with self.subTest(family=family):
                kit = _kit(module)
                app = _app(self.tmp / f'{family}-broken', BROKEN)
                runs = _lint_only(family, app, kit['kit_dir'])
                for run in runs:
                    self.assertTrue(run.get('crashed'), run)
                    # The first error line survives in the stored stderr (it was cut away by a tail-only slice).
                    self.assertTrue(run['stderr'].startswith('LINT_CRASH Error: Expression expected.'), run['stderr'][:300])
                    self.assertTrue(run['appParse']['ran'], run['appParse'])
                    self.assertEqual(run['appParse']['parsed'], ['src/index.js', 'src/numbers.js'])
                    self.assertEqual([(f['file'], f['message']) for f in run['appParse']['failures']],
                                     [('src/index.js', 'Expression expected.')])
                rows, verdict, _ = _score(family, runs)
                self.assertEqual(rows['l_deployable']['score'], 0.0, rows['l_deployable'])
                self.assertFalse(rows['l_deployable'].get('unavailable'), rows['l_deployable'])
                self.assertIn('Expression expected.', rows['l_deployable']['detail'])
                self.assertNotIn('l_deployable', verdict['probe_unavailable'])

    def test_clean_app_lints_without_a_crash(self):
        for family, (module, _probe) in PROBES.items():
            with self.subTest(family=family):
                kit = _kit(module)
                runs = _lint_only(family, _app(self.tmp / f'{family}-clean', CLEAN), kit['kit_dir'])
                self.assertFalse(any(run.get('crashed') for run in runs), runs)

    def test_a_kit_with_no_modules_is_unavailable_even_for_a_broken_app(self):
        """Control: the linter (and the parse check) cannot load at all — nothing about the app was measured."""
        for family, (module, _probe) in PROBES.items():
            with self.subTest(family=family):
                kit = Path(_kit(module)['kit_dir'])
                bare = self.tmp / f'{family}-bare-kit'
                for part in ('bin', 'lib'):
                    shutil.copytree(kit / part, bare / part)
                env = {k: v for k, v in os.environ.items() if k not in ('FORGE_KIT', 'FORGE_KIT_MODULES')}
                runs = _lint_only(family, _app(self.tmp / f'{family}-broken-bare', BROKEN), str(bare), env=env)
                for run in runs:
                    self.assertTrue(run.get('crashed'), run)
                    self.assertFalse(run['appParse']['ran'], run['appParse'])
                    self.assertIn('REFUSED', run['appParse']['stderr'])
                rows, verdict, _ = _score(family, runs)
                self.assertTrue(rows['l_deployable'].get('unavailable'), rows['l_deployable'])
                self.assertIn('parse check did not run', rows['l_deployable']['detail'])
                self.assertFalse(verdict['publishable'])

    def test_a_kit_missing_its_server_pack_is_unavailable_for_a_clean_app(self):
        """Control (forge2): lint.cjs refuses on its own missing file while every app file parses."""
        info = _kit(forge2_kit)
        kit = Path(info['kit_dir'])
        broken = self.tmp / 'forge2-no-server-pack'
        for part in ('bin', 'lib', 'openapi'):
            shutil.copytree(kit / part, broken / part)
        shutil.copy2(kit / 'runtime-pin.json', broken / 'runtime-pin.json')
        env = {**os.environ, 'FORGE_KIT_MODULES': info['modules_dir']}
        env.pop('FORGE_KIT', None)
        runs = _lint_only('forge2', _app(self.tmp / 'forge2-clean-for-pack', CLEAN), str(broken), env=env)
        for run in runs:
            self.assertTrue(run.get('crashed'), run)
            self.assertIn('LINT_CRASH REFUSED', run['stderr'])
            self.assertTrue(run['appParse']['ran'], run['appParse'])
            self.assertEqual(run['appParse']['failures'], [])
        rows, verdict, _ = _score('forge2', runs)
        self.assertTrue(rows['l_deployable'].get('unavailable'), rows['l_deployable'])
        self.assertIn('reads every app source file', rows['l_deployable']['detail'])
        self.assertFalse(verdict['publishable'])


if __name__ == '__main__':
    unittest.main()
