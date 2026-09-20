import copy
import unittest
from route_oracle import plan
from score_routes import validate_route
from test_sb8_routes import fixture, request


class RouteGradingTests(unittest.TestCase):
    def test_claimed_optimum_cannot_hide_unsafe_or_nonadjacent_route(self):
        scene,state=fixture(obstacles=[dict(id='wall',x=5,z=5,y=0,w=.2,d=2,h=3)])
        goal=dict(x=8,z=5,y=0,yaw=0)
        body=request(state,goal,x=[2,4,8],z=[2,5],y=[0,3],yaw=[0])
        code,answer=plan(scene,state,body)
        self.assertEqual(code,200)
        self.assertTrue(validate_route(scene,state,body,answer,answer['cost'])[0])
        forged=dict(answer,route=[state['pose'],goal])
        self.assertFalse(validate_route(scene,state,body,forged,answer['cost'])[0])
        unsafe=dict(answer,route=[state['pose'],dict(state['pose'],x=4),goal])
        self.assertFalse(validate_route(scene,state,body,unsafe,answer['cost'])[0])
        self.assertFalse(validate_route(scene,state,body,dict(answer,cost=0),answer['cost'])[0])
        self.assertFalse(validate_route(scene,state,body,answer,answer['cost']-1)[0])

    def test_feasible_expensive_route_does_not_earn_optimality(self):
        scene,state=fixture(obstacles=[dict(id='wall',x=5,z=5,y=0,w=.2,d=2,h=3)])
        goal=dict(x=8,z=5,y=0,yaw=0)
        body=request(state,goal,x=[2,8],z=[2,5],y=[0,3],yaw=[0])
        _,best=plan(scene,state,body)
        restricted=copy.deepcopy(body);restricted['lattice']['z']=[5]
        _,climb=plan(scene,state,restricted)
        self.assertEqual((best['cost'],climb['cost']),(15,21))
        self.assertFalse(validate_route(scene,state,body,climb,best['cost'])[0])


class LiveScenarioMutationTests(unittest.TestCase):
    def collect(self,seed,mutant=None):
        import gantry_oracle as geometry
        import route_oracle
        import score_routes
        from unittest.mock import patch
        scene=geometry.scene(seed);state=geometry.initial(scene)
        real_cost=route_oracle.edge_cost
        def request_api(path,body=None):
            nonlocal state
            if path=='/api/state':return 200,copy.deepcopy(state)
            if path=='/api/commands':
                status,result=geometry.apply(scene,state,body)
                if status==200:state=result
                return status,result
            if mutant=='fewest_edges':
                with patch.object(route_oracle,'edge_cost',return_value=1):status,result=plan(scene,state,body)
                if status==200:
                    result['cost']=sum(real_cost(a,b) for a,b in zip(result['route'],result['route'][1:]))
                return status,result
            if mutant=='no_rotations' and body['goal']['yaw']==state['pose']['yaw']:
                body=copy.deepcopy(body);body['lattice']['yaw']=[state['pose']['yaw']]
            return plan(scene,state,body)
        return {row['name']:row for row in score_routes.gather(scene,request_api)}

    def test_actual_http_scenarios_discriminate_planner_mutations(self):
        for seed in (0,1,2,3,4,7123,90067452):
            with self.subTest(seed=seed):
                healthy=self.collect(seed)
                self.assertTrue(all(row['score']==1 for row in healthy.values()),healthy)
                shortest=self.collect(seed,'fewest_edges')
                self.assertEqual(shortest['plan_energy_optimal']['score'],0,shortest)
                fixed_yaw=self.collect(seed,'no_rotations')
                self.assertEqual(fixed_yaw['plan_rotation_clearance']['score'],0,fixed_yaw)

if __name__=='__main__':unittest.main()
