import sqlite3,json,threading,datetime
from zoneinfo import ZoneInfo
class Store:
 def __init__(self,path):
  self.db=sqlite3.connect(path,check_same_thread=False); self.db.row_factory=sqlite3.Row; self.lock=threading.RLock()
  with self.db:self.db.executescript('''CREATE TABLE IF NOT EXISTS payments (id TEXT PRIMARY KEY, amount_minor INTEGER,currency TEXT,created_at TEXT,settled_at TEXT,status TEXT,version INTEGER,note TEXT,counterparty_name TEXT,country TEXT,raw TEXT); CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY); CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY,v TEXT)''')
 def _row(self,p): return (p['id'],p['amount_minor'],p['currency'],p['created_at'],p.get('settled_at'),p['status'],p['version'],p.get('note',''),p.get('counterparty',{}).get('name',p.get('counterparty_name','')),p.get('counterparty',{}).get('country',p.get('country','')),json.dumps(p))
 def upsert_many(self,ps):
  ins=upd=0
  with self.lock,self.db:
   for p in ps:
    old=self.db.execute('select version from payments where id=?',(p['id'],)).fetchone()
    if old and p['version']<old[0]:continue
    if old:upd+=1
    else:ins+=1
    self.db.execute('INSERT OR REPLACE INTO payments VALUES (?,?,?,?,?,?,?,?,?,?,?)',self._row(p))
  return ins,upd
 def query(self,limit,offset,status=None,currency=None,sort='created_at'):
  with self.lock:
   args=[]; where=[]
   if status: where.append('status=?'); args.append(status)
   if currency: where.append('currency=?'); args.append(currency)
   w=(' WHERE '+' AND '.join(where)) if where else ''
   allrows=[dict(x) for x in self.db.execute('select id,amount_minor,currency,created_at,settled_at,status,version,note,counterparty_name,country from payments'+w,args)]
  def instant(x): return datetime.datetime.fromisoformat(x['created_at'].replace('Z','+00:00')).timestamp()
  key=instant if sort in ('created_at','-created_at') else lambda x:x['amount_minor']
  allrows.sort(key=key,reverse=sort.startswith('-'))
  return allrows[offset:offset+limit],len(allrows)
 def get(self,pid):
  with self.lock:
   r=self.db.execute('select raw from payments where id=?',(pid,)).fetchone(); return json.loads(r[0]) if r else None
 def apply_event(self,e):
  p=e['data']
  with self.lock,self.db:
   if self.db.execute('select 1 from events where id=?',(e['id'],)).fetchone():return 'duplicate'
   old=self.db.execute('select version from payments where id=?',(p['id'],)).fetchone()
   if old and p['version']<=old[0]:self.db.execute('insert or ignore into events values(?)',(e['id'],));return 'stale'
   self.db.execute('INSERT OR REPLACE INTO payments VALUES (?,?,?,?,?,?,?,?,?,?,?)',self._row(p));self.db.execute('insert into events values(?)',(e['id'],));return 'applied'
 def buckets(self):
  with self.lock: ps=[json.loads(r[0]) for r in self.db.execute('select raw from payments')]
  if not ps:return []
  tz=ZoneInfo('Europe/Berlin'); dates=[datetime.datetime.fromisoformat(p['created_at'].replace('Z','+00:00')).astimezone(tz).date() for p in ps]; a,b=min(dates),max(dates); counts={}
  for p,d in zip(ps,dates):counts[(d.isoformat(),p['status'])]=counts.get((d.isoformat(),p['status']),0)+1
  out=[]; cur=a
  while cur<=b:
   for st in ['settled','pending','refunded','failed']:out.append({'day':cur.isoformat(),'status':st,'count':counts.get((cur.isoformat(),st),0)})
   cur+=datetime.timedelta(days=1)
  return out
 def count(self):
  with self.lock:return self.db.execute('select count(*) from payments').fetchone()[0]
 def last_sync(self):
  with self.lock:r=self.db.execute('select v from meta where k="last_sync"').fetchone();return r[0] if r else None
 def set_last_sync(self,w):
  with self.lock,self.db:self.db.execute('insert or replace into meta values("last_sync",?)',(w,))
