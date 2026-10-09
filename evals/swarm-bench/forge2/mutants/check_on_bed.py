#!/usr/bin/env python3
"""Run every mutant against WP3's own offline bed (bench/golden-forge/dev) and print which bed checks it
breaks — the proof that each patch applies, builds and actually bites, independent of the scorer.
Usage: python3 forge/mutants/check_on_bed.py [mutant ids...]"""
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOLDEN = HERE.parent.parent / 'bench' / 'golden-forge'
SPIKE = HERE.parent / 'spike'


def run(cmd, cwd, env=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env={**os.environ, **(env or {})})
    return p.returncode, p.stdout + p.stderr


def main():
    ids = sys.argv[1:] or sorted(p.stem for p in HERE.glob('*.patch'))
    env = {'GOLDEN_YAML_MODULE': str(SPIKE / 'node_modules' / 'yaml')}
    for mid in ids:
        with tempfile.TemporaryDirectory(prefix=f'{mid}-') as tmp:
            tree = Path(tmp) / 'app'
            shutil.copytree(GOLDEN, tree, ignore=shutil.ignore_patterns('node_modules'))
            os.symlink(GOLDEN / 'node_modules', tree / 'node_modules')
            code, out = run(['git', 'apply', '--whitespace=nowarn', str(HERE / f'{mid}.patch')], tree)
            if code:
                print(f'{mid}: PATCH DOES NOT APPLY {out.strip()}')
                continue
            code, out = run(['npm', 'run', 'build'], tree)
            if code:
                print(f'{mid}: BUILD FAILED {out.strip()[-300:]}')
                continue
            _, lint = run(['node', 'harness/lint-offline.cjs', str(tree)], SPIKE)
            m = re.search(r'LINT_JSON (.*)', lint)
            lint_s = m.group(1)[:200] if m else lint.strip()[-200:]
            fails = []
            for test in ('test-backend.cjs', 'test-ui.cjs'):
                code, out = run(['node', f'dev/{test}', str(Path(tmp) / test)], tree, env)
                fails += [f'{test[5:-4]}: {line[6:].splitlines()[0][:110]}' for line in out.split('\n') if line.startswith('FAIL')]
                if code == 2:
                    fails.append(f'{test[5:-4]}: CRASH {out.strip().splitlines()[-1][:160] if out.strip() else ""}')
            print(f'== {mid}\n   lint {lint_s}\n   ' + ('\n   '.join(fails) if fails else 'NO BED CHECK FAILED'))


if __name__ == '__main__':
    main()
