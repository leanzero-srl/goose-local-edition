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
from decimal import ROUND_FLOOR, Decimal
import hashlib
import json
from pathlib import Path

import isolated_tiers
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
# x_m2_pair_conservation grades the same predicate as x_l5_group_atomicity in SB7.1's scorer
# (pair_conservation_result: 0 iff confirmed_half_states > 0, exactly x_l5's test), so one defect
# was counted twice — in X's mean and in the 0.899 band. SB7.2 keeps it as a report-only row
# (DIAGNOSTIC: computed and shown, weight zero; X's mean re-normalises over the remaining rows).
REPORT_ONLY = ('x_m2_pair_conservation',)
BACKEND_EXCELLENCE = tuple(name for name in sb71.BACKEND_EXCELLENCE if name not in REPORT_ONLY)
passed = sb71.passed
# Graded bands (owner 2026-10-03: "it's impossible to have 3 models with the same score"): the
# 0.699, 0.799 and 0.899 bands step like Forge's (score_forge.GRADED_BAND) - with n of a band's OWN
# checks failing, its ceiling is max(next lower cap, cap - 0.03·(n - 1)). One failure caps exactly
# as the binary band did, so the severity ordering holds. 0.599 (a visible scene) stays binary.
GRADED_STEP = .03
GRADED_FLOOR = {.699: .599, .799: .699, .899: .799}


# Below a cap (owner 2026-10-03: a final sitting exactly on a cap for two models "is a big red flag"):
# final = min(earned, cap - 0.05·(1 - earned)). Continuous and monotone in earned, equal to the cap
# only at earned = 1; the deepest pull, 0.05·(1 - cap) (0.005 at 0.899 .. 0.020 at 0.599), stays
# under the 0.03 graded step. No cap (ceiling 1.0) leaves earned untouched.
CAP_PULL = .05


def capped(earned, ceiling):
    if ceiling >= 1.0:
        return earned
    # A capped final is TRUNCATED to 4 decimals (Forge's rule): rounding could put an earned < 1
    # back on the cap (0.99998 under 0.899 -> 0.8989, never 0.8990). round(.., 9) first drops float
    # dust (0.794 computed as 0.79399999...) so the floor cuts real digits only.
    exact = min(earned, ceiling - CAP_PULL * (1.0 - earned))
    return float(Decimal(repr(round(exact, 9))).quantize(Decimal('0.0001'), rounding=ROUND_FLOOR))


def band_ceiling(limit, failed):
    if limit not in GRADED_FLOOR:
        return limit
    return round(max(GRADED_FLOOR[limit], limit - GRADED_STEP * (max(failed, 1) - 1)), 4)


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
             base.TH['e_stream_apply_ms_rungs'], base.DIAGNOSTIC, base.NOTIFY_PAGED, base.SB72_STRICT,
             sb71.CHARGE_VIZ_CAP, sb71.CHARGE_UNREACHED_FAULTS)
    blocks = dict(base.ROOT_BLOCKS)
    # Without synced data there is no field to see: the visual rows are downstream of sync.
    blocks['sync_completeness'] = (*blocks['sync_completeness'],
                                   *(name for name in VISUAL_CHECKS if name != 'v_presentation_text'))
    sb71.PROBE_NAME, sb71.VERSION = PROBE_NAME, VERSION
    base.TIER_WEIGHT_SB7, base.ROOT_BLOCKS = TIER_WEIGHT_SB72, blocks
    base.TH['e_stream_apply_ms_rungs'] = overlay['e_stream_apply_ms_rungs']
    base.DIAGNOSTIC = base.DIAGNOSTIC | set(REPORT_ONLY)
    base.NOTIFY_PAGED = True
    base.SB72_STRICT = True
    # A viz probe that hits its cap is a 3D view that did not finish: scored 0, never refused.
    sb71.CHARGE_VIZ_CAP = True
    # A sync-1 fault the app's own walk never reached is the app's behaviour: scored 0, never refused.
    sb71.CHARGE_UNREACHED_FAULTS = True
    try:
        yield
    finally:
        (sb71.PROBE_NAME, sb71.VERSION, base.TIER_WEIGHT_SB7, base.ROOT_BLOCKS,
         base.TH['e_stream_apply_ms_rungs'], base.DIAGNOSTIC, base.NOTIFY_PAGED, base.SB72_STRICT,
         sb71.CHARGE_VIZ_CAP, sb71.CHARGE_UNREACHED_FAULTS) = saved


def _probe_preflight():
    with tier_runtime():
        return sb71._probe_preflight()


def gather(root, vendor_port, db_dir, trace_path, mark_phase=None, seed=None):
    with tier_runtime():
        return sb71.gather(root, vendor_port, db_dir, trace_path, mark_phase, seed)


def unreached(what, why):
    return f'Required SB7.2 {what} observation was not reached' + (f': {why}' if why else '')


def split_presentation(row, why_unreached=None):
    """SB7.1's q_legible_presentation mixed 3D framing with DOM text readability. SB7.2 keeps the
    3D half in Q (and in the 0.799 band) and moves text/control readability to V as points."""
    parts = (row or {}).get('parts') or {}
    framing = parts.get('framing') or []
    groups = parts.get('elements') or []
    framed = sum(bool(f.get('ok')) for f in framing)
    readable = sum(bool(g.get('ok')) for g in groups)
    unreadable = parts.get('unreadable') or []
    reached = row is not None and 'parts' in row
    misses = []
    for pose in framing:
        if pose.get('ok') or 'labelChecks' not in pose:
            continue
        why = [f"{c['part']}: {', '.join(c['failures'])}" for c in pose['labelChecks'] if not c['ok']]
        if not pose.get('geometryOk', True):
            why.append('part pixels below 97% match')
        if not pose.get('unclipped', True) or not .4 <= (pose.get('heightFraction') or 0) <= .9:
            why.append(f"tower spans {pose.get('heightFraction', 0):.2f} of the canvas height"
                       + ('' if pose.get('unclipped', True) else ', clipped'))
        misses.append(f"{pose['id']}@{pose['yaw']}: " + '; '.join(why or ['canvas below 600 x 460']))
    return [
        {'check': 'q_inspector_framing', 'tier': 'Q',
         'score': round(framed / 8, 4) if len(framing) == 8 else 0.0,
         'detail': (f'{framed}/8 inspector poses frame the tower at 40-90% of the canvas height, unclipped, '
                    'with part callouts beside it and matching structural pixels'
                    + ('; ' + ' | '.join(misses[:3]) if misses else '')) if reached
                   else unreached('inspector', why_unreached),
         'parts': {'framing': framing}},
        {'check': 'v_presentation_text', 'tier': 'V',
         'score': round(readable / 4, 4) if len(groups) == 4 else 0.0,
         'detail': (f'{readable}/4 presentation groups fully readable (12 px, 4.5:1 effective contrast, '
                    'unclipped, uncovered, inked)' + ('; ' + '; '.join(
                        f"{e['group']} / {e['text']}: {', '.join(e['failures'])}" for e in unreadable[:6])
                        if unreadable else '')) if reached
                   else unreached('presentation', why_unreached),
         'consequence': 'text or controls are hard to read; a cosmetic defect that never caps the 3D bands',
         'parts': {'elements': groups, 'unreadable': unreadable}},
    ]


def visual_rows(observed, why_unreached=None):
    """`why_unreached` names the measured cause on every row the probe never observed (the viz cap)."""
    by = {row['check']: row for row in observed}
    rows = []
    for name in ('s_visible_surface', 's_tower_geometry', 's_currency_collar', 'q_payment_context',
                 'q_overview_legibility', 'm_committed_event_replay'):
        rows.append(copy.deepcopy(by.get(name)) or {
            'check': name, 'tier': VISUAL_CHECKS[name], 'score': 0.0,
            'detail': unreached('visual', why_unreached)})
    rows += split_presentation(by.get('q_legible_presentation'), why_unreached)
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
    lower = set()
    for condition, limit, label, names in bands:
        own = [name for name in names if name not in lower]
        lower |= set(names)
        if condition:
            continue
        failed = [name for name in names if not passed(by.get(name))]
        graded = [name for name in failed if name in own]
        cap = band_ceiling(limit, len(graded))
        ceiling = min(ceiling, cap)
        failures_by_band.append({'band': limit, 'ceiling': cap, 'checks': failed, 'graded_failures': len(graded)})
        cause = ', '.join(failed) if failed else 'an earlier admission band is incomplete'
        if limit == .899 and (raw.get('harness_missing') or raw.get('sched_unreached')):
            cause += '; required infrastructure or recovery schedule evidence is missing'
        reasons.append(f'{label}: {cause} (maximum {cap:.3f})')
    result.update(scorer_version=VERSION, scorerVersion=VERSION, rawScore=raw['score'],
                  score=capped(raw['score'], ceiling),
                  # `good` keeps SB7.1's verdict shape (the desktop's Admission reads it): it is the
                  # same 0.799 band, now 3D interaction and overview legibility.
                  admission={'visible': visible, 'matching': matching, 'interactive': interactive, 'good': interactive,
                             'excellence': {'visual': visual_excellent, 'backend': backend_excellent},
                             'ceiling': ceiling, 'reasons': reasons, 'failedChecksByBand': failures_by_band})
    result['excellent'] = visual_excellent and backend_excellent and result['score'] >= .9
    result['solid'] = result['score'] >= .6
    return result


def report_only_rows(rows, half_samples):
    """Mark the report-only rows and archive the confirmed half-state samples behind x_l5/x_m2 —
    the read pairs (previous + confirming) whose reversal total disagreed with the refunded row."""
    out = []
    for row in rows:
        row = copy.deepcopy(row)
        if row['check'] in ('x_l5_group_atomicity', *REPORT_ONLY) and half_samples is not None \
                and isinstance(row.get('parts'), dict) and 'confirmed_half_states' in row['parts']:
            row['parts']['half_state_samples'] = half_samples
        if row['check'] in REPORT_ONLY:
            row['report_only'] = True
            row['detail'] = (f"{row.get('detail', '')} [report-only in SB7.2: the same confirmed-half-state "
                             'predicate as x_l5_group_atomicity, which carries the points and the band]')
        out.append(row)
    return out


CARRIED = ('initial_api', 'threshold_policy', 'calibration', 'media', 'stream_witness', 'telemetry')


def evaluate(ctx):
    with tier_runtime():
        inherited = sb71.evaluate(ctx)
        rows = [r for r in inherited['checks'] if r['check'] not in sb71.VISUAL_CHECKS]
        viz = ctx.probes.get('viz', {})
        observation = viz.get('sb71', {})
        cap_note, cap_missing = sb71.viz_cap_note(viz), sb71.viz_cap_missing(viz)
        rows = report_only_rows(rows, getattr(ctx, '_m2_half_samples', None))
        raw = base.compose_from_rows(rows + visual_rows(observation.get('checks', []), cap_note),
                                     base._nominal_console_errors(ctx), ctx)
    why = {r['check']: r.get('why') for r in inherited.get('critical', {}).get('rows', [])}
    for critical in raw['critical']['rows']:
        critical['why'] = why.get(critical['check'], critical['why'])
    result = admit(raw)
    result.update({key: inherited[key] for key in CARRIED if key in inherited})
    if 'harnessLoad' in viz:
        result['harness_load'] = viz['harnessLoad']
    if cap_note:
        result['viz_cap'] = {
            'detail': cap_note, 'elapsedMs': viz.get('elapsedMs'), 'hardMs': viz.get('hardMs'),
            'charged': [r['check'] for r in result['checks']
                        if 'viz_cap' in (r.get('parts') or {}) or cap_note in r.get('detail', '')],
            # Kept in harness_missing (it did fail); named here as the cap's consequence, which is why it did
            # not refuse the run.
            'harness_missing_from_cap': [name for name in result.get('harness_missing') or []
                                         if name in cap_missing]}
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
                                 for name in isolated_tiers.SB72.contracts[1:]}
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
             *[f"FAULT UNREACHED {r['check']}: {r['detail']}" for r in result['checks']
               if 'fault_unreached' in (r.get('parts') or {})],
             *([f"VIZ CAP: {result['viz_cap']['detail']}; charged 0: {', '.join(result['viz_cap']['charged'])}"
                + (f"; harness steps the cap prevented: {', '.join(result['viz_cap']['harness_missing_from_cap'])}"
                   if result['viz_cap']['harness_missing_from_cap'] else '')]
               if result.get('viz_cap') else []),
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
            # The graded bands (2026-10-03): one failure caps exactly as the binary band did, more
            # failures never raise the ceiling, and no band undercuts the next lower cap.
            for top, band in ((.899, ('m_committed_event_replay', *BACKEND_EXCELLENCE)),
                              (.799, INTERACTION_CHECKS),
                              (.699, ('s_tower_geometry', 's_currency_collar', *STRUCTURE_CHECKS))):
                ceilings = [score_with({n: .5 for n in band[:k]})['admission']['ceiling'] for k in range(1, len(band) + 1)]
                if (ceilings[0] != top or any(b > a for a, b in zip(ceilings, ceilings[1:]))
                        or min(ceilings) < GRADED_FLOOR[top]):
                    fails.append(f'SB7.2: graded {top} band is not monotone within [{GRADED_FLOOR[top]}, {top}]: '
                                 f'{ceilings[:6]}')
            # Below-cap pull: at equal earned, one more graded failure never ranks above one fewer,
            # and a capped final reaches its cap only at earned 1.
            for earned in (.80, .85, .90, .95, .99):
                for top in (.899, .799, .699):
                    a_, b_ = capped(earned, band_ceiling(top, 1)), capped(earned, band_ceiling(top, 2))
                    if b_ > a_ or capped(earned, top) >= top:
                        fails.append(f'SB7.2: below-cap pull breaks ranking at earned {earned}, cap {top}')
            for name in REPORT_ONLY:
                if score_with({name: 0.0})['score'] != 1.0:
                    fails.append(f'SB7.2: report-only row {name} moved the score')
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
