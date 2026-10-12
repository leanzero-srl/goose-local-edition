import contextlib
import io
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
import urllib.error
import urllib.parse
from unittest.mock import patch

import bench_cost
import run_build

KEY = 'sk-or-test-key-must-not-leak'
# Shapes copied from real /api/v1/generation answers for run c605215d (2026-10-02).
GENERATIONS = {
    'gen-1790911486-OyqP2p7UE9dNJc1weFAo': {
        'usage': '0.000187524', 'native_tokens_prompt': 2527, 'native_tokens_completion': 89,
        'native_tokens_cached': 2304, 'native_tokens_reasoning': 0, 'provider_name': 'DeepSeek'},
    'gen-1790911491-UC2xpT4PzuGCPFCEgusY': {
        'usage': '0.001249632', 'native_tokens_prompt': 10080, 'native_tokens_completion': 58,
        'native_tokens_cached': 6272, 'native_tokens_reasoning': 7, 'provider_name': 'DeepSeek'},
    'gen-1790911490-NRYJ057k1I93PNWidl6r': {
        'usage': '0.1', 'native_tokens_prompt': 6615, 'native_tokens_completion': 92,
        'native_tokens_cached': 5120, 'native_tokens_reasoning': None, 'provider_name': 'Fireworks'},
    'gen-1790911500-FPkijwft1Rbew5GzlrZj': {
        'usage': '0.2', 'native_tokens_prompt': 28219, 'native_tokens_completion': 400,
        'native_tokens_cached': 0, 'native_tokens_reasoning': 120, 'provider_name': 'DeepSeek'},
}
IDS = list(GENERATIONS)

# Native usage copied from the retained seven-call Sol qualification, unit896902fb2203-qualification-2.
# The test bill amounts are inert; these counters and generation identities are the actual fixture.
SOL_MODEL = 'openai/gpt-6.1-sol'
SOL_TITLE_MODEL = 'google/gemini-2.5-flash'
SOL_USAGE = (
    ('gen-1791744994-X3TvW9jjrHMRU5scpEjO', 16158, 15905, 203, SOL_MODEL),
    ('gen-1791744992-dzoSg74ySeX9iQv5G6b2', 15908, 15325, 47, SOL_MODEL),
    ('gen-1791744990-TVuJsuuuATrv6nqW940r', 15328, 14171, 47, SOL_MODEL),
    ('gen-1791744988-hTYOFGSODadk2QhJE4wp', 14174, 13686, 47, SOL_MODEL),
    ('gen-1791744986-Xtsvg5EMzuDTRQt3fYe8', 13689, 13342, 47, SOL_MODEL),
    ('gen-1791744984-n7sFqUKmdQF4gkAnhseg', 13313, 13271, 346, SOL_TITLE_MODEL),
    ('gen-1791744984-hbL5fzTQWnpbEOWwlS4f', 13345, 0, 47, SOL_MODEL),
)


def sol_fixture():
    found = {gen: {'id': gen, 'usage': '0.001', 'native_tokens_prompt': prompt,
                   'native_tokens_cached': cached, 'native_tokens_completion': completion,
                   'native_tokens_reasoning': 340 if model == SOL_TITLE_MODEL else 0,
                   'provider_name': 'Google' if model == SOL_TITLE_MODEL else 'OpenAI'}
             for gen, prompt, cached, completion, model in SOL_USAGE}
    lines = [{'response_id': gen, 'model': model, 'ended': 'completed', 'usage': True,
              'prompt_tokens': prompt, 'completion_tokens': completion}
             for gen, prompt, cached, completion, model in SOL_USAGE]
    usage = {'status': 'recorded', 'sessions': [{'accumulated_input_tokens': 88602,
             'accumulated_cache_read_tokens': 72429, 'accumulated_output_tokens': 438}]}
    return list(found), found, lines, usage


def counters(ids):
    return {'status': 'recorded', 'sessions': [{
        'accumulated_input_tokens': sum(GENERATIONS[g]['native_tokens_prompt'] for g in ids),
        'accumulated_output_tokens': sum(GENERATIONS[g]['native_tokens_completion'] for g in ids),
        'accumulated_cache_read_tokens': sum(GENERATIONS[g]['native_tokens_cached'] for g in ids)}]}


class FakeOpenRouter:
    """urlopen stand-in: serves GENERATIONS, 404s an id until it has been asked `lag[id]` times."""

    def __init__(self, absent=(), lag=None):
        self.absent, self.lag, self.asked, self.urls = set(absent), dict(lag or {}), {}, []

    def __call__(self, request, timeout):
        assert request.get_header('Authorization') == 'Bearer ' + KEY
        self.urls.append(request.full_url)
        gen = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)['id'][0]
        self.asked[gen] = self.asked.get(gen, 0) + 1
        if gen in self.absent or self.asked[gen] <= self.lag.get(gen, 0):
            raise urllib.error.HTTPError(request.full_url, 404, 'Not Found', {}, None)
        record = dict(GENERATIONS[gen], id=gen)
        body = json.dumps({'data': record}).replace(f'"{record["usage"]}"', record['usage'])
        return contextlib.closing(io.BytesIO(body.encode()))


def runtime_with(root: Path, db_ids, log_ids, extra_messages=()):
    sessions = root / 'goose' / 'data' / 'sessions'
    sessions.mkdir(parents=True)
    connection = sqlite3.connect(sessions / 'sessions.db')
    connection.execute('CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, message_id TEXT, role TEXT)')
    for message_id in list(db_ids) + list(extra_messages):
        connection.execute('INSERT INTO messages (message_id, role) VALUES (?, ?)', (message_id, 'assistant'))
    connection.execute("INSERT INTO messages (message_id, role) VALUES ('gen-1-userShouldNotCount', 'user')")
    connection.commit()
    connection.close()
    logs = root / 'goose' / 'state' / 'logs'
    logs.mkdir(parents=True)
    for index, gen in enumerate(log_ids):
        (logs / f'llm_request.{index}.jsonl').write_text('\n'.join([
            json.dumps({'model_config': {}, 'input': {}}),
            json.dumps({'data': {'id': gen, 'role': 'assistant'}, 'usage': None}),
            json.dumps({'data': None, 'usage': {'input_tokens': 1}})]) + '\n')
    return root


class BilledCostTests(unittest.TestCase):
    def record(self, root, fake, usage, provider='openrouter', credentials=None):
        sleeps = []
        result = bench_cost.record(provider, 'deepseek/deepseek-v4.1-flash',
                                   {'OPENROUTER_API_KEY': KEY} if credentials is None else credentials,
                                   root, root, usage, urlopen=fake, sleep=sleeps.append)
        written = (root / 'model-cost.json').read_text()
        self.assertEqual(json.loads(written), result)
        self.assertNotIn(KEY, written)
        return result, sleeps

    def test_all_ids_found_sums_the_bill_exactly_and_reconciles_with_goose_counters(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:2], IDS[1:], extra_messages=['msg_tool-request-uuid'])
            result, sleeps = self.record(root, FakeOpenRouter(), counters(IDS))
        self.assertEqual(result['status'], 'complete')
        # Decimal, not float: 0.000187524 + 0.001249632 + 0.1 + 0.2 without binary drift.
        self.assertEqual(result['billed_usd'], 0.301437156)
        self.assertFalse(result['billed_usd_is_lower_bound'])
        self.assertEqual(result['request_count'], 4)
        self.assertEqual(result['tokens'], {'prompt': 47441, 'cached': 13696, 'completion': 639, 'reasoning': 127})
        self.assertEqual(result['hosts'], {'DeepSeek': 3, 'Fireworks': 1})
        self.assertEqual(result['missing'], [])
        self.assertEqual(result['late'], [])
        self.assertEqual(result['reconciliation']['status'], 'matched')
        self.assertEqual(result['generation_id_sources'],
                         {'telemetry': 0, 'sessions_db_message_ids': 2, 'request_logs': 3})
        self.assertEqual(result['telemetry']['status'], 'absent')
        self.assertEqual(result['source'], 'https://openrouter.ai/api/v1/generation (usage per generation id)')
        self.assertEqual(sleeps, [])

    def test_a_missing_id_is_listed_loudly_and_the_sum_is_flagged_incomplete(self):
        absent = IDS[2]
        late = IDS[3]
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS, [])
            fake = FakeOpenRouter(absent={absent}, lag={late: 2})
            result, sleeps = self.record(root, fake, counters(IDS))
        self.assertEqual(result['status'], 'incomplete')
        self.assertTrue(result['billed_usd_is_lower_bound'])
        self.assertEqual(result['missing'], [{'id': absent, 'reason': 'HTTP 404'}])
        self.assertEqual(result['late'], [late])
        self.assertEqual(result['billed_usd'], 0.201437156)
        self.assertEqual(result['request_count'], 3)
        self.assertIn('1 generation id(s) have no billed record', result['incomplete_reasons'])
        # The absent call's tokens are visible as goose-counted but unattributed, never as free.
        self.assertEqual(result['reconciliation']['unattributed_tokens'],
                         {'prompt': 6615, 'cached': 5120, 'completion': 92})
        self.assertEqual(sleeps, list(bench_cost.LATE_RECORD_BACKOFF))
        self.assertEqual(fake.asked[absent], 1 + len(bench_cost.LATE_RECORD_BACKOFF))

    def test_every_id_found_but_goose_counted_more_is_still_incomplete(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:3], [])
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS))
        self.assertEqual(result['status'], 'incomplete')
        self.assertEqual(result['missing'], [])
        self.assertEqual(result['reconciliation']['unattributed_tokens'],
                         {'prompt': 28219, 'cached': 0, 'completion': 400})
        self.assertTrue(any('goose counted tokens' in reason for reason in result['incomplete_reasons']))

    def write_telemetry(self, root: Path, lines):
        swarm = root / '.swarm'
        swarm.mkdir(exist_ok=True)
        (swarm / 'telemetry.jsonl').write_text(''.join(json.dumps(line) + '\n' for line in lines))

    def test_a_fresh_runs_telemetry_makes_the_bill_complete(self):
        """The data shape of a run on the engine that writes response_id: every call has a line, the
        session DB kept only the text-led reply ids, and one stream was dropped (billed, uncounted)."""
        completed, dropped = IDS[:3], IDS[3]
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:1], [], extra_messages=['msg_a', 'msg_b'])
            self.write_telemetry(root, [
                {'model': 'deepseek/deepseek-v4.1-flash', 'usage': True, 'response_id': gen,
                 'ended': 'completed'} for gen in completed] + [
                {'model': 'deepseek/deepseek-v4.1-flash', 'usage': False, 'response_id': dropped,
                 'ended': 'incomplete', 'approx_completion_chunks': 3}])
            result, _ = self.record(root, FakeOpenRouter(), counters(completed))
        self.assertEqual(result['status'], 'complete', result['incomplete_reasons'])
        self.assertEqual(result['billed_usd'], 0.301437156)
        self.assertEqual(result['request_count'], 4)
        self.assertEqual(result['incomplete_calls'], {'count': 1, 'billed_usd': 0.2, 'ids': [dropped]})
        self.assertEqual(result['reconciliation']['status'], 'matched')
        self.assertEqual(result['generation_id_sources'],
                         {'telemetry': 4, 'sessions_db_message_ids': 1, 'request_logs': 0})
        self.assertEqual(result['telemetry']['calls'], 4)
        self.assertNotIn('ids_outside_telemetry', result['telemetry'])

    def native_telemetry(self, ids):
        return [{'model': 'm', 'usage': True, 'response_id': gen, 'ended': 'completed',
                 'prompt_tokens': GENERATIONS[gen]['native_tokens_prompt'],
                 'completion_tokens': GENERATIONS[gen]['native_tokens_completion']} for gen in ids]

    def test_billed_auxiliary_call_exceeding_session_counters_has_exact_native_proof(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:1], [])
            self.write_telemetry(root, self.native_telemetry(IDS))
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS[:3]))
        self.assertEqual(result['status'], 'complete', result['incomplete_reasons'])
        self.assertEqual(result['billed_usd'], 0.301437156)
        self.assertFalse(result['billed_usd_is_lower_bound'])
        self.assertEqual(result['reconciliation']['status'], 'billed_calls_outside_session_counters')
        self.assertEqual(result['reconciliation']['billed_tokens_outside_session_counters'],
                         {'prompt': 28219, 'cached': 0, 'completion': 400})
        proof = result['reconciliation']['telemetry_native_proof']
        self.assertEqual(proof['status'], 'matched')
        self.assertEqual(proof['completed_native_counts_matched'], 4)
        self.assertEqual(proof['reasons'], [])
        self.assertNotIn('native_usage_by_id', result['telemetry'])

    def test_negative_residual_without_complete_native_telemetry_is_not_proof(self):
        for defect in ('absent', 'missing_counts', 'invalid_counts', 'conflict', 'no_id',
                       'unparseable', 'unmarked', 'outside', 'cached_residual'):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as tmp:
                root = runtime_with(Path(tmp), IDS, [])
                lines = self.native_telemetry(IDS)
                usage = counters(IDS[:3])
                if defect == 'missing_counts':
                    lines[-1].pop('prompt_tokens')
                elif defect == 'invalid_counts':
                    lines[-1]['completion_tokens'] = True
                elif defect == 'conflict':
                    lines.append(dict(lines[-1], prompt_tokens=1))
                elif defect == 'no_id':
                    lines.append({'ended': 'incomplete', 'response_id': None})
                elif defect == 'unmarked':
                    lines.append({'model': 'm', 'usage': True, 'prompt_tokens': 100})
                elif defect == 'outside':
                    lines.pop()
                elif defect == 'cached_residual':
                    usage['sessions'][0]['accumulated_cache_read_tokens'] -= 1
                if defect != 'absent':
                    self.write_telemetry(root, lines)
                if defect == 'unparseable':
                    with (root / '.swarm' / 'telemetry.jsonl').open('a') as stream:
                        stream.write('broken line\n')
                result, _ = self.record(root, FakeOpenRouter(), usage)
                self.assertEqual(result['status'], 'incomplete', result)
                self.assertTrue(result['billed_usd_is_lower_bound'])
                self.assertTrue(any('without complete per-call proof' in reason
                                    for reason in result['incomplete_reasons']))

    def test_native_counts_must_match_each_bill_even_when_mismatches_cancel_in_the_sum(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS, [])
            lines = self.native_telemetry(IDS)
            lines[0]['prompt_tokens'] += 1
            lines[1]['prompt_tokens'] -= 1
            self.write_telemetry(root, lines)
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS[:3]))
        self.assertEqual(result['status'], 'incomplete')
        proof = result['reconciliation']['telemetry_native_proof']
        self.assertEqual(proof['mismatched_generation_ids'], IDS[:2])
        self.assertEqual(proof['status'], 'unproven')

    def test_missing_bill_cannot_be_hidden_by_a_larger_billed_auxiliary_call(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS, [])
            self.write_telemetry(root, self.native_telemetry(IDS))
            # The absent call is smaller than the auxiliary call. Its remaining residual is negative.
            usage = counters(IDS[:3])
            usage['sessions'][0]['accumulated_cache_read_tokens'] -= GENERATIONS[IDS[0]]['native_tokens_cached']
            result, _ = self.record(root, FakeOpenRouter(absent=[IDS[0]]), usage)
        self.assertEqual(result['status'], 'incomplete')
        self.assertEqual(result['missing'], [{'id': IDS[0], 'reason': 'HTTP 404'}])
        self.assertTrue(result['billed_usd_is_lower_bound'])

    def test_complete_native_telemetry_cannot_clear_positive_unattributed_tokens(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS, [])
            self.write_telemetry(root, self.native_telemetry(IDS))
            usage = counters(IDS)
            usage['sessions'][0]['accumulated_output_tokens'] += 1
            result, _ = self.record(root, FakeOpenRouter(), usage)
        self.assertEqual(result['status'], 'incomplete')
        self.assertEqual(result['reconciliation']['unattributed_tokens']['completion'], 1)
        self.assertTrue(any('goose counted tokens' in reason for reason in result['incomplete_reasons']))

    def sol_summary(self, ids, found, lines, usage, model=SOL_MODEL):
        with tempfile.TemporaryDirectory() as tmp:
            self.write_telemetry(Path(tmp), lines)
            telemetry = bench_cost.telemetry_calls(Path(tmp))
        return bench_cost.summarize(ids, found, [], {}, usage, telemetry, model=model)

    def test_actual_sol_cached_title_residual_is_proven_without_retained_request_logs(self):
        result = self.sol_summary(*sol_fixture())
        self.assertEqual(result['status'], 'complete', result)
        self.assertEqual(result['request_count'], 7)
        self.assertEqual(result['billed_usd'], 0.007)  # Inert bill fixture, not measured campaign cost.
        self.assertEqual(result['tokens'], {'prompt': 101915, 'cached': 85700, 'completion': 784, 'reasoning': 340})
        self.assertEqual(result['hosts'], {'OpenAI': 6, 'Google': 1})
        self.assertEqual(result['reconciliation']['unattributed_tokens'],
                         {'prompt': -13313, 'cached': -13271, 'completion': -346})
        proof = result['reconciliation']['telemetry_native_proof']
        self.assertEqual(proof['status'], 'matched')
        self.assertEqual(proof['completed_native_counts_matched'], 7)
        self.assertEqual(proof['entrant_native_tokens'], {'prompt': 88602, 'cached': 72429, 'completion': 438})
        self.assertEqual(proof['auxiliary_native_tokens'], {'prompt': 13313, 'cached': 13271, 'completion': 346})
        self.assertEqual(proof['auxiliary_generation_ids'], [SOL_USAGE[5][0]])
        self.assertEqual(len(proof['entrant_generation_ids']), 6)
        self.assertFalse(result['billed_usd_is_lower_bound'])

    def test_cached_title_record_passes_explicit_launch_model_to_native_summary(self):
        ids, found, lines, usage = sol_fixture()

        def fake(request, timeout):
            gen = urllib.parse.parse_qs(urllib.parse.urlsplit(request.full_url).query)['id'][0]
            data = dict(found[gen], usage=0.001)
            return contextlib.closing(io.BytesIO(json.dumps({'data': data}).encode()))

        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), ids[-1:], [])
            self.write_telemetry(root, lines)
            result = bench_cost.record('openrouter', SOL_MODEL, {'OPENROUTER_API_KEY': KEY}, root,
                                       root, usage, urlopen=fake, sleep=lambda _: None)
            self.assertEqual(result, json.loads((root / 'model-cost.json').read_text()))
        self.assertEqual(result['status'], 'complete', result)
        self.assertEqual(result['model'], SOL_MODEL)
        self.assertEqual(result['generation_id_sources']['request_logs'], 0)
        self.assertNotIn('model_by_id', result['telemetry'])

    def test_cached_auxiliary_proof_covers_long_build_without_request_log_retention(self):
        ids, found, lines = [], {}, []
        for index in range(300):
            gen = f'gen-{index + 1}-InertEntrant'
            ids.append(gen)
            found[gen] = {'id': gen, 'usage': '0.001', 'native_tokens_prompt': 100,
                          'native_tokens_cached': 90, 'native_tokens_completion': 10}
            lines.append({'response_id': gen, 'model': SOL_MODEL, 'ended': 'completed',
                          'usage': True, 'prompt_tokens': 100, 'completion_tokens': 10})
        title = 'gen-301-InertTitle'
        ids.append(title)
        found[title] = {'id': title, 'usage': '0.002', 'native_tokens_prompt': 50,
                        'native_tokens_cached': 40, 'native_tokens_completion': 5}
        lines.append({'response_id': title, 'model': SOL_TITLE_MODEL, 'ended': 'completed',
                      'usage': True, 'prompt_tokens': 50, 'completion_tokens': 5})
        usage = {'status': 'recorded', 'sessions': [{'accumulated_input_tokens': 30000,
                 'accumulated_cache_read_tokens': 27000, 'accumulated_output_tokens': 3000}]}
        result = self.sol_summary(ids, found, lines, usage)
        self.assertEqual(result['status'], 'complete', result)
        self.assertEqual(result['request_count'], 301)
        self.assertEqual(result['billed_usd'], 0.302)
        self.assertEqual(result['reconciliation']['telemetry_native_proof']['completed_native_counts_matched'], 301)

    def test_cached_auxiliary_proof_keeps_original_missing_model_callers_incomplete(self):
        self.assertEqual(self.sol_summary(*sol_fixture(), model='')['status'], 'incomplete')
        self.assertEqual(self.sol_summary(*sol_fixture(), model='unknown')['status'], 'incomplete')
        self.assertEqual(self.sol_summary(*sol_fixture(), model='openai/wrong-model')['status'], 'incomplete')

    def test_cached_auxiliary_proof_refuses_missing_invalid_and_conflicting_evidence(self):
        for defect in ('missing_bill', 'missing_telemetry_id', 'extra_telemetry_id', 'duplicate_id',
                       'wrong_bill_id', 'missing_usage', 'invalid_usage', 'conflicting_usage', 'missing_model',
                       'invalid_model', 'wrong_entrant_model', 'wrong_auxiliary_model', 'conflicting_model',
                       'missing_cache', 'negative_cache', 'boolean_cache', 'float_cache', 'cache_exceeds_prompt',
                       'wrong_entrant_cache', 'wrong_session_cache', 'wrong_session_prompt',
                       'wrong_session_completion', 'invalid_session_cache', 'positive_residual',
                       'unmarked', 'no_response_id'):
            with self.subTest(defect=defect):
                ids, found, lines, usage = sol_fixture()
                first, title = ids[0], ids[5]
                if defect == 'missing_bill':
                    found.pop(first)
                elif defect == 'missing_telemetry_id':
                    lines.pop(0)
                elif defect == 'extra_telemetry_id':
                    lines.append(dict(lines[0], response_id='gen-1-Extra'))
                elif defect == 'duplicate_id':
                    ids.append(first)
                elif defect == 'wrong_bill_id':
                    found[first]['id'] = 'gen-1-Wrong'
                elif defect == 'missing_usage':
                    lines[0].pop('prompt_tokens')
                elif defect == 'invalid_usage':
                    lines[0]['usage'] = False
                elif defect == 'conflicting_usage':
                    lines.append(dict(lines[0], completion_tokens=1))
                elif defect == 'missing_model':
                    lines[0].pop('model')
                elif defect == 'invalid_model':
                    lines[0]['model'] = 'unknown'
                elif defect == 'wrong_entrant_model':
                    lines[0]['model'] = 'openai/wrong-model'
                elif defect == 'wrong_auxiliary_model':
                    lines[5]['model'] = SOL_MODEL
                elif defect == 'conflicting_model':
                    lines.append(dict(lines[0], model=SOL_TITLE_MODEL))
                elif defect == 'missing_cache':
                    found[title].pop('native_tokens_cached')
                elif defect == 'negative_cache':
                    found[title]['native_tokens_cached'] = -1
                elif defect == 'boolean_cache':
                    found[title]['native_tokens_cached'] = True
                elif defect == 'float_cache':
                    found[title]['native_tokens_cached'] = 13271.0
                elif defect == 'cache_exceeds_prompt':
                    found[title]['native_tokens_cached'] = found[title]['native_tokens_prompt'] + 1
                elif defect == 'wrong_entrant_cache':
                    found[first]['native_tokens_cached'] -= 1
                elif defect.startswith('wrong_session_'):
                    column = {'wrong_session_cache': 'accumulated_cache_read_tokens',
                              'wrong_session_prompt': 'accumulated_input_tokens',
                              'wrong_session_completion': 'accumulated_output_tokens'}[defect]
                    usage['sessions'][0][column] -= 1
                elif defect == 'invalid_session_cache':
                    usage['sessions'][0]['accumulated_cache_read_tokens'] = True
                elif defect == 'positive_residual':
                    usage['sessions'][0]['accumulated_output_tokens'] = 785
                elif defect == 'unmarked':
                    lines.append({'model': SOL_MODEL, 'prompt_tokens': 1})
                elif defect == 'no_response_id':
                    lines.append({'model': SOL_MODEL, 'response_id': None, 'ended': 'incomplete'})
                result = self.sol_summary(ids, found, lines, usage)
                self.assertEqual(result['status'], 'incomplete', result)
                self.assertTrue(result['billed_usd_is_lower_bound'])

    def test_cached_title_proof_keeps_dropped_generation_bill_and_excludes_its_usage(self):
        ids, found, lines, usage = sol_fixture()
        dropped = 'gen-1-Dropped'
        ids.append(dropped)
        found[dropped] = {'id': dropped, 'usage': '0.002', 'native_tokens_prompt': 2,
                         'native_tokens_cached': 1, 'native_tokens_completion': 3}
        lines.append({'response_id': dropped, 'model': SOL_MODEL, 'ended': 'incomplete', 'usage': False})
        result = self.sol_summary(ids, found, lines, usage)
        self.assertEqual(result['status'], 'complete', result)
        self.assertEqual(result['billed_usd'], 0.009)
        self.assertEqual(result['incomplete_calls']['ids'], [dropped])
        self.assertEqual(result['tokens']['cached'], 85701)

    def test_a_telemetry_call_without_a_response_id_keeps_the_bill_incomplete(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), [], [])
            self.write_telemetry(root, [{'response_id': gen, 'ended': 'completed'} for gen in IDS]
                                 + [{'response_id': None, 'ended': 'incomplete'}])
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS))
        self.assertEqual(result['status'], 'incomplete')
        self.assertIn('1 telemetry call(s) carried no response id to bill', result['incomplete_reasons'])
        self.assertEqual(result['reconciliation']['status'], 'matched')

    def test_an_id_the_telemetry_missed_is_billed_and_named(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[3:], [])
            self.write_telemetry(root, [{'response_id': gen, 'ended': 'completed'} for gen in IDS[:3]])
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS))
        self.assertEqual(result['status'], 'complete')
        self.assertEqual(result['telemetry']['ids_outside_telemetry'], [IDS[3]])

    def test_telemetry_from_an_engine_before_response_id_is_named_and_not_trusted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:2], [])
            self.write_telemetry(root, [{'model': 'm', 'usage': True, 'prompt_tokens': 1}])
            result, _ = self.record(root, FakeOpenRouter(), counters(IDS))
        self.assertEqual(result['telemetry']['status'], 'predates_response_id')
        self.assertEqual(result['generation_id_sources']['telemetry'], 0)
        self.assertEqual(result['status'], 'incomplete')

    def test_without_goose_counters_completeness_is_unproven(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS, [])
            result, _ = self.record(root, FakeOpenRouter(), {'status': 'unavailable', 'reason': 'x'})
        self.assertEqual(result['status'], 'incomplete')
        self.assertEqual(result['reconciliation']['status'], 'unavailable')
        self.assertEqual(result['billed_usd'], 0.301437156)

    def test_openrouter_host_override_is_the_host_queried(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:1], [])
            fake = FakeOpenRouter()
            self.record(root, fake, counters(IDS[:1]),
                        credentials={'OPENROUTER_API_KEY': KEY, 'OPENROUTER_HOST': 'https://or.example/'})
        self.assertEqual(fake.urls, ['https://or.example/api/v1/generation?id=' + IDS[0]])

    def test_non_openrouter_providers_are_unavailable_never_estimated(self):
        for provider, needle in [('google', 'Google'), ('aws_bedrock', 'Bedrock'), (None, 'local fleet'),
                                 ('anthropic', 'anthropic')]:
            with self.subTest(provider=provider), tempfile.TemporaryDirectory() as tmp:
                root = runtime_with(Path(tmp), IDS, IDS)
                fake = FakeOpenRouter()
                result, _ = self.record(root, fake, counters(IDS), provider=provider)
                self.assertEqual(result['status'], 'unavailable')
                self.assertIn(needle, result['reason'])
                self.assertNotIn('billed_usd', result)
                self.assertEqual(fake.urls, [])

    def test_no_recovered_ids_and_no_key_are_named_not_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), [], [], extra_messages=['msg_only-uuids'])
            result, _ = self.record(root, FakeOpenRouter(), counters([]))
            self.assertEqual(result['status'], 'unavailable')
            self.assertIn('no OpenRouter generation id', result['reason'])
            self.assertNotIn('billed_usd', result)
            result, _ = self.record(root, FakeOpenRouter(), counters([]), credentials={})
            self.assertEqual(result['status'], 'error')
            self.assertIn('OPENROUTER_API_KEY', result['reason'])

    def test_an_unexpected_failure_is_a_named_error_record_without_the_key(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = runtime_with(Path(tmp), IDS[:1], [])
            with patch.object(bench_cost, 'summarize', side_effect=RuntimeError('broke ' + KEY)):
                result, _ = self.record(root, FakeOpenRouter(), counters(IDS[:1]))
        self.assertEqual(result['status'], 'error')
        self.assertIn('[REDACTED]', result['reason'])

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS sandbox integration')
    def test_invoke_records_the_bill_for_an_isolated_openrouter_entrant(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            engine = root / 'engine'
            # A stand-in goose: writes the isolated session store the way goose does, under GOOSE_PATH_ROOT.
            engine.write_text('#!' + sys.executable + '''
import os, pathlib, sqlite3
db = pathlib.Path(os.environ['GOOSE_PATH_ROOT']) / 'data' / 'sessions'
db.mkdir(parents=True)
c = sqlite3.connect(db / 'sessions.db')
c.execute("CREATE TABLE sessions (id TEXT, accumulated_input_tokens INTEGER, accumulated_output_tokens INTEGER, "
          "accumulated_cache_read_tokens INTEGER, accumulated_cache_write_tokens INTEGER, accumulated_cost REAL)")
c.execute("INSERT INTO sessions VALUES ('20261002_1', %d, %d, %d, 0, 0.0001)")
c.execute("CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, message_id TEXT, role TEXT)")
c.execute("INSERT INTO messages (message_id, role) VALUES ('msg_x', 'assistant')")
c.commit()
# The engine's telemetry sink, at the path the harness hands it (GOOSE_SWARM_TELEMETRY_FILE).
pathlib.Path('telemetry-path.txt').write_text(os.environ['GOOSE_SWARM_TELEMETRY_FILE'])
with open(os.environ['GOOSE_SWARM_TELEMETRY_FILE'], 'a') as t:
    t.write('{"model": "m", "usage": true, "response_id": "%s", "ended": "completed"}\\n')
    t.write('{"model": "m", "usage": false, "response_id": "%s", "ended": "incomplete"}\\n')
print('session written')
''' % (2527, 89, 2304, IDS[0], IDS[1]))
            engine.chmod(0o700)
            work = root / 'candidate'
            work.mkdir()
            fake = FakeOpenRouter()
            with patch.object(run_build, 'GOOSE', engine), \
                    patch.object(bench_cost.urllib.request, 'urlopen', fake), \
                    patch.dict(os.environ, {'BENCH_SB71': '1'}, clear=True), \
                    contextlib.redirect_stdout(io.StringIO()):
                result = run_build.invoke('openrouter-fixture', work, 8850, {'OPENROUTER_API_KEY': KEY}, 0,
                                          'openrouter', 'deepseek/deepseek-v4.1-flash')
            self.assertEqual(result['exit'], 0, result['tail'])
            self.assertEqual(result['billed_cost']['status'], 'complete', result['billed_cost'])
            # Both calls billed; the dropped one is outside goose's counters and listed as such.
            self.assertEqual(result['billed_cost']['billed_usd'], 0.001437156)
            self.assertEqual(result['billed_cost']['incomplete_calls']['ids'], [IDS[1]])
            self.assertEqual(result['billed_cost']['generation_id_sources'],
                             {'telemetry': 2, 'sessions_db_message_ids': 0, 'request_logs': 0})
            self.assertEqual(json.loads((work / 'model-cost.json').read_text()), result['billed_cost'])
            # The sink was in the runtime, outside the tree, and landed in the tree for every reader.
            sink = Path((work / 'telemetry-path.txt').read_text())
            self.assertEqual(sink, work.resolve().parent / '.candidate-runtime' / 'telemetry.jsonl')
            self.assertEqual((work / '.swarm' / 'telemetry.jsonl').read_text(), sink.read_text())
            self.assertEqual(result['telemetry_landing']['status'], 'copied')
            self.assertEqual(sorted(fake.urls), ['https://openrouter.ai/api/v1/generation?id=' + gen
                                                 for gen in sorted(IDS[:2])])


if __name__ == '__main__':
    unittest.main()
