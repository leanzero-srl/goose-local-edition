import datetime, hashlib, json, os, pathlib, subprocess
os.umask(0o077)
h=pathlib.Path.home()
dest=h/'.local/share/goose-upgrade-snapshots'/('controller-nodes-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
paths=['.config/goose/config.yaml','.config/goose/tls/server.key','.config/goose/tls/server.pem','.leanzero/identity.json','.leanzero/host-secret.json','.leanzero/link-intent.json','.leanzero/tailscale/derpmap.cached.json','.leanzero/tailscale/tailscaled.state']
paths += ['.local/share/goose/sessions/controller-host/'+name+'.json' for name in ['account','cursor-key','host-credential','owner','runtime','tls']]
paths += ['Library/Application Support/Goose/'+name for name in ['Preferences','settings.json','window-state.json']]
required={'.config/goose/config.yaml','.leanzero/identity.json','.local/share/goose/sessions/controller-host/owner.json'}
assert all((h/p).is_file() for p in required), 'Expected original profile missing'
dest.mkdir(parents=True,mode=0o700,exist_ok=False)
rows=[]
for rel in paths:
 p=h/rel
 if not p.exists():
  rows.append({'path':rel,'exists':False});continue
 assert p.is_file() and not p.is_symlink(),rel
 content=p.read_bytes();q=dest/'files'/rel;q.parent.mkdir(parents=True,mode=0o700,exist_ok=True);q.write_bytes(content);q.chmod(0o600)
 digest=hashlib.sha256(content).hexdigest();assert hashlib.sha256(q.read_bytes()).hexdigest()==digest
 rows.append({'path':rel,'exists':True,'sha256':digest,'original_mode':oct(p.stat().st_mode&0o777)})
assert hashlib.sha256(pathlib.Path('/tmp/goose-laptop-profile-snapshot.py').read_bytes()).hexdigest() == '7cf9ba0ae1e062e45ca49a07e03ef3819da73098cad19367a3f3f9f4d73df98b'
summary=json.loads(subprocess.check_output(['python3','/tmp/goose-laptop-profile-snapshot.py'],text=True))
assert summary['config_sha256'] == next(r['sha256'] for r in rows if r['path']=='.config/goose/config.yaml'), 'Config changed while capturing baseline'
manifest={'snapshot':str(dest),'files':rows,'profile_before':summary,'database_backup_created':False,'database_proof':'Read-only complete session/message row hashes; whole-app swap does not write the profile.'}
(dest/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'snapshot':str(dest),'files':sum(x['exists'] for x in rows),'session_count':summary['session_count'],'message_count':summary['message_count']}))
