"""Filesystem isolation for macOS benchmark entrants, verified before model dispatch."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zoneinfo


def node_runtime() -> Path | None:
    executable = shutil.which('node')
    if executable is None:
        return None
    result = subprocess.run([executable, '-p', 'process.execPath'], capture_output=True,
                            text=True, timeout=20)
    actual = Path(result.stdout.strip())
    if result.returncode or not actual.is_absolute() or not actual.is_file():
        raise RuntimeError('REFUSED: could not resolve the actual Node runtime')
    return actual.resolve()


def profile(workdir: Path, engine: Path, runtime: Path, node: Path | None = None) -> str:
    def quote(value):
        return json.dumps(str(Path(value).resolve()))
    read_dirs = ['/System', '/usr', '/bin', '/sbin', '/Library/Apple',
                 '/Library/Developer', '/Library/Frameworks', '/opt/homebrew',
                 '/private/etc', '/dev', str(workdir), str(runtime)]
    read_dirs.extend(str(Path(path).resolve()) for path in zoneinfo.TZPATH if Path(path).is_dir())
    node = node or node_runtime()
    if node is not None:
        read_dirs.append(str(node.parent))
        node_lib = node.parent.parent / 'lib'
        if node_lib.is_dir():
            read_dirs.append(str(node_lib))
    python = Path(sys.executable).resolve()
    if not any(str(python).startswith(p + '/') for p in read_dirs):
        read_dirs.append(str(python.parent.parent))
    read_rules = ' '.join('(subpath ' + quote(p) + ')' for p in read_dirs)
    return '\n'.join([
        '(version 1)', '(deny default)',
        '(allow process-fork)', '(allow process-exec)', '(allow signal (target same-sandbox))',
        '(allow sysctl-read)', '(allow mach-lookup)', '(allow ipc-posix*)',
        '(allow network*)', '(allow system-socket)',
        '(allow file-read-metadata)', '(allow file-read-data (literal "/"))',
        '(allow file-read* ' + read_rules + ' (literal ' + quote(engine) + '))',
        '(allow file-map-executable ' + read_rules + ' (literal ' + quote(engine) + '))',
        '(deny file-read-data (literal ' + quote(workdir / 'engine-console.log') + ') (subpath ' + quote(runtime / 'goose/state/logs') + '))',
        '(allow file-write* (subpath ' + quote(workdir) + ') (subpath ' + quote(runtime) + ') (subpath "/dev"))',
    ])


def prepare(workdir: Path, engine: Path, private_root: Path) -> tuple[list[str], dict[str, str]]:
    if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').exists():
        raise RuntimeError('REFUSED: SB7.1 requires the tested macOS entrant isolation runtime')
    workdir = workdir.resolve()
    runtime = workdir.parent / ('.' + workdir.name + '-runtime')
    runtime.mkdir(mode=0o700)
    (runtime / 'tmp').mkdir()
    node = node_runtime()
    policy = profile(workdir, engine, runtime, node)
    prefix = ['/usr/bin/sandbox-exec', '-p', policy]
    # These are actual forbidden and allowed reads, not an assertion about policy text.
    control = private_root / ('isolation-control-' + os.urandom(8).hex())
    control.write_text('private benchmark reference control')
    allowed = workdir / '.isolation-positive-control'
    allowed.write_text('candidate workspace')
    probe = '''import pathlib,sys,zoneinfo
zoneinfo.ZoneInfo('Europe/Berlin')
assert pathlib.Path(sys.argv[1]).read_text() == 'candidate workspace'
for name in sys.argv[2:]:
 try:
  pathlib.Path(name).read_bytes()
 except PermissionError:
  pass
 else:
  raise SystemExit('private benchmark file readable')
pathlib.Path(sys.argv[1]).write_text('candidate write works')
'''
    try:
        result = subprocess.run(prefix + [sys.executable, '-c', probe, str(allowed), str(control), str(Path(__file__).resolve())],
                                cwd=workdir, capture_output=True, text=True, timeout=20)
        if result.returncode:
            raise RuntimeError('REFUSED: entrant isolation control failed: ' + result.stderr[-1000:])
    finally:
        control.unlink(missing_ok=True)
        allowed.unlink(missing_ok=True)
    (runtime / 'profile.sb').write_text(policy)
    (workdir / 'isolation.json').write_text(json.dumps({
        'mechanism': 'macOS sandbox-exec', 'private_reference_read': 'denied',
        'candidate_read_write': 'passed', 'runtime': str(runtime),
    }, indent=2))
    return prefix, {'PATH': (str(node.parent) + os.pathsep + os.environ.get('PATH', ''))
                    if node is not None else os.environ.get('PATH', ''), 'GOOSE_PATH_ROOT': str(runtime / 'goose'), 'TMPDIR': str(runtime / 'tmp'),
                    'GOOSE_ADDITIONAL_CONFIG_FILES': '', 'BENCH_SB71_RUNTIME': str(runtime)}


def usage(runtime: Path) -> dict:
    """Preserve measured session counters; absent billing metadata is not a zero bill."""
    import sqlite3
    databases = list((runtime / 'goose').rglob('sessions.db'))
    if len(databases) != 1:
        return {'status': 'unavailable', 'reason': 'Expected one isolated session database',
                'database_count': len(databases)}
    try:
        connection = sqlite3.connect('file:' + str(databases[0]) + '?mode=ro', uri=True)
        connection.row_factory = sqlite3.Row
        try:
            rows = connection.execute("SELECT id, accumulated_input_tokens, accumulated_output_tokens, "
                                      "accumulated_cache_read_tokens, accumulated_cache_write_tokens, "
                                      "accumulated_cost FROM sessions").fetchall()
            return {'status': 'recorded', 'source': 'isolated Goose session counters',
                    'billing_verified': False, 'sessions': [dict(row) for row in rows]}
        finally:
            connection.close()
    except sqlite3.Error as error:
        return {'status': 'unavailable', 'reason': str(error)}
