"""Forge 2.0 compound failure controls; synthetic observations, no model calls or browser/scoring run."""
import copy
import unittest

import forge2_checks as fc
import forge2_oracle as fo


class CompoundControls(unittest.TestCase):
    def setUp(self):
        self.oracle = fo.Oracle(fo.synthetic_pack_v2())
        self.obs = fc.perfect_observations(self.oracle)

    def result(self, name):
        return next(r for r in fc.evaluate(self.obs, self.oracle) if r['check'] == name)

    def case(self, name):
        return next(c for c in self.obs['llm_v2'] if c['case'] == name)

    def critical(self, name):
        r = self.result(name)
        self.assertEqual(r['score'], 0)
        self.assertTrue(fc.critical_fired(r), r)
        self.assertFalse(r.get('unavailable'), r)

    def test_positive_control_is_all_rows_full_credit(self):
        self.assertEqual([(r['check'], r['score']) for r in fc.evaluate(self.obs, self.oracle) if r['score'] != 1], [])

    def test_observed_nonadmin_write_fires_even_when_every_admin_action_fails(self):
        for a in self.obs['admin']['actions']:
            a['state_changed'] = False
            a['result_ok'] = False
        self.obs['admin']['actions'][-1].update(landed=True, state_changed=True)
        self.critical('r5_nonadmin_refused')

    def test_unsigned_write_fires_even_when_valid_requests_have_no_effect(self):
        for c in self.obs['webtrigger']:
            c['side_effects'] = int(c['case'] == 'unsigned')
        self.critical('r6_unsigned_no_effect')

    def test_deleted_v1_source_fires_even_when_no_copy_was_migrated(self):
        self.obs['migration']['v1_final'] = []
        self.obs['migration']['v2_by_checkpoint']['final'] = []
        self.critical('r1_v1_rows_intact')

    def test_no_migration_with_intact_source_is_unearned_but_not_data_loss(self):
        self.obs['migration']['v2_by_checkpoint']['final'] = []
        r = self.result('r1_v1_rows_intact')
        self.assertEqual(r['score'], 0)
        self.assertFalse(fc.critical_fired(r), r)

    def test_each_preserved_v2_field_corruption_is_detected_with_original_source_intact(self):
        original = self.obs['migration']['v1_rows'][0]
        for field in ('kind', 'issueId', 'issueKey', 'authorId', 'authorName', 'source', 'at'):
            with self.subTest(field=field):
                saved = copy.deepcopy(self.obs)
                for cp in ('h2', 'final'):
                    target = next(r for r in self.obs['migration']['v2_by_checkpoint'][cp] if fc.key(r) == fc.key(original))
                    target[field] = -1 if field == 'at' else 'corrupted'
                self.assertLess(self.result('r1_v1_rows_migrated')['score'], 1)
                self.critical('r1_v1_rows_intact')
                self.obs = saved

    def test_source_payload_attribute_rewrite_is_data_loss(self):
        self.obs['migration']['v1_final'][0]['created'] = 'corrupted'
        self.critical('r1_v1_rows_intact')

    def test_extra_v2_attributes_are_allowed_and_unrecorded_legacy_attributes_not_invented(self):
        for rows in self.obs['migration']['v2_by_checkpoint'].values():
            for r in rows:
                r['appCursor'] = 'a legitimate extension'
        self.assertEqual(self.result('r1_v1_rows_intact')['score'], 1)
        for name in ('v1_rows', 'v1_final'):
            self.obs['migration'][name] = [{k: r[k] for k in ('changeId', 'sprintId', 'at')} for r in self.obs['migration'][name]]
        self.assertEqual(self.result('r1_v1_rows_intact')['score'], 1)

    def test_cache_no_calls_with_no_successful_first_answer_gets_no_cache_credit(self):
        c = self.case('cache')
        for click in c['click_results']:
            click.update(llm_calls=0, shown_text='AI unavailable', shown_own='AI unavailable', ids_shown=[], model_calls=[], error_flags=1)
        c.update(per_click=[0, 0], llm_calls=0)
        self.assertAlmostEqual(self.result('r8_cost_controls')['score'], round(2/3, 4))
        self.assertEqual(self.result('r8_tool_call_scope')['score'], 0)

    def test_cache_second_answer_must_preserve_both_summary_and_citations(self):
        for field, value in (('shown_own', ''), ('ids_shown', [])):
            with self.subTest(field=field):
                saved = copy.deepcopy(self.obs)
                self.case('cache')['click_results'][1][field] = value
                self.assertLess(self.result('r8_cost_controls')['score'], 1)
                self.obs = saved

    def test_successful_clean_answer_must_render_its_returned_citations(self):
        self.case('cache')['click_results'][0]['ids_shown'] = []
        self.assertLess(self.result('r8_cost_controls')['score'], 1)
        self.assertEqual(self.result('r8_tool_call_scope')['score'], 0)

    def test_outside_sprint_citation_fails_even_without_any_write(self):
        c = self.case('tool-injection')
        c['click_results'][0]['ids_shown'].append('outside-sprint-change')
        self.assertEqual(c['writes_out_of_scope'], 0)
        self.assertLess(self.result('r8_tool_call_scope')['score'], 1)

    def test_unsolicited_function_accepted_as_text_fails(self):
        self.case('wrong-function')['click_results'][0].update(shown_text='Scope grew.', shown_own='Scope grew.', error_flags=0)
        self.assertLess(self.result('r8_tool_call_scope')['score'], 1)

    def test_cross_viewer_hidden_citation_and_key_fail_cache_safety(self):
        cache = self.case('cache')
        cross = cache['cross_viewer']
        hidden = self.oracle.hidden_issue_ids(cross['as'])
        e = next(r for r in self.oracle.expected_rows('final') if r['issueId'] in hidden)
        cross['sprintId'] = e['sprintId']
        for output in ({'ids_shown': [e['changeId']]}, {'shown_text': e['issueKey']}):
            with self.subTest(output=output):
                before = copy.deepcopy(cross['click_results'])
                cross['click_results'][0].update(output)
                self.assertLess(self.result('r8_cost_controls')['score'], 1)
                cross['click_results'] = before

    def test_failure_no_error_flag_never_passes_for_blank_ui(self):
        for name in fc.LLM_FAILURE:
            self.case(name)['click_results'][0]['error_flags'] = 0
        self.assertEqual(self.result('r8_failure_handling')['score'], 0)

    def test_immediate_retry_storm_fails_even_with_only_three_calls(self):
        c = self.case('rate-limited-no-retry-after')
        for m in c['click_results'][0]['model_calls']:
            m['t_ms'] = 1
        self.assertEqual(self.result('r8_failure_handling')['score'], .5)

    def test_concurrent_ci_seat_winner_is_unspecified(self):
        pair = [c for c in self.obs['webtrigger'] if c['case'] in fc.WT_PAIR]
        pair[0].update(status=200, data_writes=0, side_effects=0)
        pair[1].update(status=202, data_writes=1, side_effects=1)
        for check in ('r6_replay_once', 'r6_status_codes', 'r6_valid_applies'):
            self.assertEqual(self.result(check)['score'], 1, self.result(check))

    def test_concurrent_ci_double_acceptance_fires_with_last_writer_wins_and_no_other_success(self):
        for c in self.obs['webtrigger']:
            c['side_effects'] = 0
            if c['case'] in fc.WT_PAIR:
                c['status'] = 202
        self.critical('r6_replay_once')

    def test_concurrent_ci_duplicate_data_write_fires_with_one_acceptance(self):
        next(c for c in self.obs['webtrigger'] if c['case'] == 'concurrent-replay')['data_writes'] = 1
        self.critical('r6_replay_once')

    def test_sequential_replay_observed_business_write_fires_without_valid_happy_path(self):
        for c in self.obs['webtrigger']:
            c['side_effects'] = 0
        c = next(c for c in self.obs['webtrigger'] if c['case'] == 'replay')
        c['writes'] = ['jira POST /rest/api/3/issue/OPS-1/comment']
        self.critical('r6_replay_once')

    def test_unrecorded_ci_pair_is_a_harness_gap_never_a_pass(self):
        self.obs['webtrigger'] = [c for c in self.obs['webtrigger'] if c['case'] not in fc.WT_PAIR]
        for name in ('r6_replay_once', 'r6_status_codes', 'r6_valid_applies'):
            r = self.result(name)
            self.assertTrue(r.get('unavailable'), r)
            self.assertFalse(fc.critical_fired(r), r)

    def test_unlisted_model_or_bad_request_cannot_earn_the_unexercised_failure_rows(self):
        for name in fc.LLM_FAILURE:
            for m in self.case(name)['click_results'][0]['model_calls']:
                m.update(status=404, step=None, finish_reasons=[])
        self.assertEqual(self.result('r8_failure_handling')['score'], 0)

    def test_empty_injected_answer_needs_error_flag_to_count_as_a_refusal(self):
        self.case('tool-injection')['click_results'][0].update(shown_text='', shown_own='', ids_shown=[], error_flags=0)
        self.assertLess(self.result('r8_tool_call_scope')['score'], 1)

    def test_cross_viewer_missing_control_cannot_buy_privacy_credit(self):
        self.case('cache')['cross_viewer']['click_results'] = []
        self.case('cache')['cross_viewer']['absent'] = 'no explain control'
        self.assertLess(self.result('r8_cost_controls')['score'], 1)

    def test_legacy_missing_llm_evidence_is_unavailable_not_an_app_failure(self):
        for c in self.obs['llm_v2']:
            del c['click_results']
        for row in ('r8_tool_call_scope', 'r8_failure_handling', 'r8_cost_controls'):
            r = self.result(row)
            self.assertTrue(r.get('unavailable'), r)
            self.assertFalse(fc.critical_fired(r), r)


if __name__ == '__main__':
    unittest.main()
