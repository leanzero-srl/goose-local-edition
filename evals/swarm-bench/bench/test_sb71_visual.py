"""Serial real-browser controls for SB7.1, enabled with SB71_BROWSER_TESTS=1.

Supply GOOSE_SWARM_PLAYWRIGHT_MODULE and GOOSE_SWARM_CHROMIUM_EXECUTABLE when
using a bundled runtime. These are local reference tests, never model runs.
"""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import threading
import unittest
import urllib.request

import fixtures_v3
import vendor_service_v3


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def fetch(url):
    with urllib.request.urlopen(url, timeout=2) as response:
        return json.load(response)


class PixelAndVersionOracleControls(unittest.TestCase):
    def test_late_read_uses_actual_render_clock_without_inventing_distinct_frames(self):
        bench = Path(__file__).resolve().parent
        prefix = (bench / 'product_probe_sb71.mjs').read_text().split('// ── selfcheck:')[0]
        # Primary golden replay: delayed screenshot/read tasks, ten correct samples,
        # seven distinct actual draws. Keep exact measured timestamps and pixel counts.
        frames = [{'elapsed': 122.20000004768372, 'renderElapsed': 48.09999990463257, 'compared': 4156, 'matched': 4156, 'witnesses': 272, 'witnessMatches': 272, 'positiveWitnesses': 105, 'positiveMatches': 105, 'cameraFixed': True}, {'elapsed': 302.59999990463257, 'renderElapsed': 301.7000000476837, 'compared': 4037, 'matched': 4037, 'witnesses': 221, 'witnessMatches': 221, 'positiveWitnesses': 54, 'positiveMatches': 54, 'cameraFixed': True}, {'elapsed': 374.2000000476837, 'renderElapsed': 301.7000000476837, 'compared': 4037, 'matched': 4037, 'witnesses': 221, 'witnessMatches': 221, 'positiveWitnesses': 54, 'positiveMatches': 54, 'cameraFixed': True}, {'elapsed': 386.89999985694885, 'renderElapsed': 386, 'compared': 4030, 'matched': 4030, 'witnesses': 218, 'witnessMatches': 218, 'positiveWitnesses': 51, 'positiveMatches': 51, 'cameraFixed': True}, {'elapsed': 451.2999999523163, 'renderElapsed': 450.59999990463257, 'compared': 4027, 'matched': 4027, 'witnesses': 215, 'witnessMatches': 215, 'positiveWitnesses': 48, 'positiveMatches': 48, 'cameraFixed': True}, {'elapsed': 523.8999998569489, 'renderElapsed': 523.0999999046326, 'compared': 4018, 'matched': 4018, 'witnesses': 214, 'witnessMatches': 214, 'positiveWitnesses': 47, 'positiveMatches': 47, 'cameraFixed': True}, {'elapsed': 601.2999999523163, 'renderElapsed': 598.0999999046326, 'compared': 4021, 'matched': 4021, 'witnesses': 214, 'witnessMatches': 214, 'positiveWitnesses': 47, 'positiveMatches': 47, 'cameraFixed': True}, {'elapsed': 862.8999998569489, 'renderElapsed': 862.2000000476837, 'compared': 4075, 'matched': 4075, 'witnesses': 88, 'witnessMatches': 88, 'positiveWitnesses': 31, 'positiveMatches': 31, 'cameraFixed': True}, {'elapsed': 947.5999999046326, 'renderElapsed': 862.2000000476837, 'compared': 4075, 'matched': 4075, 'witnesses': 88, 'witnessMatches': 88, 'positiveWitnesses': 31, 'positiveMatches': 31, 'cameraFixed': True}, {'elapsed': 959.7999999523163, 'renderElapsed': 862.2000000476837, 'compared': 4075, 'matched': 4075, 'witnesses': 88, 'witnessMatches': 88, 'positiveWitnesses': 31, 'positiveMatches': 31, 'cameraFixed': True}, {'elapsed': 1152.7999999523163, 'renderElapsed': 1031.7999999523163, 'compared': 4162, 'matched': 4162, 'witnesses': 0, 'witnessMatches': 0, 'positiveWitnesses': 0, 'positiveMatches': 0, 'cameraFixed': True}]
        assertions = r"""
const require=(condition,message)=>{if(!condition)throw Error(message);};
const summary=summarizeAnimationFrames(FRAMES);
require(summary.ok&&summary.eligibleMotionFrames===10&&summary.distinctMovingDraws===7,'Late read incorrectly rejected or unique draws invented');
const afterWindow=structuredClone(FRAMES);afterWindow[8].renderElapsed=901;afterWindow[9].renderElapsed=901;
require(!summarizeAnimationFrames(afterWindow).ok,'Post-window rendered frames counted');
const missingPositive=structuredClone(FRAMES);for(const f of missingPositive)f.positiveMatches=0;
require(!summarizeAnimationFrames(missingPositive).ok,'Missing occupied collar accepted');
const wrongLatePixels=structuredClone(FRAMES);wrongLatePixels[9].matched=0;
require(!summarizeAnimationFrames(wrongLatePixels).ok,'Late-read geometry unchecked');
const frozenRest=structuredClone(FRAMES);frozenRest[10].renderElapsed=862.2;
require(!summarizeAnimationFrames(frozenRest).ok,'Frozen midflight image accepted as settled');
console.log(JSON.stringify(summary));
"""
        with tempfile.TemporaryDirectory(prefix='sb71-render-clock-control-') as directory:
            script = Path(directory) / 'control.mjs'
            script.write_text(prefix + '\nconst FRAMES=' + json.dumps(frames) + ';\n' + assertions)
            result = subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'), str(script)],
                                    capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_actual_seed_pixel_witness_and_rejected_boundary_samples(self):
        bench = Path(__file__).resolve().parent
        fixture = fixtures_v3.build('5cd00e961d80464f')
        rows = sorted(fixture.payments, key=lambda row: (row['_instant'], row['id']))
        pack = {'seed': '5cd00e961d80464f', 'records': {
            key: [row[key if key != 'day' else '_day'] for row in rows]
            for key in ('id', 'amount_minor', 'currency', 'status', 'day')}}
        prefix = (bench / 'product_probe_sb71.mjs').read_text().split('// ── selfcheck:')[0]
        assertions = r"""
const model=buildModel(PACK),n=model.byId.get('pay_01412');
const ctx=poseCtx(model,-143.95862832828607,21.12591926008463,175.77850862014003,1190,480);
const require=(condition,message)=>{if(!condition)throw Error(message);};
const point=findPixelWitnessFor(ctx,model,n);
require(point?.sx===500&&point.sy===442&&point.factor===1,'Actual paid seed has no valid pixel witness');
const strictPick=decisiveAt(ctx,500.5,442.5);
require(strictPick.unanimous9&&!strictPick.decisive&&strictPick.depthGap<.002,'Pick oracle must remain stricter');
for(const [x,y,why] of [[501,441,'different identity'],[501,442,'mixed face factors'],[499,448,'background edge']])
  require(pixelWitnessAt(ctx,n,x,y)===null,'Accepted '+why);
for(const text of ['1','v1','V 1','version 1','Version: 1','  v: 1  '])require(parseVisibleVersion(text)===1,'Valid version rejected '+text);
for(const text of ['1.0','1e0','version1 stale2','v1/v2','v-1','version','9007199254740992','',null])require(parseVisibleVersion(text)===null,'Malformed version accepted '+text);
require(parseVisibleVersion('v2')===2&&parseVisibleVersion('v2')!==1,'Wrong version matched');
console.log(JSON.stringify({pixel:point,oldPickDepthGap:strictPick.depthGap,checks:'passed'}));
"""
        with tempfile.TemporaryDirectory(prefix='sb71-oracle-control-') as directory:
            script = Path(directory) / 'control.mjs'
            script.write_text(prefix + '\nconst PACK=' + json.dumps(pack) + ';\n' + assertions)
            result = subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'), str(script)],
                                    capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['checks'], 'passed')


@unittest.skipUnless(os.environ.get('SB71_BROWSER_TESTS') == '1', 'opt-in real-browser reference controls')
class VisualControls(unittest.TestCase):
    def collect(self, mutation=None, seed='123456789abcdef0', scenario='sb71-visual', restore_control=False):
        bench = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory(prefix='sb71-visual-control-') as temporary:
            root = Path(temporary)
            tree = root / 'candidate'
            tree.mkdir()
            for directory in ('app', 'web'):
                shutil.copytree(bench / 'golden-sb71' / directory, tree / directory,
                                ignore=shutil.ignore_patterns('__pycache__'))
            if mutation:
                file, old, new = mutation
                path = tree / file
                source = path.read_text()
                self.assertEqual(source.count(old), 1)
                path.write_text(source.replace(old, new))
            fixture = fixtures_v3.build(seed)
            (root / 'tokens.json').write_text(json.dumps(fixture.tokens))
            vendor_port, ledger_port, notifier_port = free_port(), free_port(), free_port()
            server = vendor_service_v3.serve(vendor_port, root / 'trace.jsonl', seed)
            processes = []
            with (root / 'apps.log').open('w') as log:
                try:
                    for module, args in (
                        ('notifierd', ['--port', str(notifier_port)]),
                        ('ledgerd', ['--port', str(ledger_port), '--vendor', f'http://127.0.0.1:{vendor_port}',
                                     '--notifier', f'http://127.0.0.1:{notifier_port}',
                                     '--tokens-file', str(root / 'tokens.json')]),
                    ):
                        processes.append(subprocess.Popen(
                            [sys.executable, '-m', 'app.' + module, '--db-dir', str(root / 'db'), *args],
                            cwd=tree, stdout=log, stderr=log))
                    base = f'http://127.0.0.1:{ledger_port}'
                    deadline = time.monotonic() + 90
                    while time.monotonic() < deadline:
                        try:
                            health = fetch(base + '/api/health')
                            if health.get('last_sync') and health.get('payments', 0) >= 12288:
                                break
                        except OSError:
                            pass
                        time.sleep(.2)
                    else:
                        self.fail('Reference sync failed: ' + (root / 'apps.log').read_text()[-1000:])
                    records = fetch(base + '/api/viz/records')
                    (root / 'expect.json').write_text(json.dumps(dict(seed=seed, records=records, count=records['count'], stream={'mutateIds':[fixture.d1_target['payment_id']]})))
                    env = {**os.environ, 'SB7_EXPECT_FILE': str(root / 'expect.json'),
                           'BENCH_SHOTS_DIR': str(root / 'shots'), 'BENCH_MEDIA_DIR': str(root / 'bench-media')}
                    if restore_control:
                        env['BENCH_SB71_RESTORE_CONTROL'] = '1'
                    if scenario == 'sb71-stream':
                        ready_path = root / 'stream-ready.json'
                        env['BENCH_SB71_STREAM_READY'] = str(ready_path)
                        def deliver():
                            until = time.monotonic() + 50
                            while time.monotonic() < until:
                                if ready_path.exists():
                                    if json.loads(ready_path.read_text()).get('state') == 'armed':
                                        vendor_service_v3.fire_d1_mutation()
                                    return
                                time.sleep(.02)
                        threading.Thread(target=deliver, daemon=True).start()
                    result = subprocess.run(
                        [os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'), str(bench / 'product_probe_sb71.mjs'),
                         scenario, base], env=env, capture_output=True, text=True, timeout=100)
                    self.assertEqual(result.returncode, 0, result.stderr[-1500:])
                    data = json.loads(result.stdout)
                    evidence_dir = os.environ.get('SB71_EVIDENCE_DIR')
                    if evidence_dir:
                        destination = Path(evidence_dir) / self._testMethodName
                        destination.mkdir(parents=True, exist_ok=True)
                        (destination / 'result.json').write_text(result.stdout)
                        (destination / 'probe.log').write_text(result.stderr)
                        for artifact in ('shots', 'bench-media'):
                            if (root / artifact).exists():
                                shutil.copytree(root / artifact, destination / artifact, dirs_exist_ok=True)
                    self.assertFalse(data.get('timedOut'), data)
                    self.assertEqual(data.get('consoleErrors', {}).get('count'), 0, data.get('consoleErrors'))
                    if scenario == 'sb71-stream':
                        return data
                    media = data.get('sb71', {}).get('media', {})
                    self.assertEqual(media.get('recording'), 'graded-browser')
                    manifest = json.loads((root / media['manifest']).read_text())
                    video = manifest['videos'][0]
                    self.assertGreater(video['bytes'], 0)
                    self.assertLessEqual(video['bytes'], 4 * 1024 * 1024)
                    self.assertTrue(video['publishable'])
                    self.assertFalse(manifest['errors'], manifest)
                    interval = video['sourceInterval']
                    phases = video['recordingClock']['phases']
                    self.assertGreater(interval['encodedDurationSeconds'], 0)
                    self.assertGreaterEqual(interval['encodedDurationSeconds'],
                                            (phases['motionEnd'] - phases['motionStart']) / 1000 - 2)
                    return {row['check']: row for row in data['sb71']['checks']}
                finally:
                    for process in processes:
                        process.terminate()
                    for process in processes:
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                    server.shutdown()
                    server.server_close()

    def test_actual_seed_restores_camera_on_error_before_real_background_clear(self):
        data = self.collect(('web/viz.js', 'if (brush.has(it.id)) {', 'if (false) {'),
            scenario='sb71-stream', seed='5cd00e961d80464f', restore_control=True)
        control = data['restorationControl']
        self.assertIn('intentional restoration control', control['caught'])
        self.assertNotEqual(control['initial']['camera']['yaw'], 30)
        self.assertEqual(control['initial']['brush'], ['pay_01412'])
        for key, value in {'yaw': 30, 'pitch': 40, 'distance': 260}.items():
            self.assertAlmostEqual(data['streamCameraRestoration']['actual']['camera'][key], value)
            self.assertAlmostEqual(control['after']['camera'][key], value)
        self.assertIsNotNone(control['background'])
        self.assertEqual(control['after']['brush'], [])

    def test_stream_latency_requires_actual_status_pixels(self):
        data = self.collect(scenario='sb71-stream', seed='5a05d7631d9276e3')
        entry = next(e for e in data['stream']['entries'] if e.get('pixelWitness'))
        self.assertTrue(entry['pixelWitness']['applied'], entry)
        self.assertLess(entry['applyMs'], 250, entry)

    def test_stream_retaining_d1_selection_also_has_valid_pixels(self):
        data = self.collect(('web/viz.js', 'if (brush.has(it.id)) {', 'if (false) {'),
            scenario='sb71-stream', seed='5a05d7631d9276e3')
        entry = next(e for e in data['stream']['entries'] if e.get('pixelWitness'))
        self.assertTrue(data['streamBrushBeforeArm'])
        self.assertEqual(data['streamBrushAfterArm'], [entry['pixelWitness']['id']])
        self.assertTrue(entry['pixelWitness']['applied'], entry)
        self.assertLess(entry['applyMs'], 250, entry)

    def test_stream_fast_cpu_and_delayed_gpu_does_not_claim_fast_application(self):
        data = self.collect(('web/viz.js', 'patchInstance(it);',
            'setTimeout(function () { patchInstance(it); invalidate(); render(true); }, 1200);'),
            scenario='sb71-stream', seed='5a05d7631d9276e3')
        entry = next(e for e in data['stream']['entries'] if e.get('pixelWitness'))
        self.assertTrue(entry['pixelWitness']['applied'], entry)
        self.assertGreaterEqual(entry['applyMs'], 1100, entry)
        self.assertTrue(any(not sample['matched'] for sample in entry['pixelWitness']['samples']), entry)

    def test_stream_missing_gpu_application_has_no_latency_credit(self):
        data = self.collect(('web/viz.js', 'patchInstance(it);', '/* mutant: CPU status changes, GPU stays stale */'),
            scenario='sb71-stream', seed='5a05d7631d9276e3')
        entry = next(e for e in data['stream']['entries'] if e.get('pixelWitness'))
        self.assertIsNone(entry['applyMs'], entry)
        self.assertFalse(entry['pixelWitness'].get('applied'), entry)

    def test_reference_passes_visible_structure_and_live_animation(self):
        rows = self.collect()
        for name, row in rows.items():
            self.assertEqual(row['score'], 1, (name, row))

    def test_reference_pixel_centers_on_full_reference_seed(self):
        rows = self.collect(seed='5a05d7631d9276e3')
        for name, row in rows.items():
            self.assertEqual(row['score'], 1, (name, row))

    def test_solid_box_is_not_a_structured_tower(self):
        rows = self.collect(('web/viz.js', 'part(.27, .12, .90, false, false);',
                             'part(.45, .12, .90, false, false);'))
        self.assertLess(rows['s_tower_geometry']['score'], .95)

    def test_currency_width_is_measured(self):
        rows = self.collect(('web/viz.js', 'EUR: .31, USD: .35, JPY: .39, KWD: .43',
                             'EUR: .31, USD: .31, JPY: .31, KWD: .31'))
        self.assertLess(rows['s_currency_collar']['score'], .95)

    def test_static_collar_fails_live_and_replay_motion(self):
        rows = self.collect(('web/viz.js', 'if (id !== inspectedId) return;', 'return;'))
        self.assertEqual(rows['s_tower_geometry']['score'], 1)
        self.assertEqual(rows['m_committed_event_replay']['score'], 0)

    def test_unreadable_legend_loses_presentation_credit(self):
        rows = self.collect(('web/styles.css', 'color: #ffffff; background: #153eab;',
                             'color: #153eab; background: #153eab;'))
        self.assertLess(rows['q_legible_presentation']['score'], 1)

    def test_flat_side_shading_loses_depth_credit(self):
        rows = self.collect(('web/viz.js', '0.72/0.55', '1.0'))
        self.assertLess(rows['s_tower_geometry']['score'], .95)
        self.assertLess(rows['q_legible_presentation']['score'], 1)

    def test_unreadable_child_loses_presentation_credit(self):
        rows = self.collect(('web/styles.css', '#tower-legend span { display: block; }',
                             '#tower-legend span { display:block;color:#153eab; }'))
        self.assertLess(rows['q_legible_presentation']['score'], 1)

    def test_stepped_animation_loses_motion_credit(self):
        rows = self.collect(('web/viz.js', 'replay.offset = -.56 * (1 - t * t * (3 - 2 * t));',
                             't=Math.floor(t*3)/3; replay.offset = -.56 * (1 - t * t * (3 - 2 * t));'))
        self.assertLess(rows['m_committed_event_replay']['score'], 1)

    def test_disappearing_midflight_collar_loses_motion_credit(self):
        rows = self.collect(('web/viz.js', 'replay.offset = -.56 * (1 - t * t * (3 - 2 * t));',
                             'replay.offset = t>.25 && t<.75 ? -10 : -.56 * (1 - t * t * (3 - 2 * t));'))
        self.assertLess(rows['m_committed_event_replay']['score'], 1)

    def test_stale_delivery_cannot_rewind_or_animate(self):
        rows = self.collect(('web/viz.js', 'if (n !== undefined && rec.version <= store.items[n].version) continue;',
                             'if (false) continue;'))
        self.assertLess(rows['m_committed_event_replay']['score'], 1)
        self.assertFalse(rows['m_committed_event_replay']['parts']['semantics']['ok'])

    def test_covered_canvas_fails_visible_admission(self):
        rows = self.collect(('web/index.html', '<div id="viz-labels"></div>',
            '<div id="viz-labels"></div><div style="position:absolute;inset:0;background:#101828;z-index:99;pointer-events:none"></div>'))
        self.assertEqual(rows['s_visible_surface']['score'], 0)


if __name__ == '__main__':
    unittest.main()
