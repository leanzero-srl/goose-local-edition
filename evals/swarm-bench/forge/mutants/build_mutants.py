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

Usage: python3 forge/mutants/build_mutants.py        (writes the 18 patches + expectations)
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

MUTANTS = {
    'm_storage_api': {
        'edits': [
            ('src/config.js', "import { kvs } from '@forge/kvs';", "import { storage } from '@forge/api';"),
            ('src/config.js', 'export const loadConfig = () => kvs.get(CONFIG_KEY);', 'export const loadConfig = () => storage.get(CONFIG_KEY);'),
            ('src/config.js', '  await kvs.set(CONFIG_KEY, next);', '  await storage.set(CONFIG_KEY, next);'),
        ],
        'expect': {'loses': ['l_lint_warnings', 'k_current_apis'] + T_ROWS + R_ROWS + S_ROWS, 'critical': True, 'max_final': LEDGER_CAP},
        'why': 'Config load/save on the removed @forge/api storage export (storage:app is declared, so lint only '
               'WARNS deprecated-api-storage). In 8.2.0 `storage` is undefined: trigger, consumer and scheduled run '
               'throw on their first config read, so no row and no membership is ever written (T/R/S rows; '
               'r_backfill_complete is critical). No lint cap; no working ledger caps at 0.699. Downstream U/B/A '
               'rows that read an empty ledger are expected to be ROOT_BLOCKS-attributed to r_backfill_complete.',
    },
    'm_runtime18': {
        'edits': [('manifest.yml', '    name: nodejs22.x', '    name: nodejs18.x')],
        'expect': {'loses': ['l_deployable'], 'critical': False, 'max_final': LINT_CAP},
        'why': 'nodejs18.x is outside the manifest runtime enum: a lint ERROR (measured, RESEARCH §1), so the lint '
               'band caps the run at 0.499.',
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
        'expect': {'loses': ['r_backfill_complete', 'u_widget_numbers', 'a_action_result'], 'critical': True, 'max_final': LEDGER_CAP},
        'why': '/search/jql without `fields` returns ids only: the backfill still finds every changelog (bulkfetch '
               'by id) but sees no current sprint and no estimate, so membership and every committed/added/removed '
               'number after the backfill is wrong ("backfill rows/numbers").',
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
            ('src/sync.js', '  for (const row of rows) if (await recordChange(row)) written += 1;', '  for (const row of rows) if (await recordChange(row, false, `${eventId}:${row.sprintId}`)) written += 1;'),
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
        'expect': {'loses': ['t_reestimate_followed', 'u_widget_numbers', 'a_action_result'], 'critical': False},
        'why': 'One estimation field (the first board\'s) for every sprint: sprints of boards that estimate with '
               'another field read the wrong value, and re-estimates on that field never move the numbers. No band '
               'names wrong numbers on some boards, so no max_final is asserted.',
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
        'expect': {'loses': ['t_retry_after_honoured', 'r_rate_limit'], 'critical': False, 'max_final': DEFECT_CAP},
        'why': 'A 429 is retried at once (up to four attempts) instead of after Retry-After, in every path. Rate-limit '
               'defect band 0.899.',
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
            ('src/index.js', "import { RateLimited } from './jira';", "import { kvs } from '@forge/kvs';\nimport { RateLimited } from './jira';"),
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
        'expect': {'loses': ['l_deployable', 'k_rovo_skill'], 'critical': False, 'max_final': LINT_CAP},
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
            ('src/index.js', "import { RateLimited } from './jira';", "import { kvs } from '@forge/kvs';\nimport { RateLimited } from './jira';"),
            ('src/index.js', '    const boardId = payload?.boardId ?? context?.extension?.config?.boardId;', "    const boardId = await kvs.get('widget-board');"),
            ('static/widget-edit/src/index.jsx', '    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));',
             "    widgetEdit.updateConfig({ ...stored, boardId: board.id }).catch((e) => setError(e.message));\n    call('saveWidgetBoard', { boardId: board.id }).catch((e) => setError(e.message));"),
            ('static/widget/src/index.jsx', "      const data = await call('widget', { boardId: String(boardId) });", "      const data = await call('widget', {});"),
        ],
        'expect': {'loses': ['u_widget_edit_config', 'k_widget_edit_bridge'], 'critical': False, 'max_final': SURFACE_CAP},
        'why': 'The G3 control shape: edit still uses the edit API, but also stores the board in ONE app-storage key '
               'through a resolver, and the view reads that key instead of extension.config, so two widgets on one '
               'dashboard show the same (last chosen) board.',
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
