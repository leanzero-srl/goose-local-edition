#!/usr/bin/env python3
"""Opt-in real desktop CLI reauthentication proof; never uses a live control server.

The default arm includes a reset control; --skip-reset-control reproduces the
production migration without that preceding call. Nonzero exit is a failed proof.
Run with --headscale <pinned v0.29.3 binary> --bundle-bin <Goose Resources/bin>
--personal-cli <personal Tailscale CLI> --guard-socket <existing Goose socket>.
All mutations are beneath a fresh private /tmp directory, removed after cleanup.
Config adapted from ui/ios/tests/mesh-fixture/headscale_test.go. Sources:
https://raw.githubusercontent.com/juanfont/headscale/v0.29.3/config-example.yaml
https://tailscale.com/docs/reference/tailscale-cli/up
https://raw.githubusercontent.com/tailscale/tailscale/v1.98.8/cmd/tailscale/cli/up.go
https://raw.githubusercontent.com/juanfont/headscale/v0.29.3/cmd/headscale/cli/utils.go
"""
import argparse
import hashlib
import http.server
import json
import os
import re
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request

PIN = 'fb55b6d3d1ef3b850fe02837299ac1853f383a06f561d7f180d36b9ce406904b'
IDENTITY_FIELDS = ('ID', 'NodeID', 'PublicKey', 'HostName', 'DNSName', 'OS', 'UserID',
                   'TailscaleIPs', 'AllowedIPs', 'Created', 'Tags', 'CapMap', 'Capabilities')


def fixture_env():
    # Desktop children have no SSH session; control-server env must not override isolation.
    # Tailscale v1.98.8 cli/up.go checks SSH_CONNECTION before force-reauth.
    return {k: v for k, v in os.environ.items()
            if not k.startswith(('HEADSCALE_', 'TS_', 'TAILSCALE_', 'SSH_'))}


def run(argv, timeout=20):
    result = subprocess.run([str(a) for a in argv], capture_output=True, timeout=timeout,
                            env=fixture_env(), stdin=subprocess.DEVNULL)
    if result.returncode:
        # Only known CLI failure categories are safe to publish; no raw output.
        errors = result.stderr.decode(errors='replace')
        categories = [s for s in ('context deadline exceeded', 'timed out', 'invalid key',
                      'expired', 'already used', 'permission denied', 'connection refused')
                      if s in errors.lower()]
        safe_error = re.sub(r'[A-Za-z0-9_:/+=.-]{32,}', '[redacted]', errors[-1200:])
        raise RuntimeError('command failed: %s exit=%d categories=%s stderr=%s' %
                           (Path(argv[0]).name, result.returncode, categories, safe_error))
    return result.stdout


def status(argv):
    return json.loads(run([*argv, 'status', '--json']))


def identity(data):
    def node(n):
        return {k: n[k] for k in IDENTITY_FIELDS if k in n}
    return {'self': node(data['Self']),
            'peers': {k: node(v) for k, v in data.get('Peer', {}).items()},
            **{k: data.get(k) for k in ('CurrentTailnet', 'User', 'MagicDNSSuffix',
                                       'BackendState', 'TailscaleIPs', 'HaveNodeKey')}}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def free_address():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return '127.0.0.1:%d' % listener.getsockname()[1]


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--headscale', required=True, type=Path)
    parser.add_argument('--skip-reset-control', action='store_true')
    parser.add_argument('--probe-resume-after-error', action='store_true')
    parser.add_argument('--bundle-bin', required=True, type=Path)
    parser.add_argument('--personal-cli', required=True, type=Path)
    parser.add_argument('--guard-socket', required=True, type=Path)
    args = parser.parse_args()
    hs = args.headscale.resolve()
    assert hashlib.sha256(hs.read_bytes()).hexdigest() == PIN, 'wrong Headscale release'
    cli = (args.bundle_bin / 'tailscale').resolve()
    daemon = (args.bundle_bin / 'tailscaled').resolve()
    assert daemon != Path('/usr/local/bin/tailscaled'), 'system daemon forbidden'
    assert cli.is_file() and daemon.is_file()
    guards = [[args.personal_cli], [cli, '--socket=' + str(args.guard_socket)]]
    before_guards = [identity(status(g)) for g in guards]
    for snapshot in before_guards:
        assert snapshot['self'].get('ID') and snapshot['self'].get('PublicKey'), 'guard identity missing'
    version = run([cli, 'version']).decode().splitlines()[0]
    receipt = {'tailscale_version': version, 'headscale': '0.29.3', 'phases': [],
               'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
               'daemon_sha256': hashlib.sha256(daemon.read_bytes()).hexdigest(),
               'reset_control': not args.skip_reset_control}
    processes = []
    def interrupted(signum, _frame):
        raise RuntimeError('interrupted by signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    failure = None
    try:
        with tempfile.TemporaryDirectory(prefix='lz-hs-cli-', dir='/tmp') as temporary:
            root = Path(temporary)
            os.chmod(root, 0o700)
            (root / 'node').mkdir(mode=0o700)
            def write(name, value):
                p = root / name
                p.write_text(value)
                p.chmod(0o600)
                return p
            acl = write('acl.json', '{"acls":[{"action":"accept","src":["*"],"dst":["autogroup:self:*"]}]}')
            # Enrollment needs a nonempty map, but this proof sends no peer data.
            # A loopback-only unavailable relay keeps the fixture off public DERP.
            relay_port = int(free_address().split(':')[1])
            derp_data = json.dumps({'Regions': {'999': {
                'RegionID': 999, 'RegionCode': 'fixture', 'RegionName': 'fixture',
                'Nodes': [{'Name': 'fixture', 'RegionID': 999, 'HostName': '127.0.0.1',
                           'IPv4': '127.0.0.1', 'DERPPort': relay_port,
                           'STUNPort': -1, 'InsecureForTests': True}]}}}).encode()
            class MapHandler(http.server.BaseHTTPRequestHandler):
                def do_GET(self):
                    self.send_response(200)
                    self.send_header('Content-Type', 'application/json')
                    self.end_headers()
                    self.wfile.write(derp_data)
                def log_message(self, *_):
                    pass
            map_server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), MapHandler)
            threading.Thread(target=map_server.serve_forever, daemon=True).start()
            map_url = 'http://127.0.0.1:%d' % map_server.server_port
            listen, metrics, grpc = free_address(), free_address(), free_address()
            url = 'http://' + listen
            config = write('config.yaml', f'''server_url: {url}
listen_addr: {listen}
metrics_listen_addr: {metrics}
grpc_listen_addr: {grpc}
grpc_allow_insecure: false
noise:
  private_key_path: {root}/noise.key
prefixes:
  v4: 100.64.0.0/10
  v6: fd7a:115c:a1e0::/48
  allocation: sequential
derp:
  server:
    enabled: false
  urls: [{map_url}]
  paths: []
  auto_update_enabled: false
disable_check_updates: true
node:
  expiry: 0
  ephemeral:
    inactivity_timeout: 30m
database:
  type: sqlite
  sqlite:
    path: {root}/db.sqlite
    write_ahead_log: true
log:
  level: warn
  format: text
policy:
  mode: file
  path: {acl}
dns:
  magic_dns: false
  base_domain: mesh.fixture.invalid
  override_local_dns: false
unix_socket: {root}/headscale.sock
unix_socket_permission: '0600'
logtail:
  enabled: false
taildrop:
  enabled: false
auto_update:
  enabled: false
''')
            def hs_json(*command):
                return json.loads(run([hs, '-c', config, '--output', 'json', *command]))
            def start(argv, name):
                log = open(root / (name + '.log'), 'xb')
                os.chmod(log.name, 0o600)
                process = subprocess.Popen([str(a) for a in argv], stdout=log, stderr=log,
                                           env=fixture_env(), stdin=subprocess.DEVNULL)
                log.close()
                processes.append(process)
                return process
            try:
                headscale = start([hs, '-c', config, 'serve'], 'headscale')
                for _ in range(100):
                    if headscale.poll() is not None:
                        # No users or auth keys exist at this startup-only diagnostic point.
                        raise RuntimeError('Headscale startup: ' + (root / 'headscale.log').read_text()[-2400:])
                    try:
                        with urllib.request.urlopen(url + '/health', timeout=1) as r:
                            if r.status == 200:
                                break
                    except OSError:
                        pass
                    time.sleep(.1)
                else:
                    raise RuntimeError('isolated Headscale did not become healthy')
                user = hs_json('users', 'create', 'desktop-fixture')
                user_id = str(user['id'])
                def key():
                    k = hs_json('preauthkeys', 'create', '--user', user_id,
                                '--expiration', '1h', '--ephemeral')
                    # Headscale CLI encoding/json omits protobuf false defaults.
                    assert k.get('reusable', False) is False and k['ephemeral'] is True
                    assert k.get('used', False) is False
                    return k
                legacy, control_key = key(), key()
                socket_path = root / 'node' / 'tailscaled.sock'
                daemon_args = [daemon, '--tun=userspace-networking', '--statedir=' + str(root / 'node'),
                               '--socket=' + str(socket_path), '--no-logs-no-support', '--socks5-server=127.0.0.1:0']
                assert str(socket_path).startswith(str(root) + '/'), 'socket escaped isolation'
                owned = start(daemon_args, 'tailscaled')
                prefix = [cli, '--socket=' + str(socket_path)]
                for _ in range(100):
                    assert owned.poll() is None, 'owned daemon exited before readiness'
                    if socket_path.exists():
                        try:
                            status(prefix)
                            break
                        except RuntimeError:
                            pass
                    time.sleep(.1)
                else:
                    raise RuntimeError('owned daemon did not become ready')
                settings = [*prefix, 'up', '--hostname=desktop-fixture', '--accept-routes=false',
                            '--login-server=' + url, '--timeout=90s']
                def join(k, force=False):
                    auth = write('node/auth-key', k['key'])
                    try:
                        run([*settings, '--auth-key=file:' + str(auth), '--reset',
                             *(['--force-reauth'] if force else [])], timeout=100)
                    finally:
                        auth.unlink()
                    assert status(prefix)['BackendState'] == 'Running'
                def key_usage():
                    keys = hs_json('preauthkeys', 'list')
                    assert all(str(k['user']['id']) == user_id for k in keys)
                    return [{'id': k['id'], 'used': k.get('used', False),
                             'reusable': k.get('reusable', False), 'ephemeral': k['ephemeral']}
                            for k in keys]
                def capture(label):
                    nodes = hs_json('nodes', 'list')
                    assert len(nodes) == 1, 'expected exactly one provider node'
                    n = nodes[0]
                    assert n['user']['id'] == user_id or str(n['user']['id']) == user_id
                    assert n.get('expiry') in (None, {'seconds': -62135596800}), 'unexpected node expiry'
                    own = status(prefix)
                    assert own['BackendState'] == 'Running', 'fixture mesh not usable'
                    assert own['Self']['PublicKey'] == n['node_key'], 'daemon/provider key mismatch'
                    assert set(own['Self']['TailscaleIPs']) == set(n['ip_addresses'])
                    out = {k: n[k] for k in ('id', 'node_key', 'machine_key', 'ip_addresses')}
                    out['key_id'] = n['pre_auth_key']['id']
                    receipt['phases'].append({'phase': label, **out, 'usable_nodes': len(nodes),
                                              'preauth_keys': key_usage(),
                                              'backend_state': own['BackendState'], 'self_online': own['Self']['Online']})
                    return out
                receipt['before_legacy_preauth_keys'] = key_usage()
                join(legacy)
                first = capture('legacy_join')
                assert str(first['key_id']) == str(legacy['id'])
                state = root / 'node' / 'tailscaled.state'
                assert state.exists(), 'persistent state missing'
                # Separate control arm; its key is never reused for force-reauth.
                if not args.skip_reset_control:
                    join(control_key)
                    control = capture('reset_without_force')
                    assert control == first, 'negative control unexpectedly reauthenticated'
                host = key()
                usage = key_usage()
                assert next(k for k in usage if k['id'] == host['id'])['used'] is False
                receipt['before_force_preauth_keys'] = usage
                try:
                    join(host, force=True)
                except RuntimeError:
                    rebound = capture('failed_force_reauth')
                    if args.probe_resume_after_error:
                        assert str(rebound['key_id']) == str(host['id']), 'exact issued key not bound'
                        assert rebound['machine_key'] == first['machine_key']
                        run(settings, timeout=100)
                        assert capture('resume_after_failed_cli') == rebound
                        receipt['exact_binding_key_free_resume_proven'] = True
                    diagnostic = (root / 'tailscaled.log').read_text()
                    receipt['daemon_failure_categories'] = [word for word in (
                        'node key already exists', 'authkey', 'invalid key', 'context deadline exceeded',
                        'register request', 'NeedsLogin', 'connection refused') if word in diagnostic]
                    raise
                upgraded = capture('force_reauth')
                assert str(upgraded['key_id']) == str(host['id']), 'new host key not bound'
                assert str(upgraded['key_id']) != str(first['key_id'])
                assert upgraded['machine_key'] == first['machine_key'], 'machine identity changed'
                run(settings)
                assert capture('resume') == upgraded, 'key-free resume changed identity'
                receipt['state_directory_reused'] = True
            finally:
                for process in reversed(processes):
                    stop(process)
                map_server.shutdown()
                map_server.server_close()
    except Exception as error:
        failure = type(error).__name__ + ': ' + str(error)
    finally:
        after_guards = [identity(status(g)) for g in guards]
        assert before_guards == after_guards, 'personal or existing Goose identity changed'
        receipt['personal_identity_unchanged'] = True
        receipt['original_goose_identity_unchanged'] = True
        receipt['guard_identity_sha256'] = [digest(g) for g in before_guards]
        receipt['owned_pids_stopped'] = all(p.poll() is not None for p in processes)
    print(json.dumps(receipt, indent=2))
    if failure:
        raise RuntimeError(failure)


if __name__ == '__main__':
    main()
