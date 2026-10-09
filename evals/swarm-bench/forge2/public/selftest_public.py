"""Self-test for the forge-2.0 public text (P6): every SPEC number reaches the entrant verbatim.

Run: python3 -m unittest evals/swarm-bench/forge2/public/selftest_public.py   (stdlib only)

It reads SPEC.md where the SPEC carries a machine-readable list (the admin labels of §2.6, the weights and bands of §4)
and pins the rest as literals copied from SPEC §1-§2, so a drift on either side fails here. This file is the package's
own check; it is not entrant material and run_build never copies it (FORGE20's `public` tuple names files).
"""
import json
import re
import unittest
from pathlib import Path

PUBLIC = Path(__file__).resolve().parent
FORGE2 = PUBLIC.parent
SPEC = (FORGE2 / 'SPEC.md').read_text()
CONTRACT = (PUBLIC / 'FORGE2-CONTRACT.md').read_text()
PROMPT = (PUBLIC / 'spec-build-forge2.md').read_text()
STARTER = (PUBLIC / 'STARTER.md').read_text()
BROWSER = (PUBLIC / 'BROWSER-TESTING.md').read_text()
RATE = json.loads((PUBLIC / 'RATE-MODEL.json').read_text())
ENTRANT_TEXTS = {'FORGE2-CONTRACT.md': CONTRACT, 'spec-build-forge2.md': PROMPT, 'STARTER.md': STARTER,
                 'BROWSER-TESTING.md': BROWSER, 'RATE-MODEL.json': (PUBLIC / 'RATE-MODEL.json').read_text()}


def spec_section(number: str) -> str:
    match = re.search(rf'^#+ {re.escape(number)} .*?(?=^#+ )', SPEC, re.M | re.S)
    if not match:
        raise AssertionError(f'SPEC section {number} not found')
    return match.group(0)


def flat(text: str) -> str:
    """Markdown wraps lines; compare on single spaces."""
    return re.sub(r'\s+', ' ', text)


class Numbers(unittest.TestCase):
    def assert_in_contract(self, *phrases):
        body = flat(CONTRACT)
        for phrase in phrases:
            self.assertIn(phrase, body, f'contract lacks: {phrase}')

    def test_rate_model_json_equals_spec_2_1(self):
        self.assertEqual(RATE['quota']['pointsPerHour'], 2400)
        self.assertEqual(RATE['background']['maxSharePercent'], 70)
        self.assertEqual(RATE['background']['maxPointsPerHour'], 1680)
        self.assertEqual(RATE['background']['maxPointsPerHour'], RATE['quota']['pointsPerHour'] * 70 // 100)
        costs = {c['request']: c for c in RATE['costs']}
        search = next(c for k, c in costs.items() if 'search/jql' in k)
        self.assertEqual((search['base'], search['plus'], search['per']), (1, 1, 50))
        self.assertIn('GET /rest/api/3/search/jql', search['request'])
        self.assertIn('POST /rest/api/3/search/jql', search['request'])
        bulk = next(c for k, c in costs.items() if 'changelog/bulkfetch' in k)
        self.assertEqual((bulk['points'], bulk['maxIssuesPerCall']), (2, 1000))
        field = next(c for k, c in costs.items() if 'app/field' in k)
        self.assertEqual((field['base'], field['plus'], field['per'], field['maxUpdatesPerRequest']), (1, 1, 50, 200))
        self.assertEqual(next(c for k, c in costs.items() if k.startswith('any other'))['points'], 2)
        self.assertEqual(next(c for k, c in costs.items() if k.startswith('GET (any'))['points'], 1)
        self.assertEqual(next(c for k, c in costs.items() if 'agile' in k)['points'], 1)
        self.assertEqual((RATE['burst']['capacityPoints'], RATE['burst']['refillPointsPerSecond']), (30, 5))
        self.assertEqual((RATE['perIssueWrites']['maxWritesPerIssue'], RATE['perIssueWrites']['windowSeconds']), (1, 2))
        self.assertEqual(RATE['responses']['everyResponse'], {'X-RateLimit-Limit': '2400'})
        self.assertIn('480', RATE['responses']['nearLimit']['when'])
        self.assertIn('20 %', RATE['responses']['nearLimit']['when'])
        self.assertEqual(set(RATE['responses']['nearLimit']['headers']), {'X-RateLimit-Remaining', 'X-RateLimit-NearLimit'})
        self.assertEqual(RATE['responses']['429']['body'], {'errorMessages': ['Rate limit exceeded']})
        self.assertEqual(set(RATE['responses']['429']['reasons']),
                         {'jira-quota-tenant-based', 'jira-burst-based', 'jira-per-issue-on-write'})
        self.assertIn('next virtual hour', RATE['responses']['429']['reasons']['jira-quota-tenant-based'])
        # the SPEC's own words for every number above
        rate = flat(spec_section('2.1'))
        for phrase in ('2,400 points per installation per virtual hour', '70 %', '1,680 points', 'capacity 30 points',
                       'refill 5 points/s', 'at most 1 write per issue per 2 s', '≤ 200 updates per request',
                       '2 per call (≤ 1,000 issues)', '1 + 1 per 50 issues returned', '1 + 1 per 50 updates',
                       'X-RateLimit-Limit: 2400', '< 20 %', '{"errorMessages":["Rate limit exceeded"]}'):
            self.assertIn(phrase, rate, f'SPEC §2.1 changed: {phrase}')

    def test_contract_carries_the_rate_numbers(self):
        self.assert_in_contract('2,400 points per installation per virtual hour', 'default 70 % = 1,680 points',
                                '1 + 1 per 50 issues returned', '2 per call (≤ 1,000 issues)',
                                '1 + 1 per 50 updates (≤ 200 updates per request)', 'any other POST, PUT or DELETE 2',
                                'token bucket of 30 points, refilled at 5 points per virtual second',
                                'at most 1 write per issue per 2 virtual seconds', 'X-RateLimit-Limit: 2400',
                                'fewer than 20 % of the hour\'s points (480)', 'X-RateLimit-NearLimit: true',
                                '{"errorMessages":["Rate limit exceeded"]}', 'jira-quota-tenant-based',
                                'jira-burst-based', 'jira-per-issue-on-write', '65,000-point Tier 1')

    def test_time_and_limits_2_2(self):
        spec = flat(spec_section('2.2'))
        for phrase in ('GET 120 ms', 'search page 300 ms', 'bulkfetch 600 ms', 'writes 200 ms', '**25 s**', '**55 s**',
                       '**900 s**', 'retryAfter ≤ 900 s', 'up to 4 retries'):
            self.assertIn(phrase, spec, f'SPEC §2.2 changed: {phrase}')
        self.assert_in_contract('a GET 120 ms, a search page 300 ms, a `bulkfetch` 600 ms, a write 200 ms',
                                '| UI resolver (Custom UI or UI Kit) | 25 s |',
                                '| async event consumer, scheduled trigger | 55 s by default, up to 900 s with `timeoutSeconds` |',
                                '| web trigger, Rovo action | 55 s |', '`retryAfter` ≤ 900 s', 'retried up to 4 times',
                                'not retried')

    def test_scale_2_3(self):
        self.assert_in_contract('3 projects', '4 scrum boards, 2 estimating with one field and 2 with another',
                                '6 active sprints, 2 future, 6 closed', 'about 1,000 issues',
                                'about 300 of them in active sprints', 'about 200 relevant changes',
                                'about 800 irrelevant issue updates', '6 virtual hours', "first 2 days")

    def test_storage_2_4(self):
        spec = flat(spec_section('2.4'))
        attrs = re.search(r'v2: entity `scope-ledger`, attributes (.*?); index', spec).group(1)
        pairs = re.findall(r'`(\w+)` (string|float|boolean)', attrs)
        self.assertEqual(len(pairs), 11, attrs)
        body = flat(CONTRACT)
        for name, kind in pairs:
            self.assertIn(f'`{name}` {kind}', body)
        self.assert_in_contract('index `by-sprint`, partition `[sprintId]`, range `[at]`',
                                'exactly ONE range attribute',
                                '409 `KEY_CONFLICT`', '400 `CONDITIONAL_CHECK_FAILED`', '422 `UNPROCESSABLE_ENTITY`',
                                '404 `SCHEMA_NOT_FOUND`', 'more than 25 operations')

    def test_v1_layout_matches_the_starter_manifest(self):
        manifest = (FORGE2 / 'starter' / 'manifest.yml').read_text()
        block = re.search(r'- name: scope-change\n(.*?)\n      - name:', manifest, re.S).group(1)
        attributes = re.findall(r'^          (\w+):\n            type:', block, re.M)
        self.assertEqual(attributes, ['sprintId', 'changeId', 'at', 'created', 'issueId', 'issueKey', 'kind',
                                      'authorId', 'authorName', 'source'])
        value = re.search(r'value `\{ (.*?) \}`', flat(STARTER)).group(1)
        self.assertEqual(sorted(v.strip() for v in value.split(',')), sorted(attributes))
        self.assertIn('runtime:\n    name: nodejs22.x', manifest)

    def test_world_2_5(self):
        self.assert_in_contract('its ledger is final', 'it leaves the widget, which shows active sprints only',
                                'recorded as `removed` from the old sprint and `added` to the new one',
                                'use the NEW board\'s estimation field',
                                'changes after the switch use the new field; earlier rows keep their `estimate`',
                                'its rows stay as history with `deleted: true`',
                                'their next request shows none of that issue\'s rows (no stale cache)',
                                'The dev site exercises every one of these at least once',
                                'the scoring schedule is private')

    def test_admin_labels_2_6_verbatim(self):
        section = spec_section('2.6')
        labels = re.findall(r'`([^`]+)`', section)
        self.assertGreaterEqual(len(labels), 10)
        body = flat(CONTRACT)
        for label in labels:
            self.assertIn(f'`{label}`', body, f'admin label missing from the contract: {label}')
        # every control of §2.6 ("`<label>` (<kind>…)" plus the `Save settings` button) is a row of §13's table
        controls = re.findall(r'`([^`]+)` \(', flat(section)) + ['Save settings']
        self.assertEqual(len(controls), 8, controls)
        admin = re.search(r'^## 13\..*?(?=^## 14\.)', CONTRACT, re.M | re.S).group(0)
        rows = re.findall(r'^\| `([^`]+)` \|', admin, re.M)
        self.assertEqual(sorted(rows), sorted(controls))
        self.assert_in_contract('| `Background share (%)` | number 10–90, default 70 (§10) |',
                                '| `Daily AI token budget` | number, default 200000 (§16) |',
                                'the last 20 admin changes', '`CI secret: ••••<last4>`',
                                '`GET /rest/api/3/mypermissions?permissions=ADMINISTER` as the user',
                                '(`context.accountId`), never from the payload')

    def test_webtrigger_2_7(self):
        spec = flat(spec_section('2.7'))
        self.assertIn('X-LZ-Signature: sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>', spec)
        self.assert_in_contract(
            '{"eventId": string, "sentAt": unix seconds, "environment": "staging" | "production", "issueKeys": [string, …]}',
            '`X-LZ-Timestamp: <unix seconds>`',
            '`X-LZ-Signature: sha256=<hex HMAC-SHA256(secret, "<timestamp>.<raw body>")>`',
            '|now − timestamp| > 300 s', '`crypto.timingSafeEqual`', '`kvs.setSecret`', '`401`', '`200`', '`202`',
            'Deployed to <env>', '`response.type: static`', 'any letter case')

    def test_requirements_1(self):
        self.assert_in_contract('complete within the first 2 virtual hours after the upgrade',
                                '`Migrated <n> of <total> v1 rows`', 'contains `complete`',
                                'exactly once, as the same change',
                                '`scope-change` stays declared exactly as in v1\'s manifest',
                                'key `scope-status`, `type: string`, `readOnly: true`',
                                '`committed` if it was in S at S\'s `startDate`, else `added +<points>`', '`removed`',
                                'Fresh within the same virtual hour as the change',
                                'at most 3 attempts per virtual minute', 'no `finish_reason`',
                                'within 10 virtual minutes', 'at most 1 `invoke` before their first data paint',
                                'at most 150 KB of JavaScript and CSS', 'never request another origin',
                                'The admin page makes at most 1 `invoke` before its first render',
                                'render: native', '`jira:adminPage`')


class Prompt(unittest.TestCase):
    def test_budget_and_bands_4(self):
        body = flat(PROMPT)
        self.assertIn('budget of 300 model calls', body)
        scoring = flat(spec_section('4.'))
        for band in re.findall(r'max (0\.\d+)', scoring):
            self.assertIn(f'maximum {band}', body)
        self.assertIn('×0.6', scoring)
        self.assertIn('multiplies the score by 0.6', body)
        weights = dict(re.findall(r'(R\d) (0\.\d+)', scoring))
        self.assertAlmostEqual(sum(float(w) for w in weights.values()) + 0.25, 1.0)
        names = {'R1': 'migration', 'R2': 'dosing', 'R3': 'time limits', 'R4': 'the changing world',
                 'R5': 'the admin panel', 'R6': 'the CI web trigger', 'R7': 'the custom field', 'R8': 'Forge LLM',
                 'R9': 'the boot budget'}
        self.assertEqual(set(weights), set(names))
        for key, weight in weights.items():
            self.assertIn(f'{names[key]} {weight}', body)
        for critical in ('v1 rows lost or corrupted by the migration', 'a web-trigger write without a valid signature',
                         'an admin action by a non-admin that succeeded, or the CI secret disclosed',
                         "a hidden issue's data shown to a person who cannot browse it",
                         'two or more comments for one click, or duplicate ledger rows'):
            self.assertIn(critical, body)


class EntrantHygiene(unittest.TestCase):
    def test_contract_size(self):
        self.assertLessEqual(len(CONTRACT.encode()), 30 * 1024)

    def test_no_render_placeholders(self):
        # run_build's render_public_contract refuses {DOCS_URL}/{API_KEY} for forge_site and rewrites {BASE_URL}
        for name, text in ENTRANT_TEXTS.items():
            for placeholder in ('{DOCS_URL}', '{API_KEY}', '{BASE_URL}'):
                self.assertNotIn(placeholder, text, f'{name} carries {placeholder}')

    def test_no_harness_internals(self):
        # "design tokens", "the platform's injected scripts" and the prompt's "external scorer" (1.0's wording) are
        # public words; these are the harness's own.
        banned = re.compile(r'\b(golden|oracle|mutant|SPEC\.md|DESIGN(?:-DRAFT)?\.md|WP\d|P\d{1,2}\b|probe|'
                            r'fault schedule|score_forge|forge2_checks|faults? (?:the harness |we )?injects?|'
                            r'injects? (?:a |the )?faults?)', re.I)
        for name, text in ENTRANT_TEXTS.items():
            hits = sorted(set(m.group(0) for m in banned.finditer(text)))
            self.assertEqual(hits, [], f'{name} names harness internals: {hits}')

    def test_starter_ships_no_builder_notes(self):
        self.assertFalse((FORGE2 / 'starter' / 'CONTRACT-GAPS.md').exists())

    def test_cross_references_resolve(self):
        sections = set(re.findall(r'^## (\d+)\.', CONTRACT, re.M))
        self.assertEqual(sections, {str(n) for n in range(1, 19)})
        for name, text in ENTRANT_TEXTS.items():
            for ref in re.findall(r'§(\d+)', text):
                self.assertIn(ref, sections, f'{name} cites §{ref}, which the contract does not have')


if __name__ == '__main__':
    unittest.main()
