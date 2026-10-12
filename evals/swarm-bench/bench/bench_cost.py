"""The provider's own bill for one benchmark entrant, read per call — never estimated.

goose's session `accumulated_cost` is a price-table estimate: run c605215d (DeepSeek on OpenRouter,
2026-10-02) said $0.293 where OpenRouter billed $0.6241 over the generations a live collector saw.
OpenRouter answers `GET /api/v1/generation?id=<gen id>` with the billed `usage` for each call, so the
bill is exact for every generation id the run left behind, and the record says which ones it could
not find instead of counting them as free.

Where the ids come from:
  * <workdir>/.swarm/telemetry.jsonl (authoritative): goose writes one line per OpenRouter call with
    `response_id` (the generation id) and `ended` ("completed", or "incomplete" when the stream was
    dropped first — still billed). Engines before that field wrote no id there (c605215d's file was
    0 bytes: OpenRouter did not stream through the telemetry path at all).
  * sessions.db assistant `message_id` (measured on c605215d): a streamed reply keeps the provider's
    id only on its leading text message; agent.rs re-ids every tool-request message with a fresh
    `msg_` uuid, so a call that opens with a tool call persists no id (43 gen ids for >= 143 calls).
  * llm_request*.jsonl in the runtime's state/logs: every chunk carries `data.id`, but the rotation
    keeps the newest 10 requests only.
Completeness is PROVEN, not assumed: the billed native token sums of the completed calls must equal
goose's own session counters (on c605215d the 140 collected generations summed to exactly the
per-call usage goose logged for them: 28,849,582 prompt / 283,606 completion / 28,473,088 cached).
An incomplete call carries no usage goose could count, so it is billed and listed but kept out of
that sum. Background title calls are also billed and recorded in provider telemetry, but their usage
never enters session counters. When recovered tokens exceed session counters, completeness instead
requires clean telemetry covering every generation and per-generation native token counts matching
each provider bill. Positive residuals and any unproven difference remain a lower bound.
"""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal
import json
from pathlib import Path
import re
import sqlite3
import time
import urllib.error
import urllib.parse
import urllib.request

GENERATION_ID = re.compile(r'gen-[0-9]+-[A-Za-z0-9]+')
MODEL_ID = re.compile(r'[^/\s]+/[^/\s]+')
# policy: OpenRouter writes the generation record after the stream ends; ids still unknown are
# re-asked on a doubling backoff (2+4+8+16+32 s) before they are published as missing.
LATE_RECORD_BACKOFF = (2, 4, 8, 16, 32)
# policy: a handful of concurrent reads, well inside OpenRouter's per-key request rate.
CONCURRENT_READS = 4
TOKEN_FIELDS = {'prompt': 'native_tokens_prompt', 'cached': 'native_tokens_cached',
                'completion': 'native_tokens_completion', 'reasoning': 'native_tokens_reasoning'}
# goose's per-session counters for the same quantities; reasoning is inside completion for both.
GOOSE_COUNTERS = {'prompt': 'accumulated_input_tokens', 'cached': 'accumulated_cache_read_tokens',
                  'completion': 'accumulated_output_tokens'}
UNQUERYABLE = {
    'google': 'Google exposes no per-call bill to query; its billing is aggregated per account',
    'aws_bedrock': 'Bedrock exposes no per-call bill to query; its billing is aggregated per account',
}


def unavailable(provider: str | None, reason: str | None = None) -> dict:
    if reason is None:
        if provider is None:
            reason = 'no cloud provider: the entrant runs on the local fleet, which issues no bill'
        else:
            reason = UNQUERYABLE.get(provider, f'no per-call billing API is wired for provider {provider}')
    return {'status': 'unavailable', 'provider': provider, 'reason': reason}


def _session_db_ids(runtime: Path) -> list[str]:
    ids = []
    for database in sorted((runtime / 'goose').rglob('sessions.db')):
        connection = sqlite3.connect('file:' + str(database) + '?mode=ro', uri=True)
        try:
            ids.extend(row[0] for row in connection.execute(
                "SELECT message_id FROM messages WHERE role = 'assistant' ORDER BY id")
                if row[0] and GENERATION_ID.fullmatch(row[0]))
        finally:
            connection.close()
    return ids


def _request_log_ids(runtime: Path) -> list[str]:
    ids = []
    for log in sorted((runtime / 'goose').rglob('llm_request*.jsonl')):
        for line in log.read_text(errors='replace').splitlines():
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            data = entry.get('data') if isinstance(entry, dict) else None
            if isinstance(data, dict) and isinstance(data.get('id'), str) \
                    and GENERATION_ID.fullmatch(data['id']):
                ids.append(data['id'])
                break
    return ids


def telemetry_calls(workdir: Path | None) -> dict:
    """The per-call lines goose wrote to the run's telemetry file, and how each call ended."""
    path = workdir / '.swarm' / 'telemetry.jsonl' if workdir is not None else None
    if path is None or not path.is_file():
        return {'status': 'absent', 'path': str(path) if path else None}
    calls, unparseable = [], 0
    for line in path.read_text(errors='replace').splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            unparseable += 1
            continue
        if isinstance(entry, dict):
            calls.append(entry)
        else:
            unparseable += 1
    # Lines from an engine before response_id carry no `ended`; they name no call a bill can find.
    recorded = [call for call in calls if 'ended' in call]
    if not recorded:
        return {'status': 'predates_response_id' if calls else 'empty', 'path': str(path),
                'lines': len(calls), 'unparseable_lines': unparseable}
    ended, native_usage, models = {}, {}, {}
    invalid_usage, conflicting_usage = set(), set()
    invalid_models, conflicting_models = set(), set()
    without_id = 0
    for call in recorded:
        gen = call.get('response_id')
        if isinstance(gen, str) and GENERATION_ID.fullmatch(gen):
            model = call.get('model')
            if not isinstance(model, str) or MODEL_ID.fullmatch(model) is None:
                invalid_models.add(gen)
            elif gen in models and models[gen] != model:
                conflicting_models.add(gen)
            else:
                models[gen] = model
            # A completed record wins over an earlier dropped stream for the same generation.
            if ended.get(gen) != 'completed':
                ended[gen] = call['ended']
            if call['ended'] == 'completed':
                counts = {name: call.get(name + '_tokens') for name in ('prompt', 'completion')}
                if call.get('usage') is not True or any(type(value) is not int or value < 0
                                                       for value in counts.values()):
                    invalid_usage.add(gen)
                elif gen in native_usage and native_usage[gen] != counts:
                    conflicting_usage.add(gen)
                else:
                    native_usage[gen] = counts
        else:
            without_id += 1
    return {'status': 'recorded', 'path': str(path), 'calls': len(recorded), 'ended': ended,
            'native_usage_by_id': native_usage, 'invalid_native_usage_ids': sorted(invalid_usage),
            'conflicting_native_usage_ids': sorted(conflicting_usage),
            'model_by_id': models, 'invalid_model_ids': sorted(invalid_models),
            'conflicting_model_ids': sorted(conflicting_models),
            'calls_without_response_id': without_id, 'calls_without_end_marker': len(calls) - len(recorded),
            'unparseable_lines': unparseable}


def collect_generation_ids(runtime: Path, workdir: Path | None = None) -> tuple[list[str], dict, dict]:
    """Every OpenRouter generation id the run kept — telemetry first — with per-source counts."""
    telemetry = telemetry_calls(workdir)
    sources = {'telemetry': list(telemetry.get('ended', {})),
               'sessions_db_message_ids': _session_db_ids(runtime),
               'request_logs': _request_log_ids(runtime)}
    ordered = list(dict.fromkeys(gen for ids in sources.values() for gen in ids))
    return ordered, {name: len(set(ids)) for name, ids in sources.items()}, telemetry


def _fetch(url: str, key: str, urlopen) -> tuple[dict | None, str | None]:
    request = urllib.request.Request(url, headers={'Authorization': f'Bearer {key}'})
    try:
        with urlopen(request, timeout=30) as response:
            body = json.loads(response.read().decode(), parse_float=Decimal)
    except urllib.error.HTTPError as error:
        return None, f'HTTP {error.code}'
    except (OSError, ValueError) as error:
        return None, type(error).__name__
    data = body.get('data') if isinstance(body, dict) else None
    if not isinstance(data, dict) or not isinstance(data.get('usage'), (Decimal, int)):
        return None, 'generation record carries no numeric usage'
    return data, None


def bill_generations(ids: list[str], key: str, host: str, urlopen=None,
                     sleep=None) -> tuple[dict, list[str], dict]:
    """Billed generation records by id, the ids found only on a re-ask, and the last failure per missing id."""
    urlopen = urlopen or urllib.request.urlopen
    sleep = sleep or time.sleep
    base = host.rstrip('/') + '/api/v1/generation?id='
    found: dict[str, dict] = {}
    late: list[str] = []
    failures: dict[str, str] = {}
    pending = list(ids)
    for attempt, wait in enumerate((0,) + LATE_RECORD_BACKOFF):
        if not pending:
            break
        if wait:
            sleep(wait)
        with ThreadPoolExecutor(max_workers=CONCURRENT_READS) as pool:
            answers = list(pool.map(lambda gen: _fetch(base + urllib.parse.quote(gen), key, urlopen), pending))
        still = []
        for gen, (data, failure) in zip(pending, answers):
            if data is None:
                failures[gen] = failure
                still.append(gen)
            else:
                found[gen] = data
                failures.pop(gen, None)
                if attempt:
                    late.append(gen)
        pending = still
    return found, late, failures


def _goose_counters(usage: dict | None) -> dict | None:
    if not isinstance(usage, dict) or usage.get('status') != 'recorded':
        return None
    sessions = usage.get('sessions') or []
    if not sessions or any(session.get(column) is None for session in sessions
                           for column in GOOSE_COUNTERS.values()):
        return None
    return {name: sum(session[column] for session in sessions) for name, column in GOOSE_COUNTERS.items()}


def _token_sums(records: list[dict]) -> dict:
    return {name: sum(record.get(field) or 0 for record in records) for name, field in TOKEN_FIELDS.items()}


def _telemetry_native_proof(ids: list[str], found: dict, telemetry: dict | None) -> dict:
    """An auxiliary call may exceed session counters only with exact provider/telemetry evidence."""
    telemetry = telemetry or {}
    ended = telemetry.get('ended', {})
    native_usage = telemetry.get('native_usage_by_id', {})
    failures = []
    if telemetry.get('status') != 'recorded':
        failures.append('authoritative per-call telemetry is unavailable')
    if telemetry.get('unparseable_lines') or telemetry.get('calls_without_end_marker'):
        failures.append('telemetry contains unparseable or unmarked calls')
    if telemetry.get('calls_without_response_id'):
        failures.append('telemetry contains calls without generation ids')
    outside = [gen for gen in ids if gen not in ended]
    if outside:
        failures.append('recovered generation ids are absent from telemetry')
    if telemetry.get('invalid_native_usage_ids') or telemetry.get('conflicting_native_usage_ids'):
        failures.append('completed telemetry calls have invalid or conflicting native usage')
    missing_usage, mismatched, checked = [], [], 0
    for gen in ids:
        if ended.get(gen) not in ('completed', 'incomplete'):
            failures.append('generation ' + gen + ' has no valid stream end state')
        if gen not in found:
            continue  # The missing provider bill is independently refused by summarize.
        record = found[gen]
        if record.get('id') != gen:
            mismatched.append(gen)
            continue
        if ended.get(gen) != 'completed':
            continue
        counts = native_usage.get(gen)
        if counts is None:
            missing_usage.append(gen)
            continue
        if any(type(record.get(TOKEN_FIELDS[name])) is not int or
               record[TOKEN_FIELDS[name]] != counts[name] for name in ('prompt', 'completion')):
            mismatched.append(gen)
        else:
            checked += 1
    if missing_usage:
        failures.append('completed telemetry calls lack native token counts')
    if mismatched:
        failures.append('provider native counts or generation ids contradict telemetry')
    return {'status': 'matched' if not failures else 'unproven',
            'generations_covered': len(ended), 'completed_native_counts_matched': checked,
            'missing_native_usage_ids': missing_usage, 'mismatched_generation_ids': mismatched,
            'reasons': failures}


def _cached_auxiliary_proof(ids: list[str], found: dict, telemetry: dict, counters: dict,
                            unattributed: dict, model: str) -> dict:
    """A model partition proves cached auxiliary usage only when every session counter matches."""
    proof = _telemetry_native_proof(ids, found, telemetry)
    reasons = list(proof['reasons'])
    if not isinstance(model, str) or MODEL_ID.fullmatch(model) is None:
        reasons.append('the explicit entrant model identity is unavailable')
    models = telemetry.get('model_by_id', {})
    if telemetry.get('invalid_model_ids') or telemetry.get('conflicting_model_ids'):
        reasons.append('telemetry model identities are invalid or conflicting')
    if set(ids) != set(telemetry.get('ended', {})) or len(ids) != len(set(ids)):
        reasons.append('generation ids do not exactly cover authoritative telemetry')
    if any(not isinstance(models.get(gen), str) or MODEL_ID.fullmatch(models[gen]) is None for gen in ids):
        reasons.append('a generation lacks its authoritative telemetry model identity')
    if any(gen not in found for gen in ids):
        reasons.append('a generation lacks its provider bill')
    if any(type(value) is not int or value < 0 for value in counters.values()) \
            or counters['cached'] > counters['prompt']:
        reasons.append('session token counters are invalid')
    completed = [gen for gen in ids if telemetry.get('ended', {}).get(gen) == 'completed' and gen in found]
    if any(type(found[gen].get(TOKEN_FIELDS[name])) is not int or found[gen][TOKEN_FIELDS[name]] < 0
           for gen in completed for name in GOOSE_COUNTERS) or any(
               found[gen].get(TOKEN_FIELDS['cached'], 0) > found[gen].get(TOKEN_FIELDS['prompt'], 0)
               for gen in completed):
        reasons.append('completed provider bills have invalid native token counts')
    entrant = [gen for gen in completed if models.get(gen) == model]
    auxiliary = [gen for gen in completed if models.get(gen) != model]
    if not entrant or not auxiliary:
        reasons.append('completed entrant and auxiliary model partitions are both required')
    if not reasons:
        entrant_tokens = {name: sum(found[gen][field] for gen in entrant)
                          for name, field in TOKEN_FIELDS.items() if name in GOOSE_COUNTERS}
        auxiliary_tokens = {name: sum(found[gen][field] for gen in auxiliary)
                            for name, field in TOKEN_FIELDS.items() if name in GOOSE_COUNTERS}
        if entrant_tokens != counters:
            reasons.append('entrant native token sums do not exactly match session counters')
        if auxiliary_tokens != {name: -value for name, value in unattributed.items()}:
            reasons.append('auxiliary native token sums do not exactly explain the session residual')
        proof.update(entrant_native_tokens=entrant_tokens, auxiliary_native_tokens=auxiliary_tokens)
    proof.update(status='matched' if not reasons else 'unproven', reasons=reasons,
                 entrant_model=model, entrant_generation_ids=entrant, auxiliary_generation_ids=auxiliary)
    return proof


def summarize(ids: list[str], found: dict, late: list[str], failures: dict, usage: dict | None,
              telemetry: dict | None = None, model: str = '') -> dict:
    records = [found[gen] for gen in ids if gen in found]
    tokens = _token_sums(records)
    hosts: dict[str, int] = {}
    for record in records:
        name = record.get('provider_name') or '(provider_name absent)'
        hosts[name] = hosts.get(name, 0) + 1
    missing = [{'id': gen, 'reason': failures.get(gen, 'not fetched')} for gen in ids if gen not in found]
    ended = (telemetry or {}).get('ended', {})
    incomplete_ids = [gen for gen in ids if ended.get(gen) == 'incomplete']
    invalid_cached_ids = [gen for gen in ids if gen in found and gen not in incomplete_ids and
                          (type(found[gen].get(TOKEN_FIELDS['cached'])) is not int or
                           found[gen][TOKEN_FIELDS['cached']] < 0 or
                           (type(found[gen].get(TOKEN_FIELDS['prompt'])) is int and
                            found[gen][TOKEN_FIELDS['cached']] > found[gen][TOKEN_FIELDS['prompt']]))]
    counters = _goose_counters(usage)
    if counters is None:
        reconciliation = {'status': 'unavailable',
                          'reason': 'no goose session token counters to prove every call was found'}
        unattributed = None
    else:
        # goose counts a call's usage only when its stream completed, so a dropped-but-billed call
        # is billed above and kept out of the cross-check.
        counted = _token_sums([found[gen] for gen in ids if gen in found and gen not in incomplete_ids])
        unattributed = {name: counters[name] - counted[name] for name in counters}
        reconciliation = {'status': 'matched' if not any(unattributed.values()) else 'unattributed_tokens',
                          'against': 'goose session counters (sessions.db accumulated_*)',
                          'goose_tokens': counters, 'unattributed_tokens': unattributed}
        # Session naming uses complete_fast but does not update session metrics. Its provider bill
        # must still count. A negative residual alone cannot prove that all other calls were found.
        if any(unattributed.values()) and all(value <= 0 for value in unattributed.values()) \
                and unattributed['cached'] == 0:
            proof = _telemetry_native_proof(ids, found, telemetry)
            reconciliation['telemetry_native_proof'] = proof
            if proof['status'] == 'matched':
                reconciliation['status'] = 'billed_calls_outside_session_counters'
                reconciliation['billed_tokens_outside_session_counters'] = {
                    name: -value for name, value in unattributed.items()}
        elif all(value <= 0 for value in unattributed.values()) and unattributed['cached'] < 0:
            proof = _cached_auxiliary_proof(ids, found, telemetry or {}, counters, unattributed, model)
            reconciliation['telemetry_native_proof'] = proof
            if proof['status'] == 'matched':
                reconciliation['status'] = 'billed_calls_outside_session_counters'
                reconciliation['billed_tokens_outside_session_counters'] = {
                    name: -value for name, value in unattributed.items()}
    without_id = (telemetry or {}).get('calls_without_response_id', 0)
    reasons = []
    if missing:
        reasons.append(f'{len(missing)} generation id(s) have no billed record')
    if without_id:
        reasons.append(f'{without_id} telemetry call(s) carried no response id to bill')
    if invalid_cached_ids:
        reasons.append('completed provider bills have missing or invalid native cached token counts: '
                       + ', '.join(invalid_cached_ids))
    if unattributed is None:
        reasons.append(reconciliation['reason'])
    elif any(unattributed.values()) and reconciliation['status'] != 'billed_calls_outside_session_counters':
        direction = ('goose counted tokens no recovered generation id accounts for: '
                     if any(value > 0 for value in unattributed.values()) else
                     'recovered native tokens exceed session counters without complete per-call proof: ')
        reasons.append(direction
                       + ', '.join(f'{name} {value:+}' for name, value in unattributed.items() if value))
    complete = bool(records) and not reasons
    billed = sum((Decimal(record['usage']) for record in records), Decimal(0))
    incomplete_billed = sum((Decimal(found[gen]['usage']) for gen in incomplete_ids if gen in found), Decimal(0))
    return {
        'status': 'complete' if complete else 'incomplete',
        'billed_usd': float(billed) if records else None,
        'billed_usd_is_lower_bound': not complete,
        'incomplete_reasons': reasons,
        'request_count': len(records),
        'generation_ids_collected': len(ids),
        'tokens': tokens,
        'hosts': hosts,
        'missing': missing,
        'late': late,
        'incomplete_calls': {'count': len(incomplete_ids), 'billed_usd': float(incomplete_billed),
                             'ids': incomplete_ids},
        'reconciliation': reconciliation,
    }


def record(provider: str | None, model: str | None, credentials: dict, runtime: Path, workdir: Path,
           usage: dict | None, urlopen=None, sleep=None, budget: dict | None = None) -> dict:
    """The run's billed cost, written to model-cost.json; a failure is a named record, never a zero.

    `budget` is the harness's call-budget / wallet-guard record (bench_budget), kept beside the bill so
    the cost file says how the run ended.
    """
    try:
        if provider != 'openrouter':
            result = unavailable(provider)
        elif not credentials.get('OPENROUTER_API_KEY'):
            result = {'status': 'error', 'provider': provider,
                      'reason': "OPENROUTER_API_KEY is absent from the run's credentials snapshot"}
        else:
            # The host goose's OpenRouter provider called (OPENROUTER_HOST, default openrouter.ai).
            host = credentials.get('OPENROUTER_HOST') or 'https://openrouter.ai'
            ids, sources, telemetry = collect_generation_ids(runtime, workdir)
            telemetry_summary = {key: value for key, value in telemetry.items()
                                 if key not in ('ended', 'native_usage_by_id', 'model_by_id')}
            if not ids:
                result = unavailable(provider, 'the run kept no OpenRouter generation id '
                                               '(telemetry, sessions.db message ids, request logs)')
                result['generation_id_sources'] = sources
                result['telemetry'] = telemetry_summary
            else:
                found, late, failures = bill_generations(ids, credentials['OPENROUTER_API_KEY'], host,
                                                         urlopen, sleep)
                outside = [gen for gen in ids if telemetry['status'] == 'recorded' and gen not in telemetry['ended']]
                if outside:
                    # Billed all the same; a gap in the authoritative source is shown, not hidden.
                    telemetry_summary['ids_outside_telemetry'] = outside
                result = {'provider': provider,
                          'source': host.rstrip('/') + '/api/v1/generation (usage per generation id)',
                          **summarize(ids, found, late, failures, usage, telemetry, model=model),
                          'generation_id_sources': sources,
                          'telemetry': telemetry_summary}
        result['model'] = model
    except Exception as error:  # a paid run is never lost to a billing read; the failure is named
        reason = f'{type(error).__name__}: {error}'
        if credentials.get('OPENROUTER_API_KEY'):
            reason = reason.replace(credentials['OPENROUTER_API_KEY'], '[REDACTED]')
        result = {'status': 'error', 'provider': provider, 'model': model, 'reason': reason}
    if budget is not None:
        result['budget'] = budget
    (workdir / 'model-cost.json').write_text(json.dumps(result, indent=2))
    return result
