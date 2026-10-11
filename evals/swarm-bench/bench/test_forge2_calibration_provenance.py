import copy
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import forge2_kit
import score_forge2 as s


class CalibrationProvenance(unittest.TestCase):
    def verdicts(self):
        identity = {n: 'a' * 64 for n in s.CALIBRATION_IDENTITY_FIELDS}
        identity['scorer_files_sha256'] = {'scorer.py': 'b' * 64}
        identity['evaluation_inputs'] = {n: {'source': 'c' * 64} for n in ('candidate', 'site', 'public', 'v1_schema')}
        values = []
        for index in range(5):
            v = s.compose_from_rows(s._perfect_rows())
            v.update(copy.deepcopy(identity), fixture_seed=f'{index:016x}', scorerVersion='forge-2.0-rc')
            for row in v['checks']:
                if row['check'] in s.FITTED_TOPS.values():
                    row['parts'] = {'ratio': 2.5, 'calls': 5, 'optimum': 2}
            values.append(v)
        return values

    def test_complete_provenance_is_retained_without_aliases(self):
        v = self.verdicts(); fitted = s.calibrate(v, s.TH)
        for name in s.CALIBRATION_IDENTITY_FIELDS:
            self.assertEqual(fitted['calibration'][name], v[0][name])
        v[0]['evaluation_inputs']['candidate']['source'] = 'd' * 64
        self.assertEqual(fitted['calibration']['evaluation_inputs']['candidate']['source'], 'c' * 64)

    def test_each_changed_app_site_public_sdk_or_scorer_refuses(self):
        for name in s.CALIBRATION_IDENTITY_FIELDS:
            sections = ('candidate', 'site', 'public', 'v1_schema') if name == 'evaluation_inputs' else (None,)
            for section in sections:
                with self.subTest(name=name, section=section):
                    v = self.verdicts()
                    if section:
                        v[-1][name][section]['source'] = 'd' * 64
                    elif isinstance(v[-1][name], dict):
                        v[-1][name]['scorer.py'] = 'd' * 64
                    else:
                        v[-1][name] = 'd' * 64
                    with self.assertRaisesRegex(ValueError, 'different app, site'):
                        s.calibrate(v, s.TH)

    def test_equal_absent_malformed_and_untyped_provenance_refuses(self):
        for name in s.CALIBRATION_IDENTITY_FIELDS:
            for bad in (None, {}, 'not-a-sha', 'A' * 64, {'pretend': {'nested': 'a' * 64}}):
                with self.subTest(name=name, bad=bad):
                    v = self.verdicts()
                    for item in v: item[name] = bad
                    with self.assertRaisesRegex(ValueError, 'provenance is missing or malformed'):
                        s.calibrate(v, s.TH)
        for name in ('spec_sha256', 'kit_lock_sha256', 'kit_code_sha256', 'wrapper_sha256'):
            v = self.verdicts()
            for item in v: item[name] = {'pretend': 'a' * 64}
            with self.assertRaisesRegex(ValueError, 'provenance is missing or malformed'):
                s.calibrate(v, s.TH)
        v = self.verdicts()
        for item in v:
            item['evaluation_inputs'] = {n: 'a' * 64 for n in ('candidate', 'site', 'public', 'v1_schema')}
        with self.assertRaisesRegex(ValueError, 'evaluation_inputs sections'):
            s.calibrate(v, s.TH)

    def test_clone_omits_generated_pass_receipt_but_keeps_app(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory); source = base / 'source'; source.mkdir()
            (source / 'manifest.yml').write_text('actual manifest')
            (source / 'forge-reference-pass.json').write_text('first seed')
            first = base / 'first'; s.clone_tree(source, first)
            (source / 'forge-reference-pass.json').write_text('second seed')
            second = base / 'second'; s.clone_tree(source, second)
            self.assertEqual(s.input_file_hashes(first), s.input_file_hashes(second))
            self.assertNotIn('forge-reference-pass.json', s.input_file_hashes(first))
            (second / 'manifest.yml').write_text('different app')
            self.assertNotEqual(s.input_file_hashes(first), s.input_file_hashes(second))

    def probe(self, mutation=None):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory); repo = base / 'repo'
            for section in ('site', 'public', 'starter'):
                target = repo / 'forge2' / section; target.mkdir(parents=True)
                (target / 'input').write_text(section)
            (repo / 'forge2/starter/manifest.yml').write_text('v1 schema')
            app = base / 'candidate'; app.mkdir(); (app / 'manifest.yml').write_text('tested app')
            kit = base / 'kit'; (kit / 'bin').mkdir(parents=True)
            (kit / 'bin/runtime').write_text('SDK code')
            code = forge2_kit.code_sha256(kit); sources = {'source': 'a' * 64}
            def launch(command, **_kwargs):
                Path(command[command.index('--out') + 1]).write_text('{}')
                if mutation in ('site', 'public'):
                    (repo / 'forge2' / mutation / 'input').write_text('changed during probe')
                elif mutation == 'v1_schema':
                    (repo / 'forge2/starter/manifest.yml').write_text('changed preload schema')
                elif mutation == 'candidate':
                    Path(command[command.index('--app') + 1], 'manifest.yml').write_text('changed app during probe')
                elif mutation == 'sdk':
                    (kit / 'bin/runtime').write_text('changed during probe')
                elif mutation == 'scorer':
                    sources['source'] = 'b' * 64
                return type('Process', (), {'wait': lambda _self: 0})()
            with patch.object(s, 'ROOT', repo), patch.object(s, '_LOADED_SCORER_FILES', copy.deepcopy(sources)), \
                    patch.object(s, 'scorer_files_sha256', side_effect=lambda: copy.deepcopy(sources)), \
                    patch.object(s, 'build_pack', return_value={}), patch.object(s, '_render_node', return_value='node'), \
                    patch.object(s.subprocess, 'Popen', side_effect=launch):
                if mutation == 'preloaded': sources['source'] = 'b' * 64
                return s._probe_one(app, '0' * 16, {'dir': str(kit), 'kit_code_sha256': code}, 'wrapper',
                                    base / 'shots', None, base / 'observations.json')[0]

    def test_producer_binds_actual_tested_clone_and_loaded_scorer(self):
        obs = self.probe()
        self.assertEqual(set(obs['evaluationInputs']), {'candidate', 'site', 'public', 'v1_schema'})
        self.assertEqual(obs['evaluationInputs']['candidate'],
                         {'manifest.yml': hashlib.sha256(b'tested app').hexdigest()})
        self.assertEqual(obs['scorerFilesSha256'], {'source': 'a' * 64})

    def test_changed_inputs_or_loaded_scorer_cannot_get_current_stamp(self):
        for mutation in ('candidate', 'site', 'public', 'v1_schema', 'sdk', 'scorer', 'preloaded'):
            with self.subTest(mutation=mutation), self.assertRaisesRegex(RuntimeError, 'REFUSED:'):
                self.probe(mutation)


if __name__ == '__main__': unittest.main()
