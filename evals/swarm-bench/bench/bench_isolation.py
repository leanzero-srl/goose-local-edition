"""Filesystem isolation for macOS benchmark entrants, verified before model dispatch."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import zoneinfo


def node_runtime() -> Path | None:
    executable = os.environ.get('GOOSE_SWARM_RENDER_NODE') or shutil.which('node')
    if executable is None:
        return None
    result = subprocess.run([executable, '-p', 'process.execPath'], capture_output=True,
                            text=True, timeout=20)
    actual = Path(result.stdout.strip())
    if result.returncode or not actual.is_absolute() or not actual.is_file():
        raise RuntimeError('REFUSED: could not resolve the actual Node runtime')
    return actual.resolve()


def profile(workdir: Path, engine: Path, runtime: Path, node: Path | None = None,
            browser: dict | None = None, network: str = 'open', extra_read=()) -> str:
    def quote(value):
        return json.dumps(str(Path(value).resolve()))
    read_dirs = ['/System', '/usr', '/bin', '/sbin', '/Library/Apple',
                 '/Library/Developer', '/Library/Frameworks', '/opt/homebrew',
                 '/private/etc', '/private/var/db/timezone', '/dev', str(workdir), str(runtime)]
    read_dirs.extend(str(Path(path).resolve()) for path in zoneinfo.TZPATH if Path(path).is_dir())
    if browser:
        # Grant only the installed browser and its libraries, never the Resources parent
        # which also contains private scorer code and fixtures.
        read_dirs.extend([str(Path(browser['BENCH_BROWSER_EXECUTABLE']).parent),
                          str(Path(browser['BENCH_BROWSER_MODULE']).parent)])
    node = node or node_runtime()
    if node is not None:
        read_dirs.append(str(node.parent))
        node_lib = node.parent.parent / 'lib'
        if node_lib.is_dir():
            read_dirs.append(str(node_lib))
    python = Path(sys.executable).resolve()
    if not any(str(python).startswith(p + '/') for p in read_dirs):
        read_dirs.append(str(python.parent.parent))
    for extra in extra_read:
        read_dirs.extend(_grant_with_link_targets(Path(extra)))
    read_rules = ' '.join('(subpath ' + quote(p) + ')' for p in read_dirs)
    return '\n'.join([
        '(version 1)', '(deny default)',
        '(allow process-fork)', '(allow process-exec)', '(allow signal (target same-sandbox))',
        '(allow sysctl-read)', '(allow mach-lookup)', '(allow ipc-posix*)',
        # Chromium initializes AppKit preferences even in headless mode.
        '(allow user-preference-read)' if browser else '',
        '(allow iokit-open (iokit-user-client-class "RootDomainUserClient"))' if browser else '',
        '(allow mach-register (global-name-regex #"^org\\.chromium\\."))' if browser else '',
        *_network_rules(network), '(allow system-socket)',
        '(allow file-read-metadata)', '(allow file-read-data (literal "/"))',
        '(allow file-read* ' + read_rules + ' (literal ' + quote(engine) + '))',
        '(allow file-map-executable ' + read_rules + ' (literal ' + quote(engine) + '))',
        '(deny file-read-data (literal ' + quote(workdir / 'engine-console.log') + ') (subpath ' + quote(runtime / 'goose/state/logs') + '))',
        '(allow file-write* (subpath ' + quote(workdir) + ') (subpath ' + quote(runtime) + ') (subpath "/dev"))',
        # After the allow, so it wins. Measured 2026-10-02 (openrouter-cloud-73d233da): the entrant's
        # cleanup `rm -rf ... engine-console.log` unlinked the harness's console mid-run. The harness
        # writes it outside the sandbox; an inherited stdout fd stays writable (no path check).
        '(deny file-write* (literal ' + quote(workdir / 'engine-console.log') + '))',
        # The runtime telemetry sink: goose appends to it, nothing in the sandbox may remove it.
        '(deny file-write-unlink (literal ' + quote(runtime / 'telemetry.jsonl') + '))',
    ])


# The fenced network (forge-1.0, DESIGN.md §4): app traffic may reach only this machine. Binding and
# inbound stay open (dev servers, the bundled browser); outbound is loopback and unix sockets only, so
# the one way to the internet is the provider CONNECT relay, which runs OUTSIDE the sandbox and admits
# only the selected provider's host:443. Measured 2026-10-02: under this fence curl to
# https://developer.atlassian.com/ -> 000 (exit 7), node fetch -> EPERM, http://127.0.0.1:<port> -> 200.
FENCED_RULES = ('(allow network-bind)', '(allow network-inbound)',
                '(allow network-outbound (remote ip "localhost:*"))',
                '(allow network-outbound (remote unix-socket))')


def _network_rules(network: str) -> tuple[str, ...]:
    if network == 'open':
        return ('(allow network*)',)
    if network == 'fenced':
        return FENCED_RULES
    raise ValueError(f'unknown entrant network mode {network!r}')


def _grant_with_link_targets(path: Path) -> list[str]:
    """A read grant for a kit dir and for the targets of its top-level symlinks (the module trees)."""
    path = Path(path)
    grants = [str(path.resolve())]
    if path.is_dir():
        for child in path.iterdir():
            if child.is_symlink():
                target = str(child.resolve())
                if target not in grants:
                    grants.append(target)
    return grants


# ── the provider relay (forge-1.0 fenced network) ───────────────────────────────────────────────────

# Default endpoints of goose's cloud providers (crates/goose/src/providers/*: the *_HOST config-key
# defaults and the host each provider falls back to). test_bench_isolation pins this table against the
# Rust source so a provider moving its default host fails the build instead of silently fencing the run.
PROVIDER_DEFAULT_HOSTS = {
    'openrouter': ('OPENROUTER_HOST', 'https://openrouter.ai'),
    'anthropic': ('ANTHROPIC_HOST', 'https://api.anthropic.com'),
    'openai': ('OPENAI_HOST', 'https://api.openai.com'),
    'google': ('GOOGLE_HOST', 'https://generativelanguage.googleapis.com'),
    'xai': ('XAI_HOST', 'https://api.x.ai/v1'),
    'avian': ('AVIAN_HOST', 'https://api.avian.io/v1'),
    'nano-gpt': (None, 'https://nano-gpt.com/api/v1'),
    'tetrate': ('TETRATE_HOST', 'https://api.router.tetrate.ai'),
    'huggingface': ('HF_HOST', 'https://router.huggingface.co'),
}
# Providers whose endpoint is a required config key with no default.
PROVIDER_ENDPOINT_KEYS = {
    'azure_openai': 'AZURE_OPENAI_ENDPOINT', 'databricks': 'DATABRICKS_HOST', 'snowflake': 'SNOWFLAKE_HOST',
    'litellm': 'LITELLM_HOST',
}
LOCAL_PROVIDERS = frozenset({'lmstudio', 'ollama', 'local'})
_LOOPBACK = ('localhost', '127.0.0.1', '::1')


def _https_hostport(url: str, what: str) -> str:
    from urllib.parse import urlsplit
    parts = urlsplit(str(url))
    if parts.scheme != 'https' or not parts.hostname:
        raise RuntimeError(f'REFUSED: {what} is {url!r}; the fenced network relays HTTPS provider endpoints only')
    return f'{parts.hostname}:{parts.port or 443}'


def provider_hosts(snapshot: dict) -> list[str]:
    """The host:port list the relay admits: the snapshot's selected providers' endpoints, nothing else."""
    override = os.environ.get('BENCH_RELAY_ALLOW', '').strip()
    if override:
        return sorted({h.strip() for h in override.split(',') if h.strip()})
    if not snapshot:
        raise RuntimeError('REFUSED: a fenced entrant needs the benchmark configuration snapshot to know its provider')
    values = {**snapshot.get('config', {}), **snapshot.get('secrets', {})}
    hosts: set[str] = set()
    for name in snapshot.get('providers', []):
        if name in LOCAL_PROVIDERS:
            continue
        if name in snapshot.get('custom_providers', {}):
            definition = snapshot['custom_providers'][name]
            url = definition.get('base_url') or definition.get('api_url') or definition.get('host')
            if not url:
                raise RuntimeError(f'REFUSED: custom provider {name!r} names no base_url/api_url to relay to')
            hosts.add(_https_hostport(url, f'custom provider {name} endpoint'))
        elif name in PROVIDER_DEFAULT_HOSTS:
            key, default = PROVIDER_DEFAULT_HOSTS[name]
            hosts.add(_https_hostport(values.get(key) or default if key else default, f'{name} endpoint'))
        elif name in PROVIDER_ENDPOINT_KEYS:
            key = PROVIDER_ENDPOINT_KEYS[name]
            if not values.get(key):
                raise RuntimeError(f'REFUSED: {name} needs {key} in the snapshot to know where to relay')
            hosts.add(_https_hostport(values[key], f'{name} {key}'))
        elif name == 'aws_bedrock':
            region = values.get('AWS_REGION') or values.get('AWS_DEFAULT_REGION')
            if not region:
                raise RuntimeError('REFUSED: aws_bedrock needs AWS_REGION in the snapshot to know where to relay')
            hosts.add(f'bedrock-runtime.{region}.amazonaws.com:443')
        else:
            raise RuntimeError(f'REFUSED: no relay endpoint is known for provider {name!r}; set BENCH_RELAY_ALLOW=host:443 deliberately')
    if not hosts:
        raise RuntimeError('REFUSED: the snapshot selects no cloud provider, so a fenced entrant could reach no model')
    return sorted(hosts)


class ProviderRelay:
    """A 127.0.0.1 CONNECT relay outside the sandbox. It tunnels only allowlisted host:port pairs and
    records every attempt (runtime/relay.log); anything else gets 403 and never leaves the machine."""

    def __init__(self, allow: list[str], log_path: Path | None = None):
        import socket
        import socketserver
        import select
        self.allow = frozenset(allow)
        self.log_path = log_path
        self.attempts: list[dict] = []
        relay = self

        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                head = b''
                while b'\r\n\r\n' not in head and len(head) < 65536:
                    chunk = self.request.recv(4096)
                    if not chunk:
                        return
                    head += chunk
                line = head.split(b'\r\n', 1)[0].decode('latin-1')
                parts = line.split()
                target = parts[1] if len(parts) > 1 else ''
                allowed = len(parts) == 3 and parts[0] == 'CONNECT' and target in relay.allow
                relay._record(line, target, allowed)
                if not allowed:
                    self.request.sendall(b'HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')
                    return
                host, _, port = target.rpartition(':')
                try:
                    upstream = socket.create_connection((host, int(port)), timeout=30)
                except OSError:
                    self.request.sendall(b'HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')
                    return
                upstream.settimeout(None)
                self.request.sendall(b'HTTP/1.1 200 Connection Established\r\n\r\n')
                rest = head.split(b'\r\n\r\n', 1)[1]
                if rest:
                    upstream.sendall(rest)
                sockets = [self.request, upstream]
                try:
                    while True:
                        readable, _, broken = select.select(sockets, [], sockets)
                        if broken:
                            return
                        for sock in readable:
                            data = sock.recv(65536)
                            if not data:
                                return
                            (upstream if sock is self.request else self.request).sendall(data)
                finally:
                    upstream.close()

        class Server(socketserver.ThreadingTCPServer):
            daemon_threads = True
            allow_reuse_address = True

        self.server = Server(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        self.url = f'http://127.0.0.1:{self.port}'
        self._thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self._thread.start()

    def _record(self, line: str, target: str, allowed: bool) -> None:
        entry = {'request': line, 'target': target, 'allowed': allowed}
        self.attempts.append(entry)
        if self.log_path:
            with self.log_path.open('a') as handle:
                handle.write(json.dumps(entry) + '\n')

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()


_relays: list[ProviderRelay] = []


def stop_relays() -> None:
    while _relays:
        _relays.pop().stop()


def relay_environment(relay: ProviderRelay) -> dict[str, str]:
    # goose's reqwest clients are built with system-proxy and honour these; loopback stays direct.
    return {'HTTPS_PROXY': relay.url, 'https_proxy': relay.url, 'HTTP_PROXY': relay.url, 'http_proxy': relay.url,
            'NO_PROXY': ','.join(_LOOPBACK), 'no_proxy': ','.join(_LOOPBACK), 'BENCH_RELAY_URL': relay.url}


def fence_preflight(prefix: list[str], workdir: Path, relay: ProviderRelay, probe_url: str = 'https://developer.atlassian.com/') -> dict:
    """Two-sided proof inside the sandbox: the internet is unreachable directly AND through the relay,
    and one allowlisted provider endpoint answers through the relay. Any other outcome refuses the run."""
    def curl(*args):
        # Transport timeouts on a one-shot probe, not a bound on model work.
        return subprocess.run(prefix + ['/usr/bin/curl', '-sS', '-o', '/dev/null', '-w', '%{http_code}', '--connect-timeout', '15', '--max-time', '30', *args],
                              cwd=workdir, capture_output=True, text=True)
    direct = curl('--noproxy', '*', probe_url)
    if direct.returncode == 0:
        raise RuntimeError(f'REFUSED: the fence is open: {probe_url} answered {direct.stdout} from inside the sandbox')
    proxied = curl('-x', relay.url, probe_url)
    if proxied.returncode == 0:
        raise RuntimeError(f'REFUSED: the relay tunnelled a non-provider host: {probe_url} answered {proxied.stdout}')
    host = sorted(relay.allow)[0]
    name, _, port = host.rpartition(':')
    provider_url = f'https://{name}' + ('' if port == '443' else f':{port}') + '/'
    reached = curl('-x', relay.url, provider_url)
    if reached.returncode != 0 or not reached.stdout.strip().isdigit() or reached.stdout.strip() == '000':
        raise RuntimeError(f'REFUSED: the provider is unreachable through the relay ({provider_url}): {reached.stderr.strip()[-400:]}')
    return {'direct_internet': f'blocked (curl exit {direct.returncode})', 'relay_non_provider': f'refused (curl exit {proxied.returncode})',
            'relay_provider': f'{provider_url} answered HTTP {reached.stdout.strip()}'}


def browser_environment() -> dict[str, str]:
    names = {'BENCH_BROWSER_MODULE': 'GOOSE_SWARM_PLAYWRIGHT_MODULE',
             'BENCH_BROWSER_EXECUTABLE': 'GOOSE_SWARM_CHROMIUM_EXECUTABLE'}
    values = {public: str(Path(os.environ[source]).resolve())
              for public, source in names.items() if os.environ.get(source)}
    if values and len(values) != len(names):
        raise RuntimeError('REFUSED: incomplete bundled browser configuration')
    if values and (not Path(values['BENCH_BROWSER_MODULE']).is_dir() or
                   not Path(values['BENCH_BROWSER_EXECUTABLE']).is_file()):
        raise RuntimeError('REFUSED: bundled browser runtime is missing')
    return values


def prepare(workdir: Path, engine: Path, private_root: Path,
            snapshot: dict | None = None, network: str = 'open', extra_read=()) -> tuple[list[str], dict[str, str]]:
    if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').exists():
        raise RuntimeError('REFUSED: SB7.1 requires the tested macOS entrant isolation runtime')
    workdir = workdir.resolve()
    runtime = workdir.parent / ('.' + workdir.name + '-runtime')
    runtime.mkdir(mode=0o700)
    (runtime / 'tmp').mkdir()
    if snapshot is not None:
        config = runtime / 'goose' / 'config'
        config.mkdir(parents=True, mode=0o700)
        # JSON is a YAML subset; no extra Python dependency or original profile is needed.
        (config / 'config.yaml').write_text(json.dumps(snapshot['config']))
        definitions = config / 'custom_providers'
        definitions.mkdir()
        for name, definition in snapshot['custom_providers'].items():
            if not name or any(character not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-' for character in name):
                raise ValueError('Invalid custom provider ID in benchmark snapshot')
            (definitions / (name + '.json')).write_text(json.dumps(definition))
    node = node_runtime()
    browser = browser_environment()
    policy = profile(workdir, engine, runtime, node, browser, network=network, extra_read=extra_read)
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
    if browser:
        if node is None:
            raise RuntimeError('REFUSED: entrant browser requires bundled Node')
        script = runtime / 'browser-preflight.cjs'
        script.write_text('''const {chromium}=require(process.env.BENCH_BROWSER_MODULE);
(async()=>{const b=await chromium.launch({headless:true,executablePath:process.env.BENCH_BROWSER_EXECUTABLE,args:['--enable-unsafe-swiftshader']});
try {const page=await b.newPage();await page.setContent('<canvas></canvas>');
if(!await page.evaluate(()=>!!document.querySelector('canvas').getContext('webgl2')))throw new Error('WebGL2 unavailable');
await page.screenshot({path:process.argv[2]});} finally {await b.close();}})().catch(e=>{console.error(e.message);process.exitCode=1});''')
        result = subprocess.run(prefix + [str(node), str(script), str(runtime / 'browser-preflight.png')],
                                cwd=workdir, env={**os.environ, **browser, 'TMPDIR': str(runtime / 'tmp')},
                                capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise RuntimeError('REFUSED: entrant browser self-test failed: ' + result.stderr[-1000:])
    fence = {}
    relay = None
    if network == 'fenced':
        relay = ProviderRelay(provider_hosts(snapshot), runtime / 'relay.log')
        _relays.append(relay)
        fence = {'network': 'fenced', 'relay': {'url': relay.url, 'allow': sorted(relay.allow)},
                 'preflight': fence_preflight(prefix, workdir, relay)}
    (workdir / 'isolation.json').write_text(json.dumps({
        'mechanism': 'macOS sandbox-exec', 'private_reference_read': 'denied',
        'candidate_read_write': 'passed', 'runtime': str(runtime),
        'candidate_browser': 'WebGL2 and screenshot passed' if browser else 'not configured',
        **fence,
    }, indent=2))
    paths = [str(Path(sys.executable).resolve().parent)]
    if node is not None:
        paths.append(str(node.parent))
    paths.append(os.environ.get('PATH', '/usr/bin:/bin'))
    return prefix, {'PATH': os.pathsep.join(paths), 'GOOSE_PATH_ROOT': str(runtime / 'goose'),
                    'TMPDIR': str(runtime / 'tmp'), 'GOOSE_ADDITIONAL_CONFIG_FILES': '',
                    'BENCH_SB71_RUNTIME': str(runtime), **browser, **(relay_environment(relay) if relay else {})}


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
