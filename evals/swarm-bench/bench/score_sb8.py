"""SB-8.0: compact transactional 3D benchmark; no partial-probe scores, no self-reports."""
from __future__ import annotations
import argparse, concurrent.futures, copy, json, math, os, secrets, shutil, socket, subprocess, sys, tempfile, time, urllib.error, urllib.request
from pathlib import Path
import gantry_oracle as oracle
import vendor_service_v4 as vendor
HERE=Path(__file__).resolve().parent
VERSION='sb-8.0-rc'
WEIGHTS={'A':.10,'B':.20,'C':.45,'D':.20,'E':.05}
CRITICAL={'durable_state','atomic_rejection','concurrent_revision','cargo_tracks_external_backend','crane_tracks_state','swept_collision','rotated_support_overhang','short_arc_wrap','sat_disjoint_aabbs_overlap'}

def _draw_seed(): return secrets.randbelow(2**31)
def _probe_preflight():
    try:
        p=subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE','node'),str(HERE/'product_probe_v4.mjs'),'--preflight'],capture_output=True,text=True,timeout=45)
        return None if p.returncode==0 and json.loads(p.stdout).get('ok') else p.stderr[-1500:]
    except Exception as e: return str(e)
def free_port():
    with socket.socket() as s: s.bind(('127.0.0.1',0));return s.getsockname()[1]
def request(base,path,body=None):
    raw=None if body is None else json.dumps(body).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request(base+path,data=raw,headers={'Content-Type':'application/json'}),timeout=5) as r: return r.status,json.load(r)
    except urllib.error.HTTPError as e:
        try: return e.code,json.load(e)
        except Exception: return e.code,{}
def equivalent(a,b):
    if isinstance(a,dict) and isinstance(b,dict): return a.keys()==b.keys() and all(equivalent(a[k],b[k]) for k in a)
    if isinstance(a,list) and isinstance(b,list): return len(a)==len(b) and all(equivalent(x,y) for x,y in zip(a,b))
    if type(a) in (int,float) and type(b) in (int,float): return math.isfinite(b) and abs(a-b)<1e-6
    return a==b

def stop(child):
    # Reap only descendants discovered from this exact parent; never a shared process group.
    lines=subprocess.check_output(['ps','-axo','pid=,ppid=,pgid='],text=True).splitlines();pairs=[tuple(map(int,l.split())) for l in lines if len(l.split())==3]
    ids={child.pid} | {pid for pid,ppid,pgid in pairs if pgid==child.pid}
    while True:
        added={pid for pid,ppid,pgid in pairs if ppid in ids}-ids
        if not added: break
        ids|=added
    for pid in sorted(ids,reverse=True):
        try: os.kill(pid,9)
        except ProcessLookupError: pass
    child.wait(timeout=5)

def gather(tree,port,db,trace,mark_phase=None,seed=None):
    if seed is None: raise ValueError('SB-8 requires the run fixture seed')
    why=_probe_preflight()
    if why: raise RuntimeError('REFUSED: browser probe unavailable: '+why)
    tree=Path(tree).resolve();db=Path(db).resolve();sc=oracle.scene(seed);state=oracle.initial(sc);rows=[];receipts={};counter=0
    app_port=free_port();base=f'http://127.0.0.1:{app_port}';log=open(tree/'sb8-app.log','w');child=None
    def check(name,tier,passed,detail=''): rows.append(dict(name=name,tier=tier,score=float(passed),detail=detail))
    def start(vendor_url):
        c=subprocess.Popen([sys.executable,'app.py','--port',str(app_port),'--db',str(db),'--vendor',vendor_url],cwd=tree,stdout=log,stderr=log,start_new_session=True)
        end=time.monotonic()+15
        while time.monotonic()<end:
            try:
                if request(base,'/api/state')[0]==200:return c
            except Exception:pass
            if c.poll() is not None:break
            time.sleep(.1)
        if c.poll() is None:stop(c)
        raise RuntimeError('app did not expose /api/state; see sb8-app.log')
    def command(name,op,values=None,tier='B',revision=None,ident=None):
        nonlocal state,counter
        counter+=1;cmd=dict(id=ident or f'probe-{counter}',revision=state['revision'] if revision is None else revision,op=op,**(values or {}))
        key=json.dumps(cmd,sort_keys=True)
        if cmd['id'] in receipts:
            prior=receipts[cmd['id']];expected=(200,prior[1]) if prior[0]==key else (409,{'error':'id_conflict'})
        else:expected=oracle.apply(sc,state,cmd)
        actual=request(base,'/api/commands',cmd)
        expected_state=expected[1] if expected[0]==200 and cmd['id'] not in receipts else state
        check(name,tier,actual[0]==expected[0] and equivalent(actual[1],expected[1]) and equivalent(request(base,'/api/state')[1],expected_state),f'expected HTTP {expected[0]}, got {actual[0]}')
        if expected[0]==200 and cmd['id'] not in receipts:
            state=copy.deepcopy(expected[1]);receipts[cmd['id']]=(key,copy.deepcopy(state))
        return cmd,actual
    try:
        child=start(f'http://127.0.0.1:{port}')
        check('boot_state','A',equivalent(request(base,'/api/state')[1],state))
        check('seeded_scene','A',equivalent(request(base,'/api/scene')[1],sc))
        b=sc['boxes'][0]
        command('empty_release','release')
        command('unknown_box','grip',{'boxId':'absent'})
        command('misaligned_grip','grip',{'boxId':b['id']})
        first,_=command('empty_move','move',dict(x=b['x'],z=b['z'],y=b['y']+b['h'],yaw=0),tier='A')
        command('grip','grip',{'boxId':b['id']})
        command('already_holding','grip',{'boxId':b['id']})
        before=request(base,'/api/state')[1]
        command('reject_bounds','move',dict(x=-1,z=2,y=1,yaw=0))
        check('atomic_rejection','B',equivalent(before,request(base,'/api/state')[1]))
        command('lift','move',dict(x=b['x'],z=b['z'],y=4,yaw=0),tier='A')
        command('sweep_stage','move',dict(x=4,z=4,y=4,yaw=0))
        command('sweep_lower','move',dict(x=4,z=4,y=0,yaw=0))
        command('swept_collision','move',dict(x=8,z=4,y=0,yaw=0))
        command('sweep_relift','move',dict(x=4,z=4,y=4,yaw=0))
        extent=(b['w']+b['d'])/math.sqrt(8)
        corner=dict(x=6.125+extent-.08,z=5+extent-.08,y=4,yaw=45)
        command('sat_stage','move',corner)
        command('sat_disjoint_aabbs_overlap','move',dict(corner,y=2))
        command('sat_relift','move',corner)
        edge=dict(x=11.5-extent+.1,z=8,y=4,yaw=45)
        command('overhang_stage','move',edge)
        command('overhang_lower','move',dict(edge,y=1))
        command('rotated_support_overhang','release')
        command('overhang_relift','move',edge)
        projected=b['w']/2*math.cos(math.radians(10))+b['d']/2*math.sin(math.radians(10))
        radius=math.hypot(b['w'],b['d'])/2
        arc=dict(x=6.125+(projected+radius)/2,z=4,y=4,yaw=170)
        command('short_arc_stage','move',arc)
        command('short_arc_lower','move',dict(arc,y=2))
        command('short_arc_wrap','move',dict(arc,y=2,yaw=-170))
        command('short_arc_relift','move',dict(arc,yaw=-170))
        command('probe_return','move',dict(x=b['x'],z=b['z'],y=4,yaw=0))
        command('unsupported_release','release')
        command('rotate','move',dict(x=b['x'],z=b['z'],y=4,yaw=45))
        command('held_bounds','move',dict(x=.1,z=.1,y=4,yaw=45))
        command('carry','move',dict(x=10,z=8,y=4,yaw=45))
        command('support_lower','move',dict(x=10,z=8,y=1,yaw=45))
        command('supported_release','release')
        command('align_rotated','move',dict(x=10,z=8,y=1+b['h'],yaw=45))
        command('regrip_preserves_yaw','grip',{'boxId':b['id']})
        command('relift','move',dict(x=10,z=8,y=4,yaw=45))
        command('short_arc','move',dict(x=10,z=8,y=4,yaw=170))
        command('return','move',dict(x=2,z=2,y=4,yaw=0))
        command('lower','move',dict(x=2,z=2,y=0,yaw=0))
        command('floor_release','release')
        heavy=sc['boxes'][-1]
        command('overload_align','move',dict(x=heavy['x'],z=heavy['z'],y=heavy['h'],yaw=0))
        command('overload','grip',{'boxId':heavy['id']})
        command('stale_revision','move',dict(x=5,z=5,y=5,yaw=0),revision=0)
        replay=request(base,'/api/commands',first)
        check('idempotent_replay','B',replay[0]==200 and equivalent(replay[1],receipts[first['id']][1]) and equivalent(request(base,'/api/state')[1],state))
        conflict=dict(first,x=first['x']+.2);check('id_conflict','B',request(base,'/api/commands',conflict)==(409,{'error':'id_conflict'}) and equivalent(request(base,'/api/state')[1],state))
        for value in [None,True,'1']:
            cmd=dict(id='malformed',revision=state['revision'],op='move',x=value,z=1,y=1,yaw=0)
            check('invalid_number_'+str(value),'B',request(base,'/api/commands',cmd)[0]==400)
        cmds=[dict(id=f'race-{i}',revision=state['revision'],op='move',x=4+i,z=5,y=6,yaw=0) for i in range(2)]
        with concurrent.futures.ThreadPoolExecutor(2) as pool: result=list(pool.map(lambda c:request(base,'/api/commands',c),cmds))
        winners=[i for i,r in enumerate(result) if r[0]==200]
        check('concurrent_revision','B',sorted(r[0] for r in result)==[200,409] and len(winners)==1 and equivalent(request(base,'/api/state')[1],oracle.apply(sc,state,cmds[winners[0]])[1]))
        if len(winners)==1:state=oracle.apply(sc,state,cmds[winners[0]])[1]
        before=request(base,'/api/state')[1];stop(child);child=None
        child=start('http://127.0.0.1:1')
        check('durable_state','A',equivalent(request(base,'/api/state')[1],before))
        check('durable_receipt','B',equivalent(request(base,'/api/commands',first)[1],receipts[first['id']][1]) and equivalent(request(base,'/api/state')[1],before))
        # Browser starts from fresh state so every visual scenario has known, exposed targets.
        stop(child);child=None
        visual=db.with_name(db.name+'-visual');shutil.rmtree(visual,ignore_errors=True);db=visual
        child=start(f'http://127.0.0.1:{port}')
        p=subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE','node'),str(HERE/'product_probe_v4.mjs'),'--base='+base],capture_output=True,text=True,timeout=90)
        if p.returncode: raise RuntimeError('REFUSED: browser probe crashed: '+p.stderr[-1500:])
        rows+=json.loads(p.stdout)['checks']
    except Exception as e:
        if str(e).startswith('REFUSED'):raise
        check('boot_state','A',False,str(e))
    finally:
        if child is not None:stop(child)
        log.close()
    return dict(checks=rows,fixture_seed=seed)

BACKEND={'A':['boot_state','seeded_scene','empty_move','lift','durable_state'], 'B':['empty_release','unknown_box','misaligned_grip','grip','already_holding','reject_bounds','atomic_rejection','swept_collision','sat_disjoint_aabbs_overlap','rotated_support_overhang','unsupported_release','held_bounds','supported_release','regrip_preserves_yaw','short_arc_wrap','floor_release','overload','stale_revision','idempotent_replay','id_conflict','invalid_number_None','invalid_number_True','invalid_number_1','concurrent_revision','durable_receipt'], 'C':['webgl_geometry','seeded_box_geometry','columns_rails','bridge_beams','trolley_spreader','four_cables','spreader','wheels','bracing','crane_tracks_state','cargo_tracks_external_backend'], 'D':['real_3d_pick','table_selection','ui_move_reaches_backend','visible_command_error','invalid_ui_move_is_atomic','revision_is_live','ui_release','ui_grip','orbit_changes_view','zoom_changes_view','camera_is_read_only'],'E':['clean_console']}
def evaluate(ctx):
    by={r['name']:r for r in ctx['checks']};rows=[by.get(n,dict(name=n,tier=t,score=0,detail='not reached')) for t,names in BACKEND.items() for n in names]
    if by.get('webgl_geometry', {}).get('score', 0) != 1:
        for row in rows:
            if row['tier'] == 'C' or row['name'] == 'real_3d_pick':
                row['score'] = 0
                row['detail'] = 'Actual scene canvas has no observed WebGL geometry'
    tiers={t:sum(r['score'] for r in rows if r['tier']==t)/len(names) for t,names in BACKEND.items()}
    inner=sum(WEIGHTS[t]*v for t,v in tiers.items());critical=math.prod(.6+.4*by.get(n,{'score':0})['score'] for n in CRITICAL)
    # Excellence is earned only while every core category works, never by a quiet empty page.
    excellence=min(tiers[t] for t in ['A','B','C','D'])*tiers['E']
    score=(inner-WEIGHTS['E']*tiers['E']+WEIGHTS['E']*excellence)*critical
    return dict(scorerVersion=VERSION,scorer_version=VERSION,score=score,scoreInner=inner,tiers=tiers,checks=rows,criticalMultiplier=critical,fixture_seed=ctx['fixture_seed'],calibrated=False)
def format_report(v,label=''):return f"{label} {VERSION}: {v['score']:.4f} (uncalibrated)"
def main():
    p=argparse.ArgumentParser();p.add_argument('--tree',type=Path,required=True);p.add_argument('--port',type=int,default=8899);p.add_argument('--seed',type=int,required=True);p.add_argument('--json-out',type=Path);p.add_argument('--reference',action='store_true');a=p.parse_args()
    why=_probe_preflight()
    if why: print('REFUSED: '+why,file=sys.stderr);return 3
    with tempfile.TemporaryDirectory(prefix='sb8-score-') as tmp:
        db=Path(tmp)/'db';trace=Path(tmp)/'trace.jsonl';server=vendor.serve(a.port,trace,a.seed)
        try:v=evaluate(gather(a.tree,a.port,db,trace,seed=a.seed))
        finally:server.shutdown();server.server_close()
    if a.json_out:a.json_out.write_text(json.dumps(v,indent=2))
    print(format_report(v));return 1 if a.reference and v['score']<.999999 else 0
if __name__=='__main__':raise SystemExit(main())
