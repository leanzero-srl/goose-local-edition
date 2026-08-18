import json,time,urllib.parse,email.utils,datetime,urllib.request,urllib.error,socket

class MeridianError(Exception):
 def __init__(self,status,code,body=None): self.status=status; self.code=code; self.body=body; super().__init__(code)
class ConflictError(MeridianError):
 def __init__(self): super().__init__(412,'conflict')

def _wait(v):
 try:return max(0,float(v))
 except: 
  try:return max(0,email.utils.parsedate_to_datetime(v).timestamp()-time.time())
  except:return 1
class MeridianClient:
 def __init__(self,base_url,api_key): self.base_url=base_url.rstrip('/'); self.api_key=api_key; self.etag=None; self._cache=None
 def _req(self,method,path,body=None,headers=None,retry=True):
  h={'Authorization':'Bearer '+self.api_key,'Accept':'application/json'}; h.update(headers or {})
  data=None if body is None else json.dumps(body).encode();
  if data:h['Content-Type']='application/json'
  req=urllib.request.Request(self.base_url+path,data=data,headers=h,method=method)
  try:
   with urllib.request.urlopen(req,timeout=10) as r:
    raw=r.read(); return r.status,dict(r.headers),json.loads(raw) if raw else None
  except urllib.error.HTTPError as e:
   raw=e.read(); obj=json.loads(raw) if raw else None
   if e.code==304: return 304,dict(e.headers),None
   if e.code==429: time.sleep(_wait(e.headers.get('Retry-After','1'))); return self._req(method,path,body,headers,retry)
   raise MeridianError(e.code,(obj or {}).get('error','vendor_error'),obj)
  except (socket.timeout,TimeoutError):
   if retry:return self._req(method,path,body,headers,False)
   raise MeridianError(599,'vendor_unavailable')
 def fetch_all_payments(self):
  if self.etag and self._cache is not None:
   try:
    s,h,o=self._req('GET','/v2/payments?limit=100',{'x':1} if False else None,{'If-None-Match':self.etag})
    if s==304:return list(self._cache)
   except MeridianError as e:
    if e.status!=304: raise
  out=[]; cursor=None
  while True:
   path='/v2/payments?limit=100'+(('&cursor='+urllib.parse.quote(cursor)) if cursor else '')
   try:s,h,o=self._req('GET',path)
   except MeridianError as e:
    if e.status==410: out=[]; cursor=None; continue
    raise
   if h.get('ETag'):self.etag=h['ETag']
   out.extend(o.get('data',[])); cursor=o.get('next_cursor')
   if cursor is None:break
  out.sort(key=lambda p: __import__('datetime').datetime.fromisoformat(p['created_at'].replace('Z','+00:00')).timestamp()); self._cache=list(out); return out
 def get_payment(self,payment_id): return self._req('GET','/v2/payments/'+urllib.parse.quote(payment_id))[2]
 def total_count(self): return len(self.fetch_all_payments())
 def create_payment(self,value_minor,currency,counterparty,occurred_at,idempotency_key):
  try:return self._req('POST','/v2/payments',{'amount':{'value_minor':value_minor,'currency':currency},'counterparty':counterparty,'occurred_at':occurred_at},{'Idempotency-Key':idempotency_key})[2]['id']
  except MeridianError as e:
   if e.status==409:return e.body['payment_id']
   raise
 def create_batch(self,items): return self._req('POST','/v2/payments/batch',{'items':items})[2]['results']
 def update_payment(self,payment_id,fields,version):
  for attempt in range(2):
   try:return self._req('PATCH','/v2/payments/'+urllib.parse.quote(payment_id),fields,{'If-Match':'"%s"'%version})[2]
   except MeridianError as e:
    if e.status==412 and attempt==0:
     fresh=self.get_payment(payment_id); version=fresh['version']; continue
    if e.status==412: raise ConflictError()
    raise
 def register_webhook(self,url):return self._req('POST','/v2/webhooks',{'url':url})[2]
