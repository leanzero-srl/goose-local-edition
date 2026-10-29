from pathlib import Path
import json,hashlib,datetime,os
os.umask(0o077)
h=Path.home();base=h/'.local/share/goose-upgrade-snapshots';old=base/'controller-latest-history-20261029T062834Z';dest=base/'controller-cold-send-20261029T100353Z'
a=json.loads((old/'manifest.json').read_text());original=(dest/'manifest.json').read_bytes();b=json.loads(original)
assert len(b['files'])==14 and not (dest/'manifest.initial-14.json').exists()
missing={r['path'] for r in a['files']}-{r['path'] for r in b['files']}
assert missing=={'Library/Application Support/Goose/Preferences','Library/Application Support/Goose/settings.json','Library/Application Support/Goose/window-state.json'}
rows=[]
for r in a['files']:
 if r['path'] not in missing:continue
 current=h/r['path'];saved=old/'files'/r['path'];target=dest/'files'/r['path']
 assert current.is_file() and not current.is_symlink() and saved.is_file() and not saved.is_symlink() and not target.exists()
 content=saved.read_bytes();assert hashlib.sha256(content).hexdigest()==r['sha256']==hashlib.sha256(current.read_bytes()).hexdigest()
 assert oct(current.stat().st_mode&0o777)==r['original_mode']
 rows.append((r,target,content))
for r,target,content in rows:
 target.parent.mkdir(parents=True,mode=0o700,exist_ok=True);target.write_bytes(content);target.chmod(0o600)
 assert hashlib.sha256(target.read_bytes()).hexdigest()==r['sha256']
 b['files'].append(r)
b['supplemental_preferences']={'captured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_snapshot':str(old),'count':len(rows),'current_content_and_modes_match_prior_preupgrade_backup':True,'note':'Initial current snapshot contained14files. Three actual Electron preferences were verified after launch against the earlier preupgrade snapshot, then copied locally from that snapshot; not represented as newly captured before installation.'}
(dest/'manifest.initial-14.json').write_bytes(original);(dest/'manifest.initial-14.json').chmod(0o600)
(dest/'manifest.json').write_text(json.dumps(b,indent=2)+'\n');(dest/'manifest.json').chmod(0o600)
print(json.dumps({'snapshot':str(dest),'initial_files':14,'supplemental':b['supplemental_preferences'],'final_files':len(b['files']),'raw_data_transferred':False}))
