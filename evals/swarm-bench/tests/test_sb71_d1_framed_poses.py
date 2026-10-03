"""The D1 stream arm's pose search, run on the probe's own pure geometry (node, no browser, no app).

Gauntlet 7.2 typesafe/jev-router (seed acafd048ad063ceb) was REFUSED 'fire_d1_mutation:failed' because
none of the 101 seeded poses had a pixel point for its D1 target pay_11795 on its 980x480 canvas, so the
arm never clicked. That is the harness's geometry, not the candidate: the same 0/101 holds at the
published 600x460 inspector minimum. The framed poses reach it."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest

BENCH = Path(__file__).resolve().parents[1] / 'bench'
sys.path.insert(0, str(BENCH))
import fixtures_v3  # noqa: E402
import score_sb7  # noqa: E402

SEED = 'acafd048ad063ceb'
SEARCH = r"""
import {readFileSync} from 'node:fs';
const [probe, packPath, target, W, H] = process.argv.slice(2);
const source = readFileSync(probe, 'utf8');
const start = source.indexOf('const V7 = {'), end = source.indexOf('// ── label-culling expectation');
if (start < 0 || end < start) throw new Error('pure geometry section not found in ' + probe);
globalThis.__BENCH_PROBE_TIER = 'sb-7.2';
const P = (0, eval)('(()=>{' + source.slice(start, end) +
  ';return {buildModel,poseCtx,findPixelWitnessFor,seededLatencyPoses,framedPosesFor};})()');
const pack = JSON.parse(readFileSync(packPath, 'utf8')), model = P.buildModel(pack), n = model.byId.get(target);
const points = poses => poses.map(pose => P.findPixelWitnessFor(P.poseCtx(model, ...pose, +W, +H), model, n));
const seeded = P.seededLatencyPoses(model, n, pack.seed), framed = P.framedPosesFor(model, n, +W, +H);
const seededPoints = points(seeded), framedPoints = points(framed.slice(0, seeded.length));
console.log(JSON.stringify({seeded: seeded.length, seededWithPoint: seededPoints.filter(Boolean).length,
  framedWithPoint: framedPoints.filter(Boolean).length, firstFramed: framedPoints.findIndex(Boolean),
  framedTwice: JSON.stringify(P.framedPosesFor(model, n, +W, +H)) === JSON.stringify(framed)}));
"""


@unittest.skipUnless(shutil.which('node'), 'node not installed')
class D1FramedPosesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        fx = fixtures_v3.build(SEED)
        cls.target = fx.d1_target['payment_id']
        cls.pack = Path(cls.tmp.name) / 'expect.json'
        score_sb7._write_expect_pack(SimpleNamespace(pack=fx, fixture_seed=SEED), cls.pack, [], cls.target)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def search(self, width, height):
        script = Path(self.tmp.name) / 'search.mjs'
        script.write_text(SEARCH)
        out = subprocess.run(['node', str(script), str(BENCH / 'product_probe_sb71.mjs'), str(self.pack),
                              self.target, str(width), str(height)], capture_output=True, text=True, timeout=120)
        self.assertEqual(out.returncode, 0, out.stderr[-2000:])
        return json.loads(out.stdout)

    def test_the_jev_target_has_no_seeded_point_for_any_renderer_and_a_framed_one(self):
        self.assertEqual(self.target, 'pay_11795')
        for width, height in ((980, 480), (600, 460)):
            with self.subTest(canvas=(width, height)):
                found = self.search(width, height)
                self.assertEqual((found['seeded'], found['seededWithPoint']), (101, 0),
                                 'the control: the seeded search never reaches a click')
                self.assertGreater(found['framedWithPoint'], 0)
                self.assertGreaterEqual(found['firstFramed'], 0)
                self.assertTrue(found['framedTwice'], 'the framed order is deterministic')


if __name__ == '__main__':
    unittest.main()
