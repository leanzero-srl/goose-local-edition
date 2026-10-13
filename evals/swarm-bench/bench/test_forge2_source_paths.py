"""First-writer source controls using real scorer/oracle; no native runtime or browser."""
import copy
from decimal import Decimal
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import forge2_oracle as fo
import score_forge2 as sf


class FirstWriterControls(unittest.TestCase):
    def setUp(self):
        self.pack = fo.synthetic_pack_v2()
        self.oracle = fo.Oracle(self.pack)
        self.change = next(ch for ch in self.oracle.changes('final') if ch.phase == 'live'
                           and not ch.dropped and ch.sources == ('event',))
        self.time = self.change.at.timestamp()
        self.manifest = {'app': {'storage': {'entities': [{'name': 'scope-ledger'}]}},
                         'modules': {'consumer': [{'key': 'worker'}], 'trigger': [{'key': 'updated'}],
                                     'scheduledTrigger': [{'key': 'hourly'}]}}
        self.history = {'id': self.change.change_id, 'created': self.change.at_text,
                        'author': {'accountId': self.change.by}, 'items': [
                            {'fieldId': self.oracle.sprint_field,
                             'from': self.change.sprint_id if self.change.kind == 'removed' else '',
                             'to': self.change.sprint_id if self.change.kind == 'added' else ''}]}
        self.issue = {'id': self.change.issue_id, 'key': self.change.issue_key,
                      'changelog': {'histories': [self.history]}}
        self.read = self.call('jira', '/rest/api/3/search/jql', 1, status=200,
                              body={'jql': 'sprint in openSprints()'}, response={'issues': [self.issue, {'id': 'other-issue'}]})
        self.write = self.call('kvs', '/api/v1/entity/set', 2, status=204,
                               body={'entityName': 'scope-ledger', **self.item(self.change, 'reconcile'),
                                     'options': {'keyPolicy': 'FAIL_IF_EXISTS'}})

    def item(self, change, source):
        return {'key': ':'.join(change.key), 'value': {'changeId': change.change_id, 'sprintId': change.sprint_id,
            'issueId': change.issue_id, 'issueKey': change.issue_key, 'at': fo.epoch_ms(change.at),
            'kind': change.kind, 'authorId': change.by, 'authorName': change.by_name, 'source': source}}

    def call(self, service, path, offset, **extra):
        return {'t': self.time + offset, 'inv': 'first-writer', 'kind': 'consumer', 'provider': 'app',
                'service': service, 'method': 'POST', 'path': path,
                'originChange': self.change.change_id, 'scheduledRun': None, **extra}

    def ctx(self, calls=None, deliveries=None, wall=False, scheduled=False):
        obs = {'manifest': copy.deepcopy(self.manifest), 'phases': {
            'live': {'calls': copy.deepcopy([self.read, self.write] if calls is None else calls),
                     'deliveries': copy.deepcopy(deliveries or [])}}}
        if wall:
            obs['rate'] = {'wall': {'t_ms': fo.epoch_ms(self.change.at) - 1,
                                   'until_ms': fo.epoch_ms(self.change.at) + 1}}
        if scheduled:
            obs['phases']['hour-1'] = {'at': self.time + 0.5}
            obs['phases']['live']['deliveries'].append(
                {'inv': 'own-event', 'eventId': 'own#0', 'result': 'ok', 't': self.time + 10})
            obs['phases']['live']['calls'].append(self.call('kvs', '/api/v1/entity/get', 10,
                                                          inv='own-event', body={}, status=200))
        return sf.Ctx(None, obs, self.pack)

    def proof(self, calls=None, deliveries=None):
        return sf._first_recorded_source(self.ctx(calls, deliveries), self.change)

    def test_collection_first_writer_is_reconcile_without_scheduled_lineage(self):
        ctx = self.ctx()
        self.assertEqual(sf.sources_for(ctx, self.change), ('reconcile',))
        self.assertTrue(sf.row_correct(sf._tokens(self.write['body']), self.change, sf.sources_for(ctx, self.change)))
        self.assertFalse(sf._scheduled_work_between(ctx, self.change))

    def test_sanitized_native_two_call_regression_preserves_the_observed_first_path(self):
        fixture = json.loads((Path(__file__).parent / 'fixtures/forge2-first-source-native.json').read_text())
        calls = fixture['calls']
        value = calls[1]['body']['value']
        at = fo.instant(value['at'])
        change = fo.Change(value['changeId'], value['sprintId'], value['issueId'], value['issueKey'],
                           value['kind'], at, at.isoformat(), value['authorId'], value['authorName'],
                           Decimal(0), 'live', False, 0, ('event',))
        ctx = self.ctx(calls)
        ctx.oracle.sprint_field = fixture['sprintField']
        self.assertEqual(sf.sources_for(ctx, change), ('reconcile',))
        self.assertLess(calls[0]['t'], calls[1]['t'])

    def test_source_metadata_or_job_labels_without_exact_native_history_prove_nothing(self):
        mutations = {
            'missing response': lambda r: r.pop('response'),
            'read failed': lambda r: r.update(status=429),
            'user read': lambda r: r.update(provider='user'),
            'trigger read': lambda r: r.update(kind='trigger'),
            'wrong issue': lambda r: r['response']['issues'][0].update(id='wrong'),
            'wrong change': lambda r: r['response']['issues'][0]['changelog']['histories'][0].update(id='wrong'),
            'wrong created': lambda r: r['response']['issues'][0]['changelog']['histories'][0].update(created='2000-01-01T00:00:00Z'),
            'wrong author': lambda r: r['response']['issues'][0]['changelog']['histories'][0]['author'].update(accountId='wrong'),
            'wrong field': lambda r: r['response']['issues'][0]['changelog']['histories'][0]['items'][0].update(fieldId='wrong'),
            'wrong sprint': lambda r: r['response']['issues'][0]['changelog']['histories'][0]['items'][0].update(**{'from': 'wrong', 'to': 'wrong'}),
            'read after write': lambda r: r.update(t=self.write['t'] + 1),
            'same timestamp': lambda r: r.update(t=self.write['t']),
            'unjoined invocation': lambda r: r.update(inv='unrelated'),
            'unjoined lineage': lambda r: r.update(originChange='unrelated'),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                read = copy.deepcopy(self.read)
                mutate(read)
                self.assertIsNone(self.proof([read, self.write]))
        labelled = self.call('queue', '/webhook/queue/publish/example', 0, status=201,
                             body={'jobId': 'asserted', 'payload': [{'body': {'kind': 'reconcile', 'cursor': 'page'}}]})
        self.assertIsNone(self.proof([labelled, self.write]))

    def test_failed_wrong_entity_trigger_or_malformed_initial_write_cannot_prove_reconcile(self):
        variants = [dict(status=400), dict(status=None), dict(kind='trigger'), dict(provider='user'),
                    dict(body={**self.write['body'], 'entityName': 'other-entity'}),
                    dict(body={**self.write['body'], 'options': {'ttl': {'value': 1, 'unit': 'SECONDS'}}})]
        for mutation in variants:
            with self.subTest(mutation=mutation):
                self.assertIsNone(self.proof([self.read, {**self.write, **mutation}]))

    def test_direct_own_issue_and_one_issue_search_are_event_work(self):
        for path, response in ((f'/rest/api/3/issue/{self.change.issue_id}?expand=changelog', self.issue),
                               ('/rest/api/3/search/jql', {'issues': [self.issue]})):
            with self.subTest(path=path):
                read = {**self.read, 'method': 'GET', 'path': path, 'response': response}
                write = copy.deepcopy(self.write)
                write['body']['value']['source'] = 'event'
                self.assertEqual(self.proof([read, write]), 'event')
                ctx = self.ctx([read, self.write])
                self.assertEqual(sf.sources_for(ctx, self.change), ('event',))
                self.assertFalse(sf.row_correct(sf._tokens(self.write['body']), self.change, sf.sources_for(ctx, self.change)))

    def test_event_may_record_another_change_of_its_own_issue(self):
        own = next(e for e in self.oracle.history + self.oracle.live
                   if str(e['issueId']) == self.change.issue_id and str(e['changelogId']) != self.change.change_id)
        read = {**self.read, 'response': {'issues': [self.issue]}, 'originChange': own['changelogId']}
        write = copy.deepcopy(self.write)
        write['originChange'] = own['changelogId']
        write['body']['value']['source'] = 'event'
        self.assertEqual(self.proof([read, write]), 'event')
        read['originChange'] = write['originChange'] = 'not-a-trusted-product-change'
        self.assertIsNone(self.proof([read, write]))

    def test_separate_bulk_or_issue_history_requires_observed_collection_membership(self):
        read = {**self.read, 'response': {'issues': [{'id': self.change.issue_id}, {'id': 'other-issue'}]}}
        histories = [self.call('jira', '/rest/api/3/changelog/bulkfetch', 1.5, status=200,
                              response={'issueChangeLogs': [{'issueId': self.change.issue_id, 'changeHistories': [self.history]}]}),
                     self.call('jira', f'/rest/api/3/issue/{self.change.issue_id}/changelog', 1.5, status=200,
                               method='GET', response={'values': [self.history]})]
        for history in histories:
            with self.subTest(path=history['path']):
                self.assertEqual(self.proof([read, history, self.write]), 'reconcile')
                wrong = copy.deepcopy(read)
                wrong['response']['issues'][0]['id'] = 'different'
                ctx = self.ctx([wrong, history, self.write])
                self.assertEqual(sf.sources_for(ctx, self.change), ('event',))
                self.assertFalse(sf.row_correct(sf._tokens(self.write['body']), self.change, sf.sources_for(ctx, self.change)))

    def test_bulk_history_without_collection_proves_event_only_when_scoped_to_own_issue(self):
        read = self.call('jira', '/rest/api/3/changelog/bulkfetch', 1, status=200,
                         response={'issueChangeLogs': [{'issueId': self.change.issue_id, 'changeHistories': [self.history]}]})
        write = copy.deepcopy(self.write)
        write['body']['value']['source'] = 'event'
        self.assertEqual(self.proof([read, write]), 'event')
        read['response']['issueChangeLogs'].append({'issueId': 'other-issue', 'changeHistories': []})
        self.assertIsNone(self.proof([read, write]))

    def test_only_connected_collection_pages_establish_broader_scope(self):
        read = {**self.read, 'response': {'issues': [self.issue], 'nextPageToken': 'actual-next'}}
        page = {**self.read, 't': self.time + 1.5,
                'body': {**self.read['body'], 'nextPageToken': 'actual-next'}, 'response': {'issues': [{'id': 'other'}]}}
        self.assertEqual(self.proof([read, page, self.write]), 'reconcile')
        for mutation in ({'body': {**page['body'], 'nextPageToken': 'skipped'}},
                         {'body': {**page['body'], 'jql': 'other query'}}, {'status': 500}):
            with self.subTest(mutation=mutation):
                ctx = self.ctx([read, {**page, **mutation}, self.write])
                self.assertEqual(sf.sources_for(ctx, self.change), ('event',))
                self.assertFalse(sf.row_correct(sf._tokens(self.write['body']), self.change, sf.sources_for(ctx, self.change)))

    def queued(self):
        read = {**self.read, 'inv': 'parent'}
        publication = self.call('queue', '/webhook/queue/publish/example', 1.25, inv='parent', status=201,
                                body={'jobId': 'observed-job', 'payload': [{'body': {'kind': 'anything'}}]})
        delivery = {'inv': 'first-writer', 'eventId': 'observed-job#0', 'attempt': 2,
                    'result': 'ok', 't': self.time + 1.5}
        return read, publication, delivery

    def test_cached_parent_collection_page_can_feed_queued_writer(self):
        read, publication, delivery = self.queued()
        self.assertEqual(self.proof([read, publication, self.write], [delivery]), 'reconcile')
        self.assertEqual(self.proof([read, publication, self.write], [{**delivery, 'result': 'retry'}]), 'reconcile')
        # A successful write is evidence even when the consumer later asks for a retry.

    def test_queue_parent_must_be_unique_observed_success_before_delivery(self):
        read, publication, delivery = self.queued()
        cases = [([read, {**publication, 'status': 400}, self.write], [delivery]),
                 ([read, publication, self.write], [{**delivery, 'eventId': 'different#0'}]),
                 ([read, publication, self.write], [{**delivery, 'eventId': 'observed-job#1'}]),
                 ([read, publication, self.write], [{**delivery, 't': self.write['t'] + 1}]),
                 ([{**read, 't': publication['t'] + .1}, publication, self.write], [delivery]),
                 ([read, publication, {**publication, 'inv': 'other-parent', 't': self.time}, self.write], [delivery]),
                 ([read, {**publication, 'originChange': 'different'}, self.write], [delivery]),
                 ([read, publication, self.write], [delivery, {**delivery, 'eventId': 'other#0'}])]
        for calls, deliveries in cases:
            with self.subTest(calls=calls, deliveries=deliveries):
                self.assertIsNone(self.proof(calls, deliveries))

    def test_successful_transaction_and_known_batch_item_are_supported(self):
        for path, body, status, response in (
                ('/api/v1/transaction', {'set': [self.write['body']]}, 204, None),
                ('/api/v1/batch/set', [self.write['body']], 200,
                 {'successfulKeys': [{'entityName': 'scope-ledger', 'key': self.write['body']['key']}],
                  'failedKeys': [{'entityName': 'other', 'key': 'other', 'error': {'code': 'KEY_CONFLICT'}}]})):
            with self.subTest(path=path):
                write = {**self.write, 'path': path, 'body': body, 'status': status, 'response': response}
                self.assertEqual(self.proof([self.read, write]), 'reconcile')

    def test_unknown_earlier_batch_or_failed_transaction_blocks_later_proof(self):
        earlier = {**self.write, 't': self.time, 'inv': 'earlier'}
        for path, body, status, response in (
                ('/api/v1/batch/set', [self.write['body']], 200, None),
                ('/api/v1/batch/set', [self.write['body']], 200, {'successfulKeys': [], 'failedKeys': []}),
                ('/api/v1/batch/set', [self.write['body']], 200,
                 {'successfulKeys': [{'entityName': 'scope-ledger', 'key': self.write['body']['key']}],
                  'failedKeys': [{'entityName': 'scope-ledger', 'key': self.write['body']['key']}]}),
                ('/api/v1/transaction', {'set': [self.write['body']]}, 400, None)):
            with self.subTest(path=path, response=response):
                self.assertIsNone(self.proof([{**earlier, 'path': path, 'body': body, 'status': status,
                                               'response': response}, self.read, self.write]))

    def test_known_failed_individual_write_or_batch_item_does_not_become_first_writer(self):
        early = {**self.write, 't': self.time, 'inv': 'failed', 'status': 409}
        self.assertEqual(self.proof([early, self.read, self.write]), 'reconcile')
        early.update(path='/api/v1/batch/set', body=[self.write['body']], status=200,
                     response={'successfulKeys': [], 'failedKeys': [
                         {'entityName': 'scope-ledger', 'key': self.write['body']['key'], 'error': {'code': 'KEY_CONFLICT'}}]})
        self.assertEqual(self.proof([early, self.read, self.write]), 'reconcile')

    def test_invalid_unhashable_key_failure_is_not_a_scorer_error_or_first_writer(self):
        for key in ({'invalid': 'object'}, ['invalid']):
            with self.subTest(key=key):
                early = copy.deepcopy(self.write)
                early.update(t=self.time, inv='invalid-key-attempt', status=400, kvsCode='INVALID_KEY')
                early['body']['key'] = key
                self.assertEqual(self.proof([early, self.read, self.write]), 'reconcile')
                early['status'] = None
                self.assertIsNone(self.proof([early, self.read, self.write]))

    def test_earliest_malformed_write_ambiguity_and_delete_recreate_are_not_skipped(self):
        early = copy.deepcopy(self.write)
        early.update(t=self.time, inv='earlier')
        early['body']['value'].pop('authorId')
        early['body']['value'].pop('authorName')
        self.assertIsNone(self.proof([early, self.read, self.write]))
        self.assertIsNone(self.proof([self.read, self.write, {**self.write, 'inv': 'conflicting'}]))
        deleted = self.call('kvs', '/api/v1/entity/delete', 2.5, status=204,
                            body={'entityName': 'scope-ledger', 'key': self.write['body']['key']})
        self.assertIsNone(self.proof([self.read, self.write, deleted, {**self.write, 't': self.time + 3}]))
        unknown_delete = {**deleted, 'path': '/api/v1/batch/delete', 'status': 200, 'body': [deleted['body']]}
        self.assertIsNone(self.proof([self.read, self.write, unknown_delete, {**self.write, 't': self.time + 3}]))
        untimed = {**early, 't': None}
        self.assertIsNone(self.proof([untimed, self.read, self.write]))

    def test_first_path_survives_later_wrong_source_and_timing_allowances(self):
        event_read = {**self.read, 'response': {'issues': [self.issue]}}
        event_write = copy.deepcopy(self.write)
        event_write['body']['value']['source'] = 'event'
        overwritten = copy.deepcopy(self.write)
        overwritten['t'] += 1
        for wall, scheduled in ((False, False), (True, False), (False, True)):
            with self.subTest(wall=wall, scheduled=scheduled):
                ctx = self.ctx([event_read, event_write, overwritten], wall=wall, scheduled=scheduled)
                self.assertEqual(sf.sources_for(ctx, self.change), ('event',))
                self.assertFalse(sf.row_correct(sf._tokens(overwritten['body']), self.change, sf.sources_for(ctx, self.change)))
                ctx = self.ctx([self.read, self.write, {**event_write, 't': overwritten['t']}], wall=wall, scheduled=scheduled)
                self.assertEqual(sf.sources_for(ctx, self.change), ('reconcile',))

    def test_wrong_initial_source_never_substitutes_for_the_proven_native_path(self):
        wrong = copy.deepcopy(self.write)
        wrong['body']['value']['source'] = 'event'
        ctx = self.ctx([self.read, wrong])
        self.assertEqual(sf.sources_for(ctx, self.change), ('reconcile',))
        self.assertFalse(sf.row_correct(sf._tokens(wrong['body']), self.change, sf.sources_for(ctx, self.change)))
        items = [self.item(ch, ch.sources[0]) for ch in ctx.oracle.changes('live')]
        items = [wrong['body'] if item['key'] == wrong['body']['key'] else item for item in items]
        ctx.phase('live')['kvsAfter'] = {'entities': {'scope-ledger': items}, 'keys': []}
        entry = next(entry for entry in sf.CHECKS if entry[0] == 't_event_rows')
        self.assertLess(sf._run_check(ctx, *entry)['score'], 1)

    def test_unrelated_declared_metadata_never_substitutes_for_the_actual_source_field(self):
        for actual, claimed in (('reconcile', 'event'), ('event', 'reconcile')):
            with self.subTest(actual=actual, claimed=claimed):
                read = copy.deepcopy(self.read)
                if actual == 'event':
                    read['response']['issues'] = [self.issue]
                wrong = copy.deepcopy(self.write)
                wrong['body']['value'].update(source=claimed, deployedEnvs=actual)
                ctx = self.ctx([read, wrong], wall=True)
                self.assertEqual(sf.sources_for(ctx, self.change), (actual,))
                self.assertFalse(sf.row_correct(sf._tokens(wrong['body']), self.change, sf.sources_for(ctx, self.change)))
                corrected = copy.deepcopy(wrong['body'])
                corrected['value']['source'] = actual
                self.assertTrue(sf.row_correct(sf._tokens(corrected), self.change, sf.sources_for(ctx, self.change)))
                missing = copy.deepcopy(wrong['body'])
                missing['value'].pop('source')
                self.assertFalse(sf.row_correct(sf._tokens(missing), self.change, (actual,)))
                invalid = copy.deepcopy(corrected)
                invalid['value']['source'] = [actual]
                self.assertFalse(sf.row_correct(sf._tokens(invalid), self.change, (actual,)))

    def test_all_six_source_consumers_reject_initial_and_later_wrong_source_with_metadata(self):
        registry = {entry[0]: entry for entry in sf.CHECKS}
        changes = self.oracle.changes('final')
        # The synthetic pack explicitly includes a permuted pair, a multi-id move and a dropped update.
        targets = {'t_event_rows': self.change, 't_out_of_order': self.change,
                   't_multi_sprint_parse': next(ch for ch in changes if ch.change_id == '9302'),
                   'r_heal_dropped': next(ch for ch in changes if ch.dropped), 'u_ledger_table': self.change,
                   '_reconcile_outcome_complete': self.change}

        def context(change, mutation):
            when = change.at.timestamp()
            issue = {'id': change.issue_id, 'changelog': {'histories': [{
                'id': change.change_id, 'created': change.at_text, 'author': {'accountId': change.by}, 'items': [{
                    'fieldId': self.oracle.sprint_field,
                    'from': change.sprint_id if change.kind == 'removed' else '',
                    'to': change.sprint_id if change.kind == 'added' else ''}]}]}}
            read = {**self.read, 't': when + 1, 'originChange': None, 'scheduledRun': 1,
                    'response': {'issues': [issue, {'id': 'other'}]}}
            first_item = self.item(change, 'event' if mutation == 'initial' else 'reconcile')
            first_item['value']['deployedEnvs'] = 'reconcile'
            write = {**self.write, 't': when + 2, 'originChange': None, 'scheduledRun': 1,
                     'body': {'entityName': 'scope-ledger', **first_item}}
            calls = [read, write]
            if mutation == 'later':
                later = copy.deepcopy(write)
                later['t'] += 1
                later['body']['value']['source'] = 'event'
                calls.append(later)
            ctx = self.ctx(calls)
            ctx.manifest['modules']['jira:sprintAction'] = [{'key': 'table'}]
            for phase in ('backfill', 'live', 'heal', 'rerun'):
                items = [self.item(ch, ('event' if mutation else 'reconcile') if ch.key == change.key else ch.sources[0])
                         for ch in ctx.oracle.changes('backfill' if phase == 'backfill' else 'final')]
                for item in items:
                    if item['key'] == ':'.join(change.key):
                        item['value']['deployedEnvs'] = 'reconcile'
                ctx.obs['phases'].setdefault(phase, {})['kvsAfter'] = {'entities': {'scope-ledger': items}, 'keys': []}
            renders = []
            for sprint in ctx.oracle.ledger_sprints():
                rows = []
                for ch in ctx.oracle.visible_changes(sprint, ctx.oracle.viewer):
                    rows.append({'changeId': ch.change_id, 'cells': {
                        'issue': ch.issue_key, 'points': str(ch.points), 'kind': ch.kind,
                        'by': ch.by_name, 'at': ch.at_text,
                        'source': ('event' if mutation else 'reconcile') if ch.key == change.key else ch.sources[0],
                        'deployedEnvs': 'reconcile'}})
                renders.append({'sprintId': sprint, 'afterLive': False, 'headers': list(sf.TABLE_COLS),
                                'rows': rows, 'metrics': ctx.oracle.numbers(sprint).metrics_text()})
            ctx.obs['ui'] = {'sprintAction': renders}
            return ctx

        for name, change in targets.items():
            for mutation in ('initial', 'later'):
                with self.subTest(consumer=name, mutation=mutation):
                    good, bad = context(change, None), context(change, mutation)
                    if name == '_reconcile_outcome_complete':
                        self.assertTrue(sf._reconcile_outcome_complete(good))
                        self.assertFalse(sf._reconcile_outcome_complete(bad))
                    else:
                        positive = sf._run_check(good, *registry[name])
                        negative = sf._run_check(bad, *registry[name])
                        self.assertEqual(positive['score'], 1, positive)
                        self.assertFalse(negative.get('unavailable'), negative)
                        self.assertLess(negative['score'], positive['score'], negative)

    def test_permitted_metadata_update_keeps_first_source(self):
        later = copy.deepcopy(self.write)
        later['t'] += 1
        later['body']['value'].update(deleted=True, deployedEnvs='test')
        self.assertEqual(self.proof([self.read, self.write, later]), 'reconcile')

    def test_migrated_rows_keep_v1_source_and_unknown_paths_keep_existing_timing_allowance(self):
        migrated = next(ch for ch in self.oracle.changes('final') if ch.key in self.oracle.v1_keys())
        self.assertIsNone(sf._first_recorded_source(self.ctx(), migrated))
        self.assertEqual(sf.sources_for(self.ctx([], wall=True), self.change), ('event', 'reconcile'))
        self.assertEqual(sf.sources_for(self.ctx([], scheduled=True), self.change), ('event', 'reconcile'))

    def test_shared_source_answer_reaches_table_pagination_outcome_and_event_economy(self):
        ctx = self.ctx()
        for phase, changes in (('backfill', sf.backfill_changes(ctx)), ('heal', ctx.oracle.changes('final')),
                               ('live', ctx.oracle.changes('live'))):
            items = [self.item(ch, 'reconcile' if ch.key == self.change.key else ch.sources[0]) for ch in changes]
            ctx.obs['phases'].setdefault(phase, {})['kvsAfter'] = {'entities': {'scope-ledger': items}, 'keys': []}
        cells = {'issue': self.change.issue_key, 'points': str(self.change.points), 'kind': self.change.kind,
                 'by': self.change.by_name, 'at': self.change.at_text, 'source': 'reconcile'}
        self.assertTrue(sf.cells_ok({'cells': cells}, self.change, sf.sources_for(ctx, self.change)))
        self.assertTrue(sf._reconcile_outcome_complete(ctx))
        registry = {entry[0]: entry for entry in sf.CHECKS}
        before = sf._run_check(ctx, *registry['e_event_economy'])
        self.assertEqual(before['score'], 0)
        event = sf._run_check(ctx, *registry['t_event_rows'])
        self.assertEqual(event['score'], 1, event)
        ctx._row_cache['t_event_rows'] = event
        economy = sf._run_check(ctx, *registry['e_event_economy'])
        self.assertGreater(economy['score'], 0, economy)
        self.assertEqual(economy['parts']['calls'], 1)
        with patch.object(sf, 'sources_for', return_value=('event',)):
            self.assertFalse(sf._reconcile_outcome_complete(ctx))


if __name__ == '__main__':
    unittest.main()
