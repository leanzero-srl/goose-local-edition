"""Common Forge 2.0 fixtures are fixed by public task bytes, before model observations.

Development and scoring use separate hash domains. The model sees only development data;
all build episodes on a frozen task get the same hidden scoring schedule.
"""
import hashlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TASK_FILES = ('forge2/public/spec-build-forge2.md', 'forge2/public/FORGE2-CONTRACT.md',
              'forge2/public/RATE-MODEL.json', 'forge2/public/STARTER.md')


def task_digest(root=ROOT):
    digest = hashlib.sha256()
    for name in TASK_FILES:
        raw = (root / name).read_bytes()
        digest.update(name.encode() + b'\0' + str(len(raw)).encode() + b'\0' + raw)
    return digest.hexdigest()


def site_seed(domain, root=ROOT):
    if domain not in ('development', 'scoring'):
        raise ValueError('Unknown Forge fixture domain: ' + domain)
    return hashlib.sha256(('forge-2.0:' + domain + ':' + task_digest(root)).encode()).hexdigest()[:16]
