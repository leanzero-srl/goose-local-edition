"""Compare independent Forge 2.0 builds without treating fixture rows as independent model runs.

Input JSON: {"builds": [{"model": "...", "episode": 1, "engine_sha256": "...",
"settings": {...}, "verdict": "/path/verdict.json", "cost": "/path/model-cost.json"}, ...]}.
No builds, scores or publications are launched. Mixed tasks/settings, unresolved bills, duplicate
build episodes and infrastructure-interrupted results are refused.
"""
import argparse
import json
import math
from pathlib import Path
from statistics import median

COHORT_FIELDS = ('scorerVersion', 'scorer_files_sha256', 'spec_sha256', 'kit_lock_sha256', 'wrapper_sha256',
                 'fixture_seeds')


def numeric(value, label):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ValueError(label + ' is missing or non-finite')
    return value


def compare(builds):
    if not builds:
        raise ValueError('No completed builds to compare')
    cohort = None
    seen = set()
    models = {}
    spent = 0
    for build in builds:
        model, episode = build.get('model'), build.get('episode')
        if not isinstance(model, str) or not model or type(episode) is not int or episode < 1:
            raise ValueError('Build model/episode identity is missing')
        if (model, episode) in seen:
            raise ValueError('Duplicate independent build episode: ' + model)
        seen.add((model, episode))
        v, cost = build['verdict'], build['cost']
        if v.get('scorerVersion') != 'forge-2.0' or v.get('status') != 'scored' or v.get('publishable') is not True:
            raise ValueError('Only frozen, fully observed completed Forge 2.0 builds are comparable: ' + model)
        if v.get('model') is not None and v['model'] != model:
            raise ValueError('Verdict model differs from episode identity')
        if (v.get('agent', {}).get('budget') or {}).get('stopped_by') in ('wallet_guard', 'wallet_accounting_failure'):
            raise ValueError('Resource-interrupted build is not capability evidence')
        score = numeric(v.get('score'), 'Score')
        if not 0 <= score <= 1:
            raise ValueError('Score outside [0,1]')
        identity = {k: v.get(k) for k in COHORT_FIELDS}
        if any(value is None or value == {} or value == [] for value in identity.values()):
            raise ValueError('Frozen task/kit/scorer/fixture identity is incomplete')
        if len(set(identity['fixture_seeds'])) != 3 or set(v.get('seed_scores', {})) != set(identity['fixture_seeds']):
            raise ValueError('Three common fixture results are required')
        if not isinstance(build.get('engine_sha256'), str) or not build['engine_sha256'] or not isinstance(build.get('settings'), dict):
            raise ValueError('Engine/settings receipt is missing')
        names = [r['check'] for r in v.get('checks', [])]
        if not names or len(names) != len(set(names)):
            raise ValueError('Full, unique check registry is required')
        identity.update(engine_sha256=build['engine_sha256'], settings=build['settings'], checks=sorted(names))
        if cohort is None:
            cohort = identity
        elif identity != cohort:
            raise ValueError('Cannot compare mixed task, engine, settings or scoring fixtures')
        if cost.get('status') != 'complete' or cost.get('billed_usd_is_lower_bound') is not False:
            raise ValueError('Unresolved billing is not zero spend: ' + model)
        bill = numeric(cost.get('billed_usd'), 'Billed USD')
        if bill < 0:
            raise ValueError('Negative billed spend')
        tokens = cost.get('tokens') or {}
        prompt, cached = numeric(tokens.get('prompt'), 'Prompt tokens'), numeric(tokens.get('cached'), 'Cached tokens')
        if prompt <= 0 or not 0 <= cached <= prompt:
            raise ValueError('Native cache accounting is invalid')
        spent += bill
        models.setdefault(model, []).append({'episode': episode, 'score': score, 'seed_scores': v['seed_scores'],
            'critical_classes': (v.get('critical') or {}).get('classes', []), 'billed_usd': bill,
            'cached_input_fraction': cached / prompt,
            'failed_checks': {r['check']: r['score'] for r in v['checks'] if r['score'] < 1}})
    summaries = {}
    for model, episodes in models.items():
        scores = [b['score'] for b in episodes]
        summaries[model] = {'independent_builds': len(episodes), 'median': median(scores), 'min': min(scores),
                            'max': max(scores), 'episodes': sorted(episodes, key=lambda b: b['episode'])}
    direction = None
    if len(summaries) == 2 and all(m['independent_builds'] >= 2 for m in summaries.values()):
        a, b = list(summaries)
        if summaries[a]['min'] > summaries[b]['max']:
            direction = a
        elif summaries[b]['min'] > summaries[a]['max']:
            direction = b
    return {'era': 'forge-2.0', 'cohort': cohort, 'models': summaries, 'billed_usd': spent,
            'screening_direction': direction,
            'conclusion': ('Repeated-build screening signal; this is not a model-population confidence interval.'
                           if direction else 'Unresolved comparison: insufficient independent builds or overlapping episode ranges.'),
            'noise_policy': 'Report fixture variation separately from independent build variation; never count 3 fixtures as 3 model builds.'}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('manifest', type=Path)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    builds = json.loads(args.manifest.read_text())['builds']
    for build in builds:
        for key in ('verdict', 'cost'):
            build[key] = json.loads(Path(build[key]).read_text())
    report = compare(builds)
    args.out.write_text(json.dumps(report, indent=2) + '\n')
    print(report['conclusion'])


if __name__ == '__main__':
    main()
