"""Free negative controls for launch/accounting; these never request a model generation."""
from decimal import Decimal
import json
import io
import os
import plistlib
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

import campaign
from cache_probe import tool_history
from ops import (Hold, allowance, bill_receipt, cache_status, ensure_config, merge_receipts,
                 read, build_cache_evidence, request_routes, scan_requests, request_reservation, completed_reservation, reserve_from_qualification, spend, live_credit_evidence, verify_installed, write)


class AccountingControls(unittest.TestCase):
    def native(self, gen='gen-1-aa', model='xiaomi/mimo-v2.6-flash-20260921', cached=500):
        return {'id': gen, 'model': model, 'provider_name': 'actual-host', 'usage': Decimal('.10'),
                'native_tokens_prompt': 1000, 'native_tokens_completion': 10,
                'native_tokens_cached': cached, 'native_tokens_reasoning': 0}

    def route(self, model='xiaomi/mimo-v2.6-flash', kind='entrant'):
        return {'requested_model': model, 'kind': kind, 'request_sha256': 'a' * 64}

    def row(self, model='xiaomi/mimo-v2.6-flash'):
        return {'id': model, 'canonical_slug': model + '-20260921'}

    def test_served_canonical_identity_requires_same_current_catalog_entry(self):
        receipt = bill_receipt('gen-1-aa', self.native(), self.route(), self.row())
        self.assertEqual(receipt['billed_model'], self.native()['model'])
        for data, route in [(self.native(model='openai/gpt-6.1-sol'), self.route()),
                            (self.native(), self.route(model='other/model')),
                            (self.native(), None)]:
            with self.assertRaises(Hold):
                bill_receipt('gen-1-aa', data, route, self.row())

    def test_router_retains_requested_and_actual_model(self):
        receipt = bill_receipt('gen-1-aa', self.native(model='underlying/model'),
                               self.route('typesafe/jev-router'), self.row('typesafe/jev-router'))
        self.assertEqual(receipt['requested_model'], 'typesafe/jev-router')
        self.assertEqual(receipt['billed_model'], 'underlying/model')

    def test_tilde_is_never_guessed_equivalent(self):
        with self.assertRaises(Hold):
            bill_receipt('gen-1-aa', self.native(model='~openai/gpt-terra-latest'),
                         self.route('openai/gpt-terra-latest'), self.row('~openai/gpt-terra-latest'))

    def test_titles_and_failed_generation_bills_count_once_across_refreeze(self):
        ledger = {}
        title = bill_receipt('gen-1-aa', self.native(model='title/model'), self.route('title/model', 'auxiliary'), self.row())
        merge_receipts(ledger, 'old-probe', [title], stage='pilot')
        merge_receipts(ledger, 'old-probe', [title], stage='pilot')
        normal = bill_receipt('gen-2-bb', self.native('gen-2-bb'), self.route(), self.row())
        merge_receipts(ledger, 'new-pilot', [normal], stage='pilot')
        self.assertEqual(spend(ledger), Decimal('.20'))
        self.assertEqual(allowance(ledger, [{'key': 'new-pilot', 'stage': 'pilot'}], True, '.5'), Decimal('9.30'))
        with self.assertRaises(Hold):
            merge_receipts(ledger, 'another-unit', [normal], stage='pilot')
        with self.assertRaises(Hold):
            merge_receipts(ledger, 'new-pilot', [{**normal, 'billed_usd': '.11'}], stage='pilot')

    def test_aggregate_pilot_and_campaign_guards_do_not_reset_after_topup(self):
        ledger = {'one': {'unit': 'earlier-freeze', 'stage': 'pilot', 'billed_usd': '9.60'}}
        with self.assertRaises(Hold):
            allowance(ledger, [], True, '.5')
        self.assertEqual(allowance(ledger, [], False, '.5'), Decimal('64.90'))
        ledger['two'] = {'unit': 'roster', 'stage': 'roster', 'billed_usd': '65.0'}
        with self.assertRaises(Hold):
            allowance(ledger, [], False, '.5')

    def test_current_account_and_key_limit_only_tighten_no_reset_allowance(self):
        installed = SimpleNamespace(entrant_config=lambda _provider: {}, cloud_env=lambda _provider, _snapshot: {'OPENROUTER_API_KEY': 'test-only'})
        responses = [io.BytesIO(json.dumps({'data': {'total_credits': 22, 'total_usage': 3}}).encode()),
                     io.BytesIO(json.dumps({'data': {'limit': 5, 'limit_remaining': 2}}).encode())]
        opener = Mock(side_effect=responses)
        evidence = live_credit_evidence((None, None, None, None, installed), opener)
        self.assertEqual(evidence['effective_remaining_usd'], '2')
        self.assertNotIn('test-only', json.dumps(evidence))
        ledger = {'id': {'unit': 'pilot', 'stage': 'pilot', 'billed_usd': '1'}}
        self.assertEqual(allowance(ledger, [], True, '.5', '22', evidence['effective_remaining_usd']), Decimal('1.5'))
        self.assertEqual(allowance(ledger, [], True, '.5', '22', '999'), Decimal('8.5'))
        for remaining in (None, 'NaN', '-1'):
            opener = Mock(side_effect=[io.BytesIO(b'{"data":{"total_credits":22,"total_usage":3}}'),
                                      io.BytesIO(json.dumps({'data': {'limit': 5, 'limit_remaining': remaining}}).encode())])
            with self.assertRaises(Hold):
                live_credit_evidence((None, None, None, None, installed), opener)
        with self.assertRaises(Hold):
            live_credit_evidence((None, None, None, None, installed), Mock(side_effect=OSError('unavailable')))

    def test_cache_zero_missing_and_invalid_are_not_passes(self):
        receipts = [bill_receipt(f'gen-{i}-aa', self.native(f'gen-{i}-aa', cached=0), self.route(), self.row()) for i in range(6)]
        self.assertEqual(cache_status(receipts)['status'], 'no-native-cache-hit')
        with self.assertRaises(Hold):
            cache_status(receipts[:-1])
        receipts[2]['native_tokens_cached'] = None
        with self.assertRaises(Hold):
            cache_status(receipts)
        receipts[2]['native_tokens_cached'] = 800
        result = cache_status(receipts)
        self.assertEqual(result['status'], 'native-cache-hit')
        self.assertEqual(result['native_cached_generation_ids'], ['gen-2-aa'])

    def test_advertised_reserve_uses_actual_bytes_native_output_and_all_price_overrides(self):
        row = {'context_length': 1050000, 'top_provider': {'max_completion_tokens': 128000},
               'pricing': {'prompt': '.000002', 'completion': '.00001', 'input_cache_write': '.0000025',
                           'overrides': [{'prompt': '.000004', 'completion': '.000015', 'input_cache_write': '.000005'}]}}
        receipt = {'input_json_bytes': 136218, 'max_tokens_sent': None, 'billed_usd': '.10',
                   'kind': 'entrant', 'requested_model': 'exact/model'}
        value = request_reservation(receipt, row)
        self.assertEqual(Decimal(value['usd']), Decimal('2.601090'))
        self.assertIn('no max_tokens sent', value['output_source'])
        capped = request_reservation({**receipt, 'max_tokens_sent': 2801}, row)
        self.assertEqual(Decimal(capped['usd']), Decimal('.723105'))
        auxiliary = {**receipt, 'kind': 'auxiliary', 'requested_model': 'title/model'}
        reserve = reserve_from_qualification([receipt, auxiliary], {'exact/model': row, 'title/model': row})
        self.assertEqual(Decimal(reserve['usd']), Decimal('5.602180'))
        with self.assertRaises(Hold):
            request_reservation({'billed_usd': '0'}, {'id': 'unpriced'})
        ledger = {'id': {'unit': 'old', 'stage': 'pilot', 'billed_usd': '9.6'}}
        self.assertEqual(allowance(ledger, [], False, '.5', '22'), Decimal('11.9'))

    def test_full_build_cache_is_measured_independently_of_support_qualification(self):
        receipts = [{'id': 'gen-1-aa', 'kind': 'entrant', 'native_tokens_prompt': 1000, 'native_tokens_cached': 0}]
        self.assertEqual(build_cache_evidence(receipts)['status'], 'no-native-cache-observed')
        receipts.append({'id': 'gen-2-bb', 'kind': 'entrant', 'native_tokens_prompt': 2000, 'native_tokens_cached': 1500})
        value = build_cache_evidence(receipts)
        self.assertEqual(value['native_prompt_tokens_observed'], 3000)
        self.assertEqual(value['native_cached_tokens_observed'], 1500)
        self.assertEqual(value['cached_fraction'], .5)
        self.assertEqual(value['generations_with_native_cache_counts'], 2)
        receipts[1]['native_tokens_cached'] = None
        self.assertEqual(build_cache_evidence(receipts)['status'], 'unknown-native-cache')

    def test_completed_stream_reserve_uses_native_counts_not_future_output_capacity(self):
        row = {'pricing': {'prompt': '.000002', 'completion': '.00001', 'input_cache_write': '.0000025'}}
        value = completed_reservation({'prompt': 136218, 'completion': 2801}, row, '.10')
        self.assertEqual(Decimal(value['usd']), Decimal('.368555'))
        self.assertEqual(value['native_tokens_completion'], 2801)
        self.assertFalse(completed_reservation({'prompt': None, 'completion': None}, row, '.10')['advertised_bound_available'])

    def test_pre_id_title_request_is_reserved_once_without_persisting_private_content(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            log = runtime / 'goose/logs/llm_request.4242.00000000-0000-0000-0000-000000000001.jsonl'
            log.parent.mkdir(parents=True)
            body = {'input': {'model': 'title/model', 'messages': ['private title'], 'max_tokens': 123}}
            log.write_text(json.dumps(body) + '\n')
            routes, active = scan_requests(runtime)
            self.assertEqual(routes, {})
            self.assertEqual(active[log.name]['generation_ids'], [])
            self.assertEqual(active[log.name]['kind'], 'auxiliary')
            self.assertEqual(active[log.name]['max_tokens_sent'], 123)
            self.assertNotIn('private title', json.dumps(active))
            log.write_text(json.dumps(body) + '\n' + json.dumps({'data': {'id': 'gen-1-aa'}}) + '\n')
            routes, active = scan_requests(runtime)
            self.assertEqual(active[log.name]['generation_ids'], ['gen-1-aa'])
            self.assertEqual(routes['gen-1-aa']['request_sha256'], active[log.name]['request_sha256'])

    def test_actual_request_log_not_display_label_decides_route(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            log = runtime / 'goose/logs/llm_request.0.jsonl'
            log.parent.mkdir(parents=True)
            log.write_text(json.dumps({'input': {'model': 'exact/model', 'tools': [{}], 'messages': ['private']}}) + '\n' +
                           json.dumps({'data': {'id': 'gen-1-aa'}}) + '\n')
            routes = request_routes(runtime)
            self.assertEqual(routes['gen-1-aa']['requested_model'], 'exact/model')
            self.assertEqual(routes['gen-1-aa']['kind'], 'entrant')
            self.assertNotIn('private', json.dumps(routes))
            write(runtime / 'durable.json', routes)
            log.unlink()  # Rotation cannot delete the campaign's saved narrow receipt.
            self.assertEqual(read(runtime / 'durable.json'), routes)

    def test_install_gate_binds_bundle_version_engine_task_and_release_receipt(self):
        from ops import sha, SUPPORT_FILES
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            bundle = base / 'Goose Swarm.app'
            resources = bundle / 'Contents/Resources'
            engine, payload = resources / 'bin/goose', resources / 'swarm-bench'
            engine.parent.mkdir(parents=True)
            payload.mkdir()
            engine.write_bytes(b'actual-engine')
            with (bundle / 'Contents/Info.plist').open('wb') as stream:
                plistlib.dump({'CFBundleShortVersionString': '3.0.110'}, stream)
            fingerprint = 'f' * 64
            release = base / 'release.json'
            registration = base / 'registration.json'
            write(release, {'appVersion': '3.0.110', 'sourceCommit': 'a' * 40,
                            'engineSha256': sha(engine), 'taskFingerprint': fingerprint})
            write(registration, {'scorerVersion': 'forge-2.0', 'releaseManifestSha256': fingerprint})
            config = {'python': sys.executable, 'node': sys.executable, 'app_bundle': str(bundle), 'app_version': '3.0.110', 'source_commit': 'a' * 40,
                      'payload': str(payload), 'engine': str(engine), 'engine_sha256': sha(engine),
                      'taskFingerprint': fingerprint, 'release_receipt': str(release),
                      'release_receipt_sha256': sha(release), 'registration_receipt': str(registration),
                      'registration_receipt_sha256': sha(registration),
                      'support_sha256': {name: sha(Path(__file__).parent / name) for name in SUPPORT_FILES}}
            identity = type('Identity', (), {'capture': lambda _self, _root, _run: {'taskFingerprint': fingerprint}})()
            with patch.dict('os.environ', {}, clear=True):
                verify_installed(config, (None, None, identity))
                for delta in [{'support_sha256': {}}, {'app_version': 'old'}, {'source_commit': 'dev'}, {'engine_sha256': '0' * 64},
                              {'taskFingerprint': 'e' * 64}, {'payload': str(base)}, {'release_receipt_sha256': '0' * 64}]:
                    with self.assertRaises(Hold):
                        verify_installed({**config, **delta}, (None, None, identity))
                engine.write_bytes(b'replaced-engine')
                with self.assertRaises(Hold):
                    verify_installed(config, (None, None, identity))

    def test_engine_snapshot_cannot_race_wallet_or_hide_nondefault_fields(self):
        snapshot = {'config': {'BENCH_MAX_USD': '.5'}, 'secrets': {'OPENROUTER_API_KEY': 'test-only'}, 'providers': ['openrouter']}
        ensure_config(snapshot, 'exact/model', '.5')
        for changed in [{'BENCH_MAX_USD': '.4'}, {'OPENROUTER_PARAMETERS': {'provider': {'order': ['host']}}},
                        {'GOOSE_MODEL_FIELDS': {'openrouter': {'exact/model': {'temperature': 0}}}},
                        {'GOOSE_MODEL_FIELDS': {'openrouter': {'exact/model': {'effort': 'low'}}}}, {'GOOSE_MAX_TOKENS': 4096}]:
            with self.assertRaises(Hold):
                ensure_config({**snapshot, 'config': {**snapshot['config'], **changed}}, 'exact/model', '.5')


class HistoryAndResumeControls(unittest.TestCase):
    def test_six_call_plan_keeps_exact26_and_two_independent_builds_each(self):
        roster = read(Path(__file__).parent / 'ROSTER.json')
        units = campaign.plan(roster)
        self.assertEqual([u['model'] for u in units if u['kind'] == 'qualification'], roster['models'])
        for model in roster['models']:
            self.assertEqual([u['episode'] for u in units if u['kind'] == 'build' and u['model'] == model], [1, 2])
        self.assertEqual(len([u for u in units if u['stage'] == 'pilot' and u['kind'] == 'build']), 4)

    def history(self, runtime, wrong=False):
        database = runtime / 'goose/data/sessions/sessions.db'
        database.parent.mkdir(parents=True)
        connection = sqlite3.connect(database)
        connection.execute('CREATE TABLE messages(id INTEGER PRIMARY KEY,role TEXT,content_json TEXT)')
        for step in range(1, 6):
            request = {'type': 'toolRequest', 'id': 'tool-' + str(step), 'toolCall': {'status': 'success', 'value':
                       {'name': 'developer__shell', 'arguments': {'command': "'/path with spaces/node' cache-probe-task.cjs " + str(step)}}}}
            response = {'type': 'toolResponse', 'id': 'tool-' + str(step),
                        'toolResult': {'status': 'success', 'value': {'isError': bool(wrong and step == 3)}}}
            connection.execute('INSERT INTO messages(role,content_json) VALUES(?,?)', ('assistant', json.dumps([request])))
            connection.execute('INSERT INTO messages(role,content_json) VALUES(?,?)', ('user', json.dumps([response])))
        connection.execute('INSERT INTO messages(role,content_json) VALUES(?,?)', ('assistant', json.dumps([{'type': 'text', 'text': 'QUALIFICATION_COMPLETE'}])))
        connection.commit()
        connection.close()

    def test_history_requires_actual_ordered_successful_tool_results(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            self.history(runtime)
            result = tool_history(runtime, '/path with spaces/node')
            self.assertEqual(result['matching_successful_responses'], 5)
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            self.history(runtime, wrong=True)
            with self.assertRaises(Hold):
                tool_history(runtime, '/path with spaces/node')

    def test_ambiguous_click_recovery_never_starts_another_paid_unit(self):
        unit = {'key': 'unit', 'kind': 'build', 'status': 'armed', 'baseline_run_ids': ['old']}
        state = {'units': [unit], 'ledger': {}}
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign, 'attest_build_processes'), patch.object(campaign, 'ui', return_value={'sessions': []}) as mocked:
            with self.assertRaises(Hold):
                campaign.observe(Path(directory), state, unit, {}, ())
            mocked.assert_called_once_with({}, 'inspect')

    def test_ambiguous_qualification_dispatch_never_restarts(self):
        unit = {'key': 'probe', 'kind': 'qualification', 'status': 'armed'}
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign.subprocess, 'Popen') as mocked:
            with self.assertRaises(Hold):
                campaign.observe(Path(directory), {'units': [unit]}, unit, {}, ())
            mocked.assert_not_called()

    def test_live_bills_wait_for_stream_end_and_track_native_pending_usage(self):
        model = 'xiaomi/mimo-v2.6-flash'
        row = {'id': model, 'canonical_slug': model + '-20260921', 'context_length': 1050000,
               'top_provider': {'max_completion_tokens': 128000},
               'pricing': {'prompt': '.000002', 'completion': '.00001'}}
        telemetry = {'status': 'recorded', 'ended': {}, 'native_usage_by_id': {}}
        fetch = Mock(return_value=(None, 'not ready'))
        cost = SimpleNamespace(collect_generation_ids=lambda _runtime, _work: (['gen-1-aa'], {}, telemetry), _fetch=fetch)
        installed = SimpleNamespace(entrant_config=lambda _provider: {}, cloud_env=lambda _provider, _snapshot: {'OPENROUTER_API_KEY': 'test-only'})
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            work = state_dir / 'workspace'
            work.mkdir()
            runtime = campaign.runtime_for(work)
            log = runtime / 'goose/state/logs/llm_request.0.jsonl'
            log.parent.mkdir(parents=True)
            log.write_text(json.dumps({'input': {'model': model, 'tools': [{}], 'max_tokens': 1000}}) + '\n' + json.dumps({'data': {'id': 'gen-1-aa'}}) + '\n')
            unit = {'key': 'unit', 'model': model, 'workdir': str(work), 'catalog': row, 'stage': 'pilot', 'wallet': '.5'}
            state = {'ledger': {}, 'units': [unit]}
            campaign.reconcile(state_dir, state, unit, {}, (cost, None, None, None, installed))
            fetch.assert_not_called()
            state['ledger']['prior'] = {'unit': 'old-pilot', 'stage': 'pilot', 'billed_usd': '9.999'}
            with self.assertRaises(Hold):
                campaign.reconcile(state_dir, state, unit, {}, (cost, None, None, None, installed))
            del state['ledger']['prior']
            self.assertEqual(unit['unbilled']['gen-1-aa'], 'stream not ended in telemetry')
            self.assertGreater(Decimal(unit['pending_reserved_usd']), 0)
            telemetry.update(ended={'gen-1-aa': 'completed'}, native_usage_by_id={'gen-1-aa': {'prompt': 100, 'completion': 10}})
            campaign.reconcile(state_dir, state, unit, {}, (cost, None, None, None, installed))
            fetch.assert_called_once()
            self.assertEqual(Decimal(unit['pending_reserved_usd']), Decimal('.000300'))
            fetch.return_value = ({'id': 'gen-1-aa', 'model': row['canonical_slug'], 'provider_name': 'host', 'usage': Decimal('.05')}, None)
            campaign.reconcile(state_dir, state, unit, {}, (cost, None, None, None, installed))
            self.assertEqual(spend(state['ledger']), Decimal('.05'))
            self.assertEqual(unit['pending_reserved_usd'], '0')

    def test_start_real_branch_reserves_pending_calls_and_prepaid_credits(self):
        row = {'id': 'openai/gpt-6.1-sol', 'context_length': 1050000,
               'top_provider': {'max_completion_tokens': 128000},
               'pricing': {'prompt': '.000002', 'completion': '.00001', 'input_cache_write': '.0000025'}}
        model = row['id']
        qualification = {'key': 'qualification', 'kind': 'qualification', 'model': model,
                         'status': 'qualified', 'stage': 'pilot', 'cache': {'projected_build_usd': '.1'}}
        unit = {'key': 'pilot', 'model': model, 'stage': 'pilot', 'kind': 'build', 'status': 'pending'}
        receipt = {'unit': 'qualification', 'requested_model': model, 'kind': 'entrant',
                   'input_json_bytes': 10000, 'max_tokens_sent': None, 'native_tokens_prompt': 2000,
                   'native_tokens_completion': 100, 'billed_usd': '.10', 'stage': 'pilot'}
        state = {'units': [qualification, unit], 'ledger': {'gen-1-aa': receipt}, 'starting_credits_usd': '5'}
        config = {'initial_headroom_usd': '.5', 'python': '/native/python', 'payload': '/installed/payload'}
        snapshot = {'config': {}, 'secrets': {}}
        module = type('Installed', (), {'entrant_config': lambda _self, _provider: snapshot})()
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign, 'process_rows', return_value={}), patch.object(campaign, 'verify_installed'), patch.object(campaign, 'live_credit_evidence', return_value={'effective_remaining_usd': '5'}), patch.object(campaign, 'catalog', return_value=({model: row}, 'hash')), patch.object(campaign, 'ensure_config'), patch.object(campaign, 'ui', side_effect=[{'form': {'running': False}, 'sessions': []}, {}, {'runId': 'original', 'dataDir': '/tmp/retained'}]):
            campaign.start(Path(directory), state, unit, config, (None, None, None, None, module))
        self.assertEqual(unit['status'], 'running')
        self.assertEqual(unit['runId'], 'original')
        self.assertEqual(Decimal(unit['start_reservation']['usd']), Decimal('1.317'))
        self.assertEqual(Decimal(unit['wallet']), Decimal('3.583'))

    def test_accounting_failure_cancels_original_build_and_retains_ledger_tree(self):
        unit = {'key': 'pilot', 'kind': 'build', 'status': 'running', 'runId': 'original',
                'model': 'openai/gpt-6.1-sol', 'stage': 'pilot', 'workdir': '/retained-original-tree'}
        ledger = {'gen-1-aa': {'unit': 'pilot', 'stage': 'pilot', 'billed_usd': '.15'}}
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            write(state_dir / 'config.json', {'payload': '/installed/payload', 'engine': '/installed/engine'})
            write(state_dir / 'state.json', {'units': [unit], 'ledger': ledger})
            with patch.object(campaign, 'load_installed', return_value=()), patch.object(campaign, 'verify_installed'), patch.object(campaign, 'observe', side_effect=Hold('actual route identity failure')), patch.object(campaign, 'reconcile'), patch.object(campaign, 'ui', return_value={'runId': 'original', 'status': 'cancelled'}) as control, patch.object(campaign.subprocess, 'Popen') as paid_restart:
                campaign.run(state_dir)
                control.assert_called_once_with({'payload': '/installed/payload', 'engine': '/installed/engine'}, 'cancel', '--execute-mutations', 'run=original')
                paid_restart.assert_not_called()
            state = read(state_dir / 'state.json')
            self.assertEqual(state['status'], 'held')
            self.assertEqual(state['ledger'], ledger)
            self.assertEqual(state['units'][0]['status'], 'unscored')
            self.assertEqual(state['units'][0]['workdir'], '/retained-original-tree')

    def test_qualification_defaults_readback_precedes_owned_paid_dispatch(self):
        model = 'xiaomi/mimo-v2.6-flash'
        unit = {'key': 'probe', 'model': model, 'stage': 'pilot', 'kind': 'qualification', 'status': 'pending'}
        state = {'units': [unit], 'ledger': {}, 'starting_credits_usd': '5'}
        config = {'initial_headroom_usd': '.5', 'qualification_wallet_usd': '.5'}
        module = SimpleNamespace(entrant_config=lambda _provider: {'config': 'actual snapshot'})
        events = []
        def control(_config, command, *arguments):
            if command == 'inspect':
                return {'form': {'running': False}, 'sessions': []}
            events.append((command, arguments))
        def readback(snapshot, route, wallet):
            events.append(('engine-readback', route, wallet, snapshot))
        def spawn(*_args, **_kwargs):
            events.append(('paid-dispatch',))
            return SimpleNamespace(pid=123)
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign, 'verify_installed'), patch.object(campaign, 'live_credit_evidence', return_value={'effective_remaining_usd': '5'}), patch.object(campaign, 'catalog', return_value=({model: {'id': model}}, 'hash')), patch.object(campaign, 'ensure_config', side_effect=readback), patch.object(campaign, 'ui', side_effect=control), patch.object(campaign.subprocess, 'Popen', side_effect=spawn):
            campaign.start(Path(directory), state, unit, config, (None, None, None, None, module))
        self.assertEqual([event[0] for event in events], ['configure', 'engine-readback', 'paid-dispatch'])
        self.assertEqual(events[1][1:3], (model, '0.5'))
        self.assertEqual(unit['status'], 'running')

    def test_restart_install_mismatch_stops_original_before_any_new_paid_work(self):
        unit = {'key': 'pilot', 'kind': 'build', 'status': 'running', 'runId': 'original',
                'model': 'openai/gpt-6.1-sol', 'stage': 'pilot', 'workdir': '/original-tree'}
        with tempfile.TemporaryDirectory() as directory:
            state_dir = Path(directory)
            config = {'payload': '/installed/payload', 'engine': '/installed/engine'}
            write(state_dir / 'config.json', config)
            write(state_dir / 'state.json', {'units': [unit], 'ledger': {}})
            with patch.object(campaign, 'load_installed', return_value=()), patch.object(campaign, 'verify_installed', side_effect=Hold('installed task changed')), patch.object(campaign, 'reconcile'), patch.object(campaign, 'ui', return_value={'runId': 'original', 'status': 'cancelled'}) as control, patch.object(campaign, 'start') as new_paid:
                campaign.run(state_dir)
                control.assert_called_once_with(config, 'cancel', '--execute-mutations', 'run=original')
                new_paid.assert_not_called()
            state = read(state_dir / 'state.json')
            self.assertEqual(state['units'][0]['status'], 'unscored')
            self.assertIn('installed task changed', state['hold_reason'])

    def test_build_runner_identity_and_attested_orphan_ancestry_are_specific(self):
        import hashlib
        config = {'python': '/native/python', 'payload': '/installed/payload', 'engine': '/installed/goose'}
        unit = {'runId': 'cloud-original', 'model': 'exact/model'}
        args = '/native/python -B -u /installed/payload/bench/run_build.py --entrant openrouter-cloud-original --provider openrouter --model exact/model --forge2'
        snapshot = {111: {'parent': 1, 'args': args}, 112: {'parent': 111, 'args': '/installed/goose run --model exact/model'},
                    113: {'parent': 112, 'args': 'tool'}, 999: {'parent': 1, 'args': args.replace('cloud-original', 'cloud-neighbor')}}
        self.assertEqual(set(campaign.build_owned_pids(snapshot, unit, config)), {111, 112, 113})
        orphan = {'runId': 'cloud-original', 'model': 'exact/model', 'owned_processes': [{'pid': 112, 'args_sha256': hashlib.sha256(snapshot[112]['args'].encode()).hexdigest()}]}
        del snapshot[111]
        snapshot[112]['parent'] = 1
        self.assertEqual(set(campaign.build_owned_pids(snapshot, orphan, config)), {112, 113})
        snapshot[112]['args'] = 'unrelated reused pid'
        self.assertEqual(campaign.build_owned_pids(snapshot, orphan, config), [])

    def test_lost_cdp_cancel_uses_attested_owned_pid_fallback(self):
        unit = {'key': 'pilot', 'kind': 'build', 'status': 'running', 'runId': 'original', 'workdir': '/original-tree'}
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign, 'ui', side_effect=Hold('CDP unavailable')), patch.object(campaign, 'stop_build_pids', return_value={'runId': 'original', 'pids': [123], 'surface': 'owned fallback'}) as stop:
            state = {'units': [unit], 'ledger': {}}
            campaign.abort_active(Path(directory), state, unit, {'engine': '/installed/goose'}, 'budget')
            stop.assert_called_once_with(unit, {'engine': '/installed/goose'})
            self.assertEqual(unit['status'], 'stopped-unscored')
            self.assertEqual(unit['stop_receipt']['pids'], [123])

    def test_ui_timeout_and_malformed_reply_enter_owned_stop_fallback(self):
        config = {'node': '/native/node', 'engine': '/installed/goose'}
        for outcome in (subprocess.TimeoutExpired(['node', 'dispatch'], 90), SimpleNamespace(returncode=0, stdout='invalid JSON')):
            unit = {'key': 'pilot', 'kind': 'build', 'status': 'running', 'runId': 'original', 'workdir': '/original-tree'}
            side_effect = outcome if isinstance(outcome, Exception) else None
            result = None if side_effect else outcome
            with tempfile.TemporaryDirectory() as directory, patch.object(campaign.subprocess, 'run', side_effect=side_effect, return_value=result), patch.object(campaign, 'stop_build_pids', return_value={'runId': 'original', 'pids': [123]}) as stop:
                state = {'units': [unit], 'ledger': {}}
                campaign.abort_active(Path(directory), state, unit, config, 'accounting hold')
                stop.assert_called_once_with(unit, config)
                self.assertEqual(unit['status'], 'stopped-unscored')

    def test_per_pid_stop_reaches_children_in_their_own_process_sessions(self):
        with tempfile.TemporaryDirectory() as directory:
            private = Path(directory)
            script = private / 'cache_probe.py'
            unit_path = private / 'unit.json'
            unit_path.write_text('{}')
            script.write_text("import subprocess,sys,time,json,os,pathlib\n"
                "path=pathlib.Path(sys.argv[-1]).parent\n"
                "if '--leaf' not in sys.argv:\n"
                " child=subprocess.Popen([sys.executable,__file__,'--leaf',sys.argv[-1]],start_new_session=True)\n"
                " (path/'child.json').write_text(json.dumps({'pid':child.pid}))\n"
                " (path/'ready').write_text('ready')\n"
                "time.sleep(30)\n")
            proc = subprocess.Popen([sys.executable, str(script), str(unit_path)], start_new_session=True)
            child_pid = None
            try:
                end = time.monotonic() + 4
                while not (private / 'ready').exists() and time.monotonic() < end:
                    time.sleep(.01)
                self.assertTrue((private / 'ready').exists())
                child_pid = read(private / 'child.json')['pid']
                stopped = campaign.stop_qualification({'pid': proc.pid}, private, '/not-a-paid-engine')
                self.assertIn(proc.pid, stopped)
                self.assertIn(child_pid, stopped)
                self.assertFalse(campaign.alive(proc.pid))
                self.assertFalse(campaign.alive(child_pid))
                proc.wait(timeout=1)
            finally:
                for pid in (proc.pid, child_pid):
                    if pid and campaign.alive(pid):
                        try:
                            import signal
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass

    def test_unrelated_pid_is_rejected_before_any_signal(self):
        with patch.object(campaign, 'process_rows', return_value={4242: {'parent': 1, 'args': 'unrelated process'}}), patch.object(campaign.os, 'kill') as kill:
            with self.assertRaises(Hold):
                campaign.stop_qualification({'pid': 4242}, Path('/private/unit'), '/installed/goose')
            kill.assert_not_called()

    def test_sol_ceiling_latches_before_next_unit(self):
        unit = {'key': 'pilot', 'kind': 'build', 'status': 'running', 'runId': 'actual-build',
                'model': 'openai/gpt-6.1-sol', 'stage': 'pilot'}
        state = {'units': [unit], 'ledger': {'gen-1-aa': {'id': 'gen-1-aa', 'kind': 'entrant', 'native_tokens_prompt': 100, 'native_tokens_cached': 50}}}
        unit['generation_ids'] = ['gen-1-aa']
        verdict = {'taskFingerprint': 'f' * 64, 'buildId': 'actual-build', 'status': 'scored', 'publishable': True, 'score': .90}
        with tempfile.TemporaryDirectory() as directory, patch.object(campaign, 'reconcile'), patch.object(campaign, 'ui', return_value={'verdict': verdict}):
            with self.assertRaises(Hold):
                campaign.finish(Path(directory), state, unit, {'taskFingerprint': 'f' * 64}, ())
            self.assertEqual(unit['status'], 'completed')
            self.assertTrue(state['launch_blocked'])


if __name__ == '__main__':
    unittest.main()
