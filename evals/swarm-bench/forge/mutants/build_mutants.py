#!/usr/bin/env python3
"""Regenerate forge/mutants/<id>.patch + <id>.expect.json from bench/golden-forge (WP3, DESIGN §13.5, I4).

Each mutant is ONE defect expressed as exact string edits on the golden's sources; every edit asserts its
anchor exists exactly once, so a golden change that moves an anchor fails loudly here instead of producing a
patch that silently no longer applies. Patches are `git diff` output relative to the golden root
(forge_controls.py runs `git apply` inside a copy of bench/golden-forge, then `npm run build`), so they
touch sources only, never static/*/build.

The expectations come from DESIGN §13.5's table (and the public score bands for max_final), never from
scorer output. Where §13.5 names a class ("the runtime T/R/S rows") the ids are taken from §14's map.
`why` records the reasoning; forge_controls.py reads only loses / critical / max_final.

Usage: python3 forge/mutants/build_mutants.py [ids...]   (writes the 25 patches + expectations)
"""
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOLDEN = HERE.parent.parent / 'bench' / 'golden-forge'
IGNORE = shutil.ignore_patterns('node_modules', 'build', '.forge-dev', 'forge-shots')

# Score bands from forge/public/spec-build-forge.md "Score bands".
LINT_CAP = 0.499
LEDGER_CAP = 0.699
SURFACE_CAP = 0.799
DEFECT_CAP = 0.899

T_ROWS = ['t_trigger_handoff', 't_event_rows', 't_no_double_count', 't_out_of_order', 't_multi_sprint_parse',
          't_reestimate_followed', 't_retry_after_honoured']
R_ROWS = ['r_backfill_complete', 'r_removals_found', 'r_heal_dropped', 'r_pagination', 'r_rate_limit']
S_ROWS = ['s_entity_index_used', 's_index_order']

TAIL = '## Reading the result\n\nThe action returns a JSON object:\n\n- `sprintName`: the sprint\'s name; use it when you answer.\n- `committed`: story points of the issues that were in the sprint when it started.\n- `added`: story points of issues in the sprint now that were not in it at the start.\n- `removed`: story points of issues that were in the sprint at some point after the start and are not\n  in it now.\n- `creepPercent`: `100 × added / committed`, already rounded to one decimal. It is `null` when nothing\n  was committed; then say creep cannot be computed because the sprint started with no committed points.\n- `hiddenChanges`: how many changes exist on issues the person cannot browse. These are counted in the\n  totals but not listed. Mention the number when it is not 0, and never speculate about those issues.\n- `changes`: the changes the person can see, oldest first. Each has `issueKey`, `kind` (`added` or\n  `removed`), `points` (the issue\'s current estimate), `by` (who made the change) and `at` (an ISO-8601\n  UTC time).\n\nPoints use the board\'s estimation field; an issue with no estimate counts as 0. The totals are the same\nfor everyone; only the list of changes depends on what the person can browse.\n\n## Answering\n\n1. Lead with the sprint name and the creep, for example "Sprint 41 grew by 23.5% since it started".\n2. Give committed, added and removed points exactly as returned.\n3. List the relevant changes with issue key, kind, points, who made the change and when. For long lists,\n   summarise by person or by kind and offer the full list.\n4. If `hiddenChanges` is above 0, say that many changes are on issues the person cannot see.\n\n## Errors\n\nIf the result has an `error` field instead of numbers, tell the person what it says in plain words. For\na missing or unknown sprint, ask them to check the sprint id. For a rate-limit message, suggest trying\nagain after the stated number of seconds. Do not retry more than once in the same answer.\n'

MUTANTS = {
    'm_storage_api': {
        'edits': [
            ('src/config.js', "import { kvs } from '@forge/kvs';", "import { storage } from '@forge/api';"),
            ('src/config.js', 'export const loadConfig = () => kvs.get(CONFIG_KEY);', 'export const loadConfig = () => storage.get(CONFIG_KEY);'),
            ('src/config.js', '  await kvs.set(CONFIG_KEY, next);', '  await storage.set(CONFIG_KEY, next);'),
        ],
        'expect': {'loses': ['l_lint_warnings', 'k_current_apis'] + T_ROWS + [r for r in R_ROWS if r != 'r_pagination'] + S_ROWS, 'critical': True, 'max_final': LEDGER_CAP},
        'why': 'Config load/save on the removed @forge/api storage export (storage:app is declared, so lint only '
               'WARNS deprecated-api-storage). In 8.2.0 `storage` is undefined: trigger, consumer and scheduled run '
               'throw on their first config read, so no row and no membership is ever written (T/R/S rows; '
               'r_backfill_complete is critical). No lint cap; no working ledger caps at 0.699. Downstream U/B/A '
               'rows that read an empty ledger are expected to be ROOT_BLOCKS-attributed to r_backfill_complete. '
               'r_pagination is NOT lost: measured on forge-dev the failing scheduled run makes zero Jira reads (it '
               'throws on the config read), and WP2 observed every paginated read that does happen walked (9/9).',
    },
    'm_runtime18': {
        'edits': [('manifest.yml', '    name: nodejs22.x', '    name: nodejs18.x')],
        'expect': {'loses': ['l_deployable', 'k_current_apis'], 'critical': True, 'max_final': LINT_CAP},
        'why': 'nodejs18.x is outside the manifest runtime enum: a lint ERROR (measured, RESEARCH §1), so the lint '
               'band caps the run at 0.499; l_deployable is critical (as in m_skill_name) and nodejs18.x is not a current '
               'runtime, so k_current_apis goes too.',
    },
    'm_old_search': {
        'edits': [(
            'src/sync.js',
            """  const issues = [];
  let nextPageToken;
  do {
    const body = { jql, fields: issueFields(cfg), maxResults: 100 };
    if (nextPageToken) body.nextPageToken = nextPageToken;
    const page = await jiraJson('app', route`/rest/api/3/search/jql`, postJson(body), policy);
    issues.push(...(page.issues ?? []));
    nextPageToken = page.isLast === true ? undefined : page.nextPageToken ?? undefined;
  } while (nextPageToken);
  return issues;""",
            """  const issues = [];
  for (let startAt = 0; ; ) {
    const page = await jiraJson('app', route`/rest/api/3/search?jql=${jql}&fields=${issueFields(cfg).join(',')}&startAt=${startAt}&maxResults=100`, undefined, policy);
    issues.push(...(page.issues ?? []));
    startAt += page.issues?.length ?? 0;
    if (!page.issues?.length || startAt >= page.total) return issues;
  }""",
        )],
        'expect': {'loses': ['r_backfill_complete', 'r_pagination', 'k_current_apis'], 'critical': True, 'max_final': LEDGER_CAP},
        'why': 'The removed /rest/api/3/search with startAt/total: the site answers 410, the scheduled run throws, '
               'the backfill never lands (critical). Backfill rows wrong caps at 0.699.',
    },
    'm_ids_only': {
        'edits': [('src/sync.js', '    const body = { jql, fields: issueFields(cfg), maxResults: 100 };', '    const body = { jql, maxResults: 100 };')],
        'expect': {'loses': ['u_widget_numbers', 'a_action_result'], 'critical': False, 'max_final': SURFACE_CAP},
        'why': '/search/jql without `fields` returns ids only: the backfill still finds every changelog (bulkfetch '
               'by id) but sees no current sprint and no estimate, so membership and every committed/added/removed '
               'number after the backfill is wrong ("backfill rows/numbers"). The ROWS are complete (WP3 bed: ledger == '
               'oracle; WP2: 57/57 changelogs), so r_backfill_complete holds and the run lands at 0.799, not 0.699.',
    },
    'm_open_sprints_only': {
        'edits': [
            ('src/sync.js', '  const since = Math.min(...sprints.map((s) => s.startMs)) - 24 * 3600 * 1000;\n', ''),
            ('src/sync.js', '  const jql = `sprint in (${sprints.map((s) => s.id).join(\', \')}) OR updated >= "${jqlDate(since)}"`;', "  const jql = 'sprint in openSprints()';"),
        ],
        'expect': {'loses': ['r_removals_found', 'r_backfill_complete'], 'critical': True, 'max_final': LEDGER_CAP},
        'why': 'Reconcile reads only issues in an open sprint now, so changes of issues that have left every active '
               'sprint are never backfilled (removals missing; r_backfill_complete critical at 0.8).',
    },
    'm_dedupe_event_id': {
        'edits': [
            ('src/index.js', '    const result = await applyIssueEvent(cfg, event.body, policy);', '    const result = await applyIssueEvent(cfg, { ...event.body, eventId: event.eventId }, policy);'),
            ('src/sync.js', 'export async function applyIssueEvent(cfg, { issueId, changelogId, sprintChange }, policy) {', 'export async function applyIssueEvent(cfg, { issueId, changelogId, sprintChange, eventId }, policy) {'),
            ('src/sync.js', '    if (await recordChange(row)) {', '    if (await recordChange(row, false, `${eventId}:${row.sprintId}`)) {'),
            ('src/ledger.js', 'export async function recordChange(row, known = false) {\n  const key = changeKey(row.changeId, row.sprintId);', 'export async function recordChange(row, known = false, key = changeKey(row.changeId, row.sprintId)) {'),
        ],
        'expect': {'loses': ['t_no_double_count'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'Event rows are deduplicated on the queue eventId, which is new for every delivery of a duplicated '
               'product event: the same change lands twice (critical). Duplicate defect band 0.899.',
    },
    'm_one_estimate_field': {
        'edits': [(
            'src/config.js',
            """    if (!estimateFields.has(String(boardId))) estimateFields.set(String(boardId), await boardEstimateField(boardId, policy));
    sprints[id] = sprintEntry(sprint, boardId, estimateFields.get(String(boardId)));""",
            """    if (!estimateFields.size) estimateFields.set('all', await boardEstimateField(boardId, policy));
    sprints[id] = sprintEntry(sprint, boardId, estimateFields.get('all'));""",
        )],
        'expect': {'loses': ['t_reestimate_followed', 'u_widget_numbers', 'a_action_result', 't_trigger_handoff'], 'critical': False},
        'why': 'One estimation field (the first board\'s) for every sprint: sprints of boards that estimate with '
               'another field read the wrong value, and re-estimates on that field never move the numbers. The trigger '
               'recognises estimate changes by the configured fields, so it also drops those updates instead of '
               'handing them to the queue (t_trigger_handoff; WP2 measured 38/44). No band names wrong numbers on '
               'some boards, so no max_final is asserted.',
    },
    'm_retry_now': {
        'edits': [(
            'src/jira.js',
            """  for (let attempt = 0; ; attempt += 1) {
    const res = await client.requestJira(path, init);
    if (res.status !== 429) return res;
    const wait = retryAfterSeconds(res, attempt);
    if (wait > policy.maxWaitSeconds) throw new RateLimited(wait);
    // Strictly more than the header asks for, so "at least that long" holds on any clock granularity.
    await sleep(wait * 1000 + 50);
  }""",
            """  for (let attempt = 0; ; attempt += 1) {
    const res = await client.requestJira(path, init);
    if (res.status !== 429 || attempt === 3) return res;
  }""",
        )],
        'expect': {'loses': ['t_retry_after_honoured', 'r_rate_limit', 'r_backfill_complete', 'r_removals_found'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'A 429 is retried at once (up to four attempts) instead of after Retry-After, in every path. The four '
               'attempts fall inside the Retry-After window, so the scheduled run gives up on the 429 and the backfill '
               'is incomplete (r_backfill_complete critical, removals missing). Rate-limit defect band 0.899.',
    },
    'm_asapp_ui': {
        'edits': [('src/ledger.js', "    const page = await jiraJson('user', route`/rest/api/3/issue/bulkfetch`,", "    const page = await jiraJson('app', route`/rest/api/3/issue/bulkfetch`,")],
        'expect': {'loses': ['b_no_permission_leak', 'b_hidden_count', 'a_action_permissions'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'The sprint-action resolver and the Rovo action decide visibility as the app: every change is listed '
               'to everyone (critical leak) and the hidden count is 0 for people who cannot browse some issues. '
               'Permission defect band 0.899.',
    },
    'm_double_post': {
        'edits': [('static/sprint/src/index.jsx', '    if (inFlight.current) return;\n', '')],
        'expect': {'loses': ['b_comment_exactly_once'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'No in-flight guard on post-summary: a double click posts two comments (critical).',
    },
    'm_string_comment': {
        'edits': [(
            'src/views.js',
            "postJson({ body: summaryDoc(issueKey, sprint.name, view.text.creep, view.text) })",
            "postJson({ body: `Scope Ledger: ${issueKey} in sprint ${sprint.name}, scope creep ${view.text.creep}` })",
        )],
        'expect': {'loses': ['b_comment_adf_as_user', 'u_comment_flow', 'b_comment_exactly_once'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'Plain-string comment body: Jira refuses it ("Operation value must be an Atlassian Document"), the '
               'click ends with an error flag and NO comment, so exactly-once (one comment per click) also fails '
               '(critical). §13.5 names the first two rows; the third follows from the same click.',
    },
    'm_gadget': {
        'edits': [('manifest.yml', '  dashboards:widget:\n', '  jira:dashboardGadget:\n')],
        'expect': {'loses': ['k_dashboard_widget', 'k_widget_edit_bridge', 'u_widget_loads', 'u_widget_numbers', 'u_widget_chart',
                             'u_widget_edit_config', 'v_widget_sizes'], 'critical': True, 'max_final': SURFACE_CAP},
        'why': 'The deprecated jira:dashboardGadget instead of dashboards:widget: no dashboards widget exists, so the '
               'widget surface and its edit API earn nothing (u_widget_loads critical); missing current surface '
               'caps at 0.799.',
    },
    'm_config_resolver': {
        'edits': [
            ('src/index.js', "resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));",
             """resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));

const widgetKey = (context) => `widget-board:${context?.extension?.context?.widgetId ?? 'default'}`;
resolver.define('saveWidgetBoard', async ({ payload, context }) => {
  await kvs.set(widgetKey(context), String(payload.boardId));
  return { ok: true };
});
resolver.define('widgetBoard', async ({ context }) => ({ boardId: (await kvs.get(widgetKey(context))) ?? null }));"""),
            ('src/index.js', "import { JiraError, RateLimited } from './jira';", "import { kvs } from '@forge/kvs';\nimport { JiraError, RateLimited } from './jira';"),
            ('src/index.js', '    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;', '    const boardId = await kvs.get(widgetKey(context));'),
            ('static/widget-edit/src/index.jsx', """    widgetEdit
      .onProductSave(async (config) => (selectedRef.current ? { ...(config ?? {}), boardId: selectedRef.current } : null))
      .catch((e) => setError(`The dashboard did not accept the save handler: ${e.message}`));
""", """    call('widgetBoard').then((res) => {
      selectedRef.current = res.boardId;
      setSelected(res.boardId);
    });
"""),
            ('static/widget-edit/src/index.jsx', '    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));', "    call('saveWidgetBoard', { boardId: board.id }).catch((e) => setError(e.message));"),
            ('static/widget-edit/src/index.jsx', "import { widgetEdit } from '@forge/dashboards-bridge';\n", ''),
            ('static/widget/src/index.jsx', "  const boardId = context?.extension?.config?.boardId;", "  const [boardId, setBoardId] = useState(undefined);\n  useEffect(() => {\n    call('widgetBoard').then((res) => setBoardId(res.boardId ?? ''));\n  }, [context]);"),
            ('static/widget/src/index.jsx', "    if (boardId === undefined || boardId === null || boardId === '') return setState({ phase: 'needs-config' });",
             "    if (boardId === undefined) return undefined;\n    if (boardId === null || boardId === '') return setState({ phase: 'needs-config' });"),
        ],
        'expect': {'loses': ['k_widget_edit_bridge', 'u_widget_edit_config'], 'critical': False, 'max_final': SURFACE_CAP},
        'why': 'The edit surface saves the board through a resolver into app storage (per widget id) and never '
               'calls the dashboards edit API, so the dashboard\'s own Save stores nothing and reopening edit / the '
               'stored config path are broken. Missing edit API caps at 0.799.',
    },
    'm_no_theme': {
        'edits': [
            ('static/shared/bridge.js', '  await view.theme.enable();\n', ''),
            ('static/shared/base.css', """  background-color: var(--ds-surface);
  color: var(--ds-text);
  font-family""", """  background-color: #ffffff;
  color: #172b4d;
  font-family"""),
        ],
        'expect': {'loses': ['v_theme_tokens', 'v_dark_mode'], 'critical': False, 'max_final': SURFACE_CAP},
        'why': 'No view.theme.enable() and a hard-coded white page: no design tokens arrive and dark mode stays '
               'white. A surface broken in dark caps at 0.799.',
    },
    'm_abs_assets': {
        'edits': [
            ('static/widget/index.html', '<link rel="stylesheet" href="./index.css" />', '<link rel="stylesheet" href="/index.css" />'),
            ('static/widget/index.html', '<script src="./index.js"></script>', '<script src="/index.js"></script>'),
        ],
        'expect': {'loses': ['u_widget_loads', 'v_csp_clean'], 'critical': True, 'max_final': SURFACE_CAP},
        'why': 'The widget view references its assets absolutely; Forge serves the resource under its own path, so '
               'the script and stylesheet 404 and the widget never renders (critical). Other widget rows are '
               'expected to be ROOT_BLOCKS-attributed to u_widget_loads. A broken surface caps at 0.799.',
    },
    'm_skill_name': {
        'edits': [('skills/sprint-scope-analyst/SKILL.md', 'name: sprint-scope-analyst\n', 'name: scope-analyst\n')],
        'expect': {'loses': ['l_deployable', 'k_rovo_skill'], 'critical': True, 'max_final': LINT_CAP},
        'why': 'SKILL.md name differs from its directory: the Rovo skill is invalid. DESIGN §13.5 predicts k_rovo_skill '
               'and the 0.799 surface cap, but MEASURED 2026-10-02 the client-side lint (@forge/lint via the spike '
               'harness) reports an ERROR "Skill sprint-scope-analyst frontmatter field \'name\' must match the '
               'parent directory name", so the lint band applies: l_deployable lost, capped at 0.499. Reported in '
               'bench/golden-forge/CONTRACT-GAPS.md for the design table.',
    },
    'm_config_in_kvs': {
        'edits': [
            ('src/index.js', "resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));",
             """resolver.define('boards', rateLimitedAware(async () => ({ boards: await boardsView(UI_POLICY) })));

resolver.define('saveWidgetBoard', async ({ payload }) => {
  await kvs.set('widget-board', String(payload.boardId));
  return { ok: true };
});"""),
            ('src/index.js', "import { JiraError, RateLimited } from './jira';", "import { kvs } from '@forge/kvs';\nimport { JiraError, RateLimited } from './jira';"),
            ('src/index.js', '    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;', "    const boardId = await kvs.get('widget-board');"),
            ('static/widget-edit/src/index.jsx', '    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));',
             "    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));\n    call('saveWidgetBoard', { boardId: board.id }).catch((e) => setError(e.message));"),
            ('static/widget/src/index.jsx', "      const data = await call('widget', { boardId: String(boardId) });", "      const data = await call('widget', {});"),
        ],
        'expect': {'loses': ['u_widget_edit_config'], 'critical': False, 'max_final': SURFACE_CAP},
        'why': 'The G3 control shape: edit still uses the edit API, but also stores the board in ONE app-storage key '
               'through a resolver, and the view reads that key instead of extension.config, so two widgets on one '
               'dashboard show the same (last chosen) board. The edit API is still used, so k_widget_edit_bridge holds '
               '(the §13.5 row lists it; that row should drop it).',
    },
    'm_throw_on_429': {
        'edits': [(
            'src/index.js',
            """  } catch (e) {
    if (e instanceof RateLimited) {
      // retryData is not optional in practice: for a function with timeoutSeconds > 55 the Forge runtime
      // wrapper measures Buffer.byteLength(JSON.stringify(retryData)), which throws on undefined and turns
      // the retry request into a function error (measured on the real wrapper, 2026-10-02).
      return new InvocationError({
        retryAfter: Math.min(Math.max(e.retryAfterSeconds, 1), MAX_RETRY_AFTER),
        retryReason: InvocationErrorCode.FUNCTION_UPSTREAM_RATE_LIMITED,
        retryData: { retryAfterSeconds: e.retryAfterSeconds },
      });
    }
    throw e;
  }""",
            """  } catch (e) {
    throw e;
  }""",
        )],
        'expect': {'loses': ['t_retry_after_honoured'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The consumer throws on a 429 instead of returning a retry request: the platform redelivers on its own '
               'schedule (1, 2, 4, 8 min), which can come back before Retry-After; the rows still land. Rate-limit '
               'defect band 0.899.',
    },
    'm_llm_trusts_numbers': {
        'edits': [('src/explain.js', "  const summary = /\\d/.test(args.summary) || !args.summary.trim() ? ledgerSentence(view) : args.summary.trim();",
                   "  const summary = args.summary.trim();")],
        'expect': {'loses': ['u_llm_explain'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The model\'s summary is shown verbatim even when it holds digits, so a model-invented number reaches '
               'the coach next to the ledger\'s. LLM defect band 0.899.',
    },
    'm_llm_leaks_hidden': {
        'edits': [
            ('src/index.js', '    return explainSprint(await personView(sprint, UI_POLICY));', "    return explainSprint(await personView(sprint, UI_POLICY, 'app'));"),
            ('src/views.js', 'export async function personView(sprint, policy) {', "export async function personView(sprint, policy, who = 'user') {"),
            ('src/views.js', '  const visible = changes.length ? await visibleIssues(changes.map((c) => c.issueId), policy) : new Map();',
             '  const visible = changes.length ? await visibleIssues(changes.map((c) => c.issueId), policy, who) : new Map();'),
            ('src/ledger.js', 'export async function visibleIssues(issueIds, policy) {', "export async function visibleIssues(issueIds, policy, who = 'user') {"),
            ('src/ledger.js', "    const page = await jiraJson('user', route`/rest/api/3/issue/bulkfetch`,", "    const page = await jiraJson(who, route`/rest/api/3/issue/bulkfetch`,"),
            ('src/explain.js', '  const ids = [...new Set(args.changeIds)].filter((id) => visible.has(id));', '  const ids = [...new Set(args.changeIds)];'),
            ('src/explain.js', '    changes: ids.map((id) => ({ changeId: id, issueKey: visible.get(id).issueKey, kind: visible.get(id).kind })),',
             "    changes: ids.map((id) => ({ changeId: id, issueKey: visible.get(id)?.issueKey ?? id, kind: visible.get(id)?.kind ?? 'added' })),"),
        ],
        'expect': {'loses': ['b_no_permission_leak', 'u_llm_explain'], 'critical': True, 'max_final': DEFECT_CAP},
        'why': 'The explain prompt is built from every ledger row read as the app, and the ids the model returns are '
               'rendered unfiltered: changes to issues the viewer cannot browse reach the model and the screen '
               '(critical leak).',
    },
    'm_llm_unknown_model': {
        'edits': [(
            'src/explain.js',
            """  const { models } = await list();
  const active = (models ?? []).filter((m) => m.status === 'active').map((m) => m.model);
  return active.find((m) => /sonnet/i.test(m)) ?? active[0] ?? null;""",
            """  return 'claude-2.1';""",
        )],
        'expect': {'loses': ['u_llm_explain', 'k_llm_model_current'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'A hard-coded model id that list() does not return: every explain call errors (error flag, no '
               'explanation). Replaces m_llm_deprecated_model — the public models page lists no deprecated model. '
               'LLM defect band 0.899.',
    },
    'm_llm_no_refusal_path': {
        'edits': [(
            'src/explain.js',
            """  if (!call) return { ok: false, error: 'The model declined to explain this sprint.' };
  const args = call.function.arguments;""",
            """  const text = (response?.choices ?? []).flatMap((c) => c.message?.content ?? []).map((p) => (typeof p === 'string' ? p : p.text ?? '')).join(' ');
  const args = call ? call.function.arguments : { summary: text, changeIds: [] };""",
        )],
        'expect': {'loses': ['u_llm_explain'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The explain flow has no refusal path: when the model answers without calling report_scope, its raw '
               'text is rendered as the explanation (ok, no error flag) instead of an error flag. Survives the '
               'golden\'s never-throw resolver wrapper (71926c50f), which made the earlier throwing variant '
               'indistinguishable; b_invoke_contract therefore no longer drops (the §13.5 row should drop it). '
               'LLM defect band 0.899.',
    },
    'm_rt_publish_in_consumer': {
        'edits': [
            ('src/realtime.js', "import { publishGlobal } from '@forge/realtime';", "import { publish } from '@forge/realtime';"),
            ('src/realtime.js', '    const res = await publishGlobal(CHANNEL, { sprintIds: ids });', '    const res = await publish(CHANNEL, { sprintIds: ids });'),
        ],
        'expect': {'loses': ['u_widget_live'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The consumer (and the scheduled run) announce with publish(), which the realtime docs say is not '
               'available for async events: nothing reaches the widget\'s global subscription, so an open widget never '
               'updates. Realtime defect band 0.899.',
    },
    'm_rt_payload_leak': {
        'edits': [
            ('src/sync.js', '  return { rows: written, members, sprintIds: [...changed] };', '  return { rows: written, members, sprintIds: [...changed], changedRows: rows };'),
            ('src/index.js', '    await announce(result.sprintIds);\n    return result;', '    await announce(result.sprintIds, result.changedRows);\n    return result;'),
            ('src/realtime.js', 'export async function announce(sprintIds) {', 'export async function announce(sprintIds, rows = []) {'),
            ('src/realtime.js', '    const res = await publishGlobal(CHANNEL, { sprintIds: ids });',
             '    const changes = rows.map((r) => ({ changeId: r.changeId, issueKey: r.issueKey, kind: r.kind, by: r.authorName }));\n    const res = await publishGlobal(CHANNEL, { sprintIds: ids, changes });'),
        ],
        'expect': {'loses': ['b_realtime_payload_clean'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The realtime payload carries the changed rows (issue keys, authors) to every subscriber of a global '
               'channel, regardless of what each viewer may browse. The widget still updates. Realtime defect band 0.899.',
    },
    'm_rt_poll_instead': {
        'edits': [
            ('static/widget/src/index.jsx', '      if (data.realtime && !subscription.current) subscribe(data.realtime);\n', ''),
            ('static/widget/src/index.jsx', '  useEffect(() => {\n    load();\n  }, [load]);\n',
             '  useEffect(() => {\n    load();\n    const timer = setInterval(() => reload.current(), 5000);\n    return () => clearInterval(timer);\n  }, [load]);\n'),
        ],
        'expect': {'loses': ['u_widget_live'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The widget polls its resolver every five seconds instead of subscribing to Forge Realtime (the contract '
               'says no polling). Realtime defect band 0.899.',
    },
    # ── 2026-10-03 stringency (DESIGN §17.7): one mutant per newly measured clause ─────────────────────────────
    'm_rerun_rewrites': {
        'edits': [('src/sync.js',
                   "      if (await writeMember(memberRow(cfg, sprintId, issue, currentIds), storedMembers.get(`${sprintId}:${issue.id}`))) {",
                   '      if (await writeMember(memberRow(cfg, sprintId, issue, currentIds), undefined)) {')],
        'expect': {'loses': ['r_idempotent_rerun'], 'critical': False},
        'why': 'Every scheduled run rewrites every membership row it computes, changed or not: the values stay right, '
               'but a run with nothing new writes (contract §3 "a run with nothing new writes nothing"). Not banded.',
    },
    'm_resolver_throws': {
        'edits': [('src/index.js',
                   """    console.error(`resolver ${req?.call?.functionKey ?? ''} failed: ${e?.message ?? e}`);
    return { ok: false, error: e instanceof JiraError ? `Jira refused the request (${e.status}).` : `The request failed: ${e?.message ?? e}` };""",
                   '    throw e;')],
        'expect': {'loses': ['b_invoke_contract'], 'critical': False},
        'why': 'Resolvers rethrow every non-429 failure: the scoring site\'s Jira 500 on the sprint ledger read (and the '
               'forbidden comment\'s 400) reaches the page as a rejected invoke instead of a value describing it '
               '(contract §2 "Every resolver returns a value and never throws"). The page still catches it.',
    },
    'm_explain_dates': {
        'edits': [('src/explain.js',
                   "    : `${view.sprint.name} grew by ${creep}: ${added} points entered after the start against ${committed} committed, and ${removed} points left it.`;",
                   "    : `${view.sprint.name} grew by ${creep}: ${added} points entered after the start (${String(view.sprint.startDate).slice(0, 10)}) against ${committed} committed, and ${removed} points left it.`;")],
        'expect': {'loses': ['u_llm_explain'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'The replacement sentence for a digits answer adds the sprint\'s start date: a number that is not one of '
               'the ledger\'s (contract §5 "your own sentence with the ledger\'s numbers"). LLM defect band 0.899.',
    },
    'm_sort_resumes_desc': {
        'edits': [('static/sprint/src/index.jsx',
                   "    if (col === 'at') setSort((s) => ({ col: 'at', dir: s.col === 'at' && s.dir === 'ascending' ? 'descending' : 'ascending' }));",
                   "    if (col === 'at') setSort((s) => ({ col: 'at', dir: s.col === 'at' ? (s.dir === 'ascending' ? 'descending' : 'ascending') : s.dir }));")],
        'expect': {'loses': ['u_ledger_sort'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'After the points sort (descending) the first `at` click keeps that direction instead of starting with '
               'ascending (contract §5 "starting with ascending when another sort was active"). Ordering defect band 0.899.',
    },
    'm_skill_bare': {
        'edits': [('skills/sprint-scope-analyst/SKILL.md', TAIL, '')],
        'expect': {'loses': ['a_skill_instructions'], 'critical': False},
        'why': 'SKILL.md tells the agent when to call get-sprint-scope and what sprintId is, but not how to read the '
               'result or what to do with an error (contract §6). Points only.',
    },
    'm_metric_clip': {
        'edits': [('static/widget/src/widget.css', '.metric-creep dd {\n',
                   '.sprint dd {\n  max-width: 2ch;\n  overflow: hidden;\n  text-overflow: ellipsis;\n  white-space: nowrap;\n}\n\n.metric-creep dd {\n')],
        'expect': {'loses': ['v_widget_sizes'], 'critical': False},
        'why': 'The widget squeezes every number into two characters with an ellipsis (contract §4 "no number is clipped '
               'or truncated"). The DOM text is intact, so only the size row sees it. Points only.',
    },
}


def git(cwd, *args):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True).stdout


def main():
    only = set(sys.argv[1:])
    with tempfile.TemporaryDirectory(prefix='golden-mutants-') as tmp:
        tree = Path(tmp) / 'golden'
        shutil.copytree(GOLDEN, tree, ignore=IGNORE)
        git(tree, 'init', '-q')
        git(tree, 'add', '-A')
        git(tree, '-c', 'user.email=wp3@local', '-c', 'user.name=wp3', 'commit', '-q', '-m', 'golden')
        for mid, spec in MUTANTS.items():
            if only and mid not in only:
                continue
            for rel, old, new in spec['edits']:
                path = tree / rel
                text = path.read_text()
                count = text.count(old)
                if count != 1:
                    raise SystemExit(f'{mid}: anchor found {count}x in {rel}: {old[:80]!r}')
                path.write_text(text.replace(old, new))
            patch = git(tree, 'diff')
            git(tree, 'checkout', '-q', '--', '.')
            (HERE / f'{mid}.patch').write_text(patch)
            (HERE / f'{mid}.expect.json').write_text(json.dumps({**spec['expect'], 'why': spec['why']}, indent=2) + '\n')
            print(f'{mid}: {len(patch.splitlines())} patch lines')


if __name__ == '__main__':
    main()
