import os,pathlib,json,sqlite3,hashlib,subprocess,re,datetime
h=pathlib.Path.home(); db=h/'.local/share/goose/sessions/sessions.db'; config=h/'.config/goose/config.yaml'
def sha(v):return hashlib.sha256(v).hexdigest()
out={'captured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'profile_db':str(db),'permissions':{}}
with sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True) as c:
 c.execute('PRAGMA query_only=ON'); c.execute('BEGIN')
 out['session_count']=c.execute('SELECT count(*) FROM sessions').fetchone()[0]
 out['message_count']=c.execute('SELECT count(*) FROM messages').fetchone()[0]
 session_digest=hashlib.sha256()
 for row in c.execute('SELECT * FROM sessions ORDER BY id'):
  session_digest.update(json.dumps(row,separators=(',',':'),default=lambda v:v.hex()).encode());session_digest.update(b'\n')
 out['session_records_sha256']=session_digest.hexdigest()
 out['session_ids_sha256']=sha(json.dumps(c.execute('SELECT id FROM sessions ORDER BY id').fetchall(),separators=(',',':')).encode())
 digest=hashlib.sha256()
 for row in c.execute('SELECT id,message_id,session_id,role,content_json FROM messages ORDER BY id'):
  digest.update(json.dumps(row,separators=(',',':')).encode());digest.update(b'\n')
 out['message_records_sha256']=digest.hexdigest()
 full_digest=hashlib.sha256()
 for row in c.execute('SELECT * FROM messages ORDER BY id'):
  full_digest.update(json.dumps(row,separators=(',',':'),default=lambda v:v.hex()).encode());full_digest.update(b'\n')
 out['full_message_records_sha256']=full_digest.hexdigest()
 if out['session_count']:out['latest_session_update']=c.execute('SELECT max(updated_at) FROM sessions').fetchone()[0]
 c.rollback()
if config.is_file():
 content=config.read_bytes();out['config_sha256']=sha(content)
 for name in ['LEANZERO_LINK_ALLOW_REMOTE_EXECUTION','LEANZERO_LINK_ALLOW_REMOTE_JOBS','LEANZERO_LINK_SHARE_INFERENCE']:
  match=re.search(r'^'+name+r':\s*(.*)$',content.decode(),re.M)
  out['permissions'][name]={'on_disk':match.group(1) if match else None}
sock=h/'.leanzero/tailscale/tailscaled.sock';cli='/Applications/Goose Swarm.app/Contents/Resources/bin/tailscale'
out['owned_mesh']={'socket_exists':sock.exists()}
if sock.exists():
 r=subprocess.run([cli,'--socket='+str(sock),'status','--json'],capture_output=True,text=True)
 out['owned_mesh']['status_exit']=r.returncode
 if r.returncode==0:
  s=json.loads(r.stdout);n=s['Self'];out['owned_mesh'].update({'node_id':n['ID'],'node_key_sha256':sha(n['PublicKey'].encode()),'ips':n['TailscaleIPs'],'online':n['Online'],'backend_state':s['BackendState']})
identity=h/'.leanzero/identity.json';out['saved_identity_exists']=identity.exists();out['saved_identity_mode']=oct(identity.stat().st_mode&0o777) if identity.exists() else None
print(json.dumps(out))
