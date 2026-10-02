"""SB7.2 scores SB7.1's payments product with the 3D field weighted into the earned score.

Same product, behavioural contract, starter, fixtures, vendor and gather as SB7.1 (score_sb71.py);
three things change, all of them public in sb7.2/VISUAL-CONTRACT.md:
1. The visual rows earn points. Tier weights move so S+Q+M+T+P (the 3D field, its inspection, its
   animation and its rendering performance) carry 0.46 of the inner score, and text/control
   readability moves out of the 3D bands into V as weighted points.
2. The admission ceilings keep SB7.1's meaning but are 3D-pure: no text-contrast row can cap.
3. The default overview frames the field (published camera) and its legibility is measured:
   framing, lighting and status read from cap colour (q_overview_legibility).
The excellence term (0.12), the critical multiplier and the severity registry are SB7's, unchanged.
"""
from __future__ import annotations

from contextlib import contextmanager
import copy
import hashlib
import json
from pathlib import Path

import score_sb71 as sb71

base = sb71.base
_draw_seed = sb71._draw_seed
_port_holder = sb71._port_holder
HERE = Path(__file__).resolve().parent
VERSION = 'sb-7.2'
PROBE_NAME = 'product_probe_sb72.mjs'
TEMP_PREFIX = 'sb72-score-'
SPEC = 'spec-build-sb72.md'
VISUAL_CONTRACT = 'sb7.2/VISUAL-CONTRACT.md'
THRESHOLDS_FILE = HERE / 'sb72-thresholds.json'

TIER_WEIGHT_SB72 = {'S': .10, 'Q': .08, 'M': .06, 'T': .16, 'P': .06,
                    'B': .08, 'C': .08, 'X': .10, 'R': .10,
                    'A': .02, 'D': .04, 'J': .08, 'V': .04}
assert abs(sum(TIER_WEIGHT_SB72.values()) - 1.0) < 1e-9
assert set(TIER_WEIGHT_SB72) == set(base.TIER_WEIGHT_SB7) | {'S', 'Q', 'M'}
THREE_D_TIERS = ('S', 'Q', 'M', 'T', 'P')
assert abs(sum(TIER_WEIGHT_SB72[t] for t in THREE_D_TIERS) - .46) < 1e-9

VISUAL_CHECKS = {
    's_visible_surface': 'S', 's_tower_geometry': 'S', 's_currency_collar': 'S',
    'q_payment_context': 'Q', 'q_inspector_framing': 'Q', 'q_overview_legibility': 'Q',
    'm_committed_event_replay': 'M',
    'v_presentation_text': 'V',
}
STRUCTURE_CHECKS = sb71.STRUCTURE_CHECKS
INTERACTION_CHECKS = (*sb71.QUALITY_CHECKS, 'q_overview_legibility', 'q_inspector_framing')
BACKEND_EXCELLENCE = sb71.BACKEND_EXCELLENCE
passed = sb71.passed


def thresholds():
    """SB7.2's only threshold change: the stream-apply excellence rung the reference meets on the
    grading host (receipt in the file). Every other rung is SB7's, read from sb7-thresholds.json."""
    data = json.loads(THRESHOLDS_FILE.read_text())
    rungs = data['e_stream_apply_ms_rungs']
    assert [s for s, _ms in rungs] == [1.0, .75, .5, .25] and all(a[1] < b[1] for a, b in zip(rungs, rungs[1:]))
    return data


@contextmanager
def tier_runtime():
    """Swap SB7.2's probe, version, weights, thresholds and data roots into the shared machinery."""
    overlay = thresholds()
    saved = (sb71.PROBE_NAME, sb71.VERSION, base.TIER_WEIGHT_SB7, base.ROOT_BLOCKS,
             base.TH['e_stream_apply_ms_rungs'])
    blocks = dict(base.ROOT_BLOCKS)
    # Without synced data there is no field to see: the visual rows are downstream of sync.
    blocks['sync_completeness'] = (*blocks['sync_completeness'],
                                   *(name for name in VISUAL_CHECKS if name != 'v_presentation_text'))
    sb71.PROBE_NAME, sb71.VERSION = PROBE_NAME, VERSION
    base.TIER_WEIGHT_SB7, base.ROOT_BLOCKS = TIER_WEIGHT_SB72, blocks
    base.TH['e_stream_apply_ms_rungs'] = overlay['e_stream_apply_ms_rungs']
    try:
        yield
    finally:
        (sb71.PROBE_NAME, sb71.VERSION, base.TIER_WEIGHT_SB7, base.ROOT_BLOCKS,
         base.TH['e_stream_apply_ms_rungs']) = saved


def _probe_preflight():
    with tier_runtime():
        return sb71._probe_preflight()


def gather(root, vendor_port, db_dir, trace_path, mark_phase=None, seed=None):
    with tier_runtime():
        return sb71.gather(root, vendor_port, db_dir, trace_path, mark_phase, seed)


def split_presentation(row):
    """SB7.1's q_legible_presentation mixed 3D framing with DOM text readability. SB7.2 keeps the
    3D half in Q (and in the 0.799 band) and moves text/control readability to V as points."""
    parts = (row or {}).get('parts') or {}
    framing = parts.get('framing') or []
    groups = parts.get('elements') or []
    framed = sum(bool(f.get('ok')) for f in framing)
    readable = sum(bool(g.get('ok')) for g in groups)
    unreadable = parts.get('unreadable') or []
    reached = row is not None and 'parts' in row
    return [
        {'check': 'q_inspector_framing', 'tier': 'Q',
         'score': round(framed / 8, 4) if len(framing) == 8 else 0.0,
         'detail': (f'{framed}/8 inspector poses frame the tower at 40-90% of the canvas height, unclipped, '
                    'with part callouts beside it and matching structural pixels') if reached
                   else 'Required SB7.2 inspector observation was not reached',
         'parts': {'framing': framing}},
        {'check': 'v_presentation_text', 'tier': 'V',
         'score': round(readable / 4, 4) if len(groups) == 4 else 0.0,
         'detail': (f'{readable}/4 presentation groups fully readable (12 px, 4.5:1 effective contrast, '
                    'unclipped, uncovered, inked)' + ('; ' + '; '.join(
                        f"{e['group']} / {e['text']}: {', '.join(e['failures'])}" for e in unreadable[:6])
                        if unreadable else '')) if reached
                   else 'Required SB7.2 presentation observation was not reached',
         'consequence': 'text or controls are hard to read; a cosmetic defect that never caps the 3D bands',
         'parts': {'elements': groups, 'unreadable': unreadable}},
    ]


def visual_rows(observed):
    by = {row['check']: row for row in observed}
    rows = []
    for name in ('s_visible_surface', 's_tower_geometry', 's_currency_collar', 'q_payment_context',
                 'q_overview_legibility', 'm_committed_event_replay'):
        rows.append(copy.deepcopy(by.get(name)) or {
            'check': name, 'tier': VISUAL_CHECKS[name], 'score': 0.0,
            'detail': 'Required SB7.2 visual observation was not reached'})
    rows += split_presentation(by.get('q_legible_presentation'))
    assert {r['check'] for r in rows} == set(VISUAL_CHECKS)
    return rows


def admit(raw):
    """Final = min(earned, ceilings). Each band names the 3D evidence it needs; none awards points."""
    result = copy.deepcopy(raw)
    by = {r['check']: r for r in raw['checks']}
    visible = passed(by.get('s_visible_surface'))
    matching = visible and all(passed(by.get(n)) for n in ('s_tower_geometry', 's_currency_collar', *STRUCTURE_CHECKS))
    interactive = matching and all(passed(by.get(n)) for n in INTERACTION_CHECKS)
    visual_excellent = interactive and passed(by.get('m_committed_event_replay'))
    backend_excellent = (all(passed(by.get(n)) for n in BACKEND_EXCELLENCE)
                         and not raw.get('harness_missing') and not raw.get('sched_unreached'))
    bands = [
        (visible, .599, 'Visible data-backed 3D', ('s_visible_surface',)),
        (matching, .699, 'Tower structure, currency mapping and scene truth',
         ('s_visible_surface', 's_tower_geometry', 's_currency_collar', *STRUCTURE_CHECKS)),
        (interactive, .799, '3D interaction and overview legibility', INTERACTION_CHECKS),
        (visual_excellent and backend_excellent, .899, 'Event animation and backend recovery',
         ('m_committed_event_replay', *BACKEND_EXCELLENCE)),
    ]
    ceiling, reasons, failures_by_band = 1.0, [], []
    for condition, limit, label, names in bands:
        if condition:
            continue
        failed = [name for name in names if not passed(by.get(name))]
        ceiling = min(ceiling, limit)
        failures_by_band.append({'ceiling': limit, 'checks': failed})
        cause = ', '.join(failed) if failed else 'an earlier admission band is incomplete'
        if limit == .899 and (raw.get('harness_missing') or raw.get('sched_unreached')):
            cause += '; required infrastructure or recovery schedule evidence is missing'
        reasons.append(f'{label}: {cause} (maximum {limit:.3f})')
    result.update(scorer_version=VERSION, scorerVersion=VERSION, rawScore=raw['score'],
                  score=min(raw['score'], ceiling),
                  # `good` keeps SB7.1's verdict shape (the desktop's Admission reads it): it is the
                  # same 0.799 band, now 3D interaction and overview legibility.
                  admission={'visible': visible, 'matching': matching, 'interactive': interactive, 'good': interactive,
                             'excellence': {'visual': visual_excellent, 'backend': backend_excellent},
                             'ceiling': ceiling, 'reasons': reasons, 'failedChecksByBand': failures_by_band})
    result['excellent'] = visual_excellent and backend_excellent and result['score'] >= .9
    result['solid'] = result['score'] >= .6
    return result


CARRIED = ('initial_api', 'threshold_policy', 'calibration', 'media', 'stream_witness', 'telemetry')


def evaluate(ctx):
    with tier_runtime():
        inherited = sb71.evaluate(ctx)
        rows = [r for r in inherited['checks'] if r['check'] not in sb71.VISUAL_CHECKS]
        observation = ctx.probes.get('viz', {}).get('sb71', {})
        raw = base.compose_from_rows(rows + visual_rows(observation.get('checks', [])),
                                     base._nominal_console_errors(ctx), ctx)
    why = {r['check']: r.get('why') for r in inherited.get('critical', {}).get('rows', [])}
    for critical in raw['critical']['rows']:
        critical['why'] = why.get(critical['check'], critical['why'])
    result = admit(raw)
    result.update({key: inherited[key] for key in CARRIED if key in inherited})
    result['weights'] = {'inner': TIER_WEIGHT_SB72, 'three_d_share': sum(TIER_WEIGHT_SB72[t] for t in THREE_D_TIERS),
                         'excellence': base.E_WEIGHT}
    result['thresholds_overlay'] = {'file': THRESHOLDS_FILE.name,
                                    'sha256': hashlib.sha256(THRESHOLDS_FILE.read_bytes()).hexdigest(),
                                    'e_stream_apply_ms_rungs': thresholds()['e_stream_apply_ms_rungs']}
    files = ['score_sb72.py', 'score_sb71.py', 'score_sb7.py', 'product_probe_sb72.mjs',
             'product_probe_sb71.mjs', 'product_probe_v3.mjs', 'media_sb71.mjs', THRESHOLDS_FILE.name]
    result['scorer_files_sha256'] = {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest() for name in files}
    result['spec_sha256'] = hashlib.sha256((HERE.parent / SPEC).read_bytes()).hexdigest()
    result['contract_sha256'] = {name: hashlib.sha256((HERE.parent / name).read_bytes()).hexdigest()
                                 for name in ('spec-build-sb7.md', VISUAL_CONTRACT)}
    return result


def format_report(result, title=''):
    sb7_rows = [r for r in result['checks'] if r['check'] not in VISUAL_CHECKS]
    behavioral = base.format_report({**result, 'checks': sb7_rows, 'score': result.get('rawScore', result['score'])},
                                    'SB7 behavioural evidence')
    behavioral = '\n'.join(line for line in behavioral.splitlines() if line.strip() != base.UNCALIBRATED_BANNER)
    crit = result.get('critical') or {}
    unsuppressed = [r['check'] for r in crit.get('rows', []) if not r.get('suppressed')]
    tiers = result['tiers']
    lines = [f"{title} {VERSION}: {result['score']:.3f}",
             f"Earned {result['rawScore']:.4f} = (0.88 x inner {result['inner']:.4f} + 0.12 x excellence "
             f"{tiers['E']['mean']:.4f}) x critical multiplier {crit.get('multiplier')}; admission ceiling "
             f"{result['admission']['ceiling']:.3f}",
             'Unsuppressed criticals: ' + (', '.join(unsuppressed) if unsuppressed else 'none'),
             'Admission does not award points. Final score = min(earned score, applicable ceilings).',
             *result['admission']['reasons'],
             *([f"STREAM WITNESS {result['stream_witness']['status']}: {result['stream_witness']['reason']}"]
               if result.get('stream_witness') else []),
             '3D tiers: ' + ' · '.join(f"{t} {100 * tiers[t]['mean']:.0f}% (w {tiers[t]['weight']})"
                                       for t in ('S', 'Q', 'M', 'T', 'P')),
             '', behavioral, '', 'SB7.2 visual rows (weighted):']
    for row in result['checks']:
        if row['check'] in VISUAL_CHECKS:
            lines.append(f"{row['tier']} {row['score']:.3f} {row['check']}: {row.get('detail', '')}")
    return '\n'.join(lines)


def severity_selftest():
    """SB7's severity selftest through SB7.2's weights (visual rows registered by name and tier,
    downstream of sync), plus SB7.2's own orderings: text contrast costs only V points, a 3D
    legibility failure costs more than a text failure, and every visual row earns weight."""
    registered = [(name, tier, None) for name, tier in VISUAL_CHECKS.items()]
    saved = base.SB7_CHECKS
    fails = []
    with tier_runtime():
        base.SB7_CHECKS = [*saved, *registered]
        try:
            fails += base.severity_selftest()
            perfect = [{'check': n, 'tier': t, 'score': 1.0} for n, t, _ in base.SB7_CHECKS]

            def score_with(overrides):
                rows = [{**r, 'score': overrides.get(r['check'], r['score'])} for r in perfect]
                return admit(base.compose_from_rows(rows, 0))
            text = score_with({'v_presentation_text': .75})
            legibility = score_with({'q_overview_legibility': 2 / 3})
            if not (text['admission']['ceiling'] == 1.0 and text['score'] < 1.0):
                fails.append(f"SB7.2: a text-contrast failure must cost V points and never cap ({text['score']})")
            if not legibility['admission']['ceiling'] == .799:
                fails.append('SB7.2: an illegible overview must hold the 0.799 band')
            if not legibility['rawScore'] < text['rawScore']:
                fails.append('SB7.2: overview legibility must earn more than presentation text')
            for name in VISUAL_CHECKS:
                if not score_with({name: 0.0})['rawScore'] < 1.0:
                    fails.append(f'SB7.2: visual row {name} earns no weight')
        finally:
            base.SB7_CHECKS = saved
    return fails


def reference_failures(result, ctx):
    failures = base._reference_gate({**result, 'checks': [r for r in result['checks'] if r['check'] not in VISUAL_CHECKS]}, ctx)
    failures += severity_selftest()
    gates = result['admission']
    if not (gates['visible'] and gates['matching'] and gates['interactive']
            and gates['excellence']['visual'] and gates['excellence']['backend']):
        failures += ['SB7.2 admission: ' + reason for reason in gates['reasons']]
    failures += [f"{r['check']}: {r['score']}" for r in result['checks'] if r['check'] in VISUAL_CHECKS and not passed(r)]
    return failures


def main(argv=None):
    import sys
    return sb71.run_cli(sys.modules[__name__], argv)


if __name__ == '__main__':
    raise SystemExit(main())
