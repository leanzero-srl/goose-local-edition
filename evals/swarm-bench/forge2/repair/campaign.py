#!/usr/bin/env python3
"""Resumable private Forge2 controller. Full entrants start through actual Benchmark UI clicks.

init/preflight/status/stop are free. run/detach require --execute-paid after the owner freezes
and installs the release. No wallet reset, legacy chain, scorer subprocess or publisher exists.
"""
import argparse
from decimal import Decimal
import fcntl
import json
import os
import re
import signal
from pathlib import Path
import subprocess
import sys
import time

from ops import (Hold, PILOTS, SETTINGS, allowance, bill_receipt, cache_status, catalog,
                 ensure_config, load_installed, merge_receipts, metadata, read, sha, build_cache_evidence, live_credit_evidence,
                 spend, verify_installed, write, request_reservation, completed_reservation, reserve_from_qualification)
from ops import scan_requests

HERE = Path(__file__).resolve().parent


def plan(roster):
    units = [{'key': 'qualification-' + str(i + 1), 'kind': 'qualification', 'model': model,
              'stage': 'pilot', 'status': 'pending'} for i, model in enumerate(roster['models'][:2])]
    units += [{'key': 'pilot-' + str(i + 1), 'kind': 'build', 'stage': 'pilot',
               'status': 'pending', **unit} for i, unit in enumerate(roster['pilot_order'])]
    for i, model in enumerate(roster['models'][2:], 3):
        units += [{'key': f'qualification-{i}', 'kind': 'qualification', 'stage': 'roster',
                   'model': model, 'status': 'pending'}]
        units += [{'key': f'roster-{i}-{episode}', 'kind': 'build', 'stage': 'roster',
                   'model': model, 'episode': episode, 'status': 'pending'} for episode in (1, 2)]
    return units


def ui(config, command, *arguments):
    result = subprocess.run([config['node'], str(HERE / 'dispatch.mjs'), command, *arguments],
                            capture_output=True, text=True, timeout=90)
    if result.returncode:
        raise Hold('Installed Benchmark UI transport/control failed; launch state preserved')
    return json.loads(result.stdout)


def save(state_dir, state):
    state['billed_usd'] = str(spend(state['ledger']))
    state['updated_at'] = time.time()
    write(state_dir / 'state.json', state)


def runtime_for(workdir):
    return workdir.parent / ('.' + workdir.name + '-runtime')


def reconcile(state_dir, state, unit, config, modules, final=False):
    bench_cost, isolation = modules[:2]
    if not unit.get('workdir'):
        return
    workdir = Path(unit['workdir'])
    runtime = runtime_for(workdir)
    routes, active_requests = scan_requests(runtime)
    unit['active_requests'] = active_requests
    prior = unit.setdefault('routes', {})
    if any(gen in prior and prior[gen] != route for gen, route in routes.items()):
        raise Hold('Actual native request identity changed for an already observed generation')
    prior.update(routes)
    catalog_routes = unit.setdefault('catalog_routes', {unit['model']: unit['catalog']})
    unknown = {r['requested_model'] for r in [*routes.values(), *active_requests.values()]} - set(catalog_routes)
    if unknown:
        available, _ = catalog()
        for model in unknown:
            catalog_routes[model] = metadata(available[model]) if model in available else {'id': model, 'advertised_metadata': 'unavailable'}
    try:
        ids, _, telemetry = bench_cost.collect_generation_ids(runtime, workdir)
    except FileNotFoundError:
        if final:
            raise
        save(state_dir, state)
        return  # Native ten-file rotation raced this observation; the next free scan retries.
    # The protected telemetry file is authoritative until invoke() lands it into the candidate tree.
    if telemetry['status'] != 'recorded' and (runtime / 'telemetry.jsonl').is_file():
        live = state_dir / 'units' / unit['key'] / 'live'
        write(live / 'marker.json', {'runtime': str(runtime)})
        swarm = live / '.swarm'
        swarm.mkdir(exist_ok=True)
        (swarm / 'telemetry.jsonl').write_bytes((runtime / 'telemetry.jsonl').read_bytes())
        live_ids, _, telemetry = bench_cost.collect_generation_ids(runtime, live)
        ids = list(dict.fromkeys(live_ids + ids))
    unit['generation_ids'] = list(dict.fromkeys(unit.get('generation_ids', []) + ids))
    ended = telemetry.get('ended', {})
    wanted = [gen for gen in unit['generation_ids'] if gen not in state['ledger'] and (final or gen in ended)]
    snapshot = modules[4].entrant_config('openrouter')
    credentials = modules[4].cloud_env('openrouter', snapshot)
    key = credentials.get('OPENROUTER_API_KEY')
    if not key:
        raise Hold('Billing credentials are unavailable; no next paid unit')
    host = credentials.get('OPENROUTER_HOST') or 'https://openrouter.ai'
    if final:
        found, _, failures = bench_cost.bill_generations(wanted, key, host)
    else:
        import urllib.parse, urllib.request
        found, failures = {}, {}
        for gen in wanted:
            data, failure = bench_cost._fetch(host.rstrip('/') + '/api/v1/generation?id=' + urllib.parse.quote(gen), key, urllib.request.urlopen)
            if data is not None:
                found[gen] = data
            else:
                failures[gen] = failure
    for gen, data in found.items():
        write(state_dir / 'generations' / (gen + '.native.json'), data)
        try:
            receipt = bill_receipt(gen, data, unit['routes'].get(gen), unit['catalog'])
        except Hold:
            # Refusal cannot hide a bill: account its native amount before holding the campaign.
            receipt = {'id': gen, 'billed_usd': str(data['usage']), 'billed_model': data.get('model'),
                       'provider_name': data.get('provider_name'), 'kind': 'identity-unproven'}
            merge_receipts(state['ledger'], unit['key'], [receipt], stage=unit['stage'])
            save(state_dir, state)
            raise
        merge_receipts(state['ledger'], unit['key'], [receipt], stage=unit['stage'])
    unit['unbilled'] = {gen: failures.get(gen, 'stream not ended in telemetry') for gen in unit['generation_ids'] if gen not in state['ledger']}
    observed = [Decimal(row['billed_usd']) for row in state['ledger'].values() if row['unit'] == unit['key']]
    fallback = max(observed) if observed else Decimal(unit['wallet'])
    unit['pending_reservations'] = {}
    for gen in unit['generation_ids']:
        if gen in state['ledger']:
            continue
        route = unit['routes'].get(gen, {})
        row = catalog_routes.get(route.get('requested_model'), {'id': 'unknown'})
        counts = telemetry.get('native_usage_by_id', {}).get(gen)
        unit['pending_reservations'][gen] = completed_reservation(counts, row, fallback) if counts else request_reservation({**route, 'billed_usd': str(fallback)}, row)
    # A streaming request can exist before the first response ID. Record it exactly once:
    # once its ID is observed, that generation's pending reservation carries the same envelope.
    unit['pre_id_reservations'] = {name: request_reservation({**request, 'billed_usd': str(fallback)},
                                  catalog_routes.get(request.get('requested_model'), {'id': 'unknown'}))
                                  for name, request in active_requests.items() if not request['generation_ids']}
    unit['pending_reserved_usd'] = str(sum((Decimal(r['usd']) for r in
            [*unit['pending_reservations'].values(), *unit['pre_id_reservations'].values()]), Decimal(0)))
    save(state_dir, state)
    if final:
        if failures:
            raise Hold('Unresolved generation bills; aggregate spend is a lower bound')
        proof_found = {gen: read(state_dir / 'generations' / (gen + '.native.json')) for gen in unit['generation_ids']}
        usage = isolation.usage(runtime)
        proof = bench_cost.summarize(unit['generation_ids'], proof_found, [], {}, usage, telemetry)
        write(state_dir / 'units' / unit['key'] / 'accounting.json', proof)
        if proof['status'] != 'complete':
            raise Hold('Native bills do not prove complete generation coverage')
        unit['accounting'] = proof
    reserved = Decimal(unit['pending_reserved_usd'])
    campaign_limit = min(Decimal(75), Decimal(state.get('starting_credits_usd', '75')))
    pilot_spend = spend(state['ledger'], {r['unit'] for r in state['ledger'].values() if r.get('stage') == 'pilot'})
    if not unit.get('abort_reason') and (spend(state['ledger']) + reserved > campaign_limit or
            (unit['stage'] == 'pilot' and pilot_spend + reserved > 10)):
        raise Hold('Native spend plus unresolved/in-flight reservation exceeds cumulative allowance')


def qualification_for(state, model):
    return next(unit for unit in state['units'] if unit['kind'] == 'qualification' and unit['model'] == model)


def finish(state_dir, state, unit, config, modules):
    reconcile(state_dir, state, unit, config, modules, final=True)
    private = state_dir / 'units' / unit['key']
    if unit.get('abort_reason'):
        unit.update(status='unscored', reason=unit['abort_reason'])
        return
    if unit['kind'] == 'qualification':
        failure = private / 'qualification-failure.json'
        if failure.exists():
            unit.update(status='unscored', reason=read(failure)['reason'])
            return
        qualification = read(private / 'qualification.json')
        if qualification['status'] != 'tool-loop-observed':
            raise Hold('Qualification tool history is not observed')
        receipts = [state['ledger'][gen] for gen in unit['generation_ids']]
        unit['cache'] = cache_status(receipts)
        unit['status'] = 'qualified' if unit['cache']['status'] == 'native-cache-hit' else 'no-native-cache'
        if unit['model'] in PILOTS and unit['status'] != 'qualified':
            state['launch_blocked'] = 'Pilot native caching is unproven'
            raise Hold('Pilot route did not demonstrate native cached-input hits')
    else:
        result = ui(config, 'result', 'run=' + unit['runId'])
        write(private / 'app-result.json', result)
        verdict = result.get('verdict')
        if not isinstance(verdict, dict):
            unit.update(status='unscored', reason='Saved tree requires free scoring recovery; no paid restart')
            raise Hold(unit['reason'])
        if verdict.get('taskFingerprint') != config['taskFingerprint'] or verdict.get('buildId') != unit['runId']:
            raise Hold('Result does not preserve original frozen launch/build identity')
        write(private / 'verdict.json', verdict)
        if verdict.get('status') != 'scored' or verdict.get('publishable') is not True:
            unit.update(status='unscored', reason='Infrastructure/provider/wallet result, retained for free recovery')
            if unit['stage'] == 'pilot':
                raise Hold('Paired pilot lacks complete capability evidence')
            return
        unit.update(status='completed', score=verdict['score'])
        receipts = [state['ledger'][gen] for gen in unit['generation_ids']]
        unit['cache_evidence'] = build_cache_evidence(receipts)
        write(private / 'build-cache-evidence.json', unit['cache_evidence'])
        if unit['model'] == 'openai/gpt-6.1-sol' and unit['stage'] == 'pilot' and verdict['score'] >= .90:
            state['launch_blocked'] = 'Sol reached the prospective difficulty ceiling'
            raise Hold('Sol completed >=0.90: deepen the task prospectively and freeze again; spend never resets')
        if unit['cache_evidence']['status'] != 'native-cache-observed':
            state['launch_blocked'] = 'Full-build native caching is absent or unknown; free diagnosis is required'
            raise Hold(state['launch_blocked'])


def export(state_dir, state, config, modules):
    builds = []
    for unit in state['units']:
        if unit['kind'] == 'build' and unit['status'] == 'completed':
            private = state_dir / 'units' / unit['key']
            builds.append({'model': unit['model'], 'episode': unit['episode'],
                           'engine_sha256': config['engine_sha256'], 'settings': SETTINGS,
                           'verdict': read(private / 'verdict.json'), 'cost': unit['accounting']})
    if builds:
        report = modules[3].compare(builds)
        if sha_bytes(report['cohortCanonicalJson']) != report['comparisonCohortSha'] \
                or report['cohort']['taskFingerprint'] != config['taskFingerprint']:
            raise Hold('Comparison cohort digest/task identity differs')
        report['campaign_billed_usd_including_qualifications_and_failures'] = str(spend(state['ledger']))
        report['build_cache_evidence'] = [{'model': u['model'], 'episode': u['episode'], 'buildId': u['runId'], 'evidence': u.get('cache_evidence')} for u in state['units'] if u['kind'] == 'build' and u['status'] == 'completed']
        report['provisional_models'] = [model for model, value in report['models'].items() if value['independent_builds'] < 2]
        report['unscored'] = [{k: unit.get(k) for k in ('key', 'model', 'episode', 'status', 'reason')}
                              for unit in state['units'] if unit['status'] not in ('completed', 'qualified')]
        write(state_dir / 'comparison.json', report)
        pilot_builds = [b for b in builds if b['model'] in PILOTS]
        if len(pilot_builds) == 4:
            write(state_dir / 'pilot-comparison.json', modules[3].compare(pilot_builds))
        state['comparisonCohortSha'] = report['comparisonCohortSha']


def sha_bytes(value):
    import hashlib
    return hashlib.sha256(value.encode()).hexdigest()


def alive(pid):
    if not pid:
        return False
    try:
        waited, _ = os.waitpid(pid, os.WNOHANG)
        if waited:
            return False
    except (ChildProcessError, ProcessLookupError):
        pass
    try:
        status = subprocess.check_output(['ps', '-p', str(pid), '-o', 'stat='], text=True).strip()
    except subprocess.CalledProcessError:
        return False
    return bool(status) and not status.startswith('Z')


def process_rows():
    result = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,args='], text=True, timeout=4)
    rows = {}
    for line in result.splitlines():
        match = re.match(r'^\s*(\d+)\s+(\d+)\s+(.*)$', line)
        if match:
            rows[int(match[1])] = {'parent': int(match[2]), 'args': match[3]}
    return rows


def qualification_pids(snapshot, root_pid, unit_path):
    root = snapshot.get(root_pid)
    if root is None:
        return []
    if root_pid == os.getpid() or 'cache_probe.py' not in root['args'] or str(unit_path) not in root['args']:
        raise Hold('Qualification PID is not the owned private unit; shutdown refused')
    owned = [root_pid]
    while True:
        added = [pid for pid, row in snapshot.items() if row['parent'] in owned and pid not in owned and pid != os.getpid()]
        if not added:
            break
        owned.extend(added)
    return list(reversed(owned))  # descendants first; each signal is to one verified PID


def stop_qualification(unit, private, engine):
    original = process_rows()
    if not unit.get('pid'):
        matches = [pid for pid, row in original.items() if 'cache_probe.py' in row['args'] and str(private / 'unit.json') in row['args']]
        if len(matches) > 1:
            raise Hold('Qualification dispatch has multiple matching processes; owned stop is ambiguous')
        if matches:
            unit['pid'] = matches[0]
    owned = qualification_pids(original, unit.get('pid'), private / 'unit.json')
    native = [pid for pid in owned if original[pid]['args'].startswith(str(engine) + ' run ')]
    # Stop generation before slower child teardown. An unstarted probe is stopped first so it
    # cannot spawn a new engine after the ancestry snapshot.
    owned = native + [pid for pid in owned if pid not in native] if native else list(reversed(owned))
    stopped = []
    for pid in owned:
        # Recheck identity immediately before each signal; a reused pid must never be killed.
        current = process_rows().get(pid)
        if current is None:
            continue
        if current['args'] != original[pid]['args']:
            raise Hold('Owned qualification process changed identity before shutdown')
        try:
            os.kill(pid, signal.SIGKILL)
            stopped.append(pid)
        except ProcessLookupError:
            pass
    # ps can retain zombies; they cannot generate and alive() reaps direct child zombies.
    if any(alive(pid) for pid in owned):
        raise Hold('Owned qualification process remains live after per-PID shutdown')
    return stopped


def abort_active(state_dir, state, unit, config, reason):
    unit.update(abort_reason=reason, reason=reason, status='stopping-unscored')
    save(state_dir, state)  # Durable before any stop; no crash can authorize a replacement launch.
    if unit['kind'] == 'qualification':
        stopped = stop_qualification(unit, state_dir / 'units' / unit['key'], config['engine'])
        unit['stop_receipt'] = {'surface': 'owned private qualification per-PID shutdown', 'pids': stopped}
        write(state_dir / 'units' / unit['key'] / 'qualification-failure.json', {'status': 'unscored', 'reason': reason})
    else:
        if not unit.get('runId'):
            sessions = ui(config, 'inspect')['sessions']
            additions = [row for row in sessions if row['runId'] not in unit['baseline_run_ids']]
            if len(additions) != 1:
                raise Hold('Ambiguous launch still requires owned-session recovery before cancellation')
            unit.update(additions[0])
            if unit.get('dataDir'):
                unit['workdir'] = unit['dataDir']
        unit['stop_receipt'] = ui(config, 'cancel', '--execute-mutations', 'run=' + unit['runId'])
    unit['status'] = 'stopped-unscored'
    save(state_dir, state)


def observe(state_dir, state, unit, config, modules):
    private = state_dir / 'units' / unit['key']
    if unit['kind'] == 'qualification':
        if unit['status'] == 'armed' and not unit.get('pid'):
            raise Hold('Qualification dispatch interrupted at ambiguous boundary; never duplicate paid start')
        if alive(unit.get('pid')):
            # PID identity must match the exact private unit, not merely a reused process number.
            args = subprocess.check_output(['ps', '-p', str(unit['pid']), '-o', 'args='], text=True)
            if str(private / 'unit.json') not in args or 'cache_probe.py' not in args:
                raise Hold('Qualification PID was reused; require billing-only recovery')
            reconcile(state_dir, state, unit, config, modules)
            return False
        if not (private / 'qualification.json').exists() and not (private / 'qualification-failure.json').exists():
            raise Hold('Qualification process vanished without a durable outcome; paid restart refused')
    else:
        observed = ui(config, 'inspect')['sessions']
        if not unit.get('runId'):
            additions = [row for row in observed if row['runId'] not in unit['baseline_run_ids']]
            if len(additions) != 1:
                raise Hold('Benchmark click outcome is ambiguous; paid restart refused')
            unit.update(additions[0])
            save(state_dir, state)
        row = next((row for row in observed if row['runId'] == unit['runId']), None)
        if row is None:
            raise Hold('Original app session is absent; paid restart refused')
        if row.get('dataDir'):
            unit['workdir'] = row['dataDir']
        reconcile(state_dir, state, unit, config, modules)
        if row['outcome'] == 'running':
            return False
    finish(state_dir, state, unit, config, modules)
    export(state_dir, state, config, modules)
    save(state_dir, state)
    return True


def start(state_dir, state, unit, config, modules):
    verify_installed(config, modules)
    inspection = ui(config, 'inspect')
    if inspection['form']['running'] or any(row['outcome'] == 'running' for row in inspection['sessions']):
        raise Hold('Another entrant/scorer owns the app; serial launch refused')
    rows, catalog_sha = catalog()
    row = rows.get(unit['model'])
    if row is None:
        unit.update(status='unavailable', reason='Exact roster ID absent from current catalog; no substitution')
        save(state_dir, state)
        return
    unit['catalog'], unit['catalog_sha256'] = metadata(row), catalog_sha
    private = state_dir / 'units' / unit['key']
    private.mkdir(parents=True, exist_ok=True, mode=0o700)
    unit['credit_readback'] = live_credit_evidence(modules)
    current_credit = unit['credit_readback']['effective_remaining_usd']
    wallet = allowance(state['ledger'], state['units'], unit['stage'] == 'pilot', config['initial_headroom_usd'], state.get('starting_credits_usd'), current_credit)
    if unit['kind'] == 'build':
        qualification = qualification_for(state, unit['model'])
        if unit['stage'] == 'pilot' and qualification['status'] != 'qualified':
            raise Hold('Pilot requires its own six-call native cache-hit qualification')
        if qualification['status'] not in ('qualified', 'no-native-cache'):
            unit.update(status='unscored', reason='Own actual route qualification is unavailable')
            save(state_dir, state)
            return
        # Include this route's own native six-call requests and any earlier completed build's
        # larger envelopes. This is a screening reserve, never a replacement context/output cap.
        receipts = [receipt for receipt in state['ledger'].values()
                    if receipt['unit'] in {u['key'] for u in state['units']
                        if u['model'] == unit['model'] and u['status'] in ('qualified', 'no-native-cache', 'completed')}
                    and receipt.get('kind') in ('entrant', 'auxiliary')]
        unit['start_reservation'] = reserve_from_qualification(receipts, rows, config.get('billing_lag_calls_reserved', 2))
        headroom = max(Decimal(config['initial_headroom_usd']), Decimal(unit['start_reservation']['usd']))
        wallet = allowance(state['ledger'], state['units'], unit['stage'] == 'pilot', headroom, state.get('starting_credits_usd'), current_credit)
        if Decimal(qualification['cache']['projected_build_usd']) > wallet:
            unit.update(status='budget-held', reason='Own measured conservative projection exceeds remaining allowance')
            save(state_dir, state)
            if unit['stage'] == 'pilot':
                raise Hold('Paired pilot projection exceeds its aggregate $10 allowance')
            return
    else:
        wallet = min(wallet, Decimal(config['qualification_wallet_usd']))
    unit['wallet'] = str(wallet)
    if unit['kind'] == 'qualification':
        unit['workdir'] = str(private / 'workspace')
        write(private / 'unit.json', unit)
        unit['status'] = 'armed'
        save(state_dir, state)  # Crash after this boundary never authorizes another paid process.
        with (private / 'console.log').open('ab') as output:
            proc = subprocess.Popen([sys.executable, str(HERE / 'cache_probe.py'), str(state_dir / 'config.json'),
                                     str(private / 'unit.json'), unit['workdir'], '--execute-paid'],
                                    stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        unit.update(pid=proc.pid, status='running')
    else:
        ui(config, 'configure', '--execute-mutations', 'model=' + unit['model'], 'wallet=' + unit['wallet'])
        # Saving model/wallet fields is async. Verify the installed engine's actual config before the click.
        end = time.monotonic() + 60
        while True:
            try:
                ensure_config(modules[4].entrant_config('openrouter'), unit['model'], unit['wallet'])
                break
            except Hold:
                if time.monotonic() > end:
                    raise
                time.sleep(.25)  # Local config transport; no model work has started.
        unit.update(status='armed', baseline_run_ids=[row['runId'] for row in inspection['sessions']])
        save(state_dir, state)
        launched = ui(config, 'launch', '--execute-paid', 'model=' + unit['model'], 'wallet=' + unit['wallet'])
        unit.update(status='running', runId=launched['runId'], workdir=launched.get('dataDir'))
    save(state_dir, state)


def run(state_dir):
    config = read(state_dir / 'config.json')
    modules = load_installed(config['payload'], config['engine'])
    verify_installed(config, modules)
    with (state_dir / 'controller.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = read(state_dir / 'state.json')
        if state.get('launch_blocked'):
            raise Hold(state['launch_blocked'] + '; no paid resume is authorized on this task')
        state.update(controller_pid=os.getpid(), status='running', hold_reason=None)
        save(state_dir, state)
        try:
            while True:
                active = next((unit for unit in state['units'] if unit['status'] in ('armed', 'running', 'stopping-unscored', 'stopped-unscored')), None)
                if active:
                    if active['status'] == 'stopping-unscored':
                        abort_active(state_dir, state, active, config, active['abort_reason'])
                    if not observe(state_dir, state, active, config, modules):
                        time.sleep(2)  # Metadata observation cadence, never a model deadline.
                    continue
                if (state_dir / 'STOP').exists():
                    state['status'] = 'stopped-after-unit'
                    break
                pending = next((unit for unit in state['units'] if unit['status'] == 'pending'), None)
                if pending is None:
                    state['status'] = 'finished-with-recorded-absences'
                    break
                if pending['stage'] == 'roster':
                    pilot = [unit for unit in state['units'] if unit['kind'] == 'build' and unit['stage'] == 'pilot']
                    if any(unit['status'] != 'completed' for unit in pilot):
                        raise Hold('Four complete matched pilot builds are required before the roster')
                if pending['stage'] == 'pilot' and any(u['stage'] == 'pilot' and u['status'] in ('unavailable', 'unscored', 'budget-held', 'no-native-cache') for u in state['units']):
                    raise Hold('Pilot has a retained unresolved outcome; recover freely before paid continuation')
                start(state_dir, state, pending, config, modules)
        except Exception as error:
            reason = str(error) if isinstance(error, Hold) else type(error).__name__
            active = next((unit for unit in state['units'] if unit['status'] in ('armed', 'running', 'stopping-unscored')), None)
            if active:
                try:
                    abort_active(state_dir, state, active, config, reason)
                    # Capture IDs after stopped streams close. Missing bills remain explicit and
                    # the original unit is observed/billed on resume, never relaunched.
                    reconcile(state_dir, state, active, config, modules, final=True)
                    active['status'] = 'unscored'
                except Exception as stop_error:
                    active['stop_error'] = str(stop_error) if isinstance(stop_error, Hold) else type(stop_error).__name__
                    reason += '; owned stop/billing requires recovery: ' + active['stop_error']
            state.update(status='held', hold_reason=reason)
            try:
                export(state_dir, state, config, modules)
            except Exception as export_error:
                state['comparison_error'] = str(export_error) if isinstance(export_error, Hold) else type(export_error).__name__
        finally:
            save(state_dir, state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('init', 'refreeze', 'recover', 'preflight', 'status', 'run', 'detach', 'stop'))
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--config', type=Path)
    parser.add_argument('--execute-paid', action='store_true')
    args = parser.parse_args()
    state_dir = args.state.resolve()
    if args.command == 'init':
        if (state_dir / 'state.json').exists() or not args.config:
            parser.error('init needs a new state directory and a frozen config file')
        config = read(args.config)
        if Decimal(config['starting_credits_usd']) <= 0 or Decimal(config['initial_headroom_usd']) <= 0 or Decimal(config['qualification_wallet_usd']) <= 0:
            raise Hold('Positive verified starting credits and explicit reservations are required')
        modules = load_installed(config['payload'], config['engine'])
        verify_installed(config, modules)
        roster = read(HERE / 'ROSTER.json')
        if len(roster['models']) != 26 or len(set(roster['models'])) != 26 \
                or roster['pilot_max_usd'] != 10 or roster['campaign_max_usd'] != 75:
            raise Hold('Exact 26-model roster/aggregate policy differs')
        state_dir.mkdir(parents=True, mode=0o700)
        os.chmod(state_dir, 0o700)
        write(state_dir / 'config.json', config)
        units = plan(roster)
        for unit in units:
            unit['key'] = config['taskFingerprint'][:12] + '-' + unit['key']
        save(state_dir, {'schema': 1, 'status': 'ready', 'taskFingerprint': config['taskFingerprint'],
                         'units': units, 'ledger': {}, 'history': [], 'starting_credits_usd': config['starting_credits_usd']})
    elif args.command in ('refreeze', 'recover'):
        with (state_dir / 'controller.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            state = read(state_dir / 'state.json')
            config = read(args.config if args.command == 'refreeze' and args.config else state_dir / 'config.json')
            modules = load_installed(config['payload'], config['engine'])
            verify_installed(config, modules)
            active = [u for u in state['units'] if u['status'] in ('armed', 'running', 'stopping-unscored', 'stopped-unscored')]
            if active and (args.command == 'refreeze' or any(not u.get('abort_reason') for u in active)):
                raise Hold('Recover the original active launch before changing task or scoring state')
            if args.command == 'refreeze':
                if not args.config or config['taskFingerprint'] == state['taskFingerprint']:
                    raise Hold('Prospective refreeze requires a different verified frozen task')
                state['history'].extend(state['units'])
                state['units'] = plan(read(HERE / 'ROSTER.json'))
                for unit in state['units']:
                    unit['key'] = config['taskFingerprint'][:12] + '-' + unit['key']
                state.update(taskFingerprint=config['taskFingerprint'], status='ready', launch_blocked=None, hold_reason=None)
                write(state_dir / 'config.json', config)
                (state_dir / 'STOP').unlink(missing_ok=True)
            else:
                try:
                    for unit in active:
                        if unit['status'] == 'stopping-unscored':
                            abort_active(state_dir, state, unit, config, unit['abort_reason'])
                        reconcile(state_dir, state, unit, config, modules, final=True)
                        unit['status'] = 'unscored'
                    for unit in state['units']:
                        if unit['kind'] == 'build' and unit['status'] == 'unscored' and unit.get('runId'):
                            finish(state_dir, state, unit, config, modules)
                    export(state_dir, state, config, modules)
                finally:
                    save(state_dir, state)
            save(state_dir, state)
    elif args.command == 'preflight':
        config = read(state_dir / 'config.json')
        verify_installed(config, load_installed(config['payload'], config['engine']))
        print(json.dumps({'installed_payload': 'verified', 'taskFingerprint': config['taskFingerprint'],
                          'ui': ui(config, 'inspect')['form'],
                          'current_credit': live_credit_evidence(load_installed(config['payload'], config['engine']))}))
    elif args.command == 'status':
        state = read(state_dir / 'state.json')
        print(json.dumps({k: state.get(k) for k in ('status', 'hold_reason', 'updated_at', 'controller_pid',
                                                 'billed_usd', 'taskFingerprint', 'comparisonCohortSha')} | {'active': [{k: u.get(k) for k in ('key', 'model', 'kind', 'status', 'runId', 'workdir', 'pending_reserved_usd')} for u in state['units'] if u['status'] in ('armed', 'running', 'stopping-unscored', 'stopped-unscored')]}))
    elif args.command == 'stop':
        (state_dir / 'STOP').touch()
        print('Will stop after the current unit and its billing/scoring finish.')
    else:
        if not args.execute_paid:
            parser.error('--execute-paid is required after freeze')
        if args.command == 'detach':
            with (state_dir / 'controller.log').open('ab') as output:
                proc = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), 'run', '--state', str(state_dir),
                                         '--execute-paid'], stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            print(json.dumps({'controller_pid': proc.pid, 'state': str(state_dir)}))
        else:
            run(state_dir)


if __name__ == '__main__':
    main()
