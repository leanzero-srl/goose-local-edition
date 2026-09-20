"""Receipt-backed scoring retry. Never invokes a model or modifies candidate sources."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent
# Harness-owned evidence is outside the declared candidate source inventory.
EXCLUDED = {'engine-console.log', 'harness-console.log', 'trace.jsonl', 'verdict.json',
            'model-usage.json', 'model-limits.json', 'isolation.json', 'sb7-expect.json',
            'sb7-tokens.json', 'probe-observations.json', 'api-observations.json',
            'bench-shots', 'sb7-shots', 'bench-media', 'scorer-logs', '.swarm',
            'graded-sb7-db', 'sb7-empty-db', 'sb7-combined-db', 'scoring-unavailable.json',
            'run.jsonl', 'process.json', 'heartbeat', 'nodeloop-result.json'}
CONTRACTS = ['spec-build-sb71.md', 'spec-build-sb7.md', 'sb7.1/VISUAL-CONTRACT.md']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inventory(tree):
    result = {}
    for path in sorted(tree.rglob('*')):
        relative = path.relative_to(tree)
        if relative.parts[0] in EXCLUDED or '__pycache__' in relative.parts or path.name == '.DS_Store':
            continue
        if path.is_symlink():
            raise ValueError('Retry scoring requires a source tree without symlinks: ' + str(relative))
        if path.is_file():
            result[str(relative)] = sha(path)
    if not result:
        raise ValueError('Candidate source inventory is empty')
    return result


def write_completion(tree, destination, agent, *, run_id, started_at, seed, port, provider, model):
    if destination.resolve().is_relative_to(tree.resolve()):
        raise ValueError('Completion receipt must be outside the candidate tree')
    if type(agent.get('exit')) is not int or agent['exit'] != 0 or agent.get('timed_out') is not False:
        raise ValueError('No completion receipt: model process did not exit successfully')
    if not run_id or not started_at:
        raise ValueError('No completion receipt: launch identity is missing')
    receipt = {'schemaVersion': 1, 'scorerVersion': 'sb-7.1', 'runId': run_id,
               'startedAt': started_at, 'completedAt': time.time(),
               'completionEvidence': 'runner observed engine exit 0',
               'fixture_seed': seed, 'vendor_port': port, 'provider': provider, 'model': model,
               'agent': agent, 'sourceInventory': inventory(tree),
               'contracts': {name: sha(ROOT / name) for name in CONTRACTS},
               'scorerFiles': {name: sha(ROOT / 'bench' / name) for name in
                               ['score_sb71.py', 'score_sb7.py', 'product_probe_sb71.mjs', 'product_probe_v3.mjs']}}
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix('.tmp')
    temporary.write_text(json.dumps(receipt, indent=2))
    temporary.replace(destination)
    return receipt


def validate_receipt(receipt, tree, run_id):
    if receipt.get('schemaVersion') != 1 or receipt.get('scorerVersion') != 'sb-7.1' or receipt.get('runId') != run_id:
        raise ValueError('Completion receipt identity does not match this session')
    agent = receipt.get('agent', {})
    if type(agent.get('exit')) is not int or agent['exit'] != 0 or agent.get('timed_out') is not False:
        raise ValueError('Completed model build is not proven')
    if type(agent.get('secs')) not in (int, float) or not math.isfinite(agent['secs']) or agent['secs'] < 0:
        raise ValueError('Original model duration is missing')
    seed, port = receipt.get('fixture_seed'), receipt.get('vendor_port')
    if not isinstance(seed, str) or len(seed) != 16 or any(c not in '0123456789abcdef' for c in seed.lower()):
        raise ValueError('Original fixture seed is missing')
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError('Original vendor port is missing')
    if receipt.get('sourceInventory') != inventory(tree):
        raise ValueError('Candidate files changed after the completed model build; retry refused')
    if receipt.get('contracts') != {name: sha(ROOT / name) for name in CONTRACTS}:
        raise ValueError('The installed benchmark task differs from the original build; retry refused')
    header = json.loads((tree / 'trace.jsonl').read_text().splitlines()[0])
    if header.get('fixture_seed') != seed:
        raise ValueError('Original trace and completion receipt fixture seeds differ')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--tree', type=Path, required=True)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    receipt = validate_receipt(json.loads(args.receipt.read_text()), args.tree, args.run_id)
    args.out.mkdir(parents=True, exist_ok=False)
    (args.out / 'build-completed.json').write_bytes(args.receipt.read_bytes())
    report = args.out / 'verdict.json'
    attempt = {'runId': args.run_id, 'receiptSha256': sha(args.receipt),
               'startedAt': time.time(), 'status': 'scoring', 'candidate': str(args.tree.resolve())}
    status = args.out / 'attempt.json'
    status.write_text(json.dumps(attempt, indent=2))
    started = time.monotonic()
    try:
        code = subprocess.call([sys.executable, '-B', '-u', str(ROOT / 'bench/score_sb71.py'),
                                '--tree', str(args.tree), '--seed', receipt['fixture_seed'],
                                '--port', str(receipt['vendor_port']), '--json-out', str(report)])
        if code != 0:
            raise RuntimeError(f'Scorer exited with code {code}; candidate and prior evidence preserved')
        validate_receipt(receipt, args.tree, args.run_id)
        result = json.loads(report.read_text())
        score = result.get('score')
        if type(score) not in (int, float) or not math.isfinite(score) or not 0 <= score <= 1 or result.get('scorerVersion') != 'sb-7.1':
            raise ValueError('Scorer did not produce a valid SB7.1 verdict')
        if result.get('fixture_seed') != receipt['fixture_seed'] or result.get('probe_unavailable') or result.get('harness_missing') or result.get('status') == 'unavailable':
            raise ValueError('Scorer evidence is unavailable or belongs to another fixture')
        result.update({'agent': receipt['agent'], 'provider': receipt['provider'], 'model': receipt['model'],
                       'vendor_port': receipt['vendor_port'], 'scoring': {'secs': round(time.monotonic() - started, 3)},
                       'scoring_attempt': {'receiptSha256': sha(args.receipt), 'runId': args.run_id,
                                           'originalContracts': receipt['contracts'], 'originalScorerFiles': receipt['scorerFiles']}})
        report.write_text(json.dumps(result, indent=2))
        attempt['status'] = 'finished'
    except Exception as error:
        attempt.update(status='failed', error=str(error))
        raise
    finally:
        attempt['finishedAt'] = time.time()
        status.write_text(json.dumps(attempt, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
