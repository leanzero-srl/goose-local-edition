import copy, datetime, hashlib, json, os, pathlib, signal, subprocess, sys, tempfile, time


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def active_pids():
    return {int(line.strip()) for line in run('ps', '-axo', 'pid=').splitlines()}


if not __debug__:
    raise RuntimeError('Safety checks require normal Python; -O is refused')

check_only = '--check-only' in sys.argv[1:]
receipt_path, source_path, fingerprint_path, app_pid, backend_pid, owner_id = [a for a in sys.argv[1:] if a != '--check-only']
receipt_bytes = pathlib.Path(receipt_path).read_bytes()
assert hashlib.sha256(receipt_bytes).hexdigest() == 'db417ec4b16fec8b3f434ebd8abc473dd0c932e3354c90cdd7a32e9f48740124'
receipt = json.loads(receipt_bytes)
assert receipt['cliSourceSha'] == '43a8589d652f72c145f57cbe667b0b9ec2a71976'
assert receipt['desktopSourceSha'] == '57419bcc521ce6f3ef0cc00dde59b735415496c3'
assert receipt['baselineArchiveSha256'] == 'f2d7a1c2efdeaa06e63e2ea2fb7f7ea6719d24851f7309ca982b02150afc4d80'
assert receipt['inputCliSha256'] == '3d244ac27cf450a8619273fbeeff3a3edd392b4f85ca864a5336b147ed103d8c'
assert receipt['signedCliSha256'] == 'f43f47848b133350cfc037c6a3fb142683688d9410e598b2652319ee65fe460c'
assert receipt['cliEquivalence']['equivalentSha256'] == '257098dcf2c1801fe5c7c6e204e3278123e12b13c5a5c38658556a5f2938951b'
assert receipt['mainEquivalence']['equivalentSha256'] == 'f80839e46cf391f5891577a3fb269db6396cdd87b8a082d57f16b32173157ce4'
assert receipt['changed'] == ['Contents/MacOS/Goose Swarm', 'Contents/Resources/bin/goose', 'Contents/_CodeSignature/CodeResources']
assert sha('/tmp/goose-original-owner-idle-probe.py') == '99acee564df8e06beb6e31f25261d5124a8b18b044bb26fed762815e4d60c701'
assert sha('/tmp/goose-original-activity-probe.py') == '7cffee08b7f9c1bb316b7d72cf120fa030be744e12eb184ab49a112f77f7eb92'
expected_fingerprint_sha = '1188eb96ac70157c4b75272be5ccef80e676acd5ab9b6af80b6a09ea55892792'
fingerprint_bytes = pathlib.Path(fingerprint_path).read_bytes()
assert hashlib.sha256(fingerprint_bytes).hexdigest() == expected_fingerprint_sha
expected_inventory = json.loads(fingerprint_bytes)
fingerprinter = '/tmp/goose-bundle-fingerprint.py'
assert sha(fingerprinter) == 'b2eaa9d4023b8bf97db598f928270851bdaa00377bff9367b58401b206892849'
source = pathlib.Path(source_path).resolve()
app = pathlib.Path('/Applications/Goose Swarm.app')
cli = pathlib.Path('Contents/Resources/bin/goose')
expected = receipt['signedCliSha256']
old = 'cdc357f9743cabda6b4f2a7852a5e7654be13bb7a9f5c359b99855cdd9a2c51f'
assert 'PENDING_' not in expected_fingerprint_sha, 'Root-verified post-staple fingerprint required'
assert source.is_dir() and source != app and str(source).startswith('/private/tmp/goose-controller-')
assert sha(source / cli) == expected and expected != old
assert sha(app / cli) == old, 'Installed artifact changed; re-evaluate before replacing it'
req = '=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"'
run('/usr/bin/codesign', '--verify', '--deep', '--strict', '--test-requirement', req, str(source))
run('/usr/bin/xcrun', 'stapler', 'validate', str(source))
run('/usr/sbin/spctl', '--assess', '--type', 'execute', '--verbose=4', str(source))
assert run('/usr/libexec/PlistBuddy', '-c', 'Print CFBundleShortVersionString', str(source/'Contents/Info.plist')).strip() == '3.0.134'
app_pid, backend_pid = int(app_pid), int(backend_pid)
assert run('ps', '-p', str(app_pid), '-o', 'comm=').strip() == str(app/'Contents/MacOS/Goose Swarm')
rows = [tuple(map(int, line.split())) for line in run('ps', '-axo', 'pid=,ppid=').splitlines()]
owned = {app_pid}
while True:
    expanded = owned | {p for p, parent in rows if parent in owned}
    if expanded == owned:
        break
    owned = expanded
assert backend_pid in owned
rollback = pathlib.Path.home()/'.local/share/goose-controller-rollbacks'/old[:16]/'Goose Swarm.app'
incoming = app.parent/('.Goose Swarm.controller-stage-' + expected[:16] + '.app')
assert not rollback.exists(), 'Inspect previous attempt before retrying'
assert not incoming.is_symlink() and (not incoming.exists() or incoming.is_dir()), 'Incoming stage must be a real directory'
catalog = json.loads(run('python3', '/tmp/goose-original-owner-idle-probe.py', str(backend_pid), owner_id))
assert catalog['fresh'] and not catalog['active'], 'Original work is not idle'
activity = json.loads(run('python3', '/tmp/goose-original-activity-probe.py', str(backend_pid)))
assert all(activity['activity_counts'].get(k) == 0 for k in ['running', 'needsYou', 'background', 'looping', 'notesWaiting']), 'Original sessions not idle'
if check_only:
    print(json.dumps({'check_only': True, 'idle': True, 'original_backend_pid': backend_pid, 'new_cli_sha256': expected, 'installation_performed': False}))
    raise SystemExit(0)
if not incoming.exists():
    run('/usr/bin/ditto', str(source), str(incoming))
assert sha(incoming/cli) == expected
with tempfile.TemporaryDirectory(prefix='goose-incoming-fingerprint-') as checks:
    manifest = pathlib.Path(checks)/'incoming.json'
    run('python3', fingerprinter, str(incoming), str(manifest))
    actual_inventory = json.loads(manifest.read_text())
assert set(actual_inventory) == set(expected_inventory), 'Whole-bundle inventory differs'
for name, entry in expected_inventory.items():
    if actual_inventory[name] == entry:
        continue
    assert (name == '.' and entry['kind'] == 'directory') or entry['kind'] == 'symlink', 'Unexpected whole-bundle change: ' + name
    expected_entry = copy.deepcopy(entry)
    del expected_entry['xattrs']['com.apple.provenance']
    assert actual_inventory[name] == expected_entry, 'Whole-bundle content differs: ' + name
run('/usr/bin/codesign', '--verify', '--deep', '--strict', '--test-requirement', req, str(incoming))
run('/usr/bin/xcrun', 'stapler', 'validate', str(incoming))
run('/usr/sbin/spctl', '--assess', '--type', 'execute', '--verbose=4', str(incoming))
# Copy and verification take time; refuse a run that started meanwhile.
catalog = json.loads(run('python3', '/tmp/goose-original-owner-idle-probe.py', str(backend_pid), owner_id))
assert catalog['fresh'] and not catalog['active'], 'Original work is not idle'
activity = json.loads(run('python3', '/tmp/goose-original-activity-probe.py', str(backend_pid)))
assert all(activity['activity_counts'].get(k) == 0 for k in ['running', 'needsYou', 'background', 'looping', 'notesWaiting']), 'Activity began; staged app left uninstalled'
assert run('ps', '-p', str(app_pid), '-o', 'comm=').strip() == str(app/'Contents/MacOS/Goose Swarm')
os.kill(app_pid, signal.SIGTERM)
for _ in range(30):
    if not owned & active_pids():
        break
    time.sleep(1)
else:
    raise RuntimeError('Original app processes remain; staged app left uninstalled')
rollback.parent.mkdir(parents=True, mode=0o700)
app.rename(rollback)
try:
    incoming.rename(app)
except Exception:
    rollback.rename(app)
    raise
assert sha(app/cli) == expected
# Root launches through CUA after the atomic whole-app replacement.
print(json.dumps({'installed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_commit':receipt['cliSourceSha'], 'signed_cli_sha256':expected, 'rollback':str(rollback), 'normal_launch_requested':False, 'installer_profile_writes':False, 'launch_may_migrate_schema_to':19, 'saved_login_reconnection_verified':False}))
