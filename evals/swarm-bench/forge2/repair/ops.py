"""Private, exact-generation accounting for the repaired Forge 2.0 campaign.

No generation request is implemented here. Billing/catalog GETs are free. Scored entrants can
only be launched by dispatch.mjs clicking the installed app's Benchmark control.
"""
from decimal import Decimal
import hashlib
import json
import os
import plistlib
import re
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request

ROUTERS = {'typesafe/jev-router', 'unbiased/pareto-26.10-preview', 'openai/gpt-terra-latest'}
SETTINGS = {'provider': 'openrouter', 'reasoning_effort': 'medium', 'host': 'Any host',
            'model_fields': {}, 'sampling': 'native defaults', 'call_budget': 300}
PILOTS = {'xiaomi/mimo-v2.6-flash', 'openai/gpt-6.1-sol'}
SUPPORT_FILES = ('campaign.py', 'cache_probe.py', 'dispatch.mjs', 'ops.py', 'ROSTER.json')


class Hold(RuntimeError):
    pass


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_name(path.name + '.new')
    with temporary.open('w') as stream:
        os.chmod(temporary, 0o600)
        json.dump(value, stream, indent=2, allow_nan=False, default=str)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def load_installed(payload, engine):
    payload = Path(payload).resolve()
    os.environ['BENCH_GOOSE'] = str(engine)
    sys.path.insert(0, str(payload / 'bench'))
    import bench_cost, bench_isolation, bench_task_identity, forge2_compare, run_build
    if Path(run_build.ROOT).resolve() != payload or Path(run_build.GOOSE).resolve() != Path(engine).resolve():
        raise Hold('Harness or engine import came from another checkout')
    return bench_cost, bench_isolation, bench_task_identity, forge2_compare, run_build


def verify_installed(config, modules):
    pins = config.get('support_sha256')
    if not isinstance(pins, dict) or set(pins) != set(SUPPORT_FILES) or any(sha(Path(__file__).resolve().parent / name) != pins[name] for name in SUPPORT_FILES):
        raise Hold('Campaign support code differs from the frozen executable/config roster pins')
    bundle = Path(config['app_bundle']).resolve()
    resources = bundle / 'Contents/Resources'
    if Path(config['payload']).resolve() != resources / 'swarm-bench' or Path(config['engine']).resolve() != resources / 'bin/goose':
        raise Hold('Engine/payload must both belong to the verified installed app bundle')
    with (bundle / 'Contents/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    if info.get('CFBundleShortVersionString') != config['app_version']:
        raise Hold('Installed app version differs from the frozen release')
    release = read(config['release_receipt'])
    if sha(config['release_receipt']) != config['release_receipt_sha256'] or re.fullmatch('[0-9a-f]{40}', config['source_commit']) is None:
        raise Hold('Frozen release receipt or full source commit is invalid')
    expected = {'appVersion': config['app_version'], 'sourceCommit': config['source_commit'],
                'engineSha256': config['engine_sha256'], 'taskFingerprint': config['taskFingerprint']}
    if any(release.get(key) != value for key, value in expected.items()):
        raise Hold('Release receipt does not attribute this exact app engine and task to the frozen source')
    # The receipt is release-process provenance; a matching string in a binary is not that proof.
    identity = modules[2].capture(Path(config['payload']), 'preflight')
    if identity['taskFingerprint'] != config['taskFingerprint']:
        raise Hold('Installed Forge2 task differs from frozen campaign fingerprint')
    registration = read(config['registration_receipt'])
    if sha(config['registration_receipt']) != config['registration_receipt_sha256'] or registration.get('scorerVersion') != 'forge-2.0' or registration.get('releaseManifestSha256') != config['taskFingerprint']:
        raise Hold('Frozen site registration readback differs from the installed task')
    if sha(config['engine']) != config['engine_sha256']:
        raise Hold('Installed Goose engine differs from frozen binary')
    forbidden = ('BENCH_CONTEXT_LIMIT', 'BENCH_MAX_TOKENS', 'BENCH_RESUME_FROM', 'BENCH_AMEND_SPEC', 'BENCH_SPEC')
    if any(os.environ.get(name) for name in forbidden):
        raise Hold('Inherited model/task/resume override is present')
    if os.environ.get('BENCH_REASONING_EFFORT') not in (None, '', 'medium'):
        raise Hold('Inherited effort differs from the common medium policy')
    return identity


def catalog():
    with urllib.request.urlopen('https://openrouter.ai/api/v1/models', timeout=30) as response:
        raw = response.read()
    rows = json.loads(raw)['data']
    return {row['id']: row for row in rows}, hashlib.sha256(raw).hexdigest()


def metadata(row):
    return {name: row.get(name) for name in ('id', 'canonical_slug', 'context_length', 'pricing',
                                           'top_provider', 'supported_parameters', 'reasoning')}


def ensure_config(snapshot, model, wallet=None):
    config = snapshot['config']
    if config.get('OPENROUTER_PARAMETERS') not in (None, {}, ''):
        raise Hold('Any host must remove the saved OpenRouter routing parameters')
    fields = config.get('GOOSE_MODEL_FIELDS') or {}
    if isinstance(fields, str):
        fields = json.loads(fields)
    saved = fields.get('openrouter', {}).get(model, {})
    if any(key != 'effort' for key in saved):
        raise Hold('Non-default model fields remain in the actual engine snapshot')
    if config.get('GOOSE_MAX_TOKENS') not in (None, ''):
        raise Hold('Saved output-token override would change the model treatment')
    if wallet is not None:
        try:
            saved_wallet = Decimal(str(config.get('BENCH_MAX_USD')))
        except ArithmeticError:
            raise Hold('UI wallet change has not reached the installed engine config') from None
        if not saved_wallet.is_finite() or saved_wallet != Decimal(str(wallet)):
            raise Hold('UI wallet change has not reached the installed engine config')
    if not snapshot['secrets'].get('OPENROUTER_API_KEY'):
        raise Hold('Selected OpenRouter credentials are unavailable')
    return snapshot


def scan_requests(runtime):
    """Read only request identity/hash; never persist prompts, keys or provider request headers."""
    records, active = {}, {}
    for log in (Path(runtime) / 'goose').rglob('llm_request*.jsonl'):
        try:
            lines = log.read_text(errors='replace').splitlines()
        except FileNotFoundError:
            continue  # The engine may rename a live request into its ten-file rotation.
        payload, ids = None, []
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue  # Only complete JSON lines are eligible; the next scan rereads the live file.
            if isinstance(record, dict) and isinstance(record.get('input'), dict):
                payload = record['input']
            data = record.get('data') if isinstance(record, dict) else None
            if isinstance(data, dict) and isinstance(data.get('id'), str) and data['id'].startswith('gen-'):
                ids.append(data['id'])
        if payload is None:
            continue
        identity = {'input_json_bytes': len(json.dumps(payload, ensure_ascii=True).encode()),
                    'max_tokens_sent': payload.get('max_tokens', payload.get('max_completion_tokens')),
                    'reasoning_sent': payload.get('reasoning'),
                    'sampling_sent': {k: payload[k] for k in ('temperature','top_p','top_k','min_p','top_a','frequency_penalty','presence_penalty','repetition_penalty') if k in payload},
                    'requested_model': payload.get('model'),
                    'kind': 'entrant' if payload.get('tools') else 'auxiliary',
                    'request_sha256': hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(',', ':')).encode()).hexdigest()}
        # Only writer-pid/UUID names are live. Rotated files can be old failed requests without IDs.
        if re.fullmatch(r'llm_request\.[0-9]+\.[0-9a-f-]{36}\.jsonl', log.name):
            active[log.name] = {'generation_ids': sorted(set(ids)), **identity}
        for gen in set(ids):
            if gen in records and records[gen] != identity:
                raise Hold('Conflicting actual request identities for ' + gen)
            records[gen] = identity
    return records, active


def request_routes(runtime):
    return scan_requests(runtime)[0]


def bill_receipt(gen, data, route, row):
    if data.get('id') != gen or not isinstance(data.get('model'), str) or not data.get('provider_name'):
        raise Hold('Native generation model/provider identity is incomplete')
    amount = Decimal(str(data['usage']))
    if not amount.is_finite() or amount < 0:
        raise Hold('Invalid native billed amount')
    if not route or not route.get('requested_model'):
        raise Hold('Actual outgoing request identity was not retained for ' + gen)
    requested = row['id']
    if route['kind'] == 'entrant':
        if route['requested_model'] != requested:
            raise Hold('Exact requested route was substituted before generation')
        accepted = {requested, row.get('canonical_slug')}
        if requested not in ROUTERS and data['model'] not in accepted:
            raise Hold('Served route is not the exact ID or its current primary catalog canonical_slug')
    return {'id': gen, 'billed_usd': str(amount), 'billed_model': data['model'],
            'provider_name': data['provider_name'], 'router': data.get('router'),
            'native_tokens_prompt': data.get('native_tokens_prompt'),
            'native_tokens_completion': data.get('native_tokens_completion'),
            'native_tokens_cached': data.get('native_tokens_cached'),
            'native_tokens_reasoning': data.get('native_tokens_reasoning'),
            'cache_discount': data.get('cache_discount'), **route}


def merge_receipts(ledger, unit, receipts, stage=None):
    for receipt in receipts:
        old = ledger.get(receipt['id'])
        record = {'unit': unit, **receipt}
        if stage is not None:
            record['stage'] = stage
        if old is not None and old != record:
            raise Hold('Generation has conflicting billing or cross-unit provenance')
        ledger[receipt['id']] = record


def spend(ledger, units=None):
    return sum((Decimal(row['billed_usd']) for row in ledger.values()
                if units is None or row['unit'] in units), Decimal(0))


def allowance(ledger, units, pilot, headroom, starting_credits=None, current_credits=None):
    campaign = Decimal(75) - spend(ledger)
    if starting_credits is not None:
        campaign = min(campaign, Decimal(str(starting_credits)) - spend(ledger))
    if current_credits is not None:
        campaign = min(campaign, Decimal(str(current_credits)))
    pilot_units = {u['key'] for u in units if u['stage'] == 'pilot'} | {r['unit'] for r in ledger.values() if r.get('stage') == 'pilot'}
    remaining = min(campaign, Decimal(10) - spend(ledger, pilot_units)) if pilot else campaign
    headroom = Decimal(str(headroom))
    if remaining <= headroom:
        raise Hold('Aggregate allowance exhausted after reserving pending/in-flight spend')
    return remaining - headroom


def live_credit_evidence(modules, urlopen=urllib.request.urlopen):
    """Authenticated read-only evidence, stripped of labels, key hashes and credentials."""
    installed = modules[4]
    snapshot = installed.entrant_config('openrouter')
    credentials = installed.cloud_env('openrouter', snapshot)
    key = credentials.get('OPENROUTER_API_KEY')
    if not key:
        raise Hold('Native billing credentials are unavailable for current credit readback')
    host = (credentials.get('OPENROUTER_HOST') or 'https://openrouter.ai').rstrip('/')
    values = {}
    for name, path in (('credits', '/api/v1/credits'), ('key', '/api/v1/key')):
        request = urllib.request.Request(host + path, headers={'Authorization': 'Bearer ' + key})
        try:
            with urlopen(request, timeout=30) as response:
                body = json.loads(response.read(), parse_float=Decimal)
            values[name] = body['data']
        except (OSError, ValueError, KeyError, TypeError) as error:
            raise Hold('Current ' + name + ' readback is unavailable: ' + type(error).__name__) from None
    try:
        total, usage = (Decimal(str(values['credits'][field])) for field in ('total_credits', 'total_usage'))
        if not all(v.is_finite() and v >= 0 for v in (total, usage)):
            raise ValueError('invalid account totals')
        account_remaining = total - usage
        raw_limit, raw_remaining = values['key'].get('limit'), values['key'].get('limit_remaining')
        if raw_limit is None and raw_remaining is None:
            key_remaining = None
        else:
            limit, key_remaining = Decimal(str(raw_limit)), Decimal(str(raw_remaining))
            if not all(v.is_finite() and v >= 0 for v in (limit, key_remaining)):
                raise ValueError('invalid key limit')
        effective = account_remaining if key_remaining is None else min(account_remaining, key_remaining)
        if not effective.is_finite() or effective <= 0:
            raise ValueError('no current credit headroom')
    except (ArithmeticError, KeyError, TypeError, ValueError):
        raise Hold('Current account credit/key-limit evidence is nonfinite, missing or exhausted') from None
    return {'captured_at': time.time(), 'sources': [host + '/api/v1/credits', host + '/api/v1/key'],
            'total_credits': str(total), 'total_usage': str(usage), 'account_remaining_usd': str(account_remaining),
            'key_limit_remaining_usd': str(key_remaining) if key_remaining is not None else None,
            'effective_remaining_usd': str(effective), 'policy': 'No top-up/key mutation; current balance only tightens original cumulative allowances.'}


def cache_status(receipts):
    normal = [row for row in receipts if row['kind'] == 'entrant']
    if len(normal) != 6:
        raise Hold('Qualification did not make exactly six normal actual Goose calls')
    if any(type(r['native_tokens_cached']) is not int or type(r['native_tokens_prompt']) is not int
           or not 0 <= r['native_tokens_cached'] <= r['native_tokens_prompt'] for r in normal):
        raise Hold('Native cache token counts are missing or invalid')
    hits = [r['id'] for r in normal[1:] if r['native_tokens_cached'] > 0]
    warm = max(Decimal(r['billed_usd']) for r in normal[1:])
    # Screening estimate, not a guaranteed worst-case bill. Full builds grow their context.
    projection = max(Decimal(r['billed_usd']) for r in normal) + 300 * warm * 2
    return {'status': 'native-cache-hit' if hits else 'no-native-cache-hit',
            'native_cached_generation_ids': hits, 'entrant_calls': len(normal),
            'auxiliary_calls': len(receipts) - len(normal), 'projected_build_usd': str(projection),
            'projection_policy': 'largest first/qualified bill + 2 × largest warmed bill × 300; screening only',
            'headroom_usd': str(max(Decimal(r['billed_usd']) for r in receipts) * 2)}


def build_cache_evidence(receipts):
    normal = [r for r in receipts if r.get('kind') == 'entrant']
    invalid = [r['id'] for r in normal if type(r.get('native_tokens_prompt')) is not int or
               type(r.get('native_tokens_cached')) is not int or not
               0 <= r['native_tokens_cached'] <= r['native_tokens_prompt']]
    valid = [r for r in normal if r['id'] not in invalid]
    prompt = sum(r['native_tokens_prompt'] for r in valid)
    cached = sum(r['native_tokens_cached'] for r in valid)
    return {'status': 'unknown-native-cache' if invalid or not normal else
                     'native-cache-observed' if cached else 'no-native-cache-observed',
            'entrant_generations': len(normal), 'generations_with_native_cache_counts': len(valid),
            'unknown_generation_ids': invalid, 'native_prompt_tokens_observed': prompt,
            'native_cached_tokens_observed': cached, 'cached_fraction': cached / prompt if prompt else None,
            'generation_routes': [{k: r.get(k) for k in ('id', 'kind', 'requested_model', 'billed_model',
                'provider_name', 'native_tokens_prompt', 'native_tokens_cached', 'billed_usd')} for r in receipts],
            'policy': 'Native observed totals; no cutoff is inferred from six-call qualification or prior builds.'}


def request_reservation(receipt, row):
    """Uncached advertised-price reservation for an OBSERVED native request envelope.

    JSON bytes conservatively screen text input size; images/provider tokenization and a host's
    omitted output cap prevent this being a hard billing bound. Never changes native model limits.
    """
    pricing = row.get('pricing') or {}
    variants = [pricing, *pricing.get('overrides', [])]
    try:
        prompt = max(Decimal(str(v.get(k, pricing.get(k, 0)))) for v in variants
                     for k in ('prompt', 'input_cache_write'))
        completion = max(Decimal(str(v.get('completion', pricing.get('completion', 0)))) for v in variants)
        if prompt <= 0 or completion <= 0:
            raise ValueError('unpriced route')
        context = int(row['context_length'])
        input_bytes = int(receipt['input_json_bytes'])
        sent = receipt.get('max_tokens_sent')
        output = int(sent if sent is not None else row['top_provider']['max_completion_tokens'])
        if min(context, input_bytes, output) <= 0:
            raise ValueError('unavailable envelope')
    except (KeyError, TypeError, ValueError, ArithmeticError):
        observed = Decimal(receipt['billed_usd'])
        if observed <= 0:
            raise Hold('Unknown pending request envelope cannot receive a zero reserve')
        return {'usd': str(observed * 2),
                'source': 'twice observed native bill; advertised route envelope unavailable',
                'advertised_bound_available': False}
    return {'usd': str(min(context, input_bytes) * prompt + output * completion),
            'input_json_bytes': input_bytes, 'output_tokens': output,
            'output_source': 'actual max_tokens' if sent is not None else 'advertised top_provider maximum; no max_tokens sent',
            'uncached_prompt_or_write_rate': str(prompt), 'completion_rate': str(completion),
            'source': 'actual request byte envelope × largest advertised uncached/write prices + native/advertised output cap',
            'advertised_bound_available': True}


def completed_reservation(counts, row, observed_bill):
    """An ended stream cannot accrue another output cap; reserve its observed native usage."""
    pricing = row.get('pricing') or {}
    variants = [pricing, *pricing.get('overrides', [])]
    try:
        prompt, completion = counts['prompt'], counts['completion']
        if type(prompt) is not int or type(completion) is not int or prompt <= 0 or completion < 0:
            raise ValueError('unproven native usage')
        input_rate = max(Decimal(str(v.get(k, pricing.get(k, 0)))) for v in variants
                         for k in ('prompt', 'input_cache_write'))
        output_rate = max(Decimal(str(v.get('completion', pricing.get('completion', 0)))) for v in variants)
        if input_rate <= 0 or output_rate <= 0:
            raise ValueError('unpriced route')
        return {'usd': str(prompt * input_rate + completion * output_rate),
                'source': 'ended-stream native prompt/completion counts × largest advertised uncached/write prices',
                'native_tokens_prompt': prompt, 'native_tokens_completion': completion,
                'advertised_bound_available': True}
    except (KeyError, TypeError, ValueError, ArithmeticError):
        bill = Decimal(str(observed_bill))
        if bill <= 0:
            raise Hold('Unproven completed generation cannot receive a zero reserve')
        return {'usd': str(bill * 2), 'source': 'twice observed native bill; completed usage/tariff unavailable',
                'advertised_bound_available': False}


def reserve_from_qualification(receipts, rows, lag_calls=2):
    if type(lag_calls) is not int or lag_calls < 2:
        raise Hold('Pending-generation reserve must include delayed completed calls')
    normal, completed, auxiliary = [], [], []
    for receipt in receipts:
        row = rows.get(receipt['requested_model']) or {'id': receipt['requested_model']}
        reserve = request_reservation(receipt, row)
        if receipt['kind'] == 'entrant':
            normal.append(reserve)
            completed.append(completed_reservation({'prompt': receipt.get('native_tokens_prompt'),
                'completion': receipt.get('native_tokens_completion')}, row, receipt['billed_usd']))
        else:
            auxiliary.append(reserve)
    if not normal:
        raise Hold('Own route request envelope is unavailable')
    total = max(Decimal(r['usd']) for r in normal) + max(Decimal(r['usd']) for r in completed) * lag_calls + sum((Decimal(r['usd']) for r in auxiliary), Decimal(0))
    return {'usd': str(total), 'entrant_active_requests_reserved': 1,
            'entrant_lag_calls_reserved': lag_calls, 'auxiliary_requests_reserved': len(auxiliary),
            'normal_envelopes': normal, 'completed_envelopes': completed, 'auxiliary_envelopes': auxiliary,
            'limit': 'Reservation screens observed requests and ended native usage; future context and billing lag can exceed it. Wallet guard and prepaid credits remain necessary.'}
