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
        self.assertEqual(result['generation_id_sources'], {'sessions_db_message_ids': 2, 'request_logs': 3})
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
c.execute("INSERT INTO messages (message_id, role) VALUES ('%s', 'assistant'), ('msg_x', 'assistant')")
c.commit()
print('session written')
''' % (2527, 89, 2304, IDS[0]))
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
            self.assertEqual(result['billed_cost']['billed_usd'], 0.000187524)
            self.assertEqual(json.loads((work / 'model-cost.json').read_text()), result['billed_cost'])
            self.assertEqual(fake.urls, ['https://openrouter.ai/api/v1/generation?id=' + IDS[0]])


if __name__ == '__main__':
    unittest.main()
