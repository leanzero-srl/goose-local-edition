import copy
from contextlib import closing
import math
import json
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.request
import unittest
from pathlib import Path

from gantry_oracle import apply
from route_oracle import AXES, edge_cost, plan, shortest_yaw_delta


def fixture(width=.5, depth=.5, yaw=0, obstacles=()):
    pose = dict(x=2, z=5, y=0, yaw=yaw)
    cargo = dict(id='load', **pose, w=width, d=depth, h=1, mass=100)
    scene = dict(width=12, depth=10, height=8, capacity=1000, obstacles=list(obstacles))
    state = dict(revision=7, pose=pose, held='load', boxes=[cargo])
    return scene, state


def request(state, goal, **lattice):
    return dict(revision=state['revision'], goal=goal, lattice=lattice)


class RouteTests(unittest.TestCase):
    def assert_route(self, scene, state, body, expected_cost):
        before = copy.deepcopy(state)
        status, result = plan(scene, state, body)
        self.assertEqual((status, result.get('cost')), (200, expected_cost), result)
        self.assertEqual(state, before)
        self.assertEqual(result['route'][0], state['pose'])
        self.assertEqual(result['route'][-1], body['goal'])
        self.assertEqual(result['revision'], state['revision'])
        walked, total = copy.deepcopy(state), 0
        for source, dest in zip(result['route'], result['route'][1:]):
            changed = [k for k in AXES if source[k] != dest[k]]
            self.assertEqual(len(changed), 1)
            key = changed[0]
            axis = body['lattice'][key]
            difference = abs(axis.index(source[key]) - axis.index(dest[key]))
            self.assertTrue(difference == 1 or key == 'yaw' and difference == len(axis)-1)
            status, walked = apply(scene, walked, dict(id=str(total), revision=walked['revision'], op='move', **dest))
            self.assertEqual(status, 200)
            total += edge_cost(source, dest)
        self.assertEqual(total, expected_cost)
        return result

    def test_rotated_clearance_is_required(self):
        obstacles = [dict(id='south', x=5, z=2, y=0, w=.2, d=4, h=8),
                     dict(id='north', x=5, z=8, y=0, w=.2, d=4, h=8)]
        scene, state = fixture(width=3, depth=.5, yaw=90, obstacles=obstacles)
        goal = dict(x=8, z=5, y=0, yaw=90)
        body = request(state, goal, x=[2,8], z=[5], y=[0], yaw=[0,90])
        self.assertEqual(apply(scene, state, dict(id='straight', revision=7, op='move', **goal)),
                         (422, {'error':'collision'}))
        result = self.assert_route(scene, state, body, 11)
        self.assertEqual([p['yaw'] for p in result['route']], [90,0,0,90])
        body['lattice']['yaw'] = [90]
        self.assertEqual(plan(scene,state,body), (422,{'error':'unreachable'}))

    def test_detour_beats_climb_with_equal_edge_count(self):
        scene, state = fixture(obstacles=[dict(id='barrier',x=5,z=5,y=0,w=.2,d=2,h=3)])
        goal = dict(x=8,z=5,y=0,yaw=0)
        body = request(state,goal,x=[2,8],z=[2,5],y=[0,3],yaw=[0])
        result = self.assert_route(scene,state,body,15)
        self.assertTrue(all(p['y']==0 for p in result['route']))
        body['lattice']['z'] = [5]
        self.assert_route(scene,state,body,21)

    def test_full_wall_unreachable(self):
        scene,state = fixture(obstacles=[dict(id='wall',x=5,z=5,y=0,w=.2,d=10,h=8)])
        body=request(state,dict(x=8,z=5,y=0,yaw=0),x=[2,8],z=[2,5,8],y=[0,3],yaw=[0,90])
        self.assertEqual(plan(scene,state,body),(422,{'error':'unreachable'}))

    def test_yaw_wrap_and_half_turn(self):
        scene,state=fixture(yaw=170)
        body=request(state,dict(state['pose'],yaw=-170),x=[2],z=[5],y=[0],yaw=[-170,0,170])
        self.assert_route(scene,state,body,1+20/90)
        self.assertEqual(shortest_yaw_delta(0,-180),180)
        self.assertEqual(shortest_yaw_delta(170,-170),20)

    def test_zero_cost_and_no_mutation(self):
        scene,state=fixture()
        body=request(state,dict(state['pose']),**{k:[state['pose'][k]] for k in AXES})
        result=self.assert_route(scene,state,body,0)
        self.assertEqual(result['route'],[state['pose']])

    def test_validation_and_error_priority(self):
        scene,state=fixture()
        body=request(state,dict(state['pose']),**{k:[state['pose'][k]] for k in AXES})
        invalid=[None,{},dict(body,revision=True),dict(body,goal=dict(body['goal'],x=True))]
        for values in ([],[2,2],[3,2],[True],[math.inf],[math.nan],[1,3]):
            invalid.append(dict(body,lattice=dict(body['lattice'],x=values)))
        for value in invalid:
            with self.subTest(value=value):
                self.assertEqual(plan(scene,state,value),(400,{'error':'invalid_plan'}))
        state['held']=None
        self.assertEqual(plan(scene,state,dict(body,revision=6)),(409,{'error':'stale_revision'}))
        self.assertEqual(plan(scene,state,body),(422,{'error':'not_holding'}))

    def test_http_plan_preserves_state_and_receipts(self):
        scene,state=fixture()
        body=request(state,dict(state['pose'],x=8),x=[2,8],z=[5],y=[0],yaw=[0])
        with tempfile.TemporaryDirectory(prefix='sb8-plan-') as temporary:
            root=Path(temporary)
            tree=root/'app'
            shutil.copytree(Path(__file__).parent/'golden-sb8',tree)
            (tree/'three.js').write_text('')
            database=root/'database'
            database.mkdir()
            with closing(sqlite3.connect(database/'state.sqlite')) as con, con:
                con.execute('create table state (id integer primary key, scene text, state text)')
                con.execute('create table receipts (id text primary key, body text, response text)')
                con.execute('insert into state values (1,?,?)',(json.dumps(scene),json.dumps(state)))
                con.execute('insert into receipts values (?,?,?)',('existing','{}',json.dumps(state)))
                before=list(con.iterdump())
            with socket.socket() as sock:
                sock.bind(('127.0.0.1',0))
                port=sock.getsockname()[1]
            proc=subprocess.Popen([sys.executable,str(tree/'app.py'),'--port',str(port),
                                   '--db',str(database),'--vendor','http://127.0.0.1:1'],
                                  stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
            try:
                deadline=time.monotonic()+5
                while True:
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state',timeout=1) as response:
                            self.assertEqual(json.load(response),state)
                        break
                    except OSError:
                        if proc.poll() is not None or time.monotonic()>deadline:
                            self.fail('Reference app did not start')
                        time.sleep(.02)
                req=urllib.request.Request(f'http://127.0.0.1:{port}/api/plan',
                    data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
                with urllib.request.urlopen(req,timeout=2) as response:
                    result=json.load(response)
                self.assertEqual(result,dict(revision=7,cost=7,route=[state['pose'],body['goal']]))
                with closing(sqlite3.connect(database/'state.sqlite')) as con, con:
                    self.assertEqual(list(con.iterdump()),before)
            finally:
                proc.terminate()
                proc.communicate(timeout=5)

    def test_reference_uses_identical_oracle(self):
        root=Path(__file__).parent
        self.assertEqual((root/'route_oracle.py').read_bytes(),(root/'golden-sb8/route_oracle.py').read_bytes())


if __name__=='__main__':
    unittest.main()
