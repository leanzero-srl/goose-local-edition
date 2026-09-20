"""HTTP planning checks: score optimal cost only after independently replaying every edge."""
import copy
import math
import gantry_oracle as geometry
import route_oracle

CHECKS=['plan_requires_load','plan_invalid_lattice','plan_stale_revision','plan_identity',
        'plan_detour_optimal','plan_lift_rotate_optimal','plan_energy_optimal','plan_rotation_clearance','plan_unreachable','plan_wrap_optimal']


def equivalent(a,b):
    if isinstance(a,dict) and isinstance(b,dict):
        return a.keys()==b.keys() and all(equivalent(a[k],b[k]) for k in a)
    if isinstance(a,list) and isinstance(b,list):
        return len(a)==len(b) and all(equivalent(x,y) for x,y in zip(a,b))
    if type(a) in (int,float) and type(b) in (int,float):
        return math.isfinite(b) and abs(a-b)<1e-6
    return a==b


def validate_route(scene,state,query,response,optimal):
    if not isinstance(response,dict) or type(response.get('revision')) is not int or response.get('revision')!=state['revision']:
        return False,'response revision differs'
    route=response.get('route')
    if not isinstance(route,list) or not route or not equivalent(route[0],state['pose']) or not equivalent(route[-1],query['goal']):
        return False,'route must start at current pose and end at goal'
    if any(not isinstance(pose,dict) or set(pose)!=set(state['pose']) or any(type(v) not in (int,float) or not math.isfinite(v) for v in pose.values()) for pose in route):
        return False,'invalid route pose'
    current=copy.deepcopy(state);cost=0
    for index,pose in enumerate(route[1:]):
        if not isinstance(pose,dict) or set(pose)!=set(current['pose']):return False,'invalid route pose'
        changed=[]
        for key,axis in query['lattice'].items():
            value=pose[key]
            if type(value) not in (int,float) or not math.isfinite(value) or value not in axis:return False,'pose outside lattice'
            if value!=current['pose'][key]:
                a,b=axis.index(value),axis.index(current['pose'][key])
                if abs(a-b)!=1 and not (key=='yaw' and {a,b}=={0,len(axis)-1}):return False,'non-adjacent transition'
                changed.append(key)
        if len(changed)!=1:return False,'transition must change exactly one axis'
        before=current['pose'];delta=(pose['yaw']-before['yaw']+180)%360-180
        if delta==-180:delta=180
        dy=pose['y']-before['y']
        cost+=1+abs(pose['x']-before['x'])+abs(pose['z']-before['z'])+3*max(dy,0)+max(-dy,0)+abs(delta)/90
        status,new=geometry.apply(scene,current,dict(id=f'grade-route-{index}',revision=current['revision'],op='move',**pose))
        if status!=200:return False,'unsafe swept transition: '+new['error']
        current=new
    claimed=response.get('cost')
    if type(claimed) not in (int,float) or not math.isfinite(claimed) or abs(claimed-cost)>1e-6:
        return False,'claimed cost differs from independently replayed route'
    return abs(cost-optimal)<1e-6,f'optimal cost={optimal:.6f}; replayed cost={cost:.6f}'


def gather(scene,request):
    state=geometry.initial(scene);rows=[];counter=0
    def row(name,score,detail):rows.append(dict(name=name,tier='F',score=float(score),detail=detail))
    def query(name,goal,lattice,revision=None):
        body=dict(revision=state['revision'] if revision is None else revision,goal=goal,lattice=lattice)
        expected=route_oracle.plan(scene,state,body)
        try:
            code,result=request('/api/plan',body)
            if expected[0]==200 and code==200:
                passed,detail=validate_route(scene,state,body,result,expected[1]['cost'])
            else:passed,detail=code==expected[0] and equivalent(result,expected[1]),f'expected HTTP {expected[0]} {expected[1].get("error", "route")}; got {code}'
            unchanged=equivalent(request('/api/state')[1],state)
            row(name,passed and unchanged,detail+('' if unchanged else '; planner mutated state'))
        except Exception as error:row(name,False,str(error))
    def move(op,**values):
        nonlocal state,counter
        counter+=1;body=dict(id=f'planner-setup-{counter}',revision=state['revision'],op=op,**values)
        expected=geometry.apply(scene,state,body);actual=request('/api/commands',body)
        if expected[0]!=200 or actual[0]!=200 or not equivalent(actual[1],expected[1]):
            raise ValueError('documented motion setup failed; planner scenario unavailable')
        state=expected[1]
    single=lambda pose:{key:[value] for key,value in pose.items()}
    query('plan_requires_load',state['pose'],single(state['pose']))
    try:
        box=scene['boxes'][0]
        move('move',x=box['x'],z=box['z'],y=box['y']+box['h'],yaw=0)
        move('grip',boxId=box['id'])
        pose=state['pose']
        invalid=single(pose);invalid['yaw']=[0,0]
        query('plan_invalid_lattice',pose,invalid)
        query('plan_stale_revision',pose,single(pose),revision=0)
        query('plan_identity',pose,single(pose))
        query('plan_detour_optimal',dict(x=8,z=4,y=0,yaw=0),dict(x=[2,4,6,8],z=[2,4,6,8],y=[0,2,4],yaw=[0,90]))
        query('plan_lift_rotate_optimal',dict(x=10,z=8,y=1,yaw=45),dict(x=[2,4,6,8,10],z=[2,4,6,8],y=[0,1,4],yaw=[0,45,90]))
        move('move',x=box['x'],z=box['z'],y=4,yaw=0)
        move('move',x=4,z=4,y=4,yaw=0)
        move('move',x=4,z=4,y=2,yaw=0)
        query('plan_energy_optimal',dict(x=8,z=4,y=2,yaw=0),dict(x=[4,8],z=[1,4],y=[2,3],yaw=[0]))
        query('plan_unreachable',dict(x=8,z=4,y=2,yaw=0),dict(x=[4,8],z=[4],y=[2],yaw=[0]))
        # At this side clearance yaw=0 fits, yaw=45 does not; endpoints are clear.
        rail_x=6.125+(box['w']/2+(box['w']+box['d'])/math.sqrt(8))/2
        move('move',x=4,z=4,y=4,yaw=0)
        move('move',x=rail_x,z=2,y=4,yaw=45)
        move('move',x=rail_x,z=2,y=2,yaw=45)
        query('plan_rotation_clearance',dict(x=rail_x,z=6,y=2,yaw=45),dict(x=[rail_x],z=[2,6],y=[2],yaw=[0,45]))
        move('move',x=box['x'],z=box['z'],y=4,yaw=170)
        query('plan_wrap_optimal',dict(x=box['x'],z=box['z'],y=4,yaw=-170),dict(x=[box['x']],z=[box['z']],y=[4],yaw=[-170,170]))
    except Exception as error:
        for name in CHECKS:
            if not any(r['name']==name for r in rows):row(name,False,str(error))
    return rows
