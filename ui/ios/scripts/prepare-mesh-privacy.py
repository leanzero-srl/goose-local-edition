#!/usr/bin/env python3
"""Apply the reviewed privacy policy to the exact pinned libtailscale checkout."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

PIN = "d1dc45d039edb7759ecbee6ba2aea0c4bca884b8"
source = Path(sys.argv[1]).resolve()
resources = Path(__file__).resolve().parents[1] / "Dependencies/TailscaleKit"
revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
if revision != PIN:
    raise SystemExit("Review the privacy patch against the new libtailscale revision before building")
module = (source / "go.mod").read_text()
if "tailscale.com v1.104.1" not in module:
    raise SystemExit("Unexpected Tailscale dependency; privacy proof needs review")
path = source / "tailscale.go"
text = path.read_text()
original = "func TsnetNewServer() C.int {\n"
patched = original + "\tdisableDiagnosticUploads()\n"
if patched not in text:
    if text.count(original) != 1:
        raise SystemExit("Pinned node-constructor anchor changed")
    path.write_text(text.replace(original, patched, 1))
for name in ["goose_privacy.go", "goose_privacy_test.go"]:
    destination = source / name
    payload = (resources / name).read_bytes()
    if destination.exists() and destination.read_bytes() != payload:
        raise SystemExit("Existing privacy patch differs; inspect it before replacement: " + str(destination))
    destination.write_bytes(payload)
print(json.dumps({"revision": PIN, "constructor_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                  "policy_sha256": hashlib.sha256((source / "goose_privacy.go").read_bytes()).hexdigest()}))
