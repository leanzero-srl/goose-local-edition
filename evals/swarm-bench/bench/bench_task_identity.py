"""Capture a Forge task identity from verified launch bytes; never bless an old build at publish time."""
import hashlib
import json
from pathlib import Path
import re

SHA256 = re.compile(r'^[0-9a-f]{64}$')


def require_fingerprint(value):
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise ValueError('Forge 2.0 task fingerprint is missing or invalid')
    return value


def capture(root: Path, run_id: str):
    if not isinstance(run_id, str) or not run_id.strip():
        raise ValueError('Forge 2.0 launch build identity is missing')
    raw = (root / 'forge2/release-manifest.json').read_bytes()
    manifest = json.loads(raw)
    if manifest.get('schemaVersion') != 1 or manifest.get('family') != 'forge' or manifest.get('scorerVersion') != 'forge-2.0':
        raise ValueError('Forge 2.0 launch manifest identity differs')
    files = manifest.get('files')
    if not isinstance(files, dict) or not files:
        raise ValueError('Forge 2.0 launch payload pins are missing')
    for name, expected in files.items():
        path = Path(name)
        if path.is_absolute() or '..' in path.parts or name == 'forge2/release-manifest.json':
            raise ValueError('Forge 2.0 launch manifest contains an invalid payload path')
        require_fingerprint(expected)
        actual = hashlib.sha256((root / path).read_bytes()).hexdigest()
        if actual != expected:
            raise ValueError('Forge 2.0 installed payload differs from its release manifest: ' + name)
    return {'taskFingerprint': hashlib.sha256(raw).hexdigest(), 'buildId': run_id}


def validate_unchanged(root, launch):
    current = capture(root, launch['buildId'])
    if current['taskFingerprint'] != launch['taskFingerprint']:
        raise ValueError('Forge 2.0 installed task changed during this build; candidate retained, result unscored')


def preserve(receipt, result):
    """Only the launch receipt can restore publication identity after free scoring retries."""
    result.pop('taskFingerprint', None)
    result.pop('buildId', None)
    launch = receipt.get('launchIdentity')
    if not isinstance(launch, dict):
        result['publishable'] = False
        result['publication_identity_missing'] = 'Original Forge 2.0 launch fingerprint was not recorded'
        return
    fingerprint = require_fingerprint(launch.get('taskFingerprint'))
    if launch.get('buildId') != receipt.get('runId'):
        raise ValueError('Forge 2.0 launch and completion build identities differ')
    result.update(taskFingerprint=fingerprint, buildId=launch['buildId'])
    for key in ('input_manifest', 'reasoning_effort'):
        if key in launch:
            result[key] = launch[key]
