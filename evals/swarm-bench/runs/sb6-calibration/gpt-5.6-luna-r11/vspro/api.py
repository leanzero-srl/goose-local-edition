import json,threading,hmac,hashlib,time,datetime,os,urllib.parse
from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
from .meridian import MeridianError,ConflictError
STAT=['settled','pending','refunded','failed']; CUR=['EUR','USD','JPY','KWD']; SORT=['created_at','-created_at','amount_minor','-amount_minor']
def envelope(code,msg,fields=None):
 e={'error':{'code':code,'message':msg}}
 if fields:e['error']['field_errors']=fields
 return e
def serve(port,store,client):
 state={'secret':None,'registered':False,'received':0,'applied':0,'ignored':0,'rejected':0}; slock=threading.Lock()
 class H(BaseHTTPRequestHandler):
  def log_message(self,*a):pass
  def sendj(self,code,obj):
   b=json.dumps(obj).encode();self.send_response(code);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
  def body(self):
   try:return json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))))
   except:return None
  def do_GET(self):
   path=urllib.parse.urlparse(self.path); q=urllib.parse.parse_qs(path.query)
   if path.path=='/':return self.asset('index.html','text/html')
   if path.path.startswith('/web/'):
    typ={'css':'text/css','js':'application/javascript'}.get(path.path.rsplit('.',1)[-1],'text/plain');return self.asset(path.path[5:],typ)
   try:
    if path.path=='/api/health':
     with slock:c=dict(state)
     c.pop('secret');c['webhook']={'registered':state['registered'],'received':c['received'],'applied':c['applied'],'ignored':c['ignored'],'rejected':c['rejected']};return self.sendj(200,{'status':'ok','payments':store.count(),'last_sync':store.last_sync(),'webhook':c['webhook']})
    if path.path=='/api/payments':
     def integer(k,d):
      v=q.get(k,[str(d)])[0]
      if not v.isdigit():raise ValueError(k)
      return int(v)
     lim=min(integer('limit',50),200); off=integer('offset',0); st=q.get('status',[None])[0];cu=q.get('currency',[None])[0];so=q.get('sort',['created_at'])[0]
     if st and st not in STAT:raise ValueError('status')
     if cu and cu not in CUR:raise ValueError('currency')
     if so not in SORT:raise ValueError('sort')
     rows,total=store.query(lim,off,st,cu,so);return self.sendj(200,{'data':rows,'total':total,'limit':lim,'offset':off})
    if path.path=='/api/summary':
     rows,_=store.query(100000,0); by=[]
     for c in sorted(set(x['currency'] for x in rows)):by.append({'currency':c,'count':sum(x['currency']==c for x in rows),'total_minor':sum(x['amount_minor'] for x in rows if x['currency']==c)})
     dates=sorted([datetime.datetime.fromisoformat(x['created_at'].replace('Z','+00:00')).astimezone(datetime.timezone.utc).isoformat().replace('+00:00','Z') for x in rows]);return self.sendj(200,{'count':len(rows),'last_sync':store.last_sync(),'oldest':dates[0] if dates else None,'newest':dates[-1] if dates else None,'by_currency':by})
    if path.path=='/api/buckets':return self.sendj(200,{'timezone':'Europe/Berlin','days':sorted(set(x['day'] for x in store.buckets())),'statuses':STAT,'cells':store.buckets()})
    if path.path.startswith('/api/payments/'):
     p=store.get(path.path.rsplit('/',1)[1]);return self.sendj(200,p if p else envelope('not_found','Payment not found')) if p else self.sendj(404,envelope('not_found','Payment not found'))
    return self.sendj(404,envelope('not_found','Path not found'))
   except ValueError as e:return self.sendj(400,envelope('bad_request','Invalid request',[{'path':str(e),'code':'bad_format'}]))
   except Exception as e:return self.sendj(500,envelope('vendor_unavailable',str(e)))
  def asset(self,n,typ):
   try:b=open(os.path.join(os.path.dirname(__file__),'web',n),'rb').read();self.send_response(200);self.send_header('Content-Type',typ);self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
   except: self.sendj(404,envelope('not_found','Asset not found'))
  def do_POST(self):
   path=urllib.parse.urlparse(self.path).path
   if path=='/api/webhooks/meridian':return self.webhook()
   try:
    if path=='/api/sync':
     result={'fetched':0,'inserted':0,'updated':0}
     def go():
      try:
       ps=client.fetch_all_payments(); result['fetched']=len(ps); result['inserted'],result['updated']=store.upsert_many(ps); store.set_last_sync(datetime.datetime.now(datetime.timezone.utc).isoformat().replace('+00:00','Z'))
      except Exception: pass
     t=threading.Thread(target=go,daemon=True);t.start();t.join();result['total']=store.count();return self.sendj(200,result)
    if path=='/api/payments/batch':return self.batch()
    if path.startswith('/api/payments/') and path.endswith('/note'):
     pid=path.split('/')[-2];b=self.body();note=b.get('note') if isinstance(b,dict) else None
     if not isinstance(note,str) or not 1<=len(note)<=280:return self.sendj(400,envelope('bad_request','Invalid note',[{'path':'note','code':'bad_format'}]))
     p=store.get(pid)
     if not p:return self.sendj(404,envelope('not_found','Payment not found'))
     try:r=client.update_payment(pid,{'note':note},p['version'])
     except ConflictError:return self.sendj(409,envelope('conflict','Payment was changed by another user'))
     store.upsert_many([r]);return self.sendj(200,{'id':r['id'],'note':r.get('note',''),'version':r['version']})
    return self.sendj(404,envelope('not_found','Path not found'))
   except MeridianError as e:return self.sendj(503,envelope('vendor_unavailable',str(e)))
   except Exception as e:return self.sendj(400,envelope('bad_request',str(e)))
  def batch(self):
   b=self.body();items=b.get('items') if isinstance(b,dict) else None;fe=[]
   if not isinstance(items,list) or not 1<=len(items)<=20:return self.sendj(400,envelope('bad_request','Invalid items',[{'path':'items','code':'bad_format'}]))
   for i,x in enumerate(items):
    if not isinstance(x,dict):fe.append({'path':f'items[{i}]','code':'bad_format'});continue
    a=x.get('amount',{});cp=x.get('counterparty',{})
    if not isinstance(a.get('value_minor'),int):fe.append({'path':f'items[{i}].amount.value_minor','code':'not_an_integer'})
    elif a['value_minor']<=0:fe.append({'path':f'items[{i}].amount.value_minor','code':'not_positive'})
    if a.get('currency') not in CUR:fe.append({'path':f'items[{i}].amount.currency','code':'unsupported'})
    if not isinstance(cp.get('name'),str) or not 1<=len(cp.get('name',''))<=80:fe.append({'path':f'items[{i}].counterparty.name','code':'too_long'})
    if not isinstance(cp.get('country'),str) or len(cp.get('country',''))!=2 or not cp['country'].isupper():fe.append({'path':f'items[{i}].counterparty.country','code':'bad_format'})
    if not x.get('idempotency_key'):fe.append({'path':f'items[{i}].idempotency_key','code':'required'})
   if fe:return self.sendj(400,envelope('bad_request','Validation failed',fe))
   r=client.create_batch(items);return self.sendj(200,{'results':r,'succeeded':sum(x.get('status')=='created' for x in r),'failed':sum(x.get('status')=='error' for x in r)})
  def webhook(self):
   raw=self.rfile.read(int(self.headers.get('Content-Length','0'))); obj=json.loads(raw or b'{}')
   if obj.get('type')=='webhook.verify':return self.sendj(200,{'challenge':obj.get('challenge')})
   with slock:state['received']+=1;secret=state['secret']
   sig=self.headers.get('Meridian-Signature','');ok=False
   try:
    parts=dict(x.split('=',1) for x in sig.split(','));want=hmac.new(secret.encode(),(parts['t']+'.').encode()+raw,hashlib.sha256).hexdigest();ok=hmac.compare_digest(want,parts['v1'])
   except:pass
   if not ok:
    with slock:state['rejected']+=1
    return self.sendj(401,envelope('bad_signature','Invalid webhook signature'))
   result=store.apply_event(obj)
   with slock:state['applied' if result=='applied' else 'ignored']+=1
   return self.sendj(200,{'received':True})
 server=ThreadingHTTPServer(('127.0.0.1',port),H)
 def reg():
  try:
   r=client.register_webhook(f'http://127.0.0.1:{port}/api/webhooks/meridian');state['secret']=r['secret'];state['registered']=True
  except Exception:pass
 threading.Thread(target=reg,daemon=True).start(); server.serve_forever()
