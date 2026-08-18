from __future__ import annotations
import datetime as dt
import sqlite3, threading
from pathlib import Path
from zoneinfo import ZoneInfo
PAYMENT_COLUMNS=("id","amount_minor","currency","created_at","settled_at","status","version","note","counterparty_name","country")
STATUSES=("settled","pending","refunded","failed")
def utc_instant(value:str)->str:
    return dt.datetime.fromisoformat(value.replace("Z","+00:00")).astimezone(dt.timezone.utc).isoformat().replace("+00:00","Z")
def flatten(payment:dict)->dict:
    cp=payment.get("counterparty") or {}
    return {"id":payment["id"],"amount_minor":int(payment["amount_minor"]),"currency":payment["currency"],"created_at":payment["created_at"],"settled_at":payment.get("settled_at"),"status":payment["status"],"version":int(payment["version"]),"note":payment.get("note","") or "","counterparty_name":cp.get("name",payment.get("counterparty_name","") or ""),"country":cp.get("country",payment.get("country","") or ""),"created_instant":utc_instant(payment["created_at"])}
class Store:
 def __init__(self,path:str)->None:
  parent=Path(path).parent
  if str(parent) not in ("", "."): parent.mkdir(parents=True,exist_ok=True)
  self.db=sqlite3.connect(path,check_same_thread=False);self.db.row_factory=sqlite3.Row;self.lock=threading.RLock()
  with self.lock,self.db:self.db.executescript("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS payments(id TEXT PRIMARY KEY,amount_minor INTEGER NOT NULL,currency TEXT NOT NULL,created_at TEXT NOT NULL,created_instant TEXT NOT NULL,settled_at TEXT,status TEXT NOT NULL,version INTEGER NOT NULL,note TEXT NOT NULL,counterparty_name TEXT NOT NULL,country TEXT NOT NULL); CREATE INDEX IF NOT EXISTS payment_lookup ON payments(status,currency,created_instant); CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY); CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);")
 def _write(self,p:dict)->int:
  cur=self.db.execute("""INSERT INTO payments(id,amount_minor,currency,created_at,created_instant,settled_at,status,version,note,counterparty_name,country) VALUES(:id,:amount_minor,:currency,:created_at,:created_instant,:settled_at,:status,:version,:note,:counterparty_name,:country) ON CONFLICT(id) DO UPDATE SET amount_minor=excluded.amount_minor,currency=excluded.currency,created_at=excluded.created_at,created_instant=excluded.created_instant,settled_at=excluded.settled_at,status=excluded.status,version=excluded.version,note=excluded.note,counterparty_name=excluded.counterparty_name,country=excluded.country WHERE excluded.version > payments.version""",p)
  return cur.rowcount
 def upsert_many(self,payments:list[dict])->tuple[int,int]:
  inserted=updated=0
  with self.lock,self.db:
   for raw in payments:
    p=flatten(raw); exists=self.db.execute("SELECT 1 FROM payments WHERE id=?",(p["id"],)).fetchone() is not None; changed=self._write(p)
    if changed: inserted+=not exists;updated+=exists
  return inserted,updated
 def query(self,limit:int,offset:int,status:str|None=None,currency:str|None=None,sort:str="created_at")->tuple[list[dict],int]:
  where=[];values=[]
  if status:where.append("status=?");values.append(status)
  if currency:where.append("currency=?");values.append(currency)
  clause=(" WHERE "+" AND ".join(where)) if where else "";ordering={"created_at":"created_instant ASC","-created_at":"created_instant DESC","amount_minor":"amount_minor ASC","-amount_minor":"amount_minor DESC"}[sort]
  tie_direction = "DESC" if sort.startswith("-") else "ASC"
  with self.lock:
   total=self.db.execute("SELECT count(*) FROM payments"+clause,values).fetchone()[0];rows=self.db.execute("SELECT "+",".join(PAYMENT_COLUMNS)+" FROM payments"+clause+" ORDER BY "+ordering+",id "+tie_direction+" LIMIT ? OFFSET ?",values+[limit,offset]).fetchall()
  return [dict(r) for r in rows],total
 def get(self,payment_id:str)->dict|None:
  with self.lock:r=self.db.execute("SELECT "+",".join(PAYMENT_COLUMNS)+" FROM payments WHERE id=?",(payment_id,)).fetchone()
  return dict(r) if r else None
 def apply_event(self,event:dict)->str:
  event_id,p=event["id"],flatten(event["data"])
  with self.lock,self.db:
   if self.db.execute("SELECT 1 FROM events WHERE id=?",(event_id,)).fetchone():return "duplicate"
   old=self.db.execute("SELECT version FROM payments WHERE id=?",(p["id"],)).fetchone()
   if old and p["version"]<=old["version"]:self.db.execute("INSERT INTO events VALUES(?)",(event_id,));return "stale"
   self._write(p);self.db.execute("INSERT INTO events VALUES(?)",(event_id,));return "applied"
 def buckets(self)->list[dict]:
  with self.lock:rows=self.db.execute("SELECT created_instant,status FROM payments").fetchall()
  berlin=ZoneInfo("Europe/Berlin");counts={};dates=[]
  for r in rows:
   day=dt.datetime.fromisoformat(r["created_instant"].replace("Z","+00:00")).astimezone(berlin).date();dates.append(day);counts[(day.isoformat(),r["status"])]=counts.get((day.isoformat(),r["status"]),0)+1
  if not dates:return []
  day,end=min(dates),max(dates);out=[]
  while day<=end:
   for status in STATUSES:out.append({"day":day.isoformat(),"status":status,"count":counts.get((day.isoformat(),status),0)})
   day+=dt.timedelta(days=1)
  return out
 def count(self)->int:
  with self.lock:return self.db.execute("SELECT count(*) FROM payments").fetchone()[0]
 def last_sync(self)->str|None:
  with self.lock:r=self.db.execute("SELECT value FROM metadata WHERE key='last_sync'").fetchone()
  return r[0] if r else None
 def set_last_sync(self,when:str)->None:
  with self.lock,self.db:self.db.execute("INSERT INTO metadata VALUES('last_sync',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",(when,))
 def summary(self)->dict:
  with self.lock:
   count,old,new=self.db.execute("SELECT count(*),min(created_instant),max(created_instant) FROM payments").fetchone();curr=self.db.execute("SELECT currency,count(*) AS count,sum(amount_minor) AS total_minor FROM payments GROUP BY currency ORDER BY currency").fetchall()
  return {"count":count,"last_sync":self.last_sync(),"oldest":old,"newest":new,"by_currency":[dict(x) for x in curr]}
