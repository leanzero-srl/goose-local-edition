"""forge-2.0 controls (forge2/SPEC.md §5): the golden, one-defect mutants, the untouched starter and the idle app.

    python3 forge2_controls.py --seed <16 hex> [--only m_x] [--out controls.json]

The explicit R1..R9 catalog in `forge2/mutants2/MUTANTS.json` and native R10 controls qualify Forge 2.0.
Optional v1 regression patches in `forge2/mutants` require --legacy-regression: old patches may no longer apply
to the v2 reference and their actual failures are reported. All sources share one copy/apply/rebuild/score/judge path. Their expectations (`{"loses": [checks],
"may_lose": [checks], "why": cause, "critical": bool | class, "max_final": n}`) are compared against the golden.
Copy `bench/golden-forge2`, apply the patch, rebuild the Custom UI when the golden carries a build script, score it SERIALLY with score_forge2.py at the same seed as the golden, and compare. A mutant passes only when
it loses its required rows and no undeclared rows (optional named secondary losses require a documented cause,
plus rows ROOT_BLOCKS attributes to a declared root), fires a
critical exactly when declared (`critical` naming a SPEC §4 class must fire THAT class), lands at or below
`max_final`, and scored with no unavailable row and no harness_missing. SPEC §5's gate, checked here: the golden passes
the reference gate (1.000), every v2 family R1..R10 has a passing mutant that loses one of its rows, the starter as
handed out (Scope Ledger v1) scores at or below 0.30, and an idle app (the starter's app id with one no-op trigger)
is SCORED — never refused — at or below 0.05.

Native JQL controls are generated from the current golden's real implementation: remove the installed module,
leave native precomputations stale, or incorrectly share the last person's visible IDs. They are built and scored
through the same runtime as patch mutants. --materialise-runtime <id> --tree <path> creates one disposable control
without scoring, for the focused HTTP tests.

--prepare-families applies, rebuilds and lints all catalog and native family controls without any scoring or model
calls. Nothing here edits the golden or the mutants; every candidate is a disposable copy.
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
GOLDEN = HERE / 'golden-forge2'
MUTANTS = ROOT / 'forge2' / 'mutants'
FAMILY_CATALOG = ROOT / 'forge2' / 'mutants2' / 'MUTANTS.json'
STARTER = ROOT / 'forge2' / 'starter'
IDLE_MAX = 0.05     # DESIGN §8.6 (6) / §13.4 item 6
STARTER_MAX = 0.30  # SPEC §5: "starter untouched <= 0.30"


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
    cmd = [sys.executable, '-B', str(HERE / 'score_forge2.py'), '--tree', str(tree), '--seed', seed, '--single-seed',
           '--json-out', str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0 or not out.is_file():
        raise RuntimeError(f'score_forge2.py exited {proc.returncode} on {tree.name}: {proc.stderr.strip()[-600:]}')
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
    optional = set(expect.get('may_lose') or [])
    if optional and not str(expect.get('why') or '').strip():
        fails.append('may_lose requires a documented causal explanation in why')
        optional = set()
    if declared & optional:
        fails.append(f'checks cannot be both required and optional: {sorted(declared & optional)}')
    absent = set(g) - set(m)
    if absent:
        fails.append(f'mutant verdict omits registered checks {sorted(absent)}')
    unknown = (declared | optional) - set(g)
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
    # declared defect, priced 0 with no critical of its own (its 0 counts in its reliability group like any score) —
    # attributed whenever a declared row was lost.
    if declared & lost:
        # (score_forge2 marks a row its app's own manifest fault left unopened `manifest:`, §17.8 G — the same shadow)
        attributed |= {n for n, row in m.items()
                       if (row.get('parts') or {}).get('vacuous_root', '').startswith(('precondition', 'manifest'))}
    missing = sorted(declared - lost)
    # Calibration-owned economy rows are ratios any code change moves; they are reported, never a mutant's defect.
    extra = sorted(lost - declared - optional - attributed - set(calibration_owned))
    if missing:
        fails.append(f'declared losses that did not happen: {missing}')
    if extra:
        fails.append(f'undeclared losses: {extra}')
    fired = bool((mutant.get('critical') or {}).get('unsuppressed'))
    classes = (mutant.get('critical') or {}).get('classes') or []
    want = expect.get('critical')
    if isinstance(want, str):
        if want not in classes:
            fails.append(f"critical class {want!r} expected, fired classes {classes}")
        unexpected = set(classes) - {want}
        if unexpected:
            fails.append(f'unexpected critical classes {sorted(unexpected)}')
    elif 'critical' in expect and bool(want) != fired:
        fails.append(f"critical expected {bool(want)}, fired {mutant['critical'].get('unsuppressed')}")
    if 'max_final' in expect and mutant['score'] > float(expect['max_final']) + 1e-9:
        fails.append(f"final {mutant['score']} > max_final {expect['max_final']}")
    return fails


def family_gaps(mutants: Dict[str, Dict], tier_of: Dict[str, str], families) -> List[str]:
    """SPEC §5: "one mutant per R-family loses its rows" — the families with no PASSING mutant that lost a row of it."""
    covered = {entry['family'] for entry in mutants.values() if entry.get('family') and not entry.get('fails')
               for n in entry.get('required_losses') or [] if tier_of.get(n) == entry['family']}
    return [fam for fam in families if fam not in covered]


def _materialise(src: Path, dest: Path) -> None:
    shutil.copytree(src, dest, ignore=shutil.ignore_patterns('node_modules', '.forge-dev', 'forge-shots', '__pycache__',
                                                             'verdict*.json', 'forge-observations.json', 'bench-media'))


RUNTIME_MUTANTS = {
    'm_jql_missing_module': {'loses': ['r10_manifest', 'r10_initial_membership', 'r10_precomputation_refresh',
                                     'r10_shared_permissions', 'r10_empty_and_closed', 'r10_invalid_arguments'],
                             'critical': False, 'family': 'R10',
                             'why': 'The native JQL module is absent, so Jira cannot invoke scopeChanged.'},
    'm_jql_stale_precomputation': {'loses': ['r10_precomputation_refresh'], 'critical': False, 'family': 'R10',
                                 'why': 'The refresh handler returns before updating existing shared precomputations.'},
    'm_jql_user_baked_cache': {'loses': ['r10_shared_permissions'], 'critical': False, 'family': 'R10',
                             'why': "A shared precomputation uses the last viewer's permitted IDs; another viewer loses visible members."},
}


def _replace_once(tree: Path, relative: str, before: str, after: str) -> None:
    path = tree / relative
    text = path.read_text()
    count = text.count(before)
    if count != 1:
        raise RuntimeError(f'{relative}: runtime control anchor occurs {count} times: {before[:100]!r}')
    path.write_text(text.replace(before, after, 1))


def _apply_runtime_mutant(dest: Path, mutant_id: str) -> None:
    """One real code defect; observations and scoring are never patched."""
    if mutant_id not in RUNTIME_MUTANTS:
        raise ValueError(f'Unknown runtime mutant {mutant_id!r}')
    if mutant_id == 'm_jql_missing_module':
        path = dest / 'manifest.yml'
        text, count = re.subn(r'(?ms)^  jira:jqlFunction:\n.*?(?=^  [^\s]|\Z)', '', path.read_text())
        if count != 1:
            raise RuntimeError(f'Installed JQL module anchor occurs {count} times')
        path.write_text(text)
    elif mutant_id == 'm_jql_stale_precomputation':
        _replace_once(dest, 'src/jql.js', 'export async function refreshScopePrecomputations(sprintIds, work) {',
                      'export async function refreshScopePrecomputations(sprintIds, work) {\n  return { skipped: true };')
    elif mutant_id == 'm_jql_user_baked_cache':
        _replace_once(dest, 'src/views.js', "import { jiraJson, route, postJson } from './jira';",
                      "import { kvs } from '@forge/kvs';\nimport { jiraJson, route, postJson } from './jira';")
        _replace_once(dest, 'src/views.js', '  return { sprint, ...totalsView(totals), hiddenCount: changes.length - rows.length, changes: rows };',
                      "  await kvs.set(`scope-jql-visible:${sprint.id}`, rows.map((row) => row.issueId));\n"
                      '  return { sprint, ...totalsView(totals), hiddenCount: changes.length - rows.length, changes: rows };')
        _replace_once(dest, 'src/jql.js', "import { changesOfSprint } from './ledger';",
                      "import { kvs } from '@forge/kvs';\nimport { changesOfSprint } from './ledger';")
        _replace_once(dest, 'src/jql.js',
                      '    return { jql: fragment(await changedIds(sprintId, { maxWaitSeconds: 0 }), request.clause.operator) };',
                      '    const ids = await kvs.get(`scope-jql-visible:${sprintId}`) ?? await changedIds(sprintId, { maxWaitSeconds: 0 });\n'
                      '    return { jql: fragment(ids, request.clause.operator) };')


def _rebuild(tree: Path, app_modules: Optional[str]) -> Optional[str]:
    pkg = tree / 'package.json'
    if not pkg.is_file() or 'build' not in (json.loads(pkg.read_text()).get('scripts') or {}):
        return None
    if not app_modules:
        return 'the golden has a build script but no kit app-modules are available to run it'
    link = tree / 'node_modules'
    created = not link.exists()
    if created:
        os.symlink(app_modules, link)
    try:
        proc = subprocess.run(['npm', 'run', 'build'], cwd=tree, capture_output=True, text=True)
    finally:
        if created:
            link.unlink()
    return None if proc.returncode == 0 else f'npm run build failed: {proc.stderr.strip()[-400:]}'


def candidates(patch_dir: Path = MUTANTS, catalog: Path = FAMILY_CATALOG, include_legacy: bool = False) -> List[Dict]:
    """Every registered control, with explicit family ownership rather than inferred incidental losses."""
    result = []
    for patch in sorted(patch_dir.glob('*.patch')) if include_legacy else []:
        expect_path = patch.with_suffix('.expect.json')
        if not expect_path.is_file():
            raise RuntimeError(f'{expect_path} is missing')
        result.append({'name': patch.stem, 'patch': patch, 'expect': json.loads(expect_path.read_text()),
                       'family': None, 'origin': 'v1-regression'})
    if include_legacy and not result:
        raise RuntimeError(f'no v1 regression patches under {patch_dir}')
    rows = json.loads(catalog.read_text())
    if not isinstance(rows, list) or not rows:
        raise RuntimeError(f'{catalog} must contain the R1..R9 mutant list')
    covered = set()
    for entry in rows:
        family = entry.get('family')
        if family not in {f'R{i}' for i in range(1, 10)}:
            raise RuntimeError(f"{entry.get('name')}: invalid requirement family {family!r}")
        patch = (catalog.parent / entry['patch']).resolve()
        if patch.parent != catalog.parent.resolve() or not patch.is_file():
            raise RuntimeError(f"{entry['name']}: missing local catalog patch {entry['patch']!r}")
        if not entry.get('loses') or not str(entry.get('why') or '').strip():
            raise RuntimeError(f"{entry['name']}: requires named losses and a causal explanation")
        covered.add(family)
        result.append({'name': entry['name'], 'patch': patch, 'expect': entry, 'family': family,
                       'origin': 'v2-catalog'})
    missing = {f'R{i}' for i in range(1, 10)} - covered
    if missing:
        raise RuntimeError(f'catalog has no registered controls for {sorted(missing)}')
    result.extend({'name': name, 'runtime_id': name, 'expect': expect, 'family': expect['family'],
                   'origin': 'native-jql'} for name, expect in RUNTIME_MUTANTS.items())
    names = [entry['name'] for entry in result]
    if len(names) != len(set(names)):
        raise RuntimeError('duplicate control names across the three control sources')
    return result


def materialise_control(dest: Path, patch: Optional[Path] = None, runtime_id: Optional[str] = None) -> None:
    """The only golden copy/mutation path, shared by the scorer and the standalone patch builder."""
    if patch is not None and runtime_id is not None:
        raise ValueError('a control can have one patch or one runtime mutation')
    if dest.exists():
        if any(dest.iterdir()):
            raise RuntimeError(f'{dest} is not empty')
        dest.rmdir()
    _materialise(GOLDEN, dest)
    if patch is not None:
        proc = subprocess.run(['git', 'apply', '--whitespace=nowarn', str(patch.resolve())], cwd=dest,
                              capture_output=True, text=True)
        if proc.returncode:
            raise RuntimeError(f'{patch.name} does not apply: {proc.stderr.strip()[-400:]}')
    elif runtime_id is not None:
        _apply_runtime_mutant(dest, runtime_id)


def runtime_mutant(dest: Path, mutant_id: str) -> None:
    materialise_control(dest, runtime_id=mutant_id)


def prepare_control(dest: Path, app_modules: Optional[str], patch: Optional[Path] = None,
                    runtime_id: Optional[str] = None) -> None:
    materialise_control(dest, patch, runtime_id)
    why = _rebuild(dest, app_modules)
    if why:
        raise RuntimeError(why)


def offline_lint(tree: Path, kit_dir: str) -> Dict:
    proc = subprocess.run(['node', str(Path(kit_dir) / 'bin' / 'lint.cjs'), '--json'], cwd=tree,
                          capture_output=True, text=True, env={**os.environ, 'FORGE_KIT': kit_dir})
    out = proc.stdout + proc.stderr
    line = next((line.removeprefix('LINT_JSON ') for line in reversed(out.splitlines())
                 if line.removeprefix('LINT_JSON ').startswith('{')), None)
    if line is None:
        raise RuntimeError(f'lint crashed: {out.strip()[-400:]}')
    problems = json.loads(line).get('problems') or []
    return {'errors': [p.get('message') for p in problems if p.get('sev') == 'error'],
            'warnings': sorted(p.get('message') for p in problems if p.get('sev') == 'warning')}


def lost_rows(golden: Dict, mutant: Dict) -> List[str]:
    g = _rows(golden)
    return sorted(name for name, row in _rows(mutant).items()
                  if name in g and row['score'] < g[name]['score'] - 1e-9)


def loss_evidence(golden: Dict, mutant: Dict, expect: Dict) -> Dict:
    """Only observed optional losses are recorded, with the named cause and both actual scored rows."""
    g, m = _rows(golden), _rows(mutant)
    optional_lost = set(lost_rows(golden, mutant)) & set(expect.get('may_lose') or [])
    return {name: {'cause': expect['why'], 'golden': g[name], 'mutant': m[name]} for name in sorted(optional_lost)}


def starter_untouched(dest: Path) -> None:
    """The starter exactly as run_build hands it out: Scope Ledger v1 (forge2/starter), placeholders stripped."""
    shutil.copytree(STARTER, dest, ignore=shutil.ignore_patterns('.gitkeep', '__pycache__', '.DS_Store', 'node_modules'))


def one_function_app(dest: Path) -> None:
    """An idle app: the starter's app id and runtime with ONE trigger whose handler does nothing (DESIGN §8.6 (6)) —
    the v2 rows must not pay it for the failures it never had the chance to make (quota, limits, leaks)."""
    starter_untouched(dest)
    for sub in ('src', 'static', 'skills'):
        shutil.rmtree(dest / sub, ignore_errors=True)
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
    ap.add_argument('--seed')
    ap.add_argument('--legacy-regression', action='store_true',
                    help='also score optional old Forge 1 regression patches against the v2 reference; report failures')
    ap.add_argument('--prepare-families', action='store_true',
                    help='apply/build/lint R1..R10 controls without scoring or model calls')
    ap.add_argument('--materialise-runtime', choices=tuple(RUNTIME_MUTANTS))
    ap.add_argument('--tree', type=Path, help='disposable destination for --materialise-runtime; must not exist')
    ap.add_argument('--only', action='append', default=[])
    ap.add_argument('--out', type=Path)
    ap.add_argument('--keep', type=Path, help='keep every verdict JSON here')
    ap.add_argument('--quiet-load', type=float, help='wait before each scoring until the 1-min load is below this')
    ap.add_argument('--yield-to', action='append', default=[], metavar='PATTERN',
                    help='wait before each scoring while `pgrep -f PATTERN` finds another scorer (repeatable)')
    a = ap.parse_args(argv)
    if a.prepare_families and a.legacy_regression:
        ap.error('--prepare-families qualifies modern families; --legacy-regression is an explicit scoring lane')
    if a.materialise_runtime:
        if not a.tree:
            ap.error('--materialise-runtime requires --tree')
        runtime_mutant(a.tree, a.materialise_runtime)
        return 0
    if not a.seed and not a.prepare_families:
        ap.error('--seed is required for scoring controls')
    global KEEP, QUIET_LOAD, YIELD_TO
    QUIET_LOAD = a.quiet_load
    YIELD_TO = a.yield_to
    if a.keep:
        a.keep.mkdir(parents=True, exist_ok=True)
        KEEP = a.keep
    if a.seed and (len(a.seed) != 16 or any(c not in '0123456789abcdefABCDEF' for c in a.seed)):
        ap.error('--seed must be 16 hex characters')
    if not GOLDEN.is_dir():
        print(f'REFUSED: {GOLDEN} does not exist (P10 deliverable)', file=sys.stderr)
        return 2
    import score_forge2
    kit, why = score_forge2._kit()  # noqa: SLF001 — one kit-discovery path for scorer and controls
    if kit is None:
        print(f'REFUSED: {why}', file=sys.stderr)
        return 3
    registered = candidates(include_legacy=a.legacy_regression)
    unknown = set(a.only) - {entry['name'] for entry in registered}
    if unknown:
        ap.error(f'unknown control names {sorted(unknown)}')
    if a.prepare_families:
        registered = [entry for entry in registered if entry['family']]
    app_modules = kit['app_modules']
    report: Dict = {'seed': a.seed, 'legacy_regression_included': a.legacy_regression, 'mutants': {}, 'controls': {}}
    failed = False
    with tempfile.TemporaryDirectory(prefix='forge2-controls-') as tmp:
        tmp = Path(tmp)
        # The golden is materialised and rebuilt exactly as every mutant is, so the comparison is like for like.
        prepare_control(tmp / 'golden', app_modules)
        if a.prepare_families:
            base = offline_lint(tmp / 'golden', kit['kit_dir'])
            report['golden'] = {'lint': base}
            failed = bool(base['errors'])
            for candidate in registered:
                mid = candidate['name']
                if a.only and mid not in a.only:
                    continue
                entry = {'family': candidate['family'], 'origin': candidate['origin']}
                try:
                    tree = tmp / mid
                    prepare_control(tree, app_modules, candidate.get('patch'), candidate.get('runtime_id'))
                    got = offline_lint(tree, kit['kit_dir'])
                    extra_warnings = [warning for warning in got['warnings'] if warning not in base['warnings']]
                    entry.update(lint=got, new_warnings=extra_warnings, fails=got['errors'] + extra_warnings)
                except Exception as error:
                    entry['fails'] = [f'{type(error).__name__}: {error}']
                failed |= bool(entry['fails'])
                report['mutants'][mid] = entry
                print(f"{mid}: {'PASS' if not entry['fails'] else 'FAIL ' + '; '.join(entry['fails'])}",
                      file=sys.stderr, flush=True)
            report['mode'] = 'apply-build-lint-only'
            if a.out:
                a.out.write_text(json.dumps(report, indent=2))
            print(json.dumps(report, indent=2))
            return 1 if failed else 0
        golden = score(tmp / 'golden', a.seed, tmp / 'golden.json')
        report['golden'] = {'score': golden['score'], 'status': golden['status']}
        gfails = score_forge2.reference_failures(golden)
        if gfails:
            report['golden']['reference_failures'] = gfails
            failed = True
        for candidate in registered:
            mid, expect = candidate['name'], candidate['expect']
            if a.only and mid not in a.only:
                continue
            entry: Dict = {'family': candidate['family'], 'origin': candidate['origin'],
                           'declared_cause': expect.get('why'), 'required_checks': expect.get('loses') or []}
            try:
                tree = tmp / mid
                prepare_control(tree, app_modules, candidate.get('patch'), candidate.get('runtime_id'))
                verdict = score(tree, a.seed, tmp / f'{mid}.json')
                lost = lost_rows(golden, verdict)
                entry.update(score=verdict['score'], lost=lost,
                             required_losses=sorted(set(lost) & set(expect.get('loses') or [])),
                             secondary_losses=loss_evidence(golden, verdict, expect),
                             criticals=verdict['critical'].get('unsuppressed'),
                             fails=judge(golden, verdict, expect, score_forge2.ROOT_BLOCKS,
                                         score_forge2.CALIBRATION_OWNED))
            except Exception as error:
                entry['fails'] = [f'{type(error).__name__}: {error}']
            failed |= bool(entry['fails'])
            report['mutants'][mid] = entry
            if a.out:
                a.out.write_text(json.dumps(report, indent=2))
            print(f"{mid}: {entry.get('score')} {'PASS' if not entry['fails'] else 'FAIL ' + '; '.join(entry['fails'])}",
                  file=sys.stderr, flush=True)
        if not a.only:
            uncovered = family_gaps(report['mutants'], score_forge2.TIER_OF, tuple(score_forge2.FAMILY_WEIGHT))
            report['family_coverage'] = {'uncovered': uncovered}
            failed |= bool(uncovered)
        for label, build, cap in (('starter_untouched', starter_untouched, STARTER_MAX),
                                  ('one_function', one_function_app, IDLE_MAX)):
            try:
                tree = tmp / label
                build(tree)
                verdict = score(tree, a.seed, tmp / f'{label}.json')
                fails = [] if verdict['status'] == 'scored' and verdict['score'] <= cap else \
                    [f"{verdict['status']} at {verdict['score']} (must be scored at <= {cap})"]
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
