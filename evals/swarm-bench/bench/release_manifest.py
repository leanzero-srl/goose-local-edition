"""Refreeze an isolated tier's release manifest over the desktop's benchmark payload.

    python3 release_manifest.py sb7.1/release-manifest.json [--source-commit SHA] [--check]

The payload is what ui/desktop/forge.config.ts mirrorSwarmBenchPayload() ships: the specs, the
SB7.1 starter, each tier's VISUAL-CONTRACT.md, sb8/, and bench/ through the same name filter. Only
`files` (and `sourceCommit` when given) change; every hand-written field survives. `--check` verifies
the pins without writing. ui/desktop/scripts/copy-bench-release-manifest.cjs refuses a package whose
bytes differ from the pins, so a bench edit without a refreeze fails packaging, never ships silently.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPECS = ('spec-build.md', 'spec-build-v2.md', 'spec-build-v3.md', 'spec-build-sb7.md',
         'spec-build-sb71.md', 'spec-build-sb72.md', 'spec-build-sb8.md')
TREES = ('sb7.1/starter', 'sb8')
CONTRACTS = ('sb7.1/VISUAL-CONTRACT.md', 'sb7.2/VISUAL-CONTRACT.md')


def _shipped(path: Path) -> bool:
    return not any(part == '__pycache__' or part == '.DS_Store' or part.endswith('.pyc') for part in path.parts)


def _bench_file(name: str) -> bool:
    return (name.endswith(('.py', '.mjs', '.json')) or name == 'sb8-three.module.js'
            or name in ('vendor_docs.md', 'vendor_docs_v3.md'))


def payload(root: Path = ROOT) -> list[str]:
    files = [name for name in SPECS if (root / name).is_file()]
    for tree in TREES:
        files += [str(path.relative_to(root)) for path in sorted((root / tree).rglob('*'))
                  if path.is_file() and _shipped(path.relative_to(root))]
    files += [name for name in CONTRACTS if (root / name).is_file()]
    files += [f'bench/{path.name}' for path in sorted((root / 'bench').iterdir())
              if path.is_file() and _bench_file(path.name)]
    files += [f'bench/probes/{path.name}' for path in sorted((root / 'bench/probes').iterdir())
              if path.is_file() and path.name.endswith('.py')]
    return sorted(files)


def pins(root: Path = ROOT) -> dict[str, str]:
    return {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in payload(root)}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('manifest')
    parser.add_argument('--source-commit')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args(argv)
    path = ROOT / args.manifest
    manifest = json.loads(path.read_text())
    current = pins()
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
