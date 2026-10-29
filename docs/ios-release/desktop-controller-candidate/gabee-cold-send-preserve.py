import os,pathlib,sqlite3,json,hashlib,subprocess,datetime
os.umask(0o077)
h=pathlib.Path.home();base=h/'.local/share/goose-upgrade-snapshots';dest=base/('controller-cold-send-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ'));dest.mkdir(parents=True,mode=0o700)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
old=json.loads((base/'controller-134-20261028T211658Z/manifest.json').read_text())
paths=[r['path'] for r in old['files']]+['Library/Application Support/Goose Swarm/Preferences','Library/Application Support/Goose Swarm/Local State']
rows=[]
for rel in paths:
 p=h/rel
 if not p.exists():continue
 assert p.is_file() and not p.is_symlink(),rel
 content=p.read_bytes();q=dest/'files'/rel;q.parent.mkdir(parents=True,mode=0o700,exist_ok=True);q.write_bytes(content);q.chmod(0o600)
 assert sha(q)==hashlib.sha256(content).hexdigest()
 rows.append({'path':rel,'sha256':sha(q),'original_mode':oct(p.stat().st_mode&0o777)})
source=h/'.local/share/goose/sessions/sessions.db'
with sqlite3.connect('file:'+str(source)+'?mode=ro',uri=True) as src,sqlite3.connect(dest/'sessions.db') as target:
 src.backup(target);target.execute('PRAGMA journal_mode=DELETE');assert target.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
manifest={'files':rows,'snapshot':str(dest),'sqlite_integrity':'ok'};(dest/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
summary=json.loads(subprocess.check_output(['python3','/tmp/goose-laptop-profile-snapshot.py'],text=True));(dest/'profile-before.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'snapshot':str(dest),'files':len(rows),'session_count':summary['session_count'],'message_count':summary['message_count'],'config_sha256':summary['config_sha256'],'session_records_sha256':summary['session_records_sha256'],'full_message_records_sha256':summary['full_message_records_sha256']}))
