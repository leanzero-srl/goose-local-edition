"""Control-catalog and judge proofs; no emulator, browser, model call or scoring subprocess."""
from contextlib import redirect_stdout
from io import StringIO
import json
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import forge2_controls as controls


def verdict(scores, classes=(), unavailable=(), missing=()):
    return {'score': 0.8, 'status': 'scored',
            'checks': [{'check': name, 'score': score, 'detail': f'actual {score}', 'parts': {}}
                       for name, score in scores.items()],
            'critical': {'unsuppressed': list(classes), 'classes': list(classes)},
            'probe_unavailable': list(unavailable), 'harness_missing': list(missing)}


class JudgeTests(unittest.TestCase):
    def setUp(self):
        self.golden = verdict({'primary': 1.0, 'secondary': 1.0, 'other': 1.0})
        self.expect = {'loses': ['primary'], 'may_lose': ['secondary'], 'critical': False,
                       'why': 'A skipped history page also omits its dependent person-view row on affected seeds.'}

    def test_named_secondary_loss_is_optional_and_reported_only_when_observed(self):
        primary_only = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 1.0})
        with_secondary = verdict({'primary': 0.0, 'secondary': 0.7, 'other': 1.0})
        self.assertEqual(controls.judge(self.golden, primary_only, self.expect, {}), [])
        self.assertEqual(controls.loss_evidence(self.golden, primary_only, self.expect), {})
        self.assertEqual(controls.judge(self.golden, with_secondary, self.expect, {}), [])
        proof = controls.loss_evidence(self.golden, with_secondary, self.expect)['secondary']
        self.assertEqual(proof['cause'], self.expect['why'])
        self.assertEqual((proof['golden']['score'], proof['mutant']['score']), (1.0, 0.7))
        self.assertEqual(proof['mutant']['detail'], 'actual 0.7')

    def test_optional_loss_cannot_replace_required_loss_or_authorize_unrelated_loss(self):
        secondary_only = verdict({'primary': 1.0, 'secondary': 0.0, 'other': 1.0})
        fails = controls.judge(self.golden, secondary_only, self.expect, {})
        self.assertTrue(any('declared losses that did not happen' in fail for fail in fails))
        unrelated = verdict({'primary': 0.0, 'secondary': 0.5, 'other': 0.0})
        fails = controls.judge(self.golden, unrelated, self.expect, {})
        self.assertTrue(any("undeclared losses: ['other']" in fail for fail in fails))

    def test_optional_whitelist_requires_cause_and_registered_names(self):
        mutant = verdict({'primary': 0.0, 'secondary': 0.0, 'other': 1.0})
        fails = controls.judge(self.golden, mutant, {**self.expect, 'why': ''}, {})
        self.assertTrue(any('documented causal explanation' in fail for fail in fails))
        self.assertTrue(any('undeclared losses' in fail for fail in fails))
        fails = controls.judge(self.golden, mutant, {**self.expect, 'may_lose': ['typo']}, {})
        self.assertTrue(any('unregistered checks' in fail for fail in fails))

    def test_missing_rows_and_unavailable_runtime_cannot_pass(self):
        mutant = verdict({'primary': 0.0, 'other': 1.0}, unavailable=['secondary'], missing=['GET /unknown'])
        fails = controls.judge(self.golden, mutant, self.expect, {})
        self.assertTrue(any('omits registered checks' in fail for fail in fails))
        self.assertTrue(any('unavailable rows' in fail for fail in fails))
        self.assertTrue(any('harness_missing' in fail for fail in fails))

    def test_unrelated_vacuous_loss_requires_an_explicit_causal_root_or_named_secondary(self):
        mutant = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 0.0})
        mutant['checks'][-1]['parts'] = {'vacuous_root': 'precondition: unrelated native JQL evidence absent'}
        fails = controls.judge(self.golden, mutant, self.expect, {})
        self.assertTrue(any("undeclared losses: ['other']" in fail for fail in fails))
        self.assertEqual(controls.judge(self.golden, mutant, self.expect, {'primary': ('other',)}), [])
        explained = {**self.expect, 'may_lose': ['secondary', 'other'],
                     'why': 'The declared root prevents the named downstream surface from being exercised.'}
        self.assertEqual(controls.judge(self.golden, mutant, explained, {}), [])

    def test_runtime_status_and_unavailable_rows_refuse_even_without_aggregate_flags(self):
        mutant = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 1.0})
        for status in ('held', 'unavailable', None):
            with self.subTest(status=status):
                mutant['status'] = status
                self.assertTrue(any('runtime status' in fail for fail in controls.judge(self.golden, mutant, self.expect, {})))
        mutant['status'] = 'scored'
        mutant['checks'][0]['unavailable'] = True
        self.assertTrue(any('row-level unavailable' in fail for fail in controls.judge(self.golden, mutant, self.expect, {})))

    def test_starter_and_idle_cap_controls_cannot_pass_on_unavailable_or_held_runtime(self):
        control = verdict({'primary': 0.0})
        control['score'] = 0.01
        self.assertEqual(controls.capped_control_failures(control, controls.IDLE_MAX), [])
        control['checks'][0]['unavailable'] = True
        self.assertTrue(controls.capped_control_failures(control, controls.IDLE_MAX))
        control['checks'][0].pop('unavailable')
        control['harness_missing'] = ['GET /unmodelled']
        self.assertTrue(controls.capped_control_failures(control, controls.STARTER_MAX))
        control['harness_missing'] = []
        control['status'] = 'held'
        self.assertTrue(controls.capped_control_failures(control, controls.IDLE_MAX))

    def test_one_declared_critical_cannot_hide_an_unrelated_critical(self):
        mutant = verdict({'primary': 0.0, 'secondary': 1.0, 'other': 1.0}, classes=['duplicate', 'leak'])
        fails = controls.judge(self.golden, mutant, {**self.expect, 'critical': 'duplicate'}, {})
        self.assertTrue(any("unexpected critical classes ['leak']" in fail for fail in fails))


class FamilyCoverageTests(unittest.TestCase):
    def test_only_passing_required_loss_in_owned_family_covers_that_family(self):
        rows = {'primary': 'R1', 'optional': 'R2', 'jql': 'R10'}
        entries = {
            'legacy_incidental': {'family': None, 'lost': ['primary'], 'required_losses': ['primary'], 'fails': []},
            'optional_neighbor': {'family': 'R1', 'lost': ['optional'], 'required_losses': [], 'fails': []},
            'wrong_family': {'family': 'R2', 'lost': ['primary'], 'required_losses': ['primary'], 'fails': []},
            'failed': {'family': 'R10', 'lost': ['jql'], 'required_losses': ['jql'], 'fails': ['wrong harm']},
        }
        self.assertEqual(controls.family_gaps(entries, rows, ('R1', 'R2', 'R10')), ['R1', 'R2', 'R10'])
        entries['real'] = {'family': 'R1', 'lost': ['primary', 'optional'],
                           'required_losses': ['primary'], 'fails': []}
        self.assertEqual(controls.family_gaps(entries, rows, ('R1', 'R2', 'R10')), ['R2', 'R10'])

    def test_all_three_control_sources_are_loaded_with_explicit_modern_families(self):
        candidates = controls.candidates(include_legacy=True)
        self.assertEqual({candidate['origin'] for candidate in candidates},
                         {'v1-regression', 'v2-catalog', 'native-jql'})
        self.assertEqual({candidate['family'] for candidate in candidates if candidate['family']},
                         {f'R{i}' for i in range(1, 11)})
        catalog = json.loads(controls.FAMILY_CATALOG.read_text())
        self.assertEqual({entry['name'] for entry in catalog},
                         {candidate['name'] for candidate in candidates if candidate['origin'] == 'v2-catalog'})
        self.assertEqual(len(candidates), len({candidate['name'] for candidate in candidates}))

    def test_default_gate_contains_only_13_actual_modern_family_controls(self):
        candidates = controls.candidates()
        self.assertEqual(len(candidates), 13)
        self.assertEqual({entry['origin'] for entry in candidates}, {'v2-catalog', 'native-jql'})

    def test_catalog_missing_a_family_is_rejected_before_running_any_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            catalog = directory / 'MUTANTS.json'
            catalog.write_text(json.dumps([{'name': 'r1', 'family': 'R1', 'patch': 'r1.patch',
                                            'loses': ['row'], 'why': 'Only this one family exists.'}]))
            (directory / 'r1.patch').write_text('not applied in catalog discovery')
            with self.assertRaisesRegex(RuntimeError, 'no registered controls'):
                controls.candidates(controls.MUTANTS, catalog)


class IdempotencyMutationTests(unittest.TestCase):
    NODE_PROBE = r"""
const fs = require('node:fs');
const vm = require('node:vm');
const paths = JSON.parse(fs.readFileSync(0, 'utf8'));
async function exercise(path, mode) {
  const entities = new Map();
  const writes = [];
  let uuid = 0;
  const table = (name) => {
    if (!entities.has(name)) entities.set(name, new Map());
    return entities.get(name);
  };
  const kvs = {
    get: async () => undefined,
    entity: (name) => ({
      get: async (key) => table(name).get(key),
      set: async (key, value, options) => {
        writes.push({ name, key, conditional: options?.keyPolicy === 'FAIL_IF_EXISTS' });
        if (options?.keyPolicy === 'FAIL_IF_EXISTS' && table(name).has(key)) throw new Error('KEY_CONFLICT');
        table(name).set(key, JSON.parse(JSON.stringify(value)));
      },
    }),
  };
  const source = fs.readFileSync(path, 'utf8').replace(/^import .*;\n/gm, '').replace(/^export /gm, '');
  const context = vm.createContext({ kvs, randomUUID: () => `delivery-${++uuid}`, setTimeout });
  vm.runInContext(source, context, { filename: path });
  const row = { changeId: '314', sprintId: '72', issueId: '91', issueKey: 'TEST-91',
    at: 1234, kind: 'added', source: 'event', authorId: 'viewer', authorName: 'Viewer' };
  let result;
  if (mode === 'concurrent') {
    context.row = row;
    result = await vm.runInContext('Promise.all([recordChange(row), recordChange(row)])', context);
  } else if (mode === 'migration') {
    context.row = row;
    context.extra = { estimate: 5, boardId: '12', estimateField: 'points', deleted: false };
    result = await vm.runInContext('(async () => [await copyV1(row, extra), await copyV1(row, extra)])()', context);
  } else if (mode === 'v1-attributes') {
    table('scope-change').set('314:72', { ...row, at: 1200, source: 'v1-event', authorName: 'Original author' });
    context.row = row;
    result = await vm.runInContext('recordChange(row)', context);
  } else {
    context.row = row;
    context.next = { ...row, changeId: mode === 'distinct' ? '315' : row.changeId };
    result = await vm.runInContext('(async () => [await recordChange(row), await recordChange(next)])()', context);
  }
  return { result, rows: [...table('scope-ledger').values()], writes };
}
(async () => {
  const result = {};
  for (const [name, path] of Object.entries(paths)) {
    result[name] = {};
    for (const mode of ['sequential', 'concurrent', 'migration', 'v1-attributes', 'distinct']) {
      result[name][mode] = await exercise(path, mode);
    }
  }
  process.stdout.write(JSON.stringify(result));
})().catch((error) => { process.stderr.write(error.stack); process.exitCode = 1; });
"""

    def test_delivery_uuid_keys_duplicate_real_recording_without_disabling_atomic_storage(self):
        candidate = next(entry for entry in controls.candidates() if entry['name'] == 'r3_delivery_idempotency_key')
        with tempfile.TemporaryDirectory() as tmp:
            tree = Path(tmp) / candidate['name']
            controls.materialise_control(tree, patch=candidate['patch'])
            proc = subprocess.run(['node', '-e', self.NODE_PROBE], input=json.dumps({
                'golden': str(controls.GOLDEN / 'src' / 'ledger.js'),
                'mutant': str(tree / 'src' / 'ledger.js'),
            }), capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        got = json.loads(proc.stdout)
        for mode in ('sequential', 'concurrent'):
            with self.subTest(mode=mode):
                self.assertEqual(len(got['golden'][mode]['rows']), 1)
                self.assertEqual(sum(got['golden'][mode]['result']), 1)
                self.assertEqual(len(got['mutant'][mode]['rows']), 2)
                self.assertEqual(got['mutant'][mode]['result'], [True, True])
                self.assertEqual({(row['changeId'], row['sprintId']) for row in got['mutant'][mode]['rows']},
                                 {('314', '72')})
        for name in ('golden', 'mutant'):
            self.assertEqual(got[name]['migration']['result'], [True, False])
            self.assertEqual(len(got[name]['migration']['rows']), 1)
            self.assertEqual(len(got[name]['distinct']['rows']), 2)
            self.assertEqual(got[name]['v1-attributes']['rows'][0]['at'], 1200)
            self.assertEqual(got[name]['v1-attributes']['rows'][0]['authorName'], 'Original author')
            self.assertEqual(got[name]['v1-attributes']['rows'][0]['source'], 'v1-event')
            for scenario in got[name].values():
                self.assertTrue(all(write['conditional'] for write in scenario['writes']))


class SourceReceiptTests(unittest.TestCase):
    def test_actual_source_and_rebuilt_bundles_are_hashed_but_runtime_caches_are_excluded(self):
        with tempfile.TemporaryDirectory() as tmp:
            tree = Path(tmp)
            (tree / 'src').mkdir()
            (tree / 'static').mkdir()
            (tree / '.forge-dev').mkdir()
            (tree / 'src' / 'index.js').write_text('export const run = () => 1;\n')
            (tree / 'static' / 'bundle.js').write_text('const rebuilt=1;\n')
            (tree / '.forge-dev' / 'state.json').write_text('first runtime cache')
            (tree / 'forge-observations.json').write_text('first observations')
            (tree / 'forge-reference-pass.json').write_text('previous seed pass receipt')
            (tree / 'trace.jsonl').write_text('previous model trace')
            first = controls.source_inventory(tree)
            self.assertEqual([entry['path'] for entry in first['files']], ['src/index.js', 'static/bundle.js'])
            (tree / '.forge-dev' / 'state.json').write_text('later runtime cache')
            (tree / 'forge-observations.json').write_text('later observations')
            (tree / 'forge-reference-pass.json').write_text('later seed pass receipt')
            (tree / 'trace.jsonl').write_text('later model trace')
            self.assertEqual(controls.source_inventory(tree), first)
            (tree / 'static' / 'bundle.js').write_text('const rebuilt=2;\n')
            self.assertNotEqual(controls.source_inventory(tree)['sha256'], first['sha256'])

    def test_native_control_receipt_preserves_tested_manifest_and_source_diffs(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            golden, mutant, keep = directory / 'golden', directory / 'native', directory / 'keep'
            for tree in (golden, mutant, keep):
                tree.mkdir()
            for tree in (golden, mutant):
                (tree / 'src').mkdir()
                (tree / 'manifest.yml').write_text('modules: native-jql\n')
                (tree / 'src' / 'jql.js').write_text('export const refresh = () => update();\n')
            (mutant / 'manifest.yml').write_text('modules: no-jql\n')
            (mutant / 'src' / 'jql.js').write_text('export const refresh = () => {};\n')
            with patch.object(controls, 'KEEP', keep):
                summary = controls.source_receipt(mutant, golden,
                    {'name': 'native', 'origin': 'native-jql', 'runtime_id': 'native'})
            got = json.loads(Path(summary['receipt']).read_text())
            self.assertEqual(got['mutation']['runtime_id'], 'native')
            self.assertEqual(set(got['diffs']), {'manifest.yml', 'src/jql.js'})
            self.assertIn('-modules: native-jql', got['diffs']['manifest.yml'])
            self.assertIn('+export const refresh = () => {};', got['diffs']['src/jql.js'])
            self.assertEqual(got['reference_inventory_sha256'], controls.source_inventory(golden)['sha256'])
            self.assertEqual(summary['inventory_sha256'], controls.source_inventory(mutant)['sha256'])


class CommonPathTests(unittest.TestCase):
    def test_catalog_mutant_reaches_the_same_scoring_and_judging_loop(self):
        import score_forge2
        calls = []

        def score(tree, seed, out):
            calls.append(tree.name)
            if tree.name in ('starter_untouched', 'one_function'):
                return {'score': 0.0, 'status': 'scored', 'checks': [], 'critical': {}}
            return verdict({'r5_nonadmin_refused': 0.0 if tree.name == 'r5_save_unguarded' else 1.0},
                           classes=['admin'] if tree.name == 'r5_save_unguarded' else [])

        with patch.object(score_forge2, '_kit', return_value=({'app_modules': None}, None)), \
                patch.object(score_forge2, 'reference_failures', return_value=[]), \
                patch.object(controls, '_rebuild', return_value=None), patch.object(controls, 'score', side_effect=score), \
                redirect_stdout(StringIO()):
            result = controls.main(['--seed', '0123456789abcdef', '--only', 'r5_save_unguarded'])
        self.assertEqual(result, 0)
        self.assertEqual(calls, ['golden', 'r5_save_unguarded', 'starter_untouched', 'one_function'])

    def test_preparation_never_scores(self):
        import score_forge2
        with patch.object(score_forge2, '_kit', return_value=({'app_modules': None, 'kit_dir': '/unused'}, None)), \
                patch.object(controls, '_rebuild', return_value=None), \
                patch.object(controls, 'offline_lint', return_value={'errors': [], 'warnings': []}), \
                patch.object(controls, 'score', side_effect=AssertionError('preparation called scorer')), \
                redirect_stdout(StringIO()):
            result = controls.main(['--prepare-families', '--only', 'r5_save_unguarded'])
        self.assertEqual(result, 0)


if __name__ == '__main__':
    unittest.main()
