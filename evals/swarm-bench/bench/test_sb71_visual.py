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


@unittest.skipUnless(os.environ.get('SB71_BROWSER_TESTS') == '1', 'opt-in real-browser reference controls')
class VisualControls(unittest.TestCase):
    def collect(self, mutation=None, seed='123456789abcdef0', scenario='sb71-visual'):
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
                    self.assertGreater(manifest['videos'][0]['bytes'], 0)
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
