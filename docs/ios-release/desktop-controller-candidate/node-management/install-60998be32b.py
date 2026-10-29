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
# Exact signed artifact reviewed on 2026-10-29; all pins belong to one receipt.
PINS = {
    'receipt_sha256': '53da481c4ad311679f68878c8bd27961288b4975b2495e328e610365449a1f50',
    'source_commit': '60998be32b57c414ec1fb54bd7ec6fe1a61ae2d9',
    'source_tree': '57749493668fd7ee51136fc3c5afc8f4359f0cad',
    'build_receipt_sha256': '835c2521d779ded7f0e9d9f6677566dc3b11d42dcb8f74a56751a1a046731b6d',
    'input_archive_sha256': 'abc7f3f027df80febc0244fd1df03ef3619bbc2d03de9a739ca56f63c003b1c6',
    'input_cli_sha256': 'a7b13a2b2071103f5427a5d1557cd01859328a406f60f1d781391fb33dd8bcd2',
    'signed_cli_sha256': '3c2bf0ecb6d5cb3e4add72ee6e9c17fa096ba3bb17851e86ef887147472f187d',
    'signed_archive_sha256': '1c05d1973d37464d1e39326aa3637e6070269a59627e65ab49ea32366e84643d',
    'fingerprint_sha256': '6afcf73759b61a93c1df4f9f42ae57850adba238c7cf5a13c0d949e33ffa7c34',
    'signing_script_sha256': 'dc8d76089549e9565693a2c4b5fb85a021cd145f0fb712e848fdd16cac3e3ba4',
    'comparator_sha256': '91b9f66db0fcd8c624c6fe4fb51cd3ac0f345a5c8cc01701eae097cfabd232a2',
}
assert all(value != 'PENDING' for value in PINS.values()), 'Exact candidate pins are pending; no preflight or installation allowed'
receipt_bytes = pathlib.Path(receipt_path).read_bytes()
assert hashlib.sha256(receipt_bytes).hexdigest() == PINS['receipt_sha256']
receipt = json.loads(receipt_bytes)
for key, pin in [
    ('sourceCommit', 'source_commit'), ('sourceTree', 'source_tree'),
    ('buildReceiptSha256', 'build_receipt_sha256'), ('inputArchiveSha256', 'input_archive_sha256'),
    ('inputCliSha256', 'input_cli_sha256'), ('signedCliSha256', 'signed_cli_sha256'),
    ('signedArchiveSha256', 'signed_archive_sha256'), ('signedFingerprintSha256', 'fingerprint_sha256'),
    ('signingScriptSha256', 'signing_script_sha256'), ('machOComparatorSha256', 'comparator_sha256'),
]:
    assert receipt[key] == PINS[pin], key
assert receipt['appVersion'] == '3.0.134' and receipt['team'] == 'ZZ8MTZ6NRZ'
assert receipt['archiveStrictSignatureVerified'] is True
assert receipt['signatures']['app'] is True and receipt['signatures']['team'] == 'ZZ8MTZ6NRZ'
assert set(receipt['machOEquivalence']) == set(receipt['classification']['machO']), 'Incomplete changed Mach-O comparison'
assert receipt['machONegativeControl']['positiveComparisonPassed'] is True
assert receipt['machONegativeControl']['nonSignatureCodeChangeRejected'] is True
assert sha('/tmp/goose-original-owner-idle-probe.py') == '99acee564df8e06beb6e31f25261d5124a8b18b044bb26fed762815e4d60c701'
assert sha('/tmp/goose-original-activity-probe.py') == '7cffee08b7f9c1bb316b7d72cf120fa030be744e12eb184ab49a112f77f7eb92'
expected_fingerprint_sha = PINS['fingerprint_sha256']
fingerprint_bytes = pathlib.Path(fingerprint_path).read_bytes()
assert hashlib.sha256(fingerprint_bytes).hexdigest() == expected_fingerprint_sha
expected_inventory = json.loads(fingerprint_bytes)
fingerprinter = '/tmp/goose-bundle-fingerprint.py'
assert sha(fingerprinter) == 'b2eaa9d4023b8bf97db598f928270851bdaa00377bff9367b58401b206892849'
source = pathlib.Path(source_path).resolve()
app = pathlib.Path('/Applications/Goose Swarm.app')
cli = pathlib.Path('Contents/Resources/bin/goose')
expected = receipt['signedCliSha256']
old = 'f43f47848b133350cfc037c6a3fb142683688d9410e598b2652319ee65fe460c'
assert source.is_dir() and source != app and str(source).startswith('/private/tmp/goose-controller-')
assert sha(source / cli) == expected and expected != old
assert sha(app / cli) == old, 'Installed artifact changed; re-evaluate before replacing it'
assert source == pathlib.Path(receipt['app']).resolve(), 'Source app is not the reviewed signed artifact'
baseline_manifest = pathlib.Path('/tmp/goose-controller-incremental-43a8589d652f/after-notarization.json')
assert sha(baseline_manifest) == '1188eb96ac70157c4b75272be5ccef80e676acd5ab9b6af80b6a09ea55892792'
baseline_inventory = json.loads(baseline_manifest.read_text())


def verify_inventory(bundle, expected_entries):
    with tempfile.TemporaryDirectory(prefix='goose-node-install-fingerprint-') as checks:
        manifest = pathlib.Path(checks)/'inventory.json'
        run('python3', fingerprinter, str(bundle), str(manifest))
        actual = json.loads(manifest.read_text())
    assert set(actual) == set(expected_entries), 'Whole-bundle inventory differs: ' + str(bundle)
    for name, entry in expected_entries.items():
        if actual[name] == entry:
            continue
        assert (name == '.' and entry['kind'] == 'directory') or entry['kind'] == 'symlink', 'Unexpected whole-bundle change: ' + name
        expected_entry = copy.deepcopy(entry)
        assert 'com.apple.provenance' in expected_entry['xattrs'], 'Unexplained metadata change: ' + name
        del expected_entry['xattrs']['com.apple.provenance']
        assert actual[name] == expected_entry, 'Whole-bundle content differs: ' + name


verify_inventory(source, expected_inventory)
verify_inventory(app, baseline_inventory)

req = '=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"'
run('/usr/bin/codesign', '--verify', '--deep', '--strict', '--test-requirement', req, str(source))
assert run('/usr/libexec/PlistBuddy', '-c', 'Print CFBundleShortVersionString', str(source/'Contents/Info.plist')).strip() == '3.0.134'
app_pid, backend_pid = int(app_pid), int(backend_pid)
app_started = run('ps', '-p', str(app_pid), '-o', 'lstart=').strip()
backend_started = run('ps', '-p', str(backend_pid), '-o', 'lstart=').strip()
assert app_started and backend_started
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
verify_inventory(incoming, expected_inventory)
run('/usr/bin/codesign', '--verify', '--deep', '--strict', '--test-requirement', req, str(incoming))
# Copy and verification take time; refuse a run that started meanwhile.
catalog = json.loads(run('python3', '/tmp/goose-original-owner-idle-probe.py', str(backend_pid), owner_id))
assert catalog['fresh'] and not catalog['active'], 'Original work is not idle'
activity = json.loads(run('python3', '/tmp/goose-original-activity-probe.py', str(backend_pid)))
assert all(activity['activity_counts'].get(k) == 0 for k in ['running', 'needsYou', 'background', 'looping', 'notesWaiting']), 'Activity began; staged app left uninstalled'
assert run('ps', '-p', str(app_pid), '-o', 'comm=').strip() == str(app/'Contents/MacOS/Goose Swarm')
assert run('ps', '-p', str(app_pid), '-o', 'lstart=').strip() == app_started, 'App PID identity changed'
assert run('ps', '-p', str(backend_pid), '-o', 'lstart=').strip() == backend_started, 'Backend PID identity changed'
rows = [tuple(map(int, line.split())) for line in run('ps', '-axo', 'pid=,ppid=').splitlines()]
current_owned = {app_pid}
while True:
    expanded = current_owned | {p for p, parent in rows if parent in current_owned}
    if expanded == current_owned:
        break
    current_owned = expanded
assert backend_pid in current_owned, 'Original backend is no longer owned by this app'
owned |= current_owned
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
# Private profile backup/record comparison is owned by the surrounding root procedure.
# This operator writes only staged/installed/rollback app bundles, never profile data.
# Root launches through CUA after the atomic whole-app replacement.
print(json.dumps({'installed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_commit':receipt['sourceCommit'], 'signed_cli_sha256':expected, 'rollback':str(rollback), 'normal_launch_requested':False, 'installer_profile_writes':False, 'launch_may_migrate_schema_to':19, 'saved_login_reconnection_verified':False}))
