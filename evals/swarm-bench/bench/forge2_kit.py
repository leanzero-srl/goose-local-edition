"""The forge-2.0 kit: materialise, pin and verify — forge_kit.py's procedure over forge2/kit (forge2/SPEC.md §3).

The repo commits only the kit's recipe (forge2/kit: package.json + package-lock.json for the
app-modules tree — forge-2.0 adds @forge/react for the UI Kit admin page — lint/package.json +
lint/package-lock.json for the lint-modules tree, bin/, lib/, openapi/, lint-pack/, runtime-pin.json).
ensure() turns it into a cache dir that is never committed:

    <cache>/<lock_sha256[:16]>/app-modules/node_modules     npm ci --ignore-scripts (integrity hashes)
    <cache>/<lock_sha256[:16]>/lint-modules/node_modules
    <cache>/<lock_sha256[:16]>/wrapper/{wrapper.js,loader.js}  Atlassian's runtime wrapper, by sha256
    <cache>/<lock_sha256[:16]>/schema/manifest-schema.json    copied out of @forge/manifest
    <cache>/<lock_sha256[:16]>/kit-<code_sha256[:16]>/        $FORGE_KIT: the code parts below
                                                              + links to the four dirs above

The cache is shared with forge_kit.py: both key it by content (lock / code hashes), so the two kits never
collide and identical module trees are fetched once. lint-pack/ (the server-side lint rules) ships when the
recipe has it; KIT.json lists the code parts each kit was built from.

Network is needed once per kit version, outside any sandbox. Every mismatch refuses (RuntimeError
starting "REFUSED:"); nothing falls back. Cache root: $FORGE_KIT_CACHE or
~/Library/Application Support/Goose/benchmark/forge-kit.

CLI: forge2_kit.py ensure | status | repin | lock-sha | install-modules <workdir>

status() answers, without the network and without writing, whether ensure() would return at once: the
desktop's Benchmark view shows it as the Forge kit's readiness before a run may launch.
"""
from __future__ import annotations

import hashlib
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

KIT_SRC = Path(__file__).resolve().parent.parent / 'forge2' / 'kit'
CODE_PARTS = ('bin', 'lib', 'openapi', 'lint-pack', 'runtime-pin.json')
WRAPPER_CDN = 'https://forge-node-runtime.prod-east.frontend.public.atl-paas.net/'


def cache_root() -> Path:
    override = os.environ.get('FORGE_KIT_CACHE')
    if override:
        return Path(override)
    return Path.home() / 'Library' / 'Application Support' / 'Goose' / 'benchmark' / 'forge-kit'


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def lock_sha256(src: Path = KIT_SRC) -> str:
    """Identity of the module trees and the runtime: both lockfiles and the wrapper pin."""
    parts = [b'app\0', (src / 'package-lock.json').read_bytes(), b'\0lint\0',
             (src / 'lint' / 'package-lock.json').read_bytes(), b'\0pin\0', (src / 'runtime-pin.json').read_bytes()]
    return _sha(b''.join(parts))


def code_parts(src: Path = KIT_SRC) -> list[str]:
    return [part for part in CODE_PARTS if (src / part).exists()]


def code_sha256(src: Path = KIT_SRC) -> str:
    h = hashlib.sha256()
    for part in code_parts(src):
        root = src / part
        files = [root] if root.is_file() else sorted(p for p in root.rglob('*') if p.is_file())
        for path in files:
            h.update(str(path.relative_to(src)).encode() + b'\0' + path.read_bytes() + b'\0')
    return h.hexdigest()


def _node_tool(name: str) -> str:
    node = os.environ.get('GOOSE_SWARM_RENDER_NODE')
    if node:
        sibling = Path(node).resolve().parent / name
        if sibling.is_file():
            return str(sibling)
    found = shutil.which(name)
    if not found:
        raise RuntimeError(f'REFUSED: {name} is not on PATH and GOOSE_SWARM_RENDER_NODE has no sibling {name}')
    return found


def _npm_ci(tree_src: Path, dest: Path) -> None:
    if (dest / '.complete').is_file():
        return
    staging = Path(tempfile.mkdtemp(prefix=dest.name + '-', dir=dest.parent))
    shutil.copy2(tree_src / 'package.json', staging / 'package.json')
    shutil.copy2(tree_src / 'package-lock.json', staging / 'package-lock.json')
    result = subprocess.run([_node_tool('npm'), 'ci', '--ignore-scripts', '--no-audit', '--no-fund'],
                            cwd=staging, capture_output=True, text=True)
    if result.returncode:
        shutil.rmtree(staging, ignore_errors=True)
        raise RuntimeError(f'REFUSED: npm ci for {tree_src} failed: {result.stderr[-1500:]}')
    (staging / '.complete').write_text('npm ci --ignore-scripts from the committed lockfile\n')
    if dest.exists():
        shutil.rmtree(dest)
    staging.rename(dest)


def _fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def _ensure_wrapper(pin: dict, dest: Path) -> str:
    dest.mkdir(exist_ok=True)
    for name, url_key, sha_key, bytes_key in (('wrapper.js', 'url', 'sha256', 'bytes'),
                                              ('loader.js', 'loader_url', 'loader_sha256', 'loader_bytes')):
        target = dest / name
        if target.is_file() and _sha(target.read_bytes()) == pin[sha_key]:
            continue
        data = _fetch(pin[url_key])
        if _sha(data) != pin[sha_key] or len(data) != pin[bytes_key]:
            raise RuntimeError(f'REFUSED: {pin[url_key]} answered {len(data)} bytes with sha256 {_sha(data)}; '
                               f'runtime-pin.json pins {pin[bytes_key]} bytes / {pin[sha_key]}. Re-pin deliberately with '
                               '`forge_kit.py repin` and rescore the golden; never run an unpinned runtime.')
        target.write_bytes(data)
    return pin['sha256']


def ensure(root: Path | None = None) -> dict:
    """Serialised across processes: two concurrent ensures (parallel test files, a scorer beside a dev run)
    once raced — the second replaced kit-<code> under the first one's running invocation."""
    root = root or cache_root()
    root.mkdir(parents=True, exist_ok=True)
    with open(root / '.ensure.lock', 'a') as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            return _ensure_locked(root)
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def _ensure_locked(root: Path) -> dict:
    lock = lock_sha256()
    code = code_sha256()
    modules = root / lock[:16]
    modules.mkdir(parents=True, exist_ok=True)
    _npm_ci(KIT_SRC, modules / 'app-modules')
    _npm_ci(KIT_SRC / 'lint', modules / 'lint-modules')
    pin = json.loads((KIT_SRC / 'runtime-pin.json').read_text())
    wrapper_sha = _ensure_wrapper(pin, modules / 'wrapper')
    schema_src = modules / 'lint-modules' / 'node_modules' / '@forge' / 'manifest' / 'out' / 'schema' / 'manifest-schema.json'
    if not schema_src.is_file():
        raise RuntimeError(f'REFUSED: {schema_src} is missing from the lint-modules tree')
    (modules / 'schema').mkdir(exist_ok=True)
    schema = modules / 'schema' / 'manifest-schema.json'
    if not schema.is_file() or schema.read_bytes() != schema_src.read_bytes():
        shutil.copy2(schema_src, schema)
    kit = modules / ('kit-' + code[:16])
    if not (kit / 'KIT.json').is_file():
        staging = Path(tempfile.mkdtemp(prefix='kit-', dir=modules))
        parts = code_parts()
        for part in parts:
            src = KIT_SRC / part
            if src.is_dir():
                shutil.copytree(src, staging / part)
            else:
                shutil.copy2(src, staging / part)
        for link in ('app-modules', 'lint-modules', 'wrapper', 'schema'):
            (staging / link).symlink_to(Path('..') / link)
        versions = {name: json.loads((modules / 'app-modules' / 'node_modules' / name / 'package.json').read_text())['version']
                    for name in json.loads((KIT_SRC / 'package.json').read_text())['dependencies']}
        (staging / 'KIT.json').write_text(json.dumps({
            'kit_lock_sha256': lock, 'kit_code_sha256': code, 'wrapper_sha256': wrapper_sha, 'app_modules': versions,
            'code_parts': parts,
        }, indent=2) + '\n')
        if kit.exists():
            shutil.rmtree(kit)
        staging.rename(kit)
    return {'kit_dir': str(kit), 'modules_dir': str(modules), 'kit_lock_sha256': lock, 'kit_code_sha256': code,
            'wrapper_sha256': wrapper_sha}


def status(root: Path | None = None) -> dict:
    """What ensure() would still have to fetch or build, read only: [] means ready (no network needed)."""
    root = root or cache_root()
    lock = lock_sha256()
    modules = root / lock[:16]
    missing = []
    for tree in ('app-modules', 'lint-modules'):
        if not (modules / tree / '.complete').is_file():
            missing.append(tree)
    pin = json.loads((KIT_SRC / 'runtime-pin.json').read_text())
    for name, sha_key in (('wrapper.js', 'sha256'), ('loader.js', 'loader_sha256')):
        target = modules / 'wrapper' / name
        if not target.is_file() or _sha(target.read_bytes()) != pin[sha_key]:
            missing.append(f'wrapper/{name}')
    if not (modules / 'schema' / 'manifest-schema.json').is_file():
        missing.append('schema')
    if not (modules / ('kit-' + code_sha256()[:16]) / 'KIT.json').is_file():
        missing.append('kit')
    return {'ready': not missing, 'missing': missing, 'cache': str(root), 'kit_lock_sha256': lock}


def install_node_modules(workdir: Path, kit: dict) -> dict:
    """The workdir's node_modules as an APFS clone of the pristine app-modules tree (zero bytes copied)."""
    source = Path(kit['modules_dir']) / 'app-modules' / 'node_modules'
    target = Path(workdir) / 'node_modules'
    if target.exists():
        raise RuntimeError(f'REFUSED: {target} already exists; the entrant workspace gets exactly one pristine tree')
    result = subprocess.run(['/bin/cp', '-cR', str(source), str(target)], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f'REFUSED: APFS clone of the kit modules failed ({result.stderr.strip()}); '
                           'the workspace must sit on the same APFS volume as the kit cache')
    return {'node_modules': str(target), 'mode': 'apfs-clone', 'source': str(source)}


def repin() -> dict:
    """Re-pin the runtime wrapper to what the CDN index serves today (@forge/bundler's discovery)."""
    index = _fetch(WRAPPER_CDN).decode()
    found = {}
    for kind in ('wrapper', 'loader'):
        match = re.search(r'<script[^>]*\ssrc=("([^"]*%s[^"]*)"|([^\s>]*%s[^\s>]*))' % (kind, kind), index)
        if not match:
            raise RuntimeError(f'REFUSED: the CDN index carries no <script src> containing {kind!r}')
        found[kind] = urllib.request.urljoin(WRAPPER_CDN, match.group(2) or match.group(3))
    wrapper, loader = _fetch(found['wrapper']), _fetch(found['loader'])
    import datetime
    pin = {'url': found['wrapper'], 'sha256': _sha(wrapper), 'bytes': len(wrapper),
           'loader_url': found['loader'], 'loader_sha256': _sha(loader), 'loader_bytes': len(loader),
           'fetched': datetime.date.today().isoformat(),
           'discovered_from': WRAPPER_CDN + " (the index @forge/bundler 7.2.3 NetworkWrapperProvider parses: the first <script src> containing 'wrapper' / 'loader')",
           'repin': 'python3 evals/swarm-bench/bench/forge2_kit.py repin'}
    (KIT_SRC / 'runtime-pin.json').write_text(json.dumps(pin, indent=2) + '\n')
    return pin


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in ('ensure', 'status', 'repin', 'lock-sha', 'install-modules'):
        print(__doc__)
        return 2
    if argv[0] == 'ensure':
        print(json.dumps(ensure(), indent=2))
    elif argv[0] == 'status':
        print(json.dumps(status(), indent=2))
    elif argv[0] == 'repin':
        print(json.dumps(repin(), indent=2))
    elif argv[0] == 'lock-sha':
        print(lock_sha256())
    else:
        print(json.dumps(install_node_modules(Path(argv[1]), ensure()), indent=2))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
