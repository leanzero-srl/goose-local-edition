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


@contextmanager
def probe_runtime():
    old_probe, old_kill = base.PROBE_SCRIPT, base._kill
    old_pack = base._write_expect_pack
    def write_pack(ctx, path, created_rows, d1_target):
        old_pack(ctx, path, created_rows, d1_target)
        pack = json.loads(path.read_text())
        pack['sb71_reserved_payment_ids'] = reserved_payment_ids(asdict(ctx.schedule), set(ctx.pack.index()))
        path.write_text(json.dumps(pack))
    base.PROBE_SCRIPT = HERE / 'product_probe_sb71.mjs'
    base._kill = _kill_owned
    base._write_expect_pack = write_pack
    try:
        yield
    finally:
        base.PROBE_SCRIPT, base._kill = old_probe, old_kill
        base._write_expect_pack = old_pack


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
        with probe_runtime():
            return base.gather(root, vendor_port, db_dir, trace_path, mark_phase, seed)
    finally:
        if old_media is None:
            os.environ.pop('BENCH_MEDIA_DIR', None)
        else:
            os.environ['BENCH_MEDIA_DIR'] = old_media


def passed(row):
    return bool(row and row.get('score') == 1 and not row.get('unavailable')
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


def evaluate(ctx):
    raw = base.evaluate(ctx)
    observation = ctx.probes.get('viz', {}).get('sb71', {})
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
            'graded*', 'sb7-shots', 'bench-media', 'sb7-empty-db', 'sb7-combined-db',
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
        for name in ['sb7-shots', 'bench-media']:
            if (candidate / name).exists():
                shutil.copytree(candidate / name, evidence / name)
        shutil.copy2(trace, evidence / 'vendor-trace.jsonl')
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
