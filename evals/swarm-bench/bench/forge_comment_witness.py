"""Correct the historic duplicate-comment critical from retained native UI evidence."""
from __future__ import annotations

import copy
from types import SimpleNamespace


COMMENT_CHECK = 'b_comment_exactly_once'


def comment_duplicate_witness(observation):
    if not isinstance(observation, dict):
        return {'observed': None, 'reason': 'observation missing', 'scenarios': []}
    ui = observation.get('ui')
    renders = ui.get('sprintAction') if isinstance(ui, dict) else None
    if not isinstance(renders, list) or not renders:
        return {'observed': None, 'reason': 'sprint-action scenarios missing', 'scenarios': []}
    scenarios, gaps = [], []
    for index, render in enumerate(renders):
        if not isinstance(render, dict):
            gaps.append(f'render {index} is unreadable')
            continue
        for name in ('post', 'doubleClick'):
            scenario = render.get(name)
            count = scenario.get('commentsAdded') if isinstance(scenario, dict) else None
            if not isinstance(count, int) or isinstance(count, bool) or count < 0:
                gaps.append(f'render {index} {name} has no retained comment count')
                continue
            scenarios.append({'render': index, 'scenario': name, 'commentsAdded': count})
    duplicate = any(scenario['commentsAdded'] > 1 for scenario in scenarios)
    complete = not gaps and observation.get('runtimePublishable') is True and not observation.get('sectionErrors')
    return {'observed': True if duplicate else False if complete else None,
            'reason': 'multiple comments observed in one action' if duplicate else
                      'every retained action added at most one comment' if complete else
                      'incomplete retained UI evidence',
            'scenarios': scenarios, 'gaps': gaps, 'complete': complete}


def recompose_comment_critical(scorer, original, observations):
    seeds = original.get('fixture_seeds')
    if not isinstance(seeds, list) or len(set(seeds)) != len(seeds) or not seeds:
        raise ValueError('ORIGINAL_FIXTURE_IDENTITIES_MISSING')
    if set(observations) != set(seeds):
        raise ValueError('ORIGINAL_FIXTURE_COVERAGE_MISMATCH')
    witnesses = {}
    for seed in seeds:
        observation = observations[seed]
        if observation.get('seed') != seed or observation.get('runtime') != original.get('runtime'):
            raise ValueError('CACHED_OBSERVATION_IDENTITY_MISMATCH')
        witnesses[seed] = comment_duplicate_witness(observation)
    observed = True if any(w['observed'] is True for w in witnesses.values()) else (
        False if all(w['observed'] is False for w in witnesses.values()) else None)
    if observed is None:
        raise ValueError('COMMENT_CRITICAL_WITNESS_UNAVAILABLE')
    context = SimpleNamespace(harness_missing=original['harness_missing'], runtime=original['runtime'],
                              fixture_seed=original['fixture_seed'], dev_seed=original.get('dev_seed'))
    baseline = scorer.compose_from_rows(copy.deepcopy(original['checks']), context)
    differences = [key for key, value in baseline.items() if value != original.get(key)]
    if differences:
        raise ValueError('ORIGINAL_COMPOSITION_NOT_REPRODUCED: ' + ','.join(differences))
    rows = copy.deepcopy(original['checks'])
    target = [row for row in rows if row['check'] == COMMENT_CHECK]
    if len(target) != 1:
        raise ValueError('ORIGINAL_COMMENT_CHECK_MISSING_OR_DUPLICATED')
    seed_grades = target[0].get('seeds')
    functions = [entry[2] for entry in scorer.CHECKS if entry[0] == COMMENT_CHECK]
    if not isinstance(seed_grades, dict) or set(seed_grades) != set(seeds) or len(functions) != 1:
        raise ValueError('ORIGINAL_COMMENT_FIXTURE_GRADES_MISSING')
    for seed, observation in observations.items():
        render_context = SimpleNamespace(sprint_renders=lambda obs=observation: obs['ui']['sprintAction'])
        grade = functions[0](render_context)
        if grade['score'] != seed_grades[seed]:
            raise ValueError('CACHED_COMMENT_FUNCTIONAL_GRADE_MISMATCH: ' + seed)
    target[0]['critical_observed'] = observed
    target[0]['critical_evidence'] = witnesses
    severity = scorer.critical_severity_input

    def witnessed_severity(row):
        if row['check'] == COMMENT_CHECK and row.get('critical_observed') is False:
            return 1.0
        return severity(row)

    scorer.critical_severity_input = witnessed_severity
    try:
        corrected = scorer.compose_from_rows(rows, context)
    finally:
        scorer.critical_severity_input = severity
    if [row['score'] for row in corrected['checks']] != [row['score'] for row in original['checks']]:
        raise ValueError('FUNCTIONAL_CHECK_GRADES_CHANGED')
    derivative = copy.deepcopy(original)
    derivative.update(corrected)
    derivative['scorer_correction'] = {
        'code': 'comment-critical-requires-observed-duplication',
        'originalScore': original['score'], 'correctedScore': corrected['score'],
        'duplicateObserved': observed, 'witnesses': witnesses,
        'functionalCheckGradesUnchanged': True,
        'compositionMethod': 'Original canonical rows and original pinned composer, with only the false duplicate-comment severity disabled when all saved fixtures prove at most one comment per action.',
        'scope': 'Saved observations only; no candidate, browser, probe, benchmark or model replay.',
    }
    return derivative
