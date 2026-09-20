"""Reference implementation, kept separate from the candidate tree used by the scorer."""
import argparse, json, sqlite3, threading, urllib.request
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from gantry_oracle import initial, apply, valid
from route_oracle import plan

p=argparse.ArgumentParser();p.add_argument('--port',type=int,required=True);p.add_argument('--db',required=True);p.add_argument('--vendor',required=True);args=p.parse_args()
root=Path(args.db);root.mkdir(parents=True,exist_ok=True)
con=sqlite3.connect(root/'state.sqlite',check_same_thread=False)
con.execute('create table if not exists state (id integer primary key, scene text, state text)')
con.execute('create table if not exists receipts (id text primary key, body text, response text)')
if con.execute('select count(*) from state').fetchone()[0]==0:
    scene=json.load(urllib.request.urlopen(args.vendor+'/scene'))
    con.execute('insert into state values (1,?,?)',(json.dumps(scene),json.dumps(initial(scene))));con.commit()
else: scene=json.loads(con.execute('select scene from state').fetchone()[0])
asset=Path(__file__).with_name('three.js')
if not asset.exists(): asset.write_bytes(urllib.request.urlopen(args.vendor+'/three.js').read())
lock=threading.Lock()
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def answer(self,status,data,kind='application/json'):
        if kind=='application/json': data=json.dumps(data).encode()
        self.send_response(status);self.send_header('Content-Type',kind);self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(data)
    def do_GET(self):
        if self.path=='/api/scene': self.answer(200,scene)
        elif self.path=='/api/state':
            with lock: value=json.loads(con.execute('select state from state').fetchone()[0])
            self.answer(200,value)
        elif self.path=='/': self.answer(200,Path(__file__).with_name('index.html').read_bytes(),'text/html')
        elif self.path=='/three.js': self.answer(200,asset.read_bytes(),'application/javascript')
        else: self.send_error(404)
    def do_POST(self):
        if self.path not in ('/api/commands','/api/plan'): self.send_error(404);return
        try: cmd=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))))
        except (ValueError,TypeError):
            self.answer(400,{'error':'invalid_plan' if self.path=='/api/plan' else 'invalid_command'});return
        if self.path=='/api/plan':
            with lock:
                state=json.loads(con.execute('select state from state').fetchone()[0])
                status,value=plan(scene,state,cmd)
            self.answer(status,value);return
        if not valid(cmd): self.answer(400,{'error':'invalid_command'});return
        body=json.dumps(cmd,sort_keys=True,separators=(',',':'))
        with lock:
            prior=con.execute('select body,response from receipts where id=?',(cmd['id'],)).fetchone()
            if prior:
                status,value=(200,json.loads(prior[1])) if prior[0]==body else (409,{'error':'id_conflict'})
            else:
                state=json.loads(con.execute('select state from state').fetchone()[0]);status,value=apply(scene,state,cmd)
                if status==200:
                    with con:
                        con.execute('update state set state=?',(json.dumps(value),))
                        con.execute('insert into receipts values (?,?,?)',(cmd['id'],body,json.dumps(value)))
        self.answer(status,value)
ThreadingHTTPServer(('127.0.0.1',args.port),Handler).serve_forever()
