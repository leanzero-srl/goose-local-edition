"""SB7.1 preserves SB7 behavioral scoring and adds explicit visual admission bands."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import subprocess
import signal
import math
import threading
import time
import urllib.request
import urllib.error
from urllib.parse import urlsplit
from dataclasses import asdict

import score_sb7 as base
import score_sb8
import vendor_service_v3 as vendor

HERE = Path(__file__).resolve().parent
VERSION = 'sb-7.1-rc'
VISUAL_CHECKS = {
    's_visible_surface': 'S', 's_tower_geometry': 'S', 's_currency_collar': 'S',
    'q_payment_context': 'Q', 'q_legible_presentation': 'Q',
    'm_committed_event_replay': 'M',
}
STRUCTURE_CHECKS = ('t_layout_basis', 't_scene_binding', 't_height_pixels', 't_vs7dbg_truth')
QUALITY_CHECKS = ('t_draw_budget', 't_pick_buffer', 't_pick_real_pass', 't_click_semantics',
                  't_camera_math', 't_coast_identity', 't_coast_reality', 't_labels_culling',
                  't_brush_link', 't_stream_diff')
BACKEND_EXCELLENCE = tuple(name for name, tier, _ in base.SB7_CHECKS if tier in {'X', 'R'})
_draw_seed = base._draw_seed
_port_holder = base._port_holder


class RecordedAppOutput:
    def __init__(self, path):
        self.path = path

    def read(self):
        return self.path.read_text(errors='replace')


class ScorerProcesses:
    """Drain-free app logs prevent HTTP request logging from filling an unread pipe."""
    def __getattr__(self, name):
        return getattr(subprocess, name)

    def Popen(self, *args, **kwargs):
        if kwargs.get('stdout') != subprocess.PIPE or not kwargs.get('start_new_session'):
            return subprocess.Popen(*args, **kwargs)
        directory = Path(kwargs['cwd']) / 'scorer-logs'
        directory.mkdir(exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix='app-', suffix='.log', dir=directory, delete=False) as log:
            proc = subprocess.Popen(*args, **{**kwargs, 'stdout': log})
        proc.stdout = RecordedAppOutput(Path(log.name))
        return proc


def reserved_payment_ids(schedule, payment_ids):
    found = set()
    def visit(value):
        if isinstance(value, str) and value in payment_ids:
            found.add(value)
        elif isinstance(value, dict):
            for child in value.values():
                visit(child)
        elif isinstance(value, (list, tuple)):
            for child in value:
                visit(child)
    visit(schedule)
    return sorted(found)


class UnavailableEvidence(RuntimeError):
    def __init__(self, ctx, raw, reason):
        super().__init__('REFUSED: ' + reason)
        self.diagnostic = {'status': 'unavailable', 'scorer_version': VERSION,
                           'reason': reason, 'fixture_seed': ctx.fixture_seed,
                           'partial_checks': raw['checks'], 'observations': ctx.probes}
        (ctx.root / 'scoring-unavailable.json').write_text(json.dumps(self.diagnostic, indent=2))


def _kill_owned(proc):
    if proc is None:
        return
    if proc.poll() is None:
        score_sb8.stop(proc)
        return
    # Every scorer spawn creates a new session; a dead wrapper can leave its children there.
    groups = subprocess.check_output(['ps', '-axo', 'pid=,pgid='], text=True)
    for line in groups.splitlines():
        pid, group = map(int, line.split())
        if group == proc.pid and pid != os.getpid() and group != os.getpgrp():
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


class StreamHandshake:
    def __init__(self, path, fire):
        self.path, self.fire = path, fire
        self.stop = threading.Event()
        self.done = threading.Event()
        self.error = None
        self.receipt = None
        self.thread = None

    def start(self):
        def watch():
            try:
                while True:
                    if not self.path.exists():
                        if self.stop.wait(.05) and not self.path.exists():
                            raise RuntimeError('SB7.1 stream probe ended without a readiness signal')
                        continue
                    signal = json.loads(self.path.read_text())
                    if signal.get('state') not in {'armed', 'app_surface_absent'}:
                        raise RuntimeError('SB7.1 stream witness unavailable: ' + str(signal))
                    self.receipt = {'signal': signal, 'delivery': self.fire()}
                    return
            except Exception as error:
                self.error = error
            finally:
                self.done.set()
        self.thread = threading.Thread(target=watch, name='sb71-stream-handshake')
        self.thread.start()

    def finish(self):
        self.stop.set()
        if self.thread is not None:
            self.thread.join()

    def result(self):
        if self.thread is None:
            raise RuntimeError('SB7.1 stream probe never started')
        self.done.wait()
        if self.error is not None:
            raise self.error
        return self.receipt['delivery']


@contextmanager
def resync_runtime(mark_phase):
    """Hold the next response so the scheduled kill really occurs during a resync."""
    old_send = vendor.Handler._send
    old_spawn, old_kill, old_tail = base._spawn_ledgerd, base._kill, base._TraceTail
    lock, held, release = threading.Lock(), threading.Event(), threading.Event()
    state = {'phase': None, 'armed': False, 'completed': [], 'held': [], 'kill': None}

    def mark(name, reset=None):
        state['phase'] = name
        if name != 'sync2':
            state['armed'] = False
            release.set()
        if mark_phase:
            return mark_phase(name, reset) if reset is not None else mark_phase(name)

    def spawn(*args, **kwargs):
        if state['phase'] == 'sync2':
            state['target'] = vendor.STATE.sch.sigkill_after_list
            state['armed'] = True
            held.clear()
            release.clear()
        return old_spawn(*args, **kwargs)

    def send(handler, code, *args, **kwargs):
        eligible = (state['armed'] and handler.command == 'GET'
                    and urlsplit(handler.path).path == vendor.LIST_PATH
                    and handler.headers.get('Authorization') == f'Bearer {vendor.API_KEY}'
                    and code in (200, 304))
        if not eligible:
            return old_send(handler, code, *args, **kwargs)
        with lock:
            if not state['armed']:
                handler.close_connection = True
                return
            if len(state['completed']) < state['target']:
                result = old_send(handler, code, *args, **kwargs)
                handler.wfile.flush()
                state['completed'].append({'status': code, 'path': handler.path,
                                           'response_sent_at': time.monotonic()})
                return result
            state['held'].append({'status': code, 'path': handler.path,
                                  'held_before_send_at': time.monotonic()})
            held.set()
        release.wait()
        handler.close_connection = True

    class ResyncTail(old_tail):
        def wait_for(self, count_fn, want, timeout):
            if not state['armed']:
                return super().wait_for(count_fn, want, timeout)
            if want != state['target']:
                raise RuntimeError('B3 response schedule disagrees with the armed vendor gate')
            reached = held.wait(timeout)
            return reached, len(state['completed'])

    def kill(proc):
        interrupting = state['armed'] and held.is_set() and proc is not None
        result = old_kill(proc)
        if interrupting:
            state['kill'] = {'pid': proc.pid, 'returncode': proc.poll(),
                             'terminated_at': time.monotonic(),
                             'response_still_held': not release.is_set()}
            if proc.poll() is None:
                raise RuntimeError('B3 ledger process survived the requested kill')
        return result

    vendor.Handler._send, base._spawn_ledgerd = send, spawn
    base._kill, base._TraceTail = kill, ResyncTail
    try:
        yield mark, state
    finally:
        state['armed'] = False
        release.set()
        vendor.Handler._send, base._spawn_ledgerd = old_send, old_spawn
        base._kill, base._TraceTail = old_kill, old_tail


@contextmanager
def probe_runtime():
    old_probe, old_kill = base.PROBE_SCRIPT, base._kill
    old_pack = base._write_expect_pack
    old_subprocess, old_wait_total = base.subprocess, base._wait_total
    old_get, old_sse_head = base._get, base._sse_head
    endpoint_absences = []
    old_browser_probe, old_fire_d1 = base._probe, vendor.fire_d1_mutation
    ready_dir = tempfile.TemporaryDirectory(prefix='sb71-stream-')
    handshake = StreamHandshake(Path(ready_dir.name) / 'ready.json', old_fire_d1)
    def browser_probe(scenario, url, *flags, env=None, timeout=None):
        if scenario != 'viz':
            return old_browser_probe(scenario, url, *flags, env=env, timeout=timeout)
        handshake.start()
        try:
            result = old_browser_probe(scenario, url, *flags,
                env={**(env or {}), 'BENCH_SB71_STREAM_READY': str(handshake.path)}, timeout=timeout)
        finally:
            handshake.finish()
        result['sb71StreamHandshake'] = (handshake.receipt if handshake.receipt is not None
                                        else {'error': str(handshake.error)})
        return result
    def get(url, *args, **kwargs):
        response = old_get(url, *args, **kwargs)
        if response[0] == 501:
            endpoint_absences.append({'url': url, 'status': response[0], 'body': response[1]})
        return response
    def sse_head(url, timeout=5):
        try:
            with urllib.request.urlopen(urllib.request.Request(url + '/api/stream'), timeout=timeout) as response:
                return {'status': response.status, 'ctype': response.headers.get('Content-Type', ''),
                        'head': response.read(160).decode(errors='replace')}
        except urllib.error.HTTPError as error:
            with error:
                return {'status': error.code, 'ctype': error.headers.get('Content-Type', ''),
                        'error': 'HTTPError', 'head': error.read(160).decode(errors='replace')}
        except Exception as error:
            return {'status': None, 'ctype': '', 'error': type(error).__name__}
    def wait_total(url, want, seconds):
        status, _body, _raw, _headers = base._get(url + '/api/payments?limit=1', timeout=8)
        if status == 501:
            return False, None, None
        return old_wait_total(url, want, seconds)
    def write_pack(ctx, path, created_rows, d1_target):
        old_pack(ctx, path, created_rows, d1_target)
        pack = json.loads(path.read_text())
        pack['sb71_reserved_payment_ids'] = reserved_payment_ids(asdict(ctx.schedule), set(ctx.pack.index()))
        path.write_text(json.dumps(pack))
    base.PROBE_SCRIPT = HERE / 'product_probe_sb71.mjs'
    base._kill = _kill_owned
    base._write_expect_pack = write_pack
    base.subprocess = ScorerProcesses()
    base._wait_total = wait_total
    base._get, base._sse_head = get, sse_head
    base._probe, vendor.fire_d1_mutation = browser_probe, handshake.result
    try:
        yield endpoint_absences
    finally:
        base.PROBE_SCRIPT, base._kill = old_probe, old_kill
        base._write_expect_pack = old_pack
        base.subprocess, base._wait_total = old_subprocess, old_wait_total
        base._get, base._sse_head = old_get, old_sse_head
        base._probe, vendor.fire_d1_mutation = old_browser_probe, old_fire_d1
        handshake.finish()
        ready_dir.cleanup()


def _probe_preflight():
    with probe_runtime():
        return base._probe_preflight()


def gather(root, vendor_port, db_dir, trace_path, mark_phase=None, seed=None):
    if not seed:
        raise ValueError('SB7.1 requires the exact run fixture seed')
    media_dir = Path(root).resolve() / 'bench-media'
    old_media = os.environ.get('BENCH_MEDIA_DIR')
    os.environ['BENCH_MEDIA_DIR'] = str(media_dir)
    try:
        with probe_runtime() as absences:
            with resync_runtime(mark_phase) as (mark, resync):
                ctx = base.gather(root, vendor_port, db_dir, trace_path, mark, seed)
        ctx.sb71_resync = resync
        ctx.sb71_endpoint_absences = absences
        (Path(root) / 'api-observations.json').write_text(json.dumps({
            'endpoint_absences': absences, 'stream_head': ctx.stream_head,
            'workflow': ctx.workflow, 'api_lat': ctx.api_lat,
            'resync_interruption': resync}, indent=2))
        (Path(root) / 'probe-observations.json').write_text(json.dumps(ctx.probes, indent=2))
        return ctx
    finally:
        if old_media is None:
            os.environ.pop('BENCH_MEDIA_DIR', None)
        else:
            os.environ['BENCH_MEDIA_DIR'] = old_media


def passed(row):
    return bool(row and isinstance(row.get('score'), (int, float))
                and math.isclose(row['score'], 1, rel_tol=0, abs_tol=1e-12) and not row.get('unavailable')
                and not (row.get('parts') or {}).get('vacuous_root')
                and row.get('admission_evidence_complete', True))


def admit(raw, visual_rows):
    result = copy.deepcopy(raw)
    by = {r['check']: r for r in visual_rows}
    rows = [by.get(name, {'check': name, 'tier': tier, 'score': 0,
                         'detail': 'Required SB7.1 visual observation was not reached'})
            for name, tier in VISUAL_CHECKS.items()]
    by = {r['check']: r for r in rows}
    original = {r['check']: r for r in raw['checks']}
    visible = passed(by['s_visible_surface'])
    matching = (visible and all(passed(by[n]) for n in ('s_tower_geometry', 's_currency_collar'))
                and all(passed(original.get(n)) for n in STRUCTURE_CHECKS))
    good = (matching and all(passed(by[n]) for n in ('q_payment_context', 'q_legible_presentation'))
            and all(passed(original.get(n)) for n in QUALITY_CHECKS))
    visual_excellent = good and passed(by['m_committed_event_replay'])
    backend_excellent = (all(passed(original.get(n)) for n in BACKEND_EXCELLENCE)
                         and not raw.get('harness_missing') and not raw.get('sched_unreached'))
    reasons = []
    ceiling = 1.0
    for condition, limit, message in [
        (visible, .599, 'No verified visible payment scene in the recorded browser state'),
        (matching, .699, 'Required payment-tower structure or currency mapping is incomplete'),
        (good, .799, 'Payment context, readability or interaction quality is incomplete'),
        (visual_excellent and backend_excellent, .899,
         'Scores above 0.9 require both verified event animation and backend recovery excellence'),
    ]:
        if not condition:
            ceiling = min(ceiling, limit)
            reasons.append(message)
    result.update(scorer_version=VERSION, scorerVersion=VERSION, rawScore=raw['score'],
                  score=min(raw['score'], ceiling),
                  admission={'visible': visible, 'matching': matching, 'good': good,
                             'excellence': {'visual': visual_excellent, 'backend': backend_excellent},
                             'ceiling': ceiling, 'reasons': reasons})
    result['checks'] += rows
    for tier in {'S', 'Q', 'M'}:
        members = [r for r in rows if r['tier'] == tier]
        result['tiers'][tier] = {'mean': sum(r['score'] for r in members) / len(members),
                                 'checks': len(members), 'weight': 0, 'admission_only': True}
    result['excellent'] = visual_excellent and backend_excellent and result['score'] >= .9
    result['solid'] = result['score'] >= .6
    return result


def label_offset_result(original, ctx):
    outcome = original(ctx)
    if 'offset' not in outcome.get('parts', {}):
        return outcome
    rows = ctx.probes.get('viz', {}).get('labels', {}).get('perLabel', [])
    rows = [r for r in rows if isinstance(r, dict) and isinstance(r.get('dx'), (int, float))]
    if not rows:
        return outcome
    old_offset = sum(abs(r['dx']) <= 2.5 and abs(r.get('dy') or 99) <= 2.5 for r in rows) / len(rows)
    offset = sum(isinstance(r.get('dy'), (int, float)) and math.isfinite(r['dx'])
                 and math.isfinite(r['dy']) and abs(r['dx']) <= 2.5 and abs(r['dy']) <= 2.5
                 for r in rows) / len(rows)
    outcome['score'] = round(outcome['score'] + .1 * (offset - old_offset), 12)
    outcome['parts']['offset'] = round(offset, 3)
    outcome['detail'] += '; SB7.1: zero label offset is valid evidence'
    return outcome


def observed_absence_result(name, original, ctx):
    outcome = label_offset_result(original, ctx) if name == 't_labels_culling' else original(ctx)
    if not outcome.get('unavailable'):
        return outcome
    if name == 'r_workflow_durability' and (ctx.workflow or {}).get('create_status') == 501:
        return base._absent('drafts endpoints: exercised create returned HTTP 501')
    if name == 'r_notification_multiset' and any(
            urlsplit(w['url']).path == '/notify/notifications' and w['status'] == 501
            and isinstance(w.get('body'), dict) and isinstance(w['body'].get('error'), dict)
            and w['body']['error'].get('code') == 'not_implemented'
            for w in getattr(ctx, 'sb71_endpoint_absences', [])):
        return base._absent('notifier notifications: exercised endpoint returned HTTP 501 not_implemented')
    if name == 'p_api_latency':
        measurements = list((ctx.api_lat or {}).values())
        if measurements and all(isinstance(m, dict) and m.get('n_ok') == 0 and m.get('errors')
                                and all(e == 'status 501' for e in m['errors']) for m in measurements):
            return base._absent('API latency: measured requests all returned HTTP 501')
    if name in {'p_stream_apply', 't_stream_diff', 'e_stream_apply_latency'}:
        if (ctx.stream_head or {}).get('status') == 501:
            return base._absent('payment stream: exercised /api/stream returned HTTP 501')
    if name in base.ROOT_BLOCKS['t_vs7dbg_truth']:
        viz = ctx.probes.get('viz', {})
        observations = viz.get('debugSurfaceObservations', [])
        signal = viz.get('sb71StreamHandshake', {}).get('signal', {})
        if (signal.get('state') == 'app_surface_absent' and viz.get('ready', {}).get('draws') == 0
                and 'contextType' in viz.get('contextReal', {})
                and viz['contextReal']['contextType'] is None
                and len(observations) >= 2
                and all(o.get('evaluationSucceeded') is True and o.get('present') is False
                        for o in observations)):
            return base._absent('required 3D interaction: no application scene or debug surface observed',
                                root='t_vs7dbg_truth')
    if name == 't_vs7dbg_truth':
        viz = ctx.probes.get('viz', {})
        observations = viz.get('debugSurfaceObservations', [])
        if len(observations) >= 2 and all(o.get('evaluationSucceeded') is True
                                         and o.get('present') is False for o in observations):
            return base._absent('window.vs7dbg: repeated successful page evaluations found no required surface')
    return outcome


def evaluate(ctx):
    original_checks = base.SB7_CHECKS
    base.SB7_CHECKS = [(name, tier, lambda c, name=name, original=fn:
                       observed_absence_result(name, original, c)) for name, tier, fn in original_checks]
    try:
        raw = base.evaluate(ctx)
    finally:
        base.SB7_CHECKS = original_checks
    observation = ctx.probes.get('viz', {}).get('sb71', {})
    resync = getattr(ctx, 'sb71_resync', None)
    if resync is not None and (ctx.b3_result or {}).get('kill_fired'):
        receipt = resync.get('kill') or {}
        if (not resync['held'] or receipt.get('returncode') is None
                or not receipt.get('response_still_held')):
            raise UnavailableEvidence(ctx, raw, 'B3 mid-resync interruption was not proven')
    if raw.get('harness_missing'):
        raise UnavailableEvidence(ctx, raw, 'missing benchmark infrastructure: ' + ', '.join(raw['harness_missing']))
    if raw.get('probe_unavailable'):
        raise UnavailableEvidence(ctx, raw, 'required benchmark observations unavailable: ' + ', '.join(raw['probe_unavailable']))
    rows = {row['check']: row for row in raw['checks']}
    layout = rows.get('t_layout_basis')
    if layout and layout['score'] == 1 and (layout.get('parts') or {}).get('unmoved_after_stream') is not True:
        layout['admission_evidence_complete'] = False
        layout['detail'] += '; SB7.1: layout stability after streaming was not observed'
    stream = ctx.probes.get('viz', {}).get('stream', {})
    stream_row = rows.get('t_stream_diff')
    if stream_row and stream_row['score'] == 1 and (stream.get('changedPixel') or {}).get('ok') is not True:
        stream_row['admission_evidence_complete'] = False
        stream_row['detail'] += '; SB7.1: changed-instance pixels were not verified'
    result = admit(raw, observation.get('checks', []))
    result['media'] = observation.get('media')
    files = ['score_sb71.py', 'score_sb7.py', 'product_probe_sb71.mjs', 'product_probe_v3.mjs']
    result['scorer_files_sha256'] = {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                                    for name in files}
    result['spec_sha256'] = hashlib.sha256((HERE.parent / 'spec-build-sb71.md').read_bytes()).hexdigest()
    result['contract_sha256'] = {
        name: hashlib.sha256((HERE.parent / name).read_bytes()).hexdigest()
        for name in ('spec-build-sb7.md', 'sb7.1/VISUAL-CONTRACT.md')
    }
    return result


def format_report(result, title=''):
    old_rows = [r for r in result['checks'] if r['check'] not in VISUAL_CHECKS]
    legacy = {**result, 'checks': old_rows, 'score': result.get('rawScore', result['score'])}
    lines = [f"{title} {VERSION}: {result['score']:.3f}",
             f"Earned behavioral score {result['rawScore']:.4f}; admission ceiling {result['admission']['ceiling']:.3f}",
             'Admission does not award points. Final score = min(earned score, applicable ceilings).',
             *result['admission']['reasons'], '', base.format_report(legacy, 'SB7 behavioral evidence'),
             '', 'SB7.1 same-session visual evidence:']
    for row in result['checks']:
        if row['check'] in VISUAL_CHECKS:
            lines.append(f"{row['tier']} {row['score']:.3f} {row['check']}: {row.get('detail', '')}")
    return '\n'.join(lines)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--tree', type=Path, required=True)
    p.add_argument('--seed', required=True)
    p.add_argument('--port', type=int, default=8899)
    p.add_argument('--json-out', type=Path, required=True)
    p.add_argument('--reference', action='store_true')
    a = p.parse_args()
    if len(a.seed) != 16 or any(c not in '0123456789abcdefABCDEF' for c in a.seed):
        p.error('--seed requires 16 hexadecimal characters from the run')
    why = _port_holder(a.port) or _probe_preflight()
    if why:
        raise RuntimeError('REFUSED: ' + why)
    with tempfile.TemporaryDirectory(prefix='sb71-score-') as tmp:
        root = a.tree.resolve()
        # Scoring a clone preserves all prior screenshots, receipts and grading databases.
        candidate = Path(tmp) / 'candidate'
        shutil.copytree(root, candidate, ignore=shutil.ignore_patterns(
            'graded*', 'sb7-shots', 'bench-media', 'scorer-logs', 'sb7-empty-db', 'sb7-combined-db',
            '__pycache__', '.swarm', 'engine-console.log'))
        trace = Path(tmp) / 'vendor-trace.jsonl'
        server = vendor.serve(a.port, trace, seed=a.seed)
        unavailable = None
        try:
            ctx = gather(candidate, a.port, Path(tmp) / 'db', trace, vendor.mark_phase, a.seed)
            result = evaluate(ctx)
        except UnavailableEvidence as error:
            unavailable = error
            result = error.diagnostic
        finally:
            server.shutdown()
            server.server_close()
        evidence = a.json_out.with_suffix('').with_name(a.json_out.stem + '-evidence')
        evidence.mkdir(parents=True, exist_ok=False)
        for name in ['sb7-shots', 'bench-media', 'scorer-logs']:
            if (candidate / name).exists():
                shutil.copytree(candidate / name, evidence / name)
        shutil.copy2(trace, evidence / 'vendor-trace.jsonl')
        for name in ['probe-observations.json', 'api-observations.json']:
            shutil.copy2(candidate / name, evidence / name)
        result['evidence_dir'] = str(evidence.resolve())
        a.json_out.write_text(json.dumps(result, indent=2))
        if unavailable:
            print(str(unavailable))
            return 2
        print(format_report(result, root.name))
        if a.reference:
            failures = base._reference_gate({**result, 'checks': [r for r in result['checks'] if r['check'] not in VISUAL_CHECKS]}, ctx)
            failures += base.severity_selftest()
            gates = result['admission']
            if not (gates['visible'] and gates['matching'] and gates['good']
                    and gates['excellence']['visual'] and gates['excellence']['backend']):
                failures += ['SB7.1 admission: ' + reason for reason in gates['reasons']]
                failures += [row['check'] + ': required admission evidence incomplete'
                             for row in result['checks']
                             if row['check'] in (*STRUCTURE_CHECKS, *QUALITY_CHECKS, *BACKEND_EXCELLENCE)
                             and not passed(row)]
            failures += [r['check'] for r in result['checks'] if r['check'] in VISUAL_CHECKS and not passed(r)]
            if failures:
                print('REFERENCE REFUSED: ' + '; '.join(failures))
                return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
