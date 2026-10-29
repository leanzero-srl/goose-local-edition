from pathlib import Path
import hashlib,json,subprocess,shutil,datetime,zipfile
stage=Path('/tmp/goose-controller-stapled-43a8589d652f-gabee')
checks={str(stage/'Goose-Swarm-stapled.zip'):'5b5334eeb8356600df34254e492a32d5a07106df23d498c45b72144c1a47a852',str(stage/'receipt.json'):'db417ec4b16fec8b3f434ebd8abc473dd0c932e3354c90cdd7a32e9f48740124',str(stage/'after-staple.json'):'1188eb96ac70157c4b75272be5ccef80e676acd5ab9b6af80b6a09ea55892792',str(stage/'transport-receipt.json'):'db6c7393b9fdad1c6657f837d172a915271c409b49a65b479479f407e723eaff','/tmp/goose-install-gabee-cold-send-controller.py':'0077abef397542effd5fe31d9a5bc5ed64742bbcce35940be354992d5ce4dc65','/tmp/goose-gabee-cold-send-preserve.py':'741d67a1a801235785d8afc2d446014151ee147a2618697ae2f5c97dd2d9cf8e','/tmp/goose-gabee-cold-send-install.sh':'254f0214dfe045fad70e1eb9bfed31f113dd0906493df8a24214f60030c7052d','/tmp/goose-laptop-profile-snapshot.py':'7cf9ba0ae1e062e45ca49a07e03ef3819da73098cad19367a3f3f9f4d73df98b','/tmp/goose-original-owner-idle-probe.py':'99acee564df8e06beb6e31f25261d5124a8b18b044bb26fed762815e4d60c701','/tmp/goose-original-activity-probe.py':'7cffee08b7f9c1bb316b7d72cf120fa030be744e12eb184ab49a112f77f7eb92','/tmp/goose-bundle-fingerprint.py':'b2eaa9d4023b8bf97db598f928270851bdaa00377bff9367b58401b206892849','/Applications/Goose Swarm.app/Contents/Resources/bin/goose':'cdc357f9743cabda6b4f2a7852a5e7654be13bb7a9f5c359b99855cdd9a2c51f'}
for p,h in checks.items():
 d=hashlib.sha256()
 with open(p,'rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):d.update(b)
 assert d.hexdigest()==h,p
with zipfile.ZipFile(stage/'Goose-Swarm-stapled.zip') as z: unpacked=sum(i.file_size for i in z.infolist())
free=shutil.disk_usage('/tmp').free
assert free>=2*unpacked+8*2**30
assert subprocess.check_output(['ps','-p','98150','-o','comm='],text=True).strip()=='/Applications/Goose Swarm.app/Contents/MacOS/Goose Swarm'
catalog=json.loads(subprocess.check_output(['python3','/tmp/goose-original-owner-idle-probe.py','98293','295d8723-74c6-49ec-af5c-9e4915786299'],text=True));assert catalog['fresh'] and not catalog['active']
activity=json.loads(subprocess.check_output(['python3','/tmp/goose-original-activity-probe.py','98293'],text=True));assert all(activity['activity_counts'][k]==0 for k in ['running','needsYou','background','looping','notesWaiting'])
profile=json.loads(subprocess.check_output(['python3','/tmp/goose-laptop-profile-snapshot.py'],text=True))
print(json.dumps({'captured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'verified_hashes':checks,'free_gib':free/2**30,'catalog':catalog,'activity':activity,'profile':profile,'installation_performed':False,'snapshot_created':False,'ui_actions':False,'next':'Hold for root release; wrapper will repeat checks and preserve original profile locally before replacement.'}))
