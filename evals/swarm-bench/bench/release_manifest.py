"""Refreeze an isolated tier's release manifest over the desktop's benchmark payload.

    python3 release_manifest.py sb7.1/release-manifest.json [--source-commit SHA] [--check]
    python3 release_manifest.py forge/release-manifest.json [--source-commit SHA] [--check]

The payload is what ui/desktop/forge.config.ts mirrorSwarmBenchPayload() ships: the specs, the
SB7.1 starter, each tier's VISUAL-CONTRACT.md, SB7.2's own SB7-CONTRACT.md and STARTER.md, sb8/, and bench/ through the same name filter. Only
`files` (and `sourceCommit` when given) change; every hand-written field survives. `--check` verifies
the pins without writing. ui/desktop/scripts/copy-bench-release-manifest.cjs refuses a package whose
bytes differ from the pins, so a bench edit without a refreeze fails packaging, never ships silently.

The payload is per FAMILY (forge/DESIGN.md §12), read from the manifest's own `family` (absent = payments):
`payments` is exactly the SB7.x payload it always was — the forge files that now share bench/ are excluded,
so the SB7.x pins do not move when forge does. `forge` is forge/public, forge/starter, forge/kit without
module trees, forge/site, and the bench files the forge tier runs: its scorer files, the harness entry points
and their top-level bench imports, followed statically. Forge pins only git-tracked files: three packages
build it in one tree, and a manifest pins committed bytes, never another package's work in progress.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

ROOT = Path(__file__).resolve().parent.parent
SPECS = ('spec-build.md', 'spec-build-v2.md', 'spec-build-v3.md', 'spec-build-sb7.md',
         'spec-build-sb71.md', 'spec-build-sb72.md', 'spec-build-sb8.md')
TREES = ('sb7.1/starter', 'sb8')
CONTRACTS = ('sb7.1/VISUAL-CONTRACT.md', 'sb7.2/VISUAL-CONTRACT.md', 'sb7.2/SB7-CONTRACT.md', 'sb7.2/STARTER.md')


def _shipped(path: Path) -> bool:
    return not any(part == '__pycache__' or part == '.DS_Store' or part.endswith('.pyc') for part in path.parts)


def _bench_file(name: str) -> bool:
    return (name.endswith(('.py', '.mjs', '.json')) or name == 'sb8-three.module.js'
            or name in ('vendor_docs.md', 'vendor_docs_v3.md'))


FORGE_BENCH = re.compile(r'^(score_forge|forge_[a-z_]+|test_score_forge)\.(py|mjs)$|^forge-thresholds\.json$')
FORGE_TREES = ('forge/public', 'forge/starter', 'forge/kit', 'forge/site')
FORGE_HARNESS = ('run_build.py', 'bench_rescore.py', 'bench_isolation.py', 'browser-self-test.mjs', 'forge_controls.py')
TOP_IMPORT = re.compile(r'^(?:import|from)\s+([A-Za-z_][A-Za-z0-9_]*)', re.M)


def payload(root: Path = ROOT, family: str = 'payments') -> list[str]:
    if family == 'forge':
        return forge_payload(root)
    if family != 'payments':
        raise ValueError(f'unknown benchmark family {family!r}')
    files = [name for name in SPECS if (root / name).is_file()]
    for tree in TREES:
        files += [str(path.relative_to(root)) for path in sorted((root / tree).rglob('*'))
                  if path.is_file() and _shipped(path.relative_to(root))]
    files += [name for name in CONTRACTS if (root / name).is_file()]
    files += [f'bench/{path.name}' for path in sorted((root / 'bench').iterdir())
              if path.is_file() and _bench_file(path.name) and not FORGE_BENCH.match(path.name)]
    files += [f'bench/probes/{path.name}' for path in sorted((root / 'bench/probes').iterdir())
              if path.is_file() and path.name.endswith('.py')]
    return sorted(files)


def _tracked(root: Path) -> set[str]:
    out = subprocess.run(['git', 'ls-files', '-z'], cwd=root, capture_output=True, check=True).stdout
    return {name for name in out.decode().split('\0') if name}


def _bench_closure(root: Path, roots: tuple) -> set[str]:
    """The bench files `roots` need: each root plus the bench-local modules imported at top level, transitively
    (a function-local import is a lazily chosen regime, never this tier's)."""
    bench = root / 'bench'
    local = {p.stem for p in bench.glob('*.py')}
    probes = {p.stem for p in (bench / 'probes').glob('*.py')} if (bench / 'probes').is_dir() else set()
    seen, todo = set(), [r for r in roots if (bench / r).is_file()]
    while todo:
        name = todo.pop()
        if name in seen:
            continue
        seen.add(name)
        if not name.endswith('.py'):
            continue
        text = (bench / name).read_text()
        for module in TOP_IMPORT.findall(text):
            if module in local:
                todo.append(module + '.py')
            elif module == 'probes':
                seen.update(f'probes/{m}.py' for m in re.findall(r'^from probes import ([\w, ]+)', text, re.M)
                            for m in (x.strip() for x in m.split(',')) if m in probes)
    return {f'bench/{name}' for name in seen}


def forge_payload(root: Path = ROOT) -> list[str]:
    import isolated_tiers
    tier = isolated_tiers.FORGE10
    files = {tier.spec, *(source for _name, source in tier.public)}
    for tree in FORGE_TREES:
        if (root / tree).is_dir():
            files |= {str(p.relative_to(root)) for p in (root / tree).rglob('*') if p.is_file()
                      and _shipped(p.relative_to(root)) and 'node_modules' not in p.parts
                      and 'lint-modules' not in p.parts and 'app-modules' not in p.parts}
    files |= _bench_closure(root, (*tier.scorer_files, *FORGE_HARNESS))
    tracked = _tracked(root)
    return sorted(name for name in files if name in tracked)


def pins(root: Path = ROOT, family: str = 'payments') -> dict[str, str]:
    return {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in payload(root, family)}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('manifest')
    parser.add_argument('--source-commit')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args(argv)
    path = ROOT / args.manifest
    manifest = json.loads(path.read_text())
    current = pins(ROOT, manifest.get('family', 'payments'))
    if args.check:
        stale = sorted(name for name in set(current) | set(manifest['files'])
                       if current.get(name) != manifest['files'].get(name))
        print('\n'.join(stale) if stale else f'{args.manifest}: all {len(current)} payload files match')
        return 1 if stale else 0
    manifest['files'] = current
    if args.source_commit:
        manifest['sourceCommit'] = args.source_commit
    path.write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'{args.manifest}: {len(current)} payload files pinned')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
