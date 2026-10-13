"""Contract-aligned pagination controls; real scorer and oracle, no model/browser calls."""
import copy
import unittest

import forge2_oracle as fo
import score_forge2 as sf


class PaginationControls(unittest.TestCase):
    def setUp(self):
        self.pack = fo.synthetic_pack_v2()
        self.oracle = fo.Oracle(self.pack)
        self.obs = sf.selftest_empty_observations(self.pack)
        self.obs['manifest']['app']['storage'] = {'entities': [{'name': 'scope-ledger'}]}
        self.obs['manifest']['modules']['scheduledTrigger'] = [{'key': 'hourly'}]
        def rows(changes):
            return [{'key': ':'.join(change.key), 'value': {
                'changeId': change.change_id, 'sprintId': change.sprint_id, 'kind': change.kind,
                'issueId': change.issue_id, 'issueKey': change.issue_key, 'at': fo.epoch_ms(change.at),
                'authorId': change.by, 'authorName': change.by_name, 'source': change.sources[0]}}
                for change in changes]
        for phase, changes in (('backfill', self.oracle.changes('backfill')),
                               ('heal', self.oracle.changes('final'))):
            self.obs['phases'][phase]['kvsAfter'] = {'entities': {'scope-ledger': rows(changes)}, 'keys': []}

    def page(self, inv, time, token=None, more=None, run=4, query='sprint in (11,12,21)', kind='consumer',
             endpoint='/rest/api/3/search/jql', origin=None):
        return {'inv': inv, 't': time, 'kind': kind, 'scheduledRun': run, 'originChange': origin,
            'service': 'jira', 'method': 'POST', 'path': endpoint, 'status': 200,
            'body': {'jql': query, 'maxResults': 100, **({'nextPageToken': token} if token else {})},
            'response': {'issues': [{'id': str(time)}], **({'nextPageToken': more} if more else {})}}

    def restart(self, query='sprint in (11,12,21)', run=4, origin=None):
        return [self.page('paused', 1, more='a'), self.page('paused', 2, token='a', more='b'),
                self.page('restart', 3, more='a', query=query, run=run, origin=origin),
                self.page('restart', 4, token='a', more='b', query=query, run=run, origin=origin),
                self.page('restart', 5, token='b', query=query, run=run, origin=origin)]

    def result(self, calls):
        self.obs['phases']['live']['calls'] = calls
        entry = next(entry for entry in sf.CHECKS if entry[0] == 'r_pagination')
        return sf._run_check(sf.Ctx(None, self.obs, self.pack), *entry)

    def test_unchanged_filter_full_restart_preserves_contract_outcome(self):
        row = self.result(self.restart())
        self.assertEqual(row['score'], 1, row)
        self.assertEqual(row['parts']['completed_restarts'], 1)

    def test_closed_sprint_current_filter_full_restart_is_supported(self):
        row = self.result(self.restart(query='sprint in (11,21)'))
        self.assertEqual(row['score'], 1, row)
        self.assertEqual(row['parts']['completed_restarts'], 1)

    def test_next_scheduled_run_and_event_path_are_supported_completions(self):
        for run, origin in ((5, None), (None, 'delivered-change')):
            with self.subTest(run=run, origin=origin):
                row = self.result(self.restart(run=run, origin=origin))
                self.assertEqual(row['score'], 1, row)

    def test_repeated_token_page_is_cost_not_lost_data(self):
        calls = [self.page('one', 1, more='a'), self.page('one', 2, token='a', more='b'),
                 self.page('one', 3, token='a', more='b'), self.page('one', 4, token='b')]
        row = self.result(calls)
        self.assertEqual(row['score'], 1, row)
        self.assertEqual(len(sf.Ctx(None, self.obs, self.pack).calls(service='jira')), 4)

    def test_restarted_first_page_inside_one_invocation_can_finish(self):
        calls = self.restart()
        for call in calls:
            call['inv'] = 'one'
        self.assertEqual(self.result(calls)['score'], 1)

    def test_repeated_offset_page_is_supported_and_served_sizes_define_next_offset(self):
        def page(start, count, last):
            return {'path': '/rest/agile/1.0/board', 'body': {'startAt': start, 'maxResults': 100},
                    'response': {'startAt': start, 'values': [{}] * count, 'isLast': last}}
        pages = [page(0, 2, False), page(2, 1, False), page(2, 1, False), page(3, 1, True)]
        self.assertTrue(sf._walk_complete('offset', pages)[0])
        self.assertFalse(sf._walk_complete('offset', [pages[0], page(100, 1, True)])[0])

    def test_skipped_site_cursor_is_still_a_failure(self):
        calls = [self.page('one', 1, more='a'), self.page('one', 2, token='b')]
        self.assertEqual(self.result(calls)['score'], 0)

    def test_cursor_cycle_is_not_completion(self):
        calls = [self.page('one', 1, more='a'), self.page('one', 2, token='a', more='a')]
        self.assertEqual(self.result(calls)['score'], 0)

    def test_unfinished_replacement_cannot_excuse_paused_walk(self):
        calls = self.restart()
        calls[-1]['response']['nextPageToken'] = 'c'
        self.assertEqual(self.result(calls)['score'], 0)

    def test_skipped_replacement_cursor_cannot_excuse_paused_walk(self):
        calls = self.restart()
        calls.pop(3)
        self.assertEqual(self.result(calls)['score'], 0)

    def test_later_tail_only_is_not_a_full_restart(self):
        calls = self.restart()
        calls[2]['body']['nextPageToken'] = 'c'
        row = self.result(calls)
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_user_read_does_not_prove_background_completion(self):
        calls = self.restart()
        for call in calls[2:]:
            call['kind'] = 'resolver'
        row = self.result(calls)
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_untraced_replacement_does_not_prove_background_completion(self):
        calls = self.restart(run=None)
        row = self.result(calls)
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_different_endpoint_is_not_completion_of_the_paused_list(self):
        calls = self.restart()
        for call in calls[2:]:
            call['path'] = '/rest/api/3/changelog/bulkfetch'
        row = self.result(calls)
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_completion_before_the_pause_does_not_excuse_it(self):
        calls = self.restart()
        for call in calls[:2]:
            call['t'] += 10
        row = self.result(calls)
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_actual_deadline_and_final_required_outcome_are_both_needed(self):
        original = copy.deepcopy(self.obs)
        cases = (('missing first-hour change', 'backfill', 'missing'),
                 ('missing final change', 'heal', 'missing'),
                 ('duplicate business row', 'heal', 'duplicate'),
                 ('corrupt final author', 'heal', 'corrupt'))
        for name, phase, mutation in cases:
            with self.subTest(name=name):
                self.obs = copy.deepcopy(original)
                rows = self.obs['phases'][phase]['kvsAfter']['entities']['scope-ledger']
                if mutation == 'missing':
                    wanted = sf.backfill_changes(sf.Ctx(None, self.obs, self.pack))[0] if phase == 'backfill' \
                             else self.oracle.changes('final')[0]
                    rows[:] = [row for row in rows if row['key'] != ':'.join(wanted.key)]
                elif mutation == 'duplicate':
                    rows.append(copy.deepcopy(rows[0]))
                else:
                    rows[0]['value']['authorId'] = 'not-the-author'
                    rows[0]['value']['authorName'] = 'not-the-name'
                row = self.result(self.restart())
                self.assertLess(row['score'], 1, row)
                self.assertNotIn('completed_restarts', row['parts'])

    def test_missing_actual_snapshot_cannot_prove_a_restart_correct(self):
        self.obs['phases']['heal'].pop('kvsAfter')
        row = self.result(self.restart())
        self.assertLess(row['score'], 1, row)
        self.assertNotIn('completed_restarts', row['parts'])

    def test_duplicate_business_effect_still_fires_when_all_pages_were_read(self):
        rows = self.obs['phases']['heal']['kvsAfter']['entities']['scope-ledger']
        rows.append(copy.deepcopy(rows[0]))
        self.obs['phases']['live']['calls'] = self.restart()[2:]
        entry = next(entry for entry in sf.CHECKS if entry[0] == 't_no_double_count')
        row = sf._run_check(sf.Ctx(None, self.obs, self.pack), *entry)
        self.assertTrue(sf.observed_defect(row), row)

    def test_original_persisted_cursor_join_is_still_supported(self):
        calls = [self.page('paused', 1, more='a'), self.page('paused', 2, token='a', more='b'),
                 self.page('resume', 3, token='b', more='c'), self.page('resume', 4, token='c')]
        row = self.result(calls)
        self.assertEqual(row['score'], 1, row)
        self.assertEqual(row['parts']['persisted_continuations'], 1)
        self.assertNotIn('completed_restarts', row['parts'])


if __name__ == '__main__':
    unittest.main()
