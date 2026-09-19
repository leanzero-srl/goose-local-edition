"""Contract oracle and deterministic fixtures for SB-8. The reference uses a copy; entrants implement the public contract."""
import copy
import math
import random

EPS = 1e-7

def scene(seed):
    r = random.Random(seed)
    colors = ['#22c55e', '#a855f7', '#f43f5e', '#3b82f6', '#eab308', '#14b8a6']
    boxes = [dict(id=f'cargo-{r.randrange(10000,99999)}', x=2+3*(i%3), z=2+4*(i//3),
                  y=0, w=round(r.uniform(.8,1.4),2), d=round(r.uniform(.8,1.4),2),
                  h=round(r.uniform(.8,1.8),2), mass=200+100*i, color=colors[i]) for i in range(6)]
    boxes[-1]['mass'] = 1500
    return dict(seed=seed, width=12, depth=10, height=8, capacity=1000,
                initial=dict(x=6,z=5,y=6,yaw=0), boxes=boxes,
                obstacles=[dict(id='pedestal',x=10,z=8,y=0,w=3,d=3,h=1),
                           dict(id='barrier',x=6,z=4,y=0,w=.25,d=2,h=3)])

def initial(scene):
    return dict(revision=0, pose=copy.deepcopy(scene['initial']), held=None,
                boxes=[dict(b,yaw=0) for b in scene['boxes']])

def corners(b):
    a=math.radians(b.get('yaw',0)); c,s=math.cos(a),math.sin(a)
    return [(b['x']+u*c+v*s,b['z']-u*s+v*c) for u,v in
            [(-b['w']/2,-b['d']/2),(b['w']/2,-b['d']/2),(b['w']/2,b['d']/2),(-b['w']/2,b['d']/2)]]

def overlaps(a,b):
    if min(a['y']+a['h'],b['y']+b['h'])-max(a['y'],b['y']) <= EPS: return False
    ac,bc=corners(a),corners(b)
    for pts in (ac,bc):
        for i in range(2):
            dx,dz=pts[i+1][0]-pts[i][0],pts[i+1][1]-pts[i][1]
            n=math.hypot(dx,dz); axis=(-dz/n,dx/n)
            aa=[x*axis[0]+z*axis[1] for x,z in ac]; bb=[x*axis[0]+z*axis[1] for x,z in bc]
            if min(max(aa),max(bb))-max(min(aa),min(bb)) <= EPS: return False
    return True

def contains(support,load):
    a=math.radians(support.get('yaw',0)); c,s=math.cos(a),math.sin(a)
    return all(abs((x-support['x'])*c-(z-support['z'])*s)<=support['w']/2+EPS and
               abs((x-support['x'])*s+(z-support['z'])*c)<=support['d']/2+EPS for x,z in corners(load))

def problem(sc,st,b):
    if b['y'] < -EPS or b['y']+b['h']>sc['height']+EPS or any(
        x < -EPS or x>sc['width']+EPS or z < -EPS or z>sc['depth']+EPS for x,z in corners(b)): return 'bounds'
    if any(overlaps(b,o) for o in st['boxes']+sc['obstacles'] if o['id']!=b['id']): return 'collision'
    return None

def valid(cmd):
    if not isinstance(cmd,dict) or not isinstance(cmd.get('id'),str) or not cmd['id'] or type(cmd.get('revision')) is not int: return False
    op=cmd.get('op')
    if op=='move': return all(type(cmd.get(k)) in (int,float) and math.isfinite(cmd[k]) for k in ('x','z','y','yaw'))
    if op=='grip': return isinstance(cmd.get('boxId'),str)
    return op=='release'

def apply(sc,state,cmd):
    if not valid(cmd): return 400,{'error':'invalid_command'}
    if cmd['revision']!=state['revision']: return 409,{'error':'stale_revision'}
    st=copy.deepcopy(state); p=st['pose']; op=cmd['op']
    held=next((b for b in st['boxes'] if b['id']==st['held']),None)
    error=None
    if op=='grip':
        b=next((b for b in st['boxes'] if b['id']==cmd['boxId']),None)
        if held: error='already_holding'
        elif b is None: error='unknown_box'
        elif b['mass']>sc['capacity']: error='overload'
        elif any(abs(p[k]-b[k])>.05+EPS for k in ('x','z')) or abs(p['y']-b['y']-b['h'])>.05+EPS: error='not_aligned'
        else: st['held']=b['id']; p['y']=b['y']; p['yaw']=b['yaw']; p['x']=b['x']; p['z']=b['z']
    elif op=='release':
        if held is None: error='not_holding'
        else:
            supports=[0] if abs(held['y'])<=.05+EPS else []
            supports += [o['y']+o['h'] for o in st['boxes']+sc['obstacles'] if o['id']!=held['id'] and abs(held['y']-o['y']-o['h'])<=.05+EPS and contains(o,held)]
            if not supports: error='unsupported'
            else:
                held['y']=max(supports); p['y']=held['y']; error=problem(sc,st,held)
                if not error: st['held']=None
    else:
        dest={k:cmd[k] for k in ('x','z','y','yaw')}
        if not (0<=dest['x']<=sc['width'] and 0<=dest['z']<=sc['depth'] and 0<=dest['y']<=sc['height'] and -180<=dest['yaw']<=180): error='bounds'
        elif held:
            delta=(dest['yaw']-p['yaw']+180)%360-180
            if delta==-180: delta=180
            steps=max(1,math.ceil(math.sqrt(sum((dest[k]-p[k])**2 for k in ('x','y','z')))/.1),math.ceil(abs(delta)/2))
            for n in range(steps+1):
                t=n/steps; b=dict(held,**{k:p[k]+(dest[k]-p[k])*t for k in ('x','y','z')},yaw=p['yaw']+delta*t)
                error=problem(sc,st,b)
                if error: break
            if not error: held.update(dest)
        if not error: st['pose']=dest
    if error: return 422,{'error':error}
    st['revision']+=1
    return 200,st
