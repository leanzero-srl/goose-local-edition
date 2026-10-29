#!/bin/bash
set -euo pipefail
umask 077
stage=/tmp/goose-controller-stapled-43a8589d652f-gabee
# Immutable pins are filled only from the root-verified stapled artifact.
python3 - <<'PY'
from pathlib import Path
import hashlib,shutil,zipfile
stage=Path('/tmp/goose-controller-stapled-43a8589d652f-gabee')
checks={str(stage/'Goose-Swarm-stapled.zip'):'5b5334eeb8356600df34254e492a32d5a07106df23d498c45b72144c1a47a852',str(stage/'receipt.json'):'db417ec4b16fec8b3f434ebd8abc473dd0c932e3354c90cdd7a32e9f48740124',str(stage/'after-staple.json'):'1188eb96ac70157c4b75272be5ccef80e676acd5ab9b6af80b6a09ea55892792','/tmp/goose-install-gabee-cold-send-controller.py':'0077abef397542effd5fe31d9a5bc5ed64742bbcce35940be354992d5ce4dc65','/tmp/goose-gabee-cold-send-preserve.py':'741d67a1a801235785d8afc2d446014151ee147a2618697ae2f5c97dd2d9cf8e','/tmp/goose-laptop-profile-snapshot.py':'7cf9ba0ae1e062e45ca49a07e03ef3819da73098cad19367a3f3f9f4d73df98b','/tmp/goose-original-owner-idle-probe.py':'99acee564df8e06beb6e31f25261d5124a8b18b044bb26fed762815e4d60c701','/tmp/goose-original-activity-probe.py':'7cffee08b7f9c1bb316b7d72cf120fa030be744e12eb184ab49a112f77f7eb92','/tmp/goose-bundle-fingerprint.py':'b2eaa9d4023b8bf97db598f928270851bdaa00377bff9367b58401b206892849'}
assert all('PENDING_' not in h for h in checks.values()),'Root-verified stapled artifact pins required'
for p,h in checks.items():
 digest=hashlib.sha256()
 with open(p,'rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):digest.update(b)
 assert digest.hexdigest()==h,p
with zipfile.ZipFile(stage/'Goose-Swarm-stapled.zip') as z: unpacked=sum(i.file_size for i in z.infolist())
free=shutil.disk_usage('/tmp').free
assert free>=2*unpacked+8*2**30,'Extraction and incoming copy must leave 8 GiB spare'
assert not (stage/'extracted').exists(),'Inspect prior extraction before retry'
print('EXACT_ARTIFACTS_VERIFIED',len(checks),'FREE_BYTES',free,'EXTRACTION_BYTES',unpacked,flush=True)
PY
mkdir -m700 "$stage/extracted"
(umask 022; ditto -x -k "$stage/Goose-Swarm-stapled.zip" "$stage/extracted")
python3 /tmp/goose-install-gabee-cold-send-controller.py --check-only "$stage/receipt.json" "$stage/extracted/Goose Swarm.app" "$stage/after-staple.json" 98150 98293 295d8723-74c6-49ec-af5c-9e4915786299 > "$stage/idle-check.json"
python3 /tmp/goose-gabee-cold-send-preserve.py > "$stage/preservation-snapshot.json"
python3 /tmp/goose-laptop-profile-snapshot.py > "$stage/profile-before.json"
python3 /tmp/goose-install-gabee-cold-send-controller.py "$stage/receipt.json" "$stage/extracted/Goose Swarm.app" "$stage/after-staple.json" 98150 98293 295d8723-74c6-49ec-af5c-9e4915786299 > "$stage/install-result.json"
python3 /tmp/goose-laptop-profile-snapshot.py > "$stage/profile-after.json"
python3 - <<'PY'
from pathlib import Path
import json,hashlib
stage=Path('/tmp/goose-controller-stapled-43a8589d652f-gabee')
b=json.loads((stage/'profile-before.json').read_text());a=json.loads((stage/'profile-after.json').read_text())
keys=['session_count','message_count','session_records_sha256','full_message_records_sha256','config_sha256','permissions']
checks={k:a[k]==b[k] for k in keys}
snapshot=Path(json.loads((stage/'preservation-snapshot.json').read_text())['snapshot'])
manifest=json.loads((snapshot/'manifest.json').read_text())
files=[]
for r in manifest['files']:
 p=Path.home()/r['path'];same=p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest()==r['sha256'];mode=p.exists() and oct(p.stat().st_mode&0o777)==r['original_mode']
 files.append({'path':r['path'],'same_bytes':same,'same_mode':mode})
checks['protected_files']=all(r['same_mode'] and (r['same_bytes'] or r['path'].endswith('/controller-host/runtime.json')) for r in files)
o={'checks':checks,'before':b,'after':a,'protected_files':files,'snapshot':str(snapshot),'installed':json.loads((stage/'install-result.json').read_text())}
(stage/'preservation-result.json').write_text(json.dumps(o,indent=2)+'\n')
assert all(checks.values()),checks
print(json.dumps({'preservation':checks,'installed':o['installed'],'snapshot':str(snapshot)}))
PY
