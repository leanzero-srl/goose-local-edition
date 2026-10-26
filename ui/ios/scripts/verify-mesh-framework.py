#!/usr/bin/env python3
"""Reject an old or modified embedded mesh binary before signing it into the app."""
import hashlib
import json
import sys
from pathlib import Path

framework = Path(sys.argv[1])
resources = Path(__file__).resolve().parents[1] / "Dependencies/TailscaleKit"
def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
try:
    receipt = json.loads((framework / "goose-mesh-build.json").read_text())
    expected = {
        "source_revision": "d1dc45d039edb7759ecbee6ba2aea0c4bca884b8",
        "diagnostic_upload_policy": "disabled-before-node-construction",
        "binary_sha256": digest(framework / "TailscaleKit"),
        "privacy_sha256": digest(resources / "PrivacyInfo.xcprivacy"),
        "policy_sha256": digest(resources / "goose_privacy.go"),
    }
    if any(receipt.get(key) != value for key, value in expected.items()):
        raise ValueError("Mesh build receipt does not match the reviewed policy and binary")
    if digest(framework / "PrivacyInfo.xcprivacy") != expected["privacy_sha256"]:
        raise ValueError("Mesh privacy manifest does not match the reviewed declaration")
except (OSError, ValueError) as error:
    raise SystemExit("error: Rebuild the embedded mesh with scripts/build-mesh-framework.sh: " + str(error))
print("Verified embedded mesh privacy policy, manifest and input binary")
