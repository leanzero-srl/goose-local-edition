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
from datetime import datetime

import score_sb7 as base
import score_sb8
import vendor_service_v3 as vendor

HERE = Path(__file__).resolve().parent
VERSION = 'sb-7.1'
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


class ReadStream(threading.Thread):
    """Preserve SB7 sampling without shadowing Python 3.12 Thread._stop()."""

    def __init__(self, url, pids):
        super().__init__(daemon=True)
        self.base, self.pids = url, pids
        self.samples = []
        self._stop_event = threading.Event()

    def stop(self):
        self._stop_event.set()

    def run(self):
        while not self._stop_event.is_set():
            timestamp = time.time()
            _status, summary, _raw, _headers = base._get(f'{self.base}/api/summary', timeout=4)
            rows = {}
            for pid in self.pids:
                _status, row, _raw, _headers = base._get(f'{self.base}/api/payments/{pid}', timeout=4)
                if isinstance(row, dict) and row.get('id'):
                    rows[pid] = row
            if isinstance(summary, dict) and summary:
                self.samples.append({'t': timestamp, 'summary': summary, 'rows': rows})
            self._stop_event.wait(0.25)


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
def committed_payment_channel(directory):
    path = Path(directory) / 'committed-payments.json'
    stopped = threading.Event()
    errors = []
    def publish():
        with vendor.STATE.lock:
            rows = [vendor._public(dict(row)) for row in vendor.STATE.created]
        temporary = path.with_suffix('.tmp')
        temporary.write_text(json.dumps({'rows': rows}))
        temporary.replace(path)
    def watch():
        while not stopped.wait(0.05):
            try:
                publish()
            except Exception as error:
                errors.append(str(error))
                return
    publish()
    thread = threading.Thread(target=watch, daemon=True)
    thread.start()
    try:
        yield path
    finally:
        stopped.set()
        thread.join()
        if errors:
            raise RuntimeError('committed payment witness failed: ' + errors[0])


@contextmanager
def probe_runtime():
    old_probe, old_kill = base.PROBE_SCRIPT, base._kill
    old_read_stream = base._ReadStream
    old_pack = base._write_expect_pack
    old_subprocess, old_wait_total = base.subprocess, base._wait_total
    old_get, old_sse_head = base._get, base._sse_head
    endpoint_absences = []
    old_browser_probe, old_fire_d1 = base._probe, vendor.fire_d1_mutation
    ready_dir = tempfile.TemporaryDirectory(prefix='sb71-stream-')
    handshake = StreamHandshake(Path(ready_dir.name) / 'ready.json', old_fire_d1)
    def browser_probe(scenario, url, *flags, env=None, timeout=None):
        if scenario == 'flow':
            with committed_payment_channel(ready_dir.name) as committed:
                return old_browser_probe(scenario, url, *flags,
                    env={**(env or {}), 'BENCH_SB71_COMMITTED_PAYMENTS': str(committed)}, timeout=timeout)
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
    base._ReadStream = ReadStream
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
        base._ReadStream = old_read_stream
        base._write_expect_pack = old_pack
        base.subprocess, base._wait_total = old_subprocess, old_wait_total
        base._get, base._sse_head = old_get, old_sse_head
        base._probe, vendor.fire_d1_mutation = old_browser_probe, old_fire_d1
        handshake.finish()
        ready_dir.cleanup()


def _probe_preflight():
    with probe_runtime():
        return base._probe_preflight()


class PartitionStatusReader(threading.Thread):
    """Observe the existing partition without postponing the immediate A1 kill."""
    def __init__(self, url, get):
        super().__init__(daemon=True)
        self.url, self.get = url, get
        self.samples = []
        self.stopped = threading.Event()

    def run(self):
        while not self.stopped.is_set():
            started = time.monotonic()
            status, body, _raw, _headers = self.get(self.url + '/api/outbox/status', timeout=3)
            self.samples.append({'started': started, 'completed': time.monotonic(),
                                 'status': status, 'body': body})
            self.stopped.wait(0.25)

    def finish(self):
        self.stopped.set()
        self.join()


def partition_status_evidence(samples):
    successful = [s for s in samples if s['status'] == 200 and isinstance(s['body'], dict)]
    matched = [s for s in successful if s['body'].get('notifier') == 'down'
               and isinstance(s['body'].get('pending'), int) and s['body']['pending'] > 0]
    span, reachable_since = 0, None
    for sample in samples:
        if sample['status'] != 200 or not isinstance(sample['body'], dict):
            reachable_since = None
            continue
        if reachable_since is None:
            reachable_since = sample['completed']
        span = max(span, sample['completed'] - reachable_since)
    return {'samples': samples, 'successful_reads': len(successful), 'reachable_span': span,
            'relay_opportunity': span >= 2, 'status_down': bool(matched),
            'matched_at': matched[0]['completed'] if matched else None}


@contextmanager
def partition_runtime(mark_phase):
    old_spawn, old_get = base._spawn_ledgerd, base._get
    state = {'phase': None, 'reader': None, 'evidence': None}
    def finish():
        reader = state['reader']
        if reader is not None:
            reader.finish()
            state['evidence'] = partition_status_evidence(reader.samples)
            state['reader'] = None
    def mark(name, reset=None):
        if name == 'heal':
            finish()
        state['phase'] = name
        if mark_phase:
            return mark_phase(name, reset) if reset is not None else mark_phase(name)
    def spawn(ctx, env, db_dir, port, *args, **kwargs):
        process = old_spawn(ctx, env, db_dir, port, *args, **kwargs)
        if state['phase'] == 'partition' and state['reader'] is None:
            state['reader'] = PartitionStatusReader(f'http://127.0.0.1:{port}', old_get)
            state['reader'].start()
        return process
    base._spawn_ledgerd = spawn
    try:
        yield mark, state
    finally:
        finish()
        base._spawn_ledgerd = old_spawn


def pair_conservation_result(original, ctx):
    outcome = original(ctx)
    evidence = getattr(ctx, '_m2_conclusion', None)
    if (not evidence or not evidence.get('samples') or outcome.get('unavailable')
            or (outcome.get('parts') or {}).get('vacuous_root')):
        return outcome
    parts = {**evidence, 'unconfirmed_disagreements':
             evidence['samples'] - evidence['ok_samples'] if not evidence['confirmed_half_states'] else None}
    confirmed = evidence['confirmed_half_states']
    return base.g(0.0 if confirmed else 1.0,
                  f"{evidence['ok_samples']}/{evidence['samples']} sequential reads agree; "
                  f"{confirmed} confirmed half states; isolated read skew is not an atomicity violation",
                  outcome.get('consequence', ''), parts=parts)


def initial_api_evidence(ctx):
    fields = ('payments', 'summary', 'buckets', 'viz_records', 'buckets_expected',
              'expected_total_at_load', 'sync1_done', 'sync1_wall_ms', 'fixture_seed')
    missing = [name for name in fields if not hasattr(ctx, name) or getattr(ctx, name) is None]
    snapshot = {name: copy.deepcopy(getattr(ctx, name)) for name in fields
                if name not in missing and name != 'buckets_expected'}
    if 'buckets_expected' not in missing:
        snapshot['buckets_expected'] = [dict(day=day, status=status, count=count)
            for (day, status), count in ctx.buckets_expected.items()]
    return {'phase': 'postsync1 API battery', 'snapshot': snapshot,
            'missing_fields': missing,
            'timing': 'Saved initial API responses; reads are sequential, not an atomic snapshot'}


def explain_initial_state(result, ctx):
    payments = getattr(ctx, 'payments', None)
    initial = payments.get('total') if isinstance(payments, dict) else None
    expected = getattr(ctx, 'expected_total_at_load', None)
    incomplete = isinstance(initial, int) and isinstance(expected, int) and initial < expected
    rows = {row['check']: row for row in result['checks']}
    sync = rows.get('sync_completeness')
    if sync:
        eventual = sync.get('parts', {}).get('evidenced_total')
        sync['detail'] = (f'Eventual evidenced total {eventual}; initial API snapshot total {initial}; '
                          f'initial expected total {expected}. ' +
                          ('Later recovery does not repair earlier API observations.' if incomplete else
                           'Totals reflect their respective measurement phases.'))
    bucket = rows.get('b_buckets_dst')
    if bucket and incomplete:
        why = f'Incorrect daily bucket counts in the incomplete initial dataset ({initial}/{expected} payments)'
        bucket['detail'] = why + '; ' + bucket['detail']
        bucket['consequence'] = 'Initial bucket counts disagree with the full fixture; this alone does not establish a timezone conversion defect'
        for critical in result.get('critical', {}).get('rows', []):
            if critical['check'] == 'b_buckets_dst':
                critical['why'] = why
    return result


def threshold_policy():
    path = HERE / 'sb7-thresholds.json'
    return {'basis': 'fixed published specification budgets',
            'empirically_calibrated': base.CALIBRATED,
            'thresholds_sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
            'qualification': 'No empirical worst-of-five fit; no hardware normalization. Release identity is separate from calibration.'}


def encode_recorded_media(ctx, root):
    observation = ctx.probes.get('viz', {}).get('sb71', {})
    media = observation.get('media')
    if media is None:
        return
    manifest = Path(root) / media['manifest']
    try:
        subprocess.run([os.environ.get('GOOSE_SWARM_RENDER_NODE', 'node'),
                        str(HERE / 'media_sb71.mjs'), str(manifest)],
                       check=True, capture_output=True, text=True)
        observation['media'] = {'manifest': media['manifest'], **json.loads(manifest.read_text())}
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        media['errors'].append(f'Publication encoding unavailable; graded observations retained: {type(error).__name__}')
        manifest.write_text(json.dumps({key: value for key, value in media.items() if key != 'manifest'}, indent=2))


def gather(root, vendor_port, db_dir, trace_path, mark_phase=None, seed=None):
    if not seed:
        raise ValueError('SB7.1 requires the exact run fixture seed')
    media_dir = Path(root).resolve() / 'bench-media'
    old_media = os.environ.get('BENCH_MEDIA_DIR')
    os.environ['BENCH_MEDIA_DIR'] = str(media_dir)
    try:
        with probe_runtime() as absences:
            with partition_runtime(mark_phase) as (partition_mark, partition):
                with resync_runtime(partition_mark) as (mark, resync):
                    ctx = base.gather(root, vendor_port, db_dir, trace_path, mark, seed)
        # Media serialization runs after all timed probes and app measurement have finished.
        encode_recorded_media(ctx, root)
        ctx.sb71_partition = partition['evidence']
        ctx.sb71_resync = resync
        ctx.sb71_endpoint_absences = absences
        (Path(root) / 'api-observations.json').write_text(json.dumps({
            'initial_api': initial_api_evidence(ctx),
            'endpoint_absences': absences, 'stream_head': ctx.stream_head,
            'workflow': ctx.workflow, 'api_lat': ctx.api_lat,
            'resync_interruption': resync, 'partition_status': ctx.sb71_partition,
            'partition_immediate_status': ctx.outbox_during_partition,
            'read_stream': ctx.read_stream}, indent=2))
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
    evidence = {**original, **by}
    bands = [
        (visible, .599, 'Visible payment scene', ('s_visible_surface',)),
        (matching, .699, 'Payment geometry, mapping and scene truth',
         ('s_visible_surface', 's_tower_geometry', 's_currency_collar', *STRUCTURE_CHECKS)),
        (good, .799, 'Readable presentation and field interaction',
         ('q_payment_context', 'q_legible_presentation', *QUALITY_CHECKS)),
        (visual_excellent and backend_excellent, .899, 'Event animation and backend recovery',
         ('m_committed_event_replay', *BACKEND_EXCELLENCE)),
    ]
    failures_by_band = []
    for condition, limit, label, names in bands:
        failed = [name for name in names if not passed(evidence.get(name))]
        if not condition:
            ceiling = min(ceiling, limit)
            failures_by_band.append({'ceiling': limit, 'checks': failed})
            cause = ', '.join(failed) if failed else 'an earlier admission band is incomplete'
            if limit == .899 and (raw.get('harness_missing') or raw.get('sched_unreached')):
                cause += '; required infrastructure or recovery schedule evidence is missing'
            reasons.append(f'{label}: {cause} (maximum {limit:.3f})')
    result.update(scorer_version=VERSION, scorerVersion=VERSION, rawScore=raw['score'],
                  score=min(raw['score'], ceiling),
                  admission={'visible': visible, 'matching': matching, 'good': good,
                             'excellence': {'visual': visual_excellent, 'backend': backend_excellent},
                             'ceiling': ceiling, 'reasons': reasons, 'failedChecksByBand': failures_by_band})
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



def premature_resync_evidence(ctx):
    """Require positive completion of a nonterminal walk, not a checkpoint timeout."""
    receipt = getattr(ctx, 'sb71_resync', None) or {}
    target = getattr(getattr(ctx, 'schedule', None), 'sigkill_after_list', None)
    completed = receipt.get('completed') or []
    if (not isinstance(target, int) or target < 1 or receipt.get('target') != target
            or not 0 < len(completed) < target or receipt.get('held') != []
            or receipt.get('kill') is not None or (ctx.b3_result or {}).get('kill_fired')):
        return None
    trace = ctx.trace
    starts = [i for i, event in enumerate(trace) if event.get('__phase__') == 'sync2']
    if len(starts) != 1:
        return None
    start = starts[0]
    end = next((i for i in range(start + 1, len(trace)) if '__phase__' in trace[i]), None)
    if end is None:
        return None
    phase = trace[start + 1:end]
    lists = [e for e in phase if e.get('method') == 'GET' and e.get('path') == '/v3/payments']
    if (len(lists) != len(completed) or any(e.get('authorized') is not True
            or e.get('status') not in (200, 304) for e in lists)
            or any(e['status'] != r.get('status') or e['path'] != r.get('path')
                   for e, r in zip(lists, completed))):
        return None
    last = lists[-1]
    if last['status'] != 304 or not last.get('if_none_match'):
        return None
    cached = [e for e in trace[:start] if e.get('method') == 'GET'
              and e.get('path') == '/v3/payments' and e.get('status') == 200
              and e.get('authorized') is True and e.get('offset') == last.get('offset')
              and e.get('generation') == last.get('generation')]
    if (not cached or cached[-1].get('last') is not False
            or last.get('offset') is None or last.get('generation') is None):
        return None
    reversals = [e for e in phase if e.get('method') == 'GET'
                 and e.get('path') == '/v3/reversals' and e.get('status') == 200
                 and e.get('authorized') is True and e.get('t', 0) > last.get('t', float('inf'))]
    if not reversals:
        return None
    samples = ctx.read_stream or []
    before = [s for s in samples if s.get('t', float('inf')) < trace[start]['t']
              and isinstance(s.get('summary'), dict) and s['summary'].get('last_sync')]
    after = [s for s in samples if reversals[0]['t'] <= s.get('t', 0) < trace[end]['t']
             and isinstance(s.get('summary'), dict) and s['summary'].get('last_sync')]
    if not before or not after:
        return None
    try:
        prior = datetime.fromisoformat(before[-1]['summary']['last_sync'].replace('Z', '+00:00'))
        advanced = next((s for s in after if datetime.fromisoformat(
            s['summary']['last_sync'].replace('Z', '+00:00')) > prior), None)
    except (ValueError, TypeError, AttributeError):
        return None
    if advanced is None:
        return None
    return {'phase_start': trace[start], 'phase_end': trace[end], 'target': target,
            'completed': completed, 'nonterminal_cached_page': cached[-1],
            'last_payment_response': last, 'reversals_response': reversals[0],
            'last_sync_before': before[-1], 'last_sync_completed': advanced,
            'kill_fired': False, 'checkpoint_unreached': True}


def observed_absence_result(name, original, ctx):
    if name == 'j_workflow_journey' and ctx.probes.get('flow', {}).get('paymentWitness', {}).get('unavailable'):
        return base.unavail(ctx.probes['flow']['paymentWitness']['unavailable'])
    if name == 'x_m2_pair_conservation':
        return pair_conservation_result(original, ctx)
    if name == 'r_b7_partition' and hasattr(ctx, 'sb71_partition'):
        evidence = ctx.sb71_partition
        if not evidence or (not evidence['status_down'] and not evidence['relay_opportunity']):
            return base.unavail('partition status lacked a successful observation window covering the declared 2s relay backoff')
        observed = copy.copy(ctx)
        observed.b7_result = {**ctx.b7_result, 'status_down': evidence['status_down']}
        outcome = original(observed)
        outcome['parts']['status_observations'] = evidence
        return outcome
    outcome = label_offset_result(original, ctx) if name == 't_labels_culling' else original(ctx)
    if not outcome.get('unavailable'):
        return outcome
    if name == 'r_b3_sigkill_resync':
        evidence = premature_resync_evidence(ctx)
        if evidence is not None:
            return base.g(0.0, 'sync #2 completed after a cached nonterminal payment page; '
                          'the candidate ended its walk before the interruption checkpoint',
                          'B3 recovery remains unproven: no kill or restart was executed',
                          parts={'premature_resync': evidence, 'kill_fired': False})
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
    if observation.get('oracleCoverageUnavailable'):
        raise UnavailableEvidence(ctx, raw, 'independent inspector geometry lacks required stable pixel coverage: ' + json.dumps(observation['oracleCoverageUnavailable']))
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
    workflow = rows.get('j_workflow_journey')
    flow = ctx.probes.get('flow', {})
    if workflow and (flow.get('approveCausal') or {}).get('reachedApproved') and not flow.get('paymentInTable'):
        workflow['consequence'] = 'approval completed, but the created payment did not appear in the UI table'
        for critical in raw.get('critical', {}).get('rows', []):
            if critical.get('check') == 'j_workflow_journey':
                critical['why'] = workflow['consequence']
    camera = rows.get('t_camera_math')
    wheel = ctx.probes.get('viz', {}).get('cameraMath', {}).get('wheel', {})
    if camera and wheel.get('pageScrolled'):
        targets = [event.get('target', {}) for event in wheel.get('events', [])]
        camera['detail'] += '; wheel over the field scrolled the page; actual event targets=' + json.dumps(targets)
    layout = rows.get('t_layout_basis')
    if layout and layout['score'] == 1 and (layout.get('parts') or {}).get('unmoved_after_stream') is not True:
        layout['admission_evidence_complete'] = False
        layout['detail'] += '; SB7.1: layout stability after streaming was not observed'
    stream = ctx.probes.get('viz', {}).get('stream', {})
    stream_row = rows.get('t_stream_diff')
    if stream_row and stream_row['score'] == 1 and (stream.get('changedPixel') or {}).get('ok') is not True:
        stream_row['admission_evidence_complete'] = False
        stream_row['detail'] += '; SB7.1: changed-instance pixels were not verified'
    explain_initial_state(raw, ctx)
    result = admit(raw, observation.get('checks', []))
    result['initial_api'] = initial_api_evidence(ctx)
    result['threshold_policy'] = threshold_policy()
    result['calibration'] = 'SB7.1 uses fixed specification budgets; empirical worst-of-five calibration not performed'
    result['media'] = observation.get('media')
    files = ['score_sb71.py', 'score_sb7.py', 'product_probe_sb71.mjs', 'product_probe_v3.mjs', 'media_sb71.mjs']
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
    behavioral = base.format_report(legacy, 'SB7 behavioral evidence')
    behavioral = '\n'.join(line for line in behavioral.splitlines()
                           if line.strip() != base.UNCALIBRATED_BANNER)
    lines = [f"{title} {VERSION}: {result['score']:.3f}",
             result.get('calibration', 'Threshold policy not recorded'),
             f"Earned behavioral score {result['rawScore']:.4f}; admission ceiling {result['admission']['ceiling']:.3f}",
             'Admission does not award points. Final score = min(earned score, applicable ceilings).',
             *result['admission']['reasons'], '', behavioral,
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
