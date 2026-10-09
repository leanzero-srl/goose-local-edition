"""forge-1.0 vendor interface for run_build (DESIGN.md §12): the DEV site the entrant's tools talk to.

    server = forge_site.serve(port, trace, seed=fixture_seed)
    server.url           FORGE_SITE_URL for the entrant: http://admin:<token>@127.0.0.1:<port>
    server.dev_seed      the dev site's own seed, drawn here, never equal to fixture_seed
    server.shutdown(); server.server_close()      stops the node process by pid (gate 4)
    forge_site.mark_phase(name)                   appends a phase marker to the trace

The trace's first line is the header the scorer reads: {trace_header, tier, fixture_seed, dev_seed,
kit_lock_sha256}. `fixture_seed` is only recorded here — the dev site serves `dev_seed`, so nothing the
entrant can observe derives from the scoring seed. The scoring site is started by score_forge itself.
"""
from __future__ import annotations

import atexit
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import subprocess
import sys
import threading
import time

HERE = Path(__file__).resolve().parent
SITE_JS = HERE.parent / 'forge' / 'site' / 'site.cjs'
DOCS_PATH = None
API_KEY = None
TIER = 'forge-1.0'
SEED_RE = re.compile(r'^[0-9a-f]{16}$')
_current_trace: Path | None = None
_live: list['DevSite'] = []


def _node() -> str:
    node = os.environ.get('GOOSE_SWARM_RENDER_NODE') or shutil.which('node')
    if not node:
        raise RuntimeError('REFUSED: no node to run the forge dev site (set GOOSE_SWARM_RENDER_NODE)')
    return node


def draw_dev_seed(fixture_seed: str | None) -> str:
    while True:
        seed = secrets.token_hex(8)
        if seed != fixture_seed:
            return seed


class DevSite:
    def __init__(self, proc: subprocess.Popen, url: str, admin_url: str, dev_seed: str, fixture_seed: str | None, trace: Path):
        self.proc, self.dev_seed, self.fixture_seed, self.trace = proc, dev_seed, fixture_seed, trace
        self.site_url = url
        self.admin_url = admin_url
        token = admin_url.rsplit('/', 1)[1]
        self.url = url.replace('http://', f'http://admin:{token}@', 1)
        self.pid = proc.pid

    def shutdown(self) -> None:
        if self.proc.poll() is None:
            os.kill(self.proc.pid, signal.SIGTERM)
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.kill(self.proc.pid, signal.SIGKILL)
                self.proc.wait()
        if self in _live:
            _live.remove(self)

    def server_close(self) -> None:
        for stream in (self.proc.stdout, self.proc.stderr):
            if stream and not stream.closed:
                stream.close()


def _reap() -> None:
    for site in list(_live):
        site.shutdown()


atexit.register(_reap)


def serve(port: int, trace: Path, seed: str | None = None) -> DevSite:
    global _current_trace
    if seed is not None and not SEED_RE.match(seed):
        raise ValueError(f'fixture seed must be 16 lowercase hex chars, got {seed!r}')
    import forge_kit
    dev_seed = draw_dev_seed(seed)
    trace = Path(trace)
    trace.parent.mkdir(parents=True, exist_ok=True)
    trace.write_text(json.dumps({'trace_header': TIER, 'tier': TIER, 'fixture_seed': seed, 'dev_seed': dev_seed,
                                 'kit_lock_sha256': forge_kit.lock_sha256()}) + '\n')
    _current_trace = trace
    token = secrets.token_hex(12)
    proc = subprocess.Popen([_node(), str(SITE_JS), '--seed', dev_seed, '--port', str(port), '--token', token, '--trace', str(trace)],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    line: list[str] = []
    reader = threading.Thread(target=lambda: line.append(proc.stdout.readline()), daemon=True)
    reader.start()
    # Transport wait for a local process to bind, not a bound on model work.
    reader.join(30)
    if not line or not line[0].strip():
        err = proc.stderr.read() if proc.poll() is not None else ''
        if proc.poll() is None:
            os.kill(proc.pid, signal.SIGKILL)
        raise RuntimeError(f'REFUSED: the forge dev site did not start on port {port}: {err[-800:]}')
    info = json.loads(line[0])
    site = DevSite(proc, info['url'], info['adminUrl'], dev_seed, seed, trace)
    _live.append(site)
    return site


def mark_phase(name: str, reset: bool = None) -> None:
    if _current_trace is not None:
        with _current_trace.open('a') as handle:
            handle.write(json.dumps({'phase': name, 'reset': reset, 'at': time.time()}) + '\n')


def trace_header(trace: Path) -> dict:
    with Path(trace).open() as handle:
        return json.loads(handle.readline())


if __name__ == '__main__':
    target = Path(sys.argv[2] if len(sys.argv) > 2 else 'forge-dev-trace.jsonl')
    site = serve(int(sys.argv[1]) if len(sys.argv) > 1 else 0, target, seed=None)
    print(json.dumps({'FORGE_SITE_URL': site.url, 'dev_seed': site.dev_seed, 'pid': site.pid}))
    try:
        site.proc.wait()
    except KeyboardInterrupt:
        site.shutdown()
