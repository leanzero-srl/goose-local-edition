"""Rebuild report evidence from pinned cached observations, without running an app or probe."""
from __future__ import annotations

import argparse
import contextlib
import copy
import hashlib
import importlib
import json
import math
import os
from pathlib import Path
import re
import stat
import sys


class Refused(RuntimeError):
    pass


def require(condition, code, detail=''):
    if not condition:
        raise Refused(f'{code}: {detail}')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular_bytes(path):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), 'NONREGULAR_INPUT', str(path))
    for parent in path.parents:
        require(not parent.is_symlink(), 'SYMLINK_INPUT_ANCESTOR', str(parent))
    return path.read_bytes()


def relative_file(root, rel):
    rel = Path(rel)
    require(not rel.is_absolute() and '..' not in rel.parts, 'UNSAFE_INPUT_PATH', str(rel))
    return Path(root) / rel


def verify_files(root, expected):
    actual = {}
    for rel, pin in expected.items():
        file = relative_file(root, rel)
        actual[str(file.resolve())] = digest = sha(regular_bytes(file))
        require(digest == pin, 'INPUT_PIN_MISMATCH', str(file))
    return actual


def kit_pins(root):
    root = Path(root)
    h = hashlib.sha256()
    inputs = {}
    for part in ('bin', 'lib', 'openapi', 'lint-pack', 'runtime-pin.json'):
        p = root / part
        if not p.exists():
            continue
        for file in [p] if p.is_file() else sorted(x for x in p.rglob('*') if x.is_file()):
            raw = regular_bytes(file)
            h.update(str(file.relative_to(root)).encode() + b'\0' + raw + b'\0')
            inputs[str(file.resolve())] = sha(raw)
    parts = []
    for label, rel in ((b'app\0', 'package-lock.json'), (b'\0lint\0', 'lint/package-lock.json'),
                       (b'\0pin\0', 'runtime-pin.json')):
        file = root / rel
        raw = regular_bytes(file)
        inputs[str(file.resolve())] = sha(raw)
        parts.extend((label, raw))
    return h.hexdigest(), sha(b''.join(parts)), inputs


_AUDIT = {'active': False}


def _audit(event, args):
    if not _AUDIT['active']:
        return
    forbidden = event.startswith(('socket.', 'subprocess.', 'os.exec', 'os.spawn', 'os.posix_spawn'))
    forbidden = forbidden or event in ('os.system', 'os.fork', 'os.forkpty', 'pty.spawn')
    if event == 'open':
        file, mode, flags = args
        writing = (isinstance(mode, str) and any(x in mode for x in 'wax+'))
        writing = writing or (isinstance(flags, int) and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC))
        if isinstance(file, (str, bytes, os.PathLike)):
            file = str(Path(os.fsdecode(file)).resolve())
            forbidden = forbidden or writing or file not in _AUDIT['allowed']
        else:
            forbidden = True
    if forbidden:
        _AUDIT['attempts'].append({'event': event})
        raise Refused(f'FORBIDDEN_OFFLINE_OPERATION: {event}')


sys.addaudithook(_audit)


@contextlib.contextmanager
def denied_runtime(allowed):
    require(not _AUDIT['active'], 'NESTED_RUNTIME_DENIAL')
    _AUDIT.update(active=True, allowed=set(allowed), attempts=[])
    try:
        yield _AUDIT
    finally:
        _AUDIT['active'] = False


def numeric_equal(actual, expected, code, label):
    require(isinstance(actual, (int, float)) and not isinstance(actual, bool), code, label)
    require(math.isfinite(actual) and actual == expected, code, label)


def check_numerics(original, row_sets, merged, seeds):
    old = {row['check']: row for row in original['checks']}
    require(len(old) == len(original['checks']), 'DUPLICATE_ORIGINAL_CHECK')
    require(set(old) == {r['check'] for r in merged}, 'CHECK_REGISTRY_MISMATCH')
    for rows, seed in zip(row_sets, seeds):
        current = {row['check']: row for row in rows}
        require(set(current) == set(old), 'FIXTURE_CHECK_REGISTRY_MISMATCH', seed)
        for name, row in current.items():
            require(seed in old[name].get('seeds', {}), 'MISSING_ORIGINAL_FIXTURE_VALUE', name)
            numeric_equal(row['score'], old[name]['seeds'][seed], 'FIXTURE_NUMERIC_MISMATCH', f'{seed}/{name}')
    for row in merged:
        numeric_equal(row['score'], old[row['check']]['score'], 'AGGREGATE_NUMERIC_MISMATCH', row['check'])
        require(row['seeds'] == old[row['check']]['seeds'], 'FIXTURE_MAP_MISMATCH', row['check'])


def private_literals(packs):
    values = set()
    sensitive = re.compile(r'(?:secret|password|token|api.?key|credential|bearer)', re.I)
    def visit(value):
        if isinstance(value, dict):
            for key, item in value.items():
                if sensitive.search(str(key)) and isinstance(item, str) and len(item) >= 8:
                    values.add(item)
                visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    for pack in packs:
        visit(pack)
    return values


def public_detail(value, secrets, limit):
    text = str(value)
    for secret in sorted(secrets, key=len, reverse=True):
        text = text.replace(secret, '[private value omitted]')
    text = re.sub(r'(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{8,}', 'Bearer [private value omitted]', text)
    text = re.sub(r'(?i)(https?://)[^\s/@:]+:[^\s/@]+@', r'\1[private credentials omitted]@', text)
    text = re.sub(r'(?i)\b([A-Z_]*(?:TOKEN|SECRET|API_KEY|PASSWORD)[A-Z_]*\s*=\s*)[^\s,;]+',
                  r'\1[private value omitted]', text)
    return text[:limit]


def public_rows(rows, packs):
    secrets = private_literals(packs)
    result = []
    for row in rows:
        evidence = []
        for item in row['seed_evidence']:
            require(re.fullmatch('[0-9a-f]{16}', item['seed']), 'INVALID_PUBLIC_FIXTURE_SEED')
            require(isinstance(item['score'], (int, float)) and 0 <= item['score'] <= 1,
                    'INVALID_PUBLIC_FIXTURE_SCORE', row['check'])
            evidence.append({'seed': item['seed'], 'score': item['score'],
                             'detail': public_detail(item['detail'], secrets, 2000)})
        result.append({'check': row['check'], 'detail': public_detail(row['detail'], secrets, 220),
                       'fixtureEvidence': evidence})
    return result


def write_json(file, value):
    with Path(file).open('x', encoding='utf-8') as stream:
        os.chmod(file, 0o600)
        json.dump(value, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write('\n')


def grade(bundle, pack_dir, output, expected_canonical_sha, expected_snapshot_sha, report_pins):
    bundle, pack_dir, output = Path(bundle), Path(pack_dir), Path(output)
    require(not output.exists(), 'OUTPUT_ALREADY_EXISTS', str(output))
    canonical_raw = regular_bytes(bundle / 'canonical.json')
    require(sha(canonical_raw) == expected_canonical_sha, 'CANONICAL_PIN_MISMATCH')
    original = json.loads(canonical_raw)
    snapshot_raw = regular_bytes(bundle / 'SNAPSHOT.json')
    require(sha(snapshot_raw) == expected_snapshot_sha, 'TRANSFER_MANIFEST_PIN_MISMATCH')
    snapshot = json.loads(snapshot_raw)
    require(snapshot.get('originalCanonicalSha256') == expected_canonical_sha and
            snapshot.get('allSourcesVerifiedBeforeAndAfter') is True, 'TRANSFER_PROOF_MISMATCH')
    transferred = {item['archivePath']: item for item in snapshot['files']}
    for rel, item in transferred.items():
        raw = regular_bytes(relative_file(bundle, rel))
        require(len(raw) == item['bytes'] and sha(raw) == item['sha256'], 'TRANSFERRED_INPUT_PIN_MISMATCH', rel)
    require(original.get('scorerVersion') == 'forge-2.0', 'WRONG_BENCHMARK_FAMILY')
    expected = original['evaluation_inputs']
    seeds = original['fixture_seeds']
    require(snapshot['nativeIdentity']['buildId'] == original.get('buildId') and
            snapshot['nativeIdentity']['fixture_seeds'] == seeds, 'TRANSFER_IDENTITY_MISMATCH')
    require(len(seeds) == 3 and len(set(seeds)) == 3, 'MISSING_ORIGINAL_SCORING_SITES')
    here = Path(__file__).resolve().parent
    root = here.parent
    roots = {'candidate': bundle / 'candidate', 'site': root / 'forge2/site',
             'public': root / 'forge2/public', 'v1_schema': root / 'forge2/starter'}
    verified = {}
    for key, mapping in expected.items():
        require(key in roots, 'UNKNOWN_EVALUATION_INPUT_CLASS', key)
        verified.update(verify_files(roots[key], mapping))
    candidate_files = {str(p.relative_to(roots['candidate'])) for p in roots['candidate'].rglob('*') if p.is_file()}
    require(candidate_files == set(expected['candidate']), 'UNPINNED_CANDIDATE_FILE_SET')
    code_pin, lock_pin, kit_files = kit_pins(root / 'forge2/kit')
    require(code_pin == original['kit_code_sha256'], 'KIT_CODE_PIN_MISMATCH')
    require(lock_pin == original['kit_lock_sha256'], 'KIT_LOCK_PIN_MISMATCH')
    verified.update(kit_files)
    old_pins = original['scorer_files_sha256']
    require(set(report_pins) == {'score_forge2.py', 'forge_seed_reports.py'}, 'MISSING_REPORT_SOURCE_PINS')
    for name, pin in {**old_pins, **report_pins}.items():
        actual = sha(regular_bytes(here / name))
        require(actual == pin, 'SCORER_SOURCE_PIN_MISMATCH', name)
        verified[str((here / name).resolve())] = actual
    observations, packs, pack_proofs = [], [], []
    for seed in seeds:
        obs_path = bundle / 'observations' / f'{seed}.json'
        obs = json.loads(regular_bytes(obs_path))
        require(obs.get('seed') == seed and obs.get('runtime') == original['runtime'],
                'OBSERVATION_SITE_IDENTITY_MISMATCH', seed)
        require(obs.get('scorerFilesSha256') == old_pins, 'OBSERVATION_SCORER_PROVENANCE_MISMATCH', seed)
        require(obs.get('evaluationInputs') == expected, 'OBSERVATION_INPUT_PROVENANCE_MISMATCH', seed)
        pack_path = pack_dir / f'{seed}.json'
        raw = regular_bytes(pack_path)
        proof = json.loads(regular_bytes(str(pack_path) + '.proof.json'))
        require(proof.get('seed') == seed and proof.get('sha256') == sha(raw), 'PACK_PROOF_MISMATCH', seed)
        require(proof.get('originalSourceVerifiedBeforeAndAfter') is True and
                proof.get('twoIndependentDataOnlyReconstructionsByteEqual') is True, 'PACK_DATA_CONTROL_MISSING', seed)
        require(set(proof['sourcePins']) == {'fixtures.cjs', 'rng.cjs', 'limits.cjs', 'world.cjs', 'llm.cjs', 'request-time.cjs'} and
                all(expected['site'].get(name) == pin for name, pin in proof['sourcePins'].items()),
                'PACK_SOURCE_PROVENANCE_MISMATCH', seed)
        pack = json.loads(raw)
        require(pack.get('seed') == seed, 'PACK_SEED_MISMATCH', seed)
        observations.append(obs)
        packs.append(pack)
        pack_proofs.append(proof)
    scorer = importlib.import_module('score_forge2')
    require(scorer.scoring_seeds(original['fixture_seed'], original['dev_seed']) == seeds,
            'ORIGINAL_SEED_DERIVATION_MISMATCH')
    with denied_runtime(verified) as denial:
        contexts = [scorer.Ctx(roots['candidate'], obs, pack, seed, original['dev_seed'], original['runtime'])
                    for seed, obs, pack in zip(seeds, observations, packs)]
        row_sets = [scorer.evaluate_rows(ctx) for ctx in contexts]
        seed_scores = {seed: scorer.compose_from_rows(rows, ctx)['score']
                       for seed, rows, ctx in zip(seeds, row_sets, contexts)}
        for ctx in contexts[1:]:
            contexts[0].harness_missing += [h for h in ctx.harness_missing if h not in contexts[0].harness_missing]
        merged = scorer.merge_seed_rows(row_sets, seeds)
        composed = scorer.compose_from_rows(merged, contexts[0])
    require(not denial['attempts'], 'FORBIDDEN_OFFLINE_OPERATION_CAUGHT_BY_SCORER')
    check_numerics(original, row_sets, merged, seeds)
    require(seed_scores == original['seed_scores'], 'FIXTURE_COMPOSITION_MISMATCH')
    differences = [key for key, value in composed.items() if key != 'checks' and value != original.get(key)]
    require(not differences, 'COMPOSITION_MISMATCH', ','.join(differences))
    for path, pin in verified.items():
        require(sha(regular_bytes(path)) == pin, 'INPUT_CHANGED_DURING_GRADING', path)
    derivative = copy.deepcopy(original)
    derivative['checks'] = merged
    derivative['offline_reporting'] = {
        'schema': 'forge-cached-observation-report-v1', 'originalCanonicalSha256': sha(canonical_raw),
        'originalScorerFilesSha256': old_pins, 'reportScorerFilesSha256': scorer.scorer_files_sha256(),
        'taskFingerprint': original.get('taskFingerprint'), 'originalInputsVerifiedBeforeAndAfter': True,
        'aggregateCheckValuesUnchanged': len(merged), 'fixtureCheckValuesUnchanged': sum(map(len, row_sets)),
        'allCompositionFieldsUnchanged': True, 'fixtureCompositionScoresUnchanged': True,
        'deniedRuntime': {'subprocessAndSocketDenied': True, 'writesDenied': True,
                          'onlyVerifiedFileReadsAllowed': True, 'forbiddenAttempts': denial['attempts']},
        'packProofs': pack_proofs,
        'scope': 'Existing cached observations and original file bytes only. No benchmark, candidate app, browser, probe or model rerun.',
    }
    output.mkdir(mode=0o700, parents=True)
    write_json(output / 'derivative.json', derivative)
    write_json(output / 'public-fixture-evidence.json', public_rows(merged, packs))
    report = {'status': 'VERIFIED_UNCHANGED_NUMERICS', 'originalCanonicalSha256': sha(canonical_raw),
              'taskFingerprint': original.get('taskFingerprint'), 'buildId': original.get('buildId'),
              'score': composed['score'], 'inner': composed['inner'],
              'criticalMultiplier': composed['critical']['multiplier'],
              'unsuppressedCriticals': composed['critical']['unsuppressed'],
              'aggregateCheckValues': len(merged), 'fixtureCheckValues': sum(map(len, row_sets)),
              'verifiedInputFiles': len(verified), 'deniedOperationAttempts': len(denial['attempts']),
              'originalScorerFilesSha256': old_pins, 'reportSourcePins': report_pins,
              'outputs': {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p.read_bytes())}
                          for p in sorted(output.iterdir()) if p.is_file()}}
    write_json(output / 'REPORT.json', report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', required=True)
    parser.add_argument('--packs', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--canonical-sha256', required=True)
    parser.add_argument('--snapshot-sha256', required=True)
    parser.add_argument('--report-pins', required=True, help='JSON with exact reporting scorer and helper hashes')
    args = parser.parse_args()
    try:
        report = grade(args.bundle, args.packs, args.out, args.canonical_sha256, args.snapshot_sha256,
                       json.loads(Path(args.report_pins).read_bytes()))
        print(json.dumps(report, sort_keys=True))
    except Refused as error:
        print(json.dumps({'status': 'OFFLINE_REPORT_REFUSED', 'reason': str(error)}))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
