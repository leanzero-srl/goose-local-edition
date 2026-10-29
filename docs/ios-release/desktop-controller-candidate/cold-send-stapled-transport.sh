#!/bin/bash
set -euo pipefail
umask 077
source=/tmp/goose-controller-incremental-43a8589d652f
stage=/tmp/goose-controller-stapled-43a8589d652f-gabee
[[ ! -e "$stage" ]]
python3 - <<'PY'
from pathlib import Path
import hashlib,shutil
p=Path('/tmp/goose-controller-incremental-43a8589d652f')
for n,h in {'Goose-Swarm-signed.zip':'36f291982387c88f357f077e1b2231333f42922a97a537857a6be6f1fe56cfb2','after-notarization.json':'1188eb96ac70157c4b75272be5ccef80e676acd5ab9b6af80b6a09ea55892792','receipt.json':'db417ec4b16fec8b3f434ebd8abc473dd0c932e3354c90cdd7a32e9f48740124'}.items():
 d=hashlib.sha256()
 with (p/n).open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):d.update(b)
 assert d.hexdigest()==h,n
assert shutil.disk_usage('/tmp').free>12*2**30
PY
codesign --verify --deep --strict --test-requirement '=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"' "$source/Goose Swarm.app"
xcrun stapler validate "$source/Goose Swarm.app"
spctl --assess --type execute --verbose=4 "$source/Goose Swarm.app"
mkdir -m700 "$stage"
cp "$source/after-notarization.json" "$stage/after-staple.json"
cp "$source/receipt.json" "$stage/receipt.json"
python3 /tmp/goose-bundle-fingerprint.py "$source/Goose Swarm.app" "$stage/source-fingerprint.json"
cmp "$stage/after-staple.json" "$stage/source-fingerprint.json"
ditto -c -k --sequesterRsrc --keepParent "$source/Goose Swarm.app" "$stage/Goose-Swarm-stapled.zip"
mkdir -m700 "$stage/roundtrip"
(umask 022; ditto -x -k "$stage/Goose-Swarm-stapled.zip" "$stage/roundtrip")
codesign --verify --deep --strict --test-requirement '=anchor apple generic and certificate leaf[subject.OU] = "ZZ8MTZ6NRZ"' "$stage/roundtrip/Goose Swarm.app"
xcrun stapler validate "$stage/roundtrip/Goose Swarm.app"
spctl --assess --type execute --verbose=4 "$stage/roundtrip/Goose Swarm.app"
python3 /tmp/goose-bundle-fingerprint.py "$stage/roundtrip/Goose Swarm.app" "$stage/roundtrip-fingerprint.json"
cmp "$stage/after-staple.json" "$stage/roundtrip-fingerprint.json"
python3 - <<'PY'
from pathlib import Path
import hashlib,json,datetime,zipfile
p=Path('/tmp/goose-controller-stapled-43a8589d652f-gabee')
def sha(f):
 d=hashlib.sha256()
 with f.open('rb') as s:
  for b in iter(lambda:s.read(1024*1024),b''):d.update(b)
 return d.hexdigest()
a=Path('/tmp/goose-controller-incremental-43a8589d652f/Goose-Swarm-signed.zip')
assert sha(a)=='36f291982387c88f357f077e1b2231333f42922a97a537857a6be6f1fe56cfb2'
with zipfile.ZipFile(p/'Goose-Swarm-stapled.zip') as z: unpacked=sum(i.file_size for i in z.infolist())
o={'created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_commit':'43a8589d652f72c145f57cbe667b0b9ec2a71976','notary_submission':'0c590721-38ae-4ea1-ae61-6b49db1b74e7','transport_zip':str(p/'Goose-Swarm-stapled.zip'),'transport_sha256':sha(p/'Goose-Swarm-stapled.zip'),'transport_bytes':(p/'Goose-Swarm-stapled.zip').stat().st_size,'unpacked_bytes':unpacked,'fingerprint_sha256':sha(p/'after-staple.json'),'strict_team_signature':True,'stapler_validate':True,'gatekeeper_accepted':True,'roundtrip_fingerprint_exact':True,'approved_upload_zip_unchanged':True,'upload_performed':False,'install_performed':False}
(p/'transport-receipt.json').write_text(json.dumps(o,indent=2)+'\n');print(json.dumps(o))
PY
