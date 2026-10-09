#!/usr/bin/env python3
"""forge-2.0 mutants (SPEC §5: one one-defect mutant of bench/golden-forge2 per requirement family R1..R9).

    python3 build.py <name | x.patch> <target dir> [--no-build] [--lint]
    python3 build.py --all <scratch dir>      every mutant in MUTANTS.json, the golden first as the baseline

<target dir> (new or empty) gets a copy of the golden without node_modules or probe output, the patch applied with
`git apply`, then the Custom UI rebuilt (`npm run build` with the kit's app-modules linked as node_modules for the build
only), as bench/forge2_controls.py materialises a candidate. --lint runs the kit's offline lint over the result. --all
proves each patch applies, builds and lints with no error and no warning the golden lacks; it scores nothing (the gate
scores).

MUTANTS.json, one entry per mutant:
  name, family, patch   the requirement family (R1..R9) and the patch file in this directory
  loses                 rows the defect must lose against the golden scored at the same seed
  may_lose              rows it loses only where it reaches a row the golden got right on that seed: never an
                        undeclared loss, never a missing one
  critical              false (no critical may fire) or the SPEC §4 class that must (score_forge2.CRITICAL_CLASSES)
  why                   the defect and why those rows
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent.parent / 'bench'
GOLDEN = BENCH / 'golden-forge2'
IGNORE = shutil.ignore_patterns('node_modules', '.forge-dev', 'forge-shots', 'bench-media', '__pycache__', 'verdict*.json',
                                'forge-observations.json')


def mutants():
    return json.loads((HERE / 'MUTANTS.json').read_text())


def kit():
    sys.path.insert(0, str(BENCH))
    import forge2_kit
    return forge2_kit.ensure()


def run(cmd, cwd, env=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env={**os.environ, **(env or {})})
    return p.returncode, (p.stdout + p.stderr).strip()


def materialise(patch: Path, target: Path, info=None) -> None:
    """Golden copy + patch (+ the Custom UI rebuilt when `info`, the kit, is given). Raises on any failure."""
    if target.exists() and any(target.iterdir()):
        raise RuntimeError(f'{target} is not empty')
    if target.exists():
        target.rmdir()
    shutil.copytree(GOLDEN, target, ignore=IGNORE)
    if patch is not None:
        code, out = run(['git', 'apply', '--whitespace=nowarn', str(patch)], target)
        if code:
            raise RuntimeError(f'{patch.name} does not apply: {out[-400:]}')
    if info is None or 'build' not in (json.loads((target / 'package.json').read_text()).get('scripts') or {}):
        return
    link = target / 'node_modules'
    link.symlink_to(Path(info['modules_dir']) / 'app-modules' / 'node_modules')
    try:
        code, out = run(['npm', 'run', 'build'], target)
    finally:
        link.unlink()
    if code:
        raise RuntimeError(f'npm run build failed: {out[-400:]}')


def lint(target: Path, info) -> dict:
    """The kit's offline lint (the probe's runLint): {errors, warnings: [message]}."""
    _code, out = run(['node', str(Path(info['kit_dir']) / 'bin' / 'lint.cjs'), '--json'], target, {'FORGE_KIT': info['kit_dir']})
    line = next((l.removeprefix('LINT_JSON ') for l in reversed(out.splitlines()) if l.removeprefix('LINT_JSON ').startswith('{')), None)
    if line is None:
        raise RuntimeError(f'lint crashed: {out[-400:]}')
    problems = json.loads(line).get('problems') or []
    return {'errors': [p.get('message') for p in problems if p.get('sev') == 'error'],
            'warnings': sorted(p.get('message') for p in problems if p.get('sev') == 'warning')}


def patch_of(arg: str) -> Path:
    for path in (Path(arg), HERE / arg, HERE / f'{arg}.patch'):
        if path.suffix == '.patch' and path.is_file():
            return path.resolve()
    raise SystemExit(f'no patch named {arg}')


def check_all(scratch: Path) -> int:
    info = kit()
    materialise(None, scratch / 'golden', info)
    base = lint(scratch / 'golden', info)
    print(f"golden: lint {len(base['errors'])} errors, {len(base['warnings'])} warnings")
    failed = bool(base['errors'])
    for m in mutants():
        try:
            materialise(patch_of(m['patch']), scratch / m['name'], info)
            got = lint(scratch / m['name'], info)
            new = [w for w in got['warnings'] if w not in base['warnings']]
            bad = got['errors'] + new
            print(f"{m['name']} ({m['family']}): applies, builds; lint {len(got['errors'])} errors, "
                  f"{len(new)} new warnings{': ' + '; '.join(bad)[:300] if bad else ''}")
            failed |= bool(bad)
        except Exception as error:
            print(f"{m['name']} ({m['family']}): FAIL {error}")
            failed = True
    return 1 if failed else 0


def main(argv) -> int:
    if len(argv) == 2 and argv[0] == '--all':
        return check_all(Path(argv[1]).resolve())
    args = [a for a in argv if not a.startswith('--')]
    if len(args) != 2:
        raise SystemExit(__doc__)
    info = None if '--no-build' in argv and '--lint' not in argv else kit()
    target = Path(args[1]).resolve()
    materialise(patch_of(args[0]), target, None if '--no-build' in argv else info)
    if '--lint' in argv:
        got = lint(target, info)
        print(json.dumps(got))
        return 1 if got['errors'] else 0
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
