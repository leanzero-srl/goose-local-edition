"""forge-1.0 controls (DESIGN §13.4 items 5, 6, 9): one-defect mutants and the idle-app controls.

    python3 forge_controls.py --seed <16 hex> [--only m_gadget] [--out controls.json]

For every `forge/mutants/<id>.patch` with its `<id>.expect.json` (`{"loses": [checks], "critical": bool,
"max_final": n}`, authored by WP3 from DESIGN's check table — interface I4): copy `bench/golden-forge`, apply
the patch, rebuild the Custom UI when the golden carries a build script, score it SERIALLY with score_forge.py at
the same seed as the golden, and compare. A mutant passes only when it loses exactly its declared rows (plus
rows ROOT_BLOCKS attributes to a declared root, never anything else), fires a critical exactly when declared,
lands at or below `max_final`, and scored with no unavailable row and no harness_missing (the lint double-run
included). The empty starter and the one-function app (a trigger that does nothing) must be SCORED — never
refused — at or below 0.05.

Nothing here edits the golden or the mutants; every candidate is a disposable copy.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Dict, List, Optional

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
GOLDEN = HERE / 'golden-forge'
MUTANTS = ROOT / 'forge' / 'mutants'
STARTER = ROOT / 'forge' / 'starter'
IDLE_MAX = 0.05  # DESIGN §8.6 (6) / §13.4 item 6


KEEP = None  # a directory where every verdict is kept (--keep)


QUIET_LOAD = None  # --quiet-load N: wait before each scoring until the 1-minute load average is below N
YIELD_TO: List[str] = []  # --yield-to PATTERN: wait while another scorer (pgrep -f PATTERN) is measuring on this host


def _busy_scorers() -> List[str]:
    found = []
    for pattern in YIELD_TO:
        for pid in subprocess.run(['pgrep', '-f', pattern], capture_output=True, text=True).stdout.split():
            args = subprocess.run(['ps', '-o', 'args=', '-p', pid], capture_output=True, text=True).stdout
            # a process that merely NAMES the pattern as an argument (another controls run's --yield-to) is not a scorer
            if int(pid) != os.getpid() and args and '--yield-to' not in args:
                found.append(f'{pattern}:{pid}')
    return found


def _wait_quiet() -> None:
    """A loaded host turns real-time rows (module timeouts, in-invocation waits) into host noise: wait it out. Another
    scorer measuring latency on this host (a Gauntlet's score_sb7) is not loaded on top of: yield until it ends."""
    import time
    while _busy_scorers():
        print(f'yielding to {_busy_scorers()} before the next scoring', file=sys.stderr, flush=True)
        time.sleep(30)
    while QUIET_LOAD is not None and os.getloadavg()[0] >= QUIET_LOAD:
        print(f'load {os.getloadavg()[0]:.1f} >= {QUIET_LOAD}: waiting before the next scoring', file=sys.stderr, flush=True)
        time.sleep(30)


def score(tree: Path, seed: str, out: Path) -> Dict:
    """One serial scoring through the real CLI (it takes the host lock itself)."""
    _wait_quiet()
    cmd = [sys.executable, '-B', str(HERE / 'score_forge.py'), '--tree', str(tree), '--seed', seed,
           '--json-out', str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0 or not out.is_file():
        raise RuntimeError(f'score_forge.py exited {proc.returncode} on {tree.name}: {proc.stderr.strip()[-600:]}')
    if KEEP:
        shutil.copy2(out, KEEP / out.name)
        if (tree / 'forge-observations.json').is_file():
            shutil.copy2(tree / 'forge-observations.json', KEEP / (out.stem + '-obs.json'))
    return json.loads(out.read_text())


def _rows(verdict: Dict) -> Dict[str, Dict]:
    return {r['check']: r for r in verdict['checks']}


def judge(golden: Dict, mutant: Dict, expect: Dict, root_blocks: Dict[str, tuple], calibration_owned=()) -> List[str]:
    """Failures (empty = the mutant behaves as declared)."""
    fails: List[str] = []
    g, m = _rows(golden), _rows(mutant)
    if mutant.get('probe_unavailable'):
        fails.append(f"unavailable rows {mutant['probe_unavailable']} (the lint double-run included) — harness defect")
    if mutant.get('harness_missing'):
        fails.append(f"harness_missing {mutant['harness_missing'][:4]} — the emulator must model the mutant's calls")
    lost = {n for n, row in m.items() if n in g and row['score'] < g[n]['score'] - 1e-9}
    declared = set(expect.get('loses') or [])
    unknown = declared - set(g)
    if unknown:
        fails.append(f'expect.json names unregistered checks {sorted(unknown)}')
    # ROOT_BLOCKS attribution is transitive: a declared root that lost attributes its dependents, and a dependent
    # that is itself a root (r_backfill_complete under l_bundles_load) attributes its own.
    attributed, frontier = set(), declared & lost
    while frontier:
        nxt = {d for root in frontier for d in root_blocks.get(root, ())} - attributed
        attributed |= nxt
        frontier = nxt & lost
    # A row the mutant left VACUOUS (its precondition unmet: the surface was never exercised) is the shadow of the
    # declared defect, priced 0 with no multiplier of its own — attributed whenever a declared row was lost.
    if declared & lost:
        attributed |= {n for n, row in m.items() if (row.get('parts') or {}).get('vacuous_root', '').startswith('precondition')}
    missing = sorted(declared - lost)
    # Calibration-owned economy rows are ratios any code change moves; they are reported, never a mutant's defect.
    extra = sorted(lost - declared - attributed - set(calibration_owned))
    if missing:
        fails.append(f'declared losses that did not happen: {missing}')
    if extra:
        fails.append(f'undeclared losses: {extra}')
    fired = bool((mutant.get('critical') or {}).get('unsuppressed'))
    if 'critical' in expect and bool(expect['critical']) != fired:
        fails.append(f"critical expected {bool(expect['critical'])}, fired {mutant['critical'].get('unsuppressed')}")
    if 'max_final' in expect and mutant['score'] > float(expect['max_final']) + 1e-9:
        fails.append(f"final {mutant['score']} > max_final {expect['max_final']}")
    return fails


def _materialise(src: Path, dest: Path) -> None:
    shutil.copytree(src, dest, ignore=shutil.ignore_patterns('node_modules', '.forge-dev', 'forge-shots', '__pycache__',
                                                             'verdict*.json', 'forge-observations.json'))


def _rebuild(tree: Path, app_modules: Optional[str]) -> Optional[str]:
    pkg = tree / 'package.json'
    if not pkg.is_file() or 'build' not in (json.loads(pkg.read_text()).get('scripts') or {}):
        return None
    if not app_modules:
        return 'the golden has a build script but no kit app-modules are available to run it'
    link = tree / 'node_modules'
    if not link.exists():
        os.symlink(app_modules, link)
    proc = subprocess.run(['npm', 'run', 'build'], cwd=tree, capture_output=True, text=True)
    link.unlink()
    return None if proc.returncode == 0 else f'npm run build failed: {proc.stderr.strip()[-400:]}'


def empty_starter(dest: Path) -> None:
    """The starter exactly as run_build hands it out: its .gitkeep placeholders stripped (STARTER.md: empty)."""
    shutil.copytree(STARTER, dest, ignore=shutil.ignore_patterns('.gitkeep', '__pycache__', '.DS_Store'))


def one_function_app(dest: Path) -> None:
    """The starter plus one trigger whose handler does nothing (DESIGN §8.6 (6))."""
    empty_starter(dest)
    manifest = (STARTER / 'manifest.yml').read_text()
    app_id = re.search(r'^\s*id:\s*(\S+)', manifest, re.M)
    runtime = re.search(r'^\s*name:\s*(nodejs\S+)', manifest, re.M)
    if not app_id or not runtime:
        raise RuntimeError('forge/starter/manifest.yml lacks app.id or runtime.name')
    (dest / 'manifest.yml').write_text(
        'modules:\n  trigger:\n    - key: noop-trigger\n      function: noop\n      events:\n'
        '        - avi:jira:updated:issue\n  function:\n    - key: noop\n      handler: index.run\n'
        f'app:\n  id: {app_id.group(1)}\n  runtime:\n    name: {runtime.group(1)}\n')
    (dest / 'src').mkdir(exist_ok=True)
    (dest / 'src' / 'index.js').write_text('export const run = async () => {};\n')


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--seed', required=True)
    ap.add_argument('--only', action='append', default=[])
    ap.add_argument('--out', type=Path)
    ap.add_argument('--keep', type=Path, help='keep every verdict JSON here')
    ap.add_argument('--quiet-load', type=float, help='wait before each scoring until the 1-min load is below this')
    ap.add_argument('--yield-to', action='append', default=[], metavar='PATTERN',
                    help='wait before each scoring while `pgrep -f PATTERN` finds another scorer (repeatable)')
    a = ap.parse_args(argv)
    global KEEP, QUIET_LOAD, YIELD_TO
    QUIET_LOAD = a.quiet_load
    YIELD_TO = a.yield_to
    if a.keep:
        a.keep.mkdir(parents=True, exist_ok=True)
        KEEP = a.keep
    if len(a.seed) != 16 or any(c not in '0123456789abcdefABCDEF' for c in a.seed):
        ap.error('--seed must be 16 hex characters')
    if not GOLDEN.is_dir():
        print(f'REFUSED: {GOLDEN} does not exist (WP3 deliverable)', file=sys.stderr)
        return 2
    import score_forge
    kit, why = score_forge._kit()  # noqa: SLF001 — one kit-discovery path for scorer and controls
    if kit is None:
        print(f'REFUSED: {why}', file=sys.stderr)
        return 3
    app_modules = kit['app_modules']
    report: Dict = {'seed': a.seed, 'mutants': {}, 'controls': {}}
    failed = False
    with tempfile.TemporaryDirectory(prefix='forge-controls-') as tmp:
        tmp = Path(tmp)
        # The golden is materialised and rebuilt exactly as every mutant is, so the comparison is like for like.
        _materialise(GOLDEN, tmp / 'golden')
        why = _rebuild(tmp / 'golden', app_modules)
        if why:
            print(f'REFUSED: the golden does not rebuild: {why}', file=sys.stderr)
            return 3
        golden = score(tmp / 'golden', a.seed, tmp / 'golden.json')
        report['golden'] = {'score': golden['score'], 'status': golden['status']}
        gfails = score_forge.reference_failures(golden)
        if gfails:
            report['golden']['reference_failures'] = gfails
            failed = True
        patches = sorted(MUTANTS.glob('*.patch')) if MUTANTS.is_dir() else []
        for patch in patches:
            mid = patch.stem
            if a.only and mid not in a.only:
                continue
            expect_path = patch.with_suffix('.expect.json')
            entry: Dict = {}
            try:
                if not expect_path.is_file():
                    raise RuntimeError(f'{expect_path.name} is missing')
                expect = json.loads(expect_path.read_text())
                tree = tmp / mid
                _materialise(GOLDEN, tree)
                proc = subprocess.run(['git', 'apply', '--whitespace=nowarn', str(patch)], cwd=tree,
                                      capture_output=True, text=True)
                if proc.returncode:
                    raise RuntimeError(f'patch does not apply: {proc.stderr.strip()[-300:]}')
                why = _rebuild(tree, app_modules)
                if why:
                    raise RuntimeError(why)
                verdict = score(tree, a.seed, tmp / f'{mid}.json')
                entry = {'score': verdict['score'], 'lost': sorted(
                    n for n, r in _rows(verdict).items() if r['score'] < _rows(golden)[n]['score'] - 1e-9),
                    'criticals': verdict['critical'].get('unsuppressed'),
                    'fails': judge(golden, verdict, expect, score_forge.ROOT_BLOCKS, score_forge.CALIBRATION_OWNED)}
            except Exception as error:
                entry = {'fails': [f'{type(error).__name__}: {error}']}
            failed |= bool(entry['fails'])
            report['mutants'][mid] = entry
            if a.out:
                a.out.write_text(json.dumps(report, indent=2))
            print(f"{mid}: {entry.get('score')} {'PASS' if not entry['fails'] else 'FAIL ' + '; '.join(entry['fails'])}",
                  file=sys.stderr, flush=True)
        if not patches:
            report['mutants_missing'] = f'no *.patch under {MUTANTS} (WP3 deliverable)'
            failed = True
        for label, build in (('empty_starter', empty_starter), ('one_function', one_function_app)):
            try:
                tree = tmp / label
                build(tree)
                verdict = score(tree, a.seed, tmp / f'{label}.json')
                fails = [] if verdict['status'] == 'scored' and verdict['score'] <= IDLE_MAX else \
                    [f"{verdict['status']} at {verdict['score']} (must be scored at <= {IDLE_MAX})"]
                report['controls'][label] = {'score': verdict['score'], 'fails': fails}
            except Exception as error:
                report['controls'][label] = {'fails': [f'{type(error).__name__}: {error}']}
            failed |= bool(report['controls'][label]['fails'])
    text = json.dumps(report, indent=2)
    if a.out:
        a.out.write_text(text)
    print(text)
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
