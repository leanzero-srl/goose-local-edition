"""Geometry counterexamples and scorer fail-closed controls, with no model calls."""
import copy
import math
import unittest
import gantry_oracle as oracle
import score_sb8 as scorer


class GeometryTests(unittest.TestCase):
    def test_seeded_sat_sweep_support_and_short_arc_counterexamples(self):
        for seed in range(100):
            sc=oracle.scene(seed); st=oracle.initial(sc); b=st['boxes'][0]
            at=lambda **pose:dict(b,**pose)
            self.assertIsNone(oracle.problem(sc,st,at(x=4,z=4,y=0)))
            self.assertIsNone(oracle.problem(sc,st,at(x=8,z=4,y=0)))
            self.assertEqual(oracle.problem(sc,st,at(x=6,z=4,y=0)),'collision')
            extent=(b['w']+b['d'])/math.sqrt(8)
            diagonal=at(x=6.125+extent-.08,z=5+extent-.08,y=2,yaw=45)
            self.assertIsNone(oracle.problem(sc,st,diagonal))
            self.assertFalse(oracle.contains(sc['obstacles'][0],at(x=11.5-extent+.1,z=8,y=1,yaw=45)))
            projected=b['w']/2*math.cos(math.radians(10))+b['d']/2*math.sin(math.radians(10))
            x=6.125+(projected+math.hypot(b['w'],b['d'])/2)/2
            self.assertTrue(all(oracle.problem(sc,st,at(x=x,z=4,y=2,yaw=a)) is None for a in range(170,191,2)))
            self.assertTrue(any(oracle.problem(sc,st,at(x=x,z=4,y=2,yaw=a))=='collision' for a in range(-170,171,2)))


class ScoringTests(unittest.TestCase):
    def context(self):
        return {'fixture_seed':7123,'checks':[dict(name=n,tier=t,score=1) for t,names in scorer.BACKEND.items() for n in names]}

    def test_missing_checks_never_shrink_denominator(self):
        self.assertEqual(scorer.evaluate({'fixture_seed':1,'checks':[]})['score'],0)

    def test_no_webgl_cannot_earn_any_visual_points(self):
        ctx=self.context()
        next(r for r in ctx['checks'] if r['name']=='webgl_geometry')['score']=0
        result=scorer.evaluate(ctx)
        self.assertEqual(result['tiers']['C'],0)
        self.assertLess(result['score'],.5)

    def test_static_scene_and_unsafe_motion_have_meaningful_penalties(self):
        for names,ceiling in [(['crane_tracks_state','cargo_tracks_external_backend'],.4),(['swept_collision'],.6),(['rotated_support_overhang'],.6)]:
            ctx=self.context()
            for row in ctx['checks']:
                if row['name'] in names:row['score']=0
            self.assertLess(scorer.evaluate(ctx)['score'],ceiling)


if __name__=='__main__':unittest.main()
