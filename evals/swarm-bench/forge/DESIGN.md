# FORGE benchmark — design (tier `forge-1.0`)

Status: design, 2026-10-02. Nothing here is built yet except the spike (`forge/spike/`, f3b71f9a2) and the
research sheet (`forge/RESEARCH.md`, e4d6adcfd). This file settles the decisions so that three builders can work
in parallel next (§13). Public task text: `forge/public/` (spec-build-forge.md, FORGE-CONTRACT.md, STARTER.md).

Owner requirements this answers (verbatim): "a pretty tough one so we can test today's models … they need to be
cheap, economically viable benchmark or we can't run it consistently"; "without forge deploy, but you need to
build into the scorer a capable forge lint …, a good screenshot taker on what was built inside, a challenging
backend resolvers, challenging custom UI setup and the various module usage. it needs to come up with some pretty
newest module usage from preview to see how capable the model really is and knowledgeable". Standing rules:
graded by running the app, not a tutorial app, hard, single model in goose, 150-call budget, empty or partial
work is scored (never refused), hermetic, deterministic, per-run seeded fixtures, severity model from day one, no
hard-coded absolutes without receipts.

## 0. Decisions at a glance

| # | decision | why (receipt) |
|---|---|---|
| D1 | Task: **Scope Ledger** — RESEARCH §7's Sprint Scope-Creep Ledger, refined (§2) | rare domain (sprint changelogs, reconcile vs event stream), every surface gradable offline (RESEARCH §3) |
| D2 | Modules: `trigger`, `consumer`, `scheduledTrigger`, `dashboards:widget` (+`edit`), `jira:sprintAction`, `action`, `rovo:skill`, `rovo:agent`; KVS custom entity | 8 module types; `dashboards:widget` GA 2026-09-22 and `rovo:skill` Preview 2026-10-02 are the newest documented (RESEARCH §0.5, §5) |
| D3 | Dropped from RESEARCH §7: `jira:jqlFunction` | its handler contract was never captured (RESEARCH §7 item 7, "drop it if the contract is thin") |
| D4 | **No internet for the entrant**: sandbox fence (localhost only) + a provider-only CONNECT relay | measured 2026-10-02: `(deny network-outbound)(allow network-outbound (remote ip "localhost:*"))` gives curl/node `000`/`EPERM` for https://developer.atlassian.com and 200 for a 127.0.0.1 server (§4) |
| D5 | Knowledge model: test recall of HOW, never of WHAT; every graded fact is stated in the contract or discoverable offline (pinned typings, manifest schema, Jira OpenAPI, the dev site) | hermetic and repeatable; recall shows up as economy and fewer defects, not as an unfair zero (§4) |
| D6 | The entrant gets a dev kit: offline `npm run lint`, `forge-dev` (real Forge runtime + Custom UI host) against a **dev site with a different seed** | the alternative is desk-auditing, which cost GPT-6 Luna 3 h / 309 calls / $9 on SB7.2 (sb7.2/DESIGN.md "Public contract trim") |
| D7 | Score-time runtime = Atlassian's runtime wrapper fetched from its public CDN, pinned by sha256; the in-repo shim runs ONLY behind an explicit `--runtime shim` flag and the verdict is then unpublishable | licence (wrapper not committed, spike .gitignore); gate 1 forbids a silent fallback (§6.1) |
| D8 | Custom UI only. UI Kit, Forge SQL, webtrigger, apiRoute, global:fullPage, objectStore, app-managed permissions: not in v1 | @forge/react closure is 1,246 MB (measured); SQL needs a MySQL engine; the others have unmeasured handler contracts (RESEARCH §5) |
| D9 | Composition = SB's: `final = min(earned, ceilings)`, `earned = (0.88·inner + 0.12·gate·e_mean) × critical multiplier` (floor 0.6) | one severity model across families; selftest machinery reused (score_sb7.py `compose_from_rows`) |
| D10 | Public input 16,850 bytes (prompt 2,942 + contract 10,664 + starter 3,244) + BROWSER-TESTING 537 | target ≤ 25 KB; SB7.2's 45.7 KB still produced desk audits |
| D11 | Its own benchmark family on leanzero.net (`family: forge`), scorer version `forge-1.0` (`forge-1.0-rc` until thresholds freeze) | not an SB era: different product, different scale |

## 1. Requirement → where it lands

| owner requirement | where |
|---|---|
| tough, tests today's models | 8 module types, async pipeline with duplicates/reorder/loss, permissions, rate limits, newest APIs (§2); bands (§8.5) |
| cheap, economically viable | 16.9 KB public input; one model, 150 calls; local scoring, no model in the scorer; ~$0.3 (Luna) to ~$40 (Astra) per run (§11) |
| no `forge deploy` | everything runs offline through the spike's proven seam (RESEARCH §0.1, §3) |
| a capable forge lint in the scorer | Forge's own client-side linter (16 linters, RESEARCH §3) plus the scorer's blind-spot rules (§7) |
| a good screenshot taker | every surface × light/dark × widget widths, PNGs + a contact sheet, DOM is the grade, pixels only for blank/dark checks (§6.6) |
| challenging backend resolvers | resolvers, trigger, consumer, scheduled backfill, Rovo action; asApp/asUser split with a permission trap (§2.4) |
| challenging Custom UI setup | three Custom UI surfaces, the dashboards edit API, router, flags, modal, theming tokens, CSP, an SVG chart graded to 1 px (§2.3) |
| various module usage | D2 |
| newest module usage from Preview | `rovo:skill` (Preview 2026-10-02), `dashboards:widget` (GA 2026-09-22) with `@forge/dashboards-bridge` 2.0.0 (published 2026-09-28T02:45Z) (§2.2) |
| see how knowledgeable | D5 plus a report-only recall trace (§8.8) |
| functional, not benchmaxxed | graded by running; no tutorial sample; no planted starter traps (§2.6) |
| scored-not-refused | absent surfaces score 0 through their checks; empty starter scores > 0 (§8.6) |
| hermetic, seeded | per-run scoring seed; dev seed differs; fresh site per scoring; pristine node_modules (§5, §9) |
| severity from day one | 7 criticals, transforms, dedup, selftest (§8.3) |
| no absolutes without receipts | every page cap and limit from the OpenAPI/docs; economy rungs are ratios of the oracle's optimum (§8.4) |

## 2. The task — Scope Ledger

### 2.1 Product

Agile coaches want to know what entered an active sprint after it started, who added it, and how many story
points it carried — on a Jira dashboard, from the sprint's own action menu, and from Rovo. The app keeps a ledger
of sprint changes from product events, backfills and heals it with an hourly reconciliation, and reports team
totals plus per-person-visible change lists. The exact definitions are FORGE-CONTRACT.md §1.

### 2.2 Modules and status (cite: RESEARCH §1 table, §5 table)

| module | role | status | what makes it hard |
|---|---|---|---|
| `trigger` (`avi:jira:updated:issue`) | hand relevant updates to a queue | GA | filter in code (expressions are not evaluated, contract §8); Sprint changelog `from`/`to` are comma-separated id lists (observable on the dev site) |
| `consumer` (`function:` shape) | ledger writes | GA | idempotency by changelog id, not by queue event id; `InvocationError({retryAfter})` vs in-function wait; asApp only |
| `scheduledTrigger` (`interval: hour`) | backfill + heal | GA | `/rest/api/3/search/jql` with `nextPageToken` and named `fields`; removed issues are no longer in any sprint; 429 with `Retry-After`; module timeout |
| `dashboards:widget` + `edit` | dashboard view + board picker | GA 2026-09-22 (`jira:dashboardGadget` "will be deprecated by 17 May 2027", CDAC 102826) | `@forge/dashboards-bridge` edit API (`getWidgetEditApi`: `updateConfig`, `onProductSave`, `onSave`), config arrives in `extension.config` |
| `jira:sprintAction` (modal) | per-sprint ledger table + comment | GA | context `extension.sprint.{id (string), state}` and `extension.board.id` (docs page, fetched 2026-10-02); router; flags; asUser comment in ADF |
| `action` `get-sprint-scope` | Rovo action | GA | user-led; permission-filtered changes; errors returned, not thrown |
| `rovo:skill` | `skills/sprint-scope-analyst/SKILL.md` | Preview 2026-10-02 | frontmatter rules are NOT in any local package (schema only has `source.dir` and `dependencies.tools`), so the contract states them |
| `rovo:agent` | lists the skill | GA | wiring |

Storage: KVS custom entity declared under `app.storage.entities` with an index partitioned by sprint and ranged
by change time (RESEARCH §1, §2 @forge/kvs). Scopes the golden needs (from the shipped OpenAPI `security`
blocks, checked 2026-10-02): `read:jira-work`, `write:jira-work`, `storage:app`, `read:sprint:jira-software`,
`read:board-scope:jira-software`, `read:board-scope.admin:jira-software`, `read:project:jira`.

### 2.3 The Custom UI surfaces

Three surfaces, all Custom UI, all built by the entrant with the installed esbuild into `static/*/build`:
widget view (numbers, an SVG chart graded to 1 px, two widths), widget edit (board picker through the dashboards
edit API, persisted by the host's Save), sprint action modal (sortable ledger table, router links, row
selection, comment post with flags, hidden count, close, not-started state). Each must call
`view.theme.enable()` and style with `var(--ds-…)` tokens; the harness renders each in light and dark.

### 2.4 The traps that separate strong from weak (each is a check in §8)

| trap | weak behaviour | check |
|---|---|---|
| removed issues | reconcile with `sprint in openSprints()` misses issues that left every sprint | `r_removals_found`, crit `r_backfill_complete` |
| two estimation fields, ids vary per seed | hard-coded `customfield_10016` or one field for both boards | `t_reestimate_followed`, `u_widget_numbers`, `a_action_result` |
| parallel active sprints | "the active sprint" singular | `u_widget_numbers` |
| multi-id Sprint changelog values | `to` parsed as one id | `t_multi_sprint_parse` |
| trigger-level duplicates | dedupe by queue `eventId` (each redelivery has a new one) | crit `t_no_double_count` |
| out-of-order delivery | membership updated from event order | `t_out_of_order` |
| dropped events | trusting the stream | `r_heal_dropped` |
| `/rest/api/3/search` (410), ids-only `/search/jql`, `startAt` | wrong or empty pages | `r_pagination`, `k_current_apis` |
| 429 `Retry-After` 30 in the consumer, 2 in reconcile, 1 on the comment POST | immediate retry; the RESEARCH `continue`-in-do-while bug | `t_retry_after_honoured`, `r_rate_limit`, `u_comment_flow` |
| asApp for person-facing data | hidden issue keys/summaries reach the viewer | crit `b_no_permission_leak` |
| asUser in background work | `NeedsAuthenticationError` | `t_no_user_in_async`, `r_as_app` |
| double click | two comments | crit `b_comment_exactly_once` |
| plain-string comment body | 400 "Operation value must be an Atlassian Document" | `b_comment_adf_as_user` |
| `jira:dashboardGadget`, `@forge/api` `storage`, `nodejs18.x`, `@forge/ui` | stale platform knowledge | `k_*`, `l_deployable` |
| config saved through a resolver instead of the dashboards edit API | widget config never reaches the dashboard | `k_widget_edit_bridge`, `u_widget_edit_config` |
| absolute `/assets` paths, inline `<script>`, CDN fonts | blank surface under the Forge CSP | `v_csp_clean`, crit `u_widget_loads` |
| hard-coded white/dark colours | unreadable in the other mode | `v_theme_tokens`, `v_dark_mode` |
| N+1 changelog reads (237 issues) | ~480 calls where ~12 suffice (`/rest/api/3/changelog/bulkfetch` exists in the shipped OpenAPI) | `e_reconcile_economy` |

### 2.5 Why it is not benchmaxxed

No hello-world panel, no todo app, no Forge tutorial example. Sprint changelog semantics, board estimation
fields and reconcile-vs-stream are rare in training data. The newest modules are ~30% of the surface. Graded
only by running it.

### 2.6 Starter: no planted traps

RESEARCH §7 proposed starter lint-traps (bare-string egress, `nodejs18.x`, a shared consumer function). Rejected:
planted traps reward puzzle-solving over building and lengthen the starter. The starter is an honest empty app
(`forge/starter/`): `manifest.yml` with `app.id` (a fixed dummy ARI) and `runtime.name: nodejs22.x`, no modules;
`package.json` with the kit's exact pins and `"lint"` script; empty `src/index.js`, `static/`, `skills/`. The
traps live in the platform's real behaviour and in the requirements.

## 3. Contract rule — what is stated and what is not

Lesson from SB7.2's trim: a golden at 1.000 proves the scorer does not read the TEXT; it cannot prove a cut
sentence is unmeasured. Rule for Forge, applied to FORGE-CONTRACT.md and enforced by §14's map:

1. **Every behavioural requirement a check measures is stated** (the numbers, the DOM hooks, exactly-once,
   visibility, the action's JSON, SKILL.md rules).
2. **Every harness deviation from production is stated** (contract §8: resources served as committed, fresh
   process per invocation, filter expressions not evaluated, different scoring seed, harness-chosen layout).
3. **Platform documentation is never restated.** It is discoverable offline: typings and sources of the pinned
   packages, the manifest schema, the Jira OpenAPI, the linter's messages, and the dev site, which behaves like
   Jira Cloud. Fairness invariant (WP1 must prove it): **every graded platform behaviour occurs at least once on
   the dev site** — multi-id Sprint changelog values, two estimation fields, a kanban board, hidden issues, a
   comment-forbidden issue, 429 with `Retry-After` on each path, 410 on `/rest/api/3/search`, ids-only results
   without `fields`, 400 on unbounded JQL.

## 4. Knowledge model and the network decision

**Decision: no internet.** Today every isolated tier's sandbox carries `(allow network*)`
(bench_isolation.py `profile`), so an entrant's shell can reach developer.atlassian.com and npm. For Forge that
would make the newest-module checks measure who fetched today's docs, make runs unrepeatable (docs move), cost
tokens, and let entrants `npm install` 1 GB of @atlaskit. Measured 2026-10-02 on this Mac:

```
P='(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote ip "localhost:*"))(allow network-outbound (remote unix-socket))'
sandbox-exec -p "$P" curl https://developer.atlassian.com/      -> 000, exit 7
sandbox-exec -p "$P" node -e "fetch('https://registry.npmjs.org/@forge%2fapi')" -> EPERM
sandbox-exec -p "$P" curl http://127.0.0.1:18778/                -> 200
```

goose itself must still reach the provider. goose's reqwest is built with `system-proxy` (crates/goose/
Cargo.toml) and no client in crates/goose/src calls `.no_proxy()` except `acp/server/mlx_remote_single.rs`, so they
honour `HTTPS_PROXY`. run_build starts, OUTSIDE the sandbox, a 127.0.0.1 CONNECT relay whose allowlist is the
selected provider's host:443 (from the benchmark config snapshot), and passes `HTTPS_PROXY`/`HTTP_PROXY` to the
entrant. Node's fetch ignores the proxy and is fenced; curl honours it and is refused by the allowlist.
Preflight (refuses the run, gate 1 — never a silent open network): inside the sandbox an
https://developer.atlassian.com request must fail AND one provider request through the relay must succeed.
Confidence MEDIUM: the fence is measured, the relay with goose's provider stack is not (§15 R1).

**Knowledge model.** The contract states WHAT (module keys included — one `grep dashboards:` in the shipped
schema finds them anyway); HOW is discoverable offline and is where knowledge pays:

| source in the workdir | answers |
|---|---|
| `node_modules/@forge/*` typings and sources (pinned) | KVS entity queries, `Queue`, `InvocationError`, `route`, `asUser`, bridge `view`/`router`/`showFlag`, `@forge/dashboards-bridge` `widgetEdit` |
| `$FORGE_KIT/schema/manifest-schema.json` (@forge/manifest 13.6.0, 211 module types) | every module shape incl. `rovo:skill`, `dashboards:widget` |
| `$FORGE_KIT/openapi/jira.json`, `jira-software.json` | `/search/jql`, `/changelog/bulkfetch`, `/issue/bulkfetch`, Agile board configuration and sprints, scopes per endpoint |
| `npm run lint` | scopes, schema errors, deprecated APIs |
| the dev site through `forge-dev` | real event payloads, changelog formats, errors, 429s |

A model that recalls the platform spends fewer calls and makes fewer defects; one that researches can still
reach 1.0. Recall itself is reported, not scored (§8.8).

## 5. The site and its fixtures (WP1)

### 5.1 One generator, two seeds

`forge/site/fixtures.cjs` is a pure function `facts(seed) → pack` (16 hex chars, xorshift128+ seeded from the
hex, no `Math.random`, no wall clock). The DEV site and the SCORING site run the same generator with different
seeds:

- `fixture_seed` — drawn at launch by `score_forge._draw_seed()` (8 random bytes), written into the trace header
  and used ONLY by the scorer.
- `dev_seed` — drawn by `forge_site.serve()` independently, served to the entrant's dev tools, written into the
  same trace header. The scorer REFUSES (exit 2) when `--seed` equals the tree's `dev_seed`.

### 5.2 What a pack holds (ranges are generator policy; the scorer reads the actual values, never these)

| fact | per seed |
|---|---|
| projects | two projects; keys drawn per seed from a realistic list (`OPS`/`PAY` below are placeholders), ids seeded |
| boards | one scrum board per project; which board estimates with "Story point estimate" and which with "Story Points" is seeded, and some issues also carry a value in the other board's field (a decoy only the board configuration resolves); one kanban board (no sprints: `/board/{id}/sprint` answers 400 as Jira does) |
| custom fields | Sprint, both estimate fields and ~30 decoys from a realistic field list; every custom field id seeded (`customfield_1xxxx`) |
| sprints | OPS: 2 active (parallel), 1 future, 2 closed; PAY: 1 active, 1 future, 1 closed; names, ids and dates seeded |
| issues | 229–245 across both projects ("237-ish"); estimates from {0.5,1,2,3,5,8,13} or none |
| history (before install) | 50–70 sprint changes after the active sprints' starts; ≥ 8 carry-over issues whose Sprint values list several ids; ≥ 4 issues removed to the backlog |
| sprint/change timing | no two sprints of a board share a `startDate`; no change is created exactly at a `startDate`; the scheduled job runs before the first live update |
| live script | 36–44 updates: ~60% sprint, ~15% estimate, ~25% irrelevant (summary, labels, status); 4 duplicated deliveries, 2 permuted pairs, 3 dropped |
| people | 6 users; `viewer` (the probe's identity); the app's own account |
| visibility | 4–6 issues under a security level the viewer cannot browse, ≥ 2 with changes in active sprints; 1 visible issue on which the viewer may not comment |
| faults | matched by WHO is calling, never by path (an efficient app may never touch a given endpoint): the first Jira request of the consumer invocation processing one scripted live change → 429 `Retry-After: 30` (if that invocation makes none, the first Jira request of the next consumer invocation that makes one); the second Jira request of the first scheduled run → 429 `Retry-After: 2`; the first comment POST → 429 `Retry-After: 1`; every 429 carries `RateLimit-Reason` |
| limits | from the OpenAPI/docs with the quoted sentence as receipt: search/jql page size, ids-only page size, `/changelog/bulkfetch` issue cap (1000) and field cap (10), `/issue/bulkfetch` caps, Agile `maxResults` |

Pack JSON (the interface WP2 consumes; `node forge/site/fixtures.cjs --seed S --out pack.json`):

```
{ "seed", "now", "cloudId", "siteUrl", "appAccountId", "viewer",
  "users":    [{ "accountId", "displayName" }],
  "fields":   [{ "id", "name", "custom", "schema" }], "sprintFieldId",
  "projects": [{ "id", "key", "name" }],
  "boards":   [{ "id", "name", "type", "projectKey", "estimationFieldId" }],
  "sprints":  [{ "id", "name", "state", "originBoardId", "startDate", "endDate", "completeDate" }],
  "issues":   [{ "id", "key", "projectKey", "summary", "fields": {…}, "hiddenFrom": [accountId],
                 "commentForbiddenFor": [accountId] }],
  "history":  [{ "changelogId", "issueId", "created", "authorId", "items": [{ "field", "fieldtype",
                 "fieldId", "from", "fromString", "to", "toString" }] }],
  "live":     [{ "changelogId", "issueId", "created", "authorId", "items": […],
                 "delivery": { "slot", "duplicates", "dropped" } }],
  "faults":   [{ "id", "match": { "scope": "consumer-of-change|scheduled-run|comment-post", "changelogId"?,
                 "nth" }, "status", "retryAfter", "reason" }],
  "limits":   { "<name>": { "value", "receipt" } } }
```

### 5.3 What the site enforces (each is a real Jira/Forge rule, RESEARCH §2–§3)

- Scopes per endpoint from the shipped OpenAPI, OAuth2 alternatives ONLY (`x-atlassian-oauth2-scopes`, or the
  `OAuth2` entry of `security` where that key is absent, as on the Agile paths); `basicAuth` and empty `{}`
  alternatives are ignored (they would allow every call). A call is allowed when the declared scopes contain the
  classic (`state: Current`) set or the full granular (`Beta`) set; otherwise 401 "Unauthorized; scope does not
  match". KVS without `storage:app` → 403. No hand-written scope table. `l_scopes` charges a declared scope as
  extra only when no call's chosen alternative uses it (classic preferred where it exists).
- `/rest/api/3/search` → 410 with Jira's message; `/search/jql` → ids only without `fields`, `nextPageToken`
  absent on the last page, no `total`, 400 on unbounded JQL, ≤ 7 ORDER BY fields.
- JQL subset (`forge/site/jql.cjs`): fields `project`, `key`/`issuekey`, `sprint`, `updated`, `created`,
  `status`, `statusCategory`, `issuetype`, `labels`, `assignee`, `reporter`, `cf[id]` and field names; operators
  `= != in not in > >= < <= is is not ~`, `AND OR NOT`, parentheses, `ORDER BY`; functions `openSprints()`,
  `closedSprints()`, `futureSprints()`, `currentUser()`, `now()`, `startOfDay()`, relative dates (`-14d`)
  evaluated on the site's virtual clock.
- Per-user visibility: `asUser` reads of a hidden issue → 404 "Issue does not exist or you do not have
  permission to see it."; searches and bulk endpoints omit hidden issues; comment POST without permission → 403.
  `POST /rest/api/3/permissions/check` (with `accountId`) is modelled, so an app may read as itself and check
  the person's permission explicitly (contract §3; the Rovo action docs give the user's `accountId` in the
  context but never mention `asUser`).
- Types as the OpenAPI declares them: every date-time (changelog `created`, sprint dates, issue `updated`) is an
  ISO-8601 string, never epoch milliseconds (WP3 found the site serving numbers — the zero-rows defect).
  `POST /rest/api/3/issue/{id}/changelog/list` is modelled. The emulator enforces KVS attribute types as
  documented (`integer` is 32-bit) and its error names the value range, not only the type.
- ADF: comment `body` must be an ADF doc; a string → 400 "Operation value must be an Atlassian Document".
- Pagination styles as Jira: `nextPageToken` (search/jql, changelog/bulkfetch), `startAt`/`maxResults`/`isLast`
  (issue changelog, Agile).
- Scripted 429s (§5.2 faults) with `Retry-After` and `RateLimit-Reason`; the per-issue write rule (20 writes per
  2 s, RESEARCH §2) on writes. No invented burst limits.
- Virtual clock: `now = real elapsed + skipped`; the emulator advances it by an `InvocationError.retryAfter`. A
  fault window is open until `now ≥ t429 + Retry-After`; an early retry gets another 429 and is recorded as
  `early_retry`.

**Unmodelled policy (gate 1).** A request for a path that exists in the shipped OpenAPI but is not modelled →
501 `EMULATOR_NOT_MODELLED` + a `harness_missing` entry; a known Jira JQL function or field the parser does not
model → the same. The run is never voided and never zeroed for it: the verdict is `status: "held"` (unpublished,
the tree and receipt kept), the gap is modelled, and `bench_rescore.py` rescores the SAME tree with the run's
seed. The reference gate refuses on any `harness_missing`. A path that is NOT in the OpenAPI (an invented
endpoint) → 404 like Jira — the app's defect. Before the freeze WP1 models the documented alternatives for every
data need, not just the golden's: field discovery (`/field`, `/field/search`), user (`/myself`, `/user`,
`/user/bulk`), issues (`/issue/{key}`, `?expand=changelog`, `/issue/bulkfetch`), changelogs (`/issue/{key}/changelog`,
`/changelog/bulkfetch`), sprints and boards (`/sprint/{id}`, `/sprint/{id}/issue`, `/board`, `/board/{id}/sprint`,
`/board/{id}/configuration`, `/board/{id}/issue`), search (`/search/jql` GET and POST, `/search/approximate-count`),
comments (POST/GET), permissions (`/permissions/check`, `/mypermissions`). Proof: a second, independent mini-app
(§13.4 item 8) runs without a single `harness_missing`.

## 6. The emulator (WP1)

### 6.1 Runtime: the real wrapper, pinned by sha256

`forge/kit/runtime-pin.json`: `{url, sha256, bytes, loader_url, loader_sha256, fetched}` for
`https://forge-node-runtime.prod-east.frontend.public.atl-paas.net/assets/wrapper.<ts>--<build>--<sha>.js` and its
loader — the CDN @forge/bundler's `NetworkWrapperProvider` uses (spike vendor/SOURCE.txt). `forge_kit.ensure()`
downloads once into the kit cache, verifies the sha, refuses on mismatch or absence. Not committed, not shipped
in the app (licence). WP1 records how @forge/bundler discovers the current wrapper so a re-pin is one command.

The in-repo shim (`spike/harness/runtime-shim.cjs`, tier A) survives only behind `--runtime shim` for offline
development of the harness itself; a shim verdict carries `runtime: "shim"`, `publishable: false` and a banner.
There is no automatic fallback.

Invocation = the spike's tier B: `fork(real-wrapper-runner.cjs)` per invocation with `_meta.proxy` pointing at
the emulator's proxy, `_meta.aaid` set ONLY for user-led invocations (resolver, Rovo action), the module's
platform timeout (resolver/trigger 25 s, action 55 s, consumer/scheduled 55 s or `timeoutSeconds` ≤ 900 —
RESEARCH §2 runtime limits). A timed-out child is killed by pid (gate 4, never a group).

Every invocation child runs under `sandbox-exec` with a deny-default profile: read the bundle dir, the kit's
runtime files, the Node install and system libraries; no writes outside `/dev`; `process-exec` only of Node itself;
network-outbound only to the proxy's port. Measured 2026-10-02 on this Mac with Node 22.22.0 under such a
profile: reading a file outside the grant → `EPERM`; `child_process.execSync` → `EPERM`; the granted
127.0.0.1 port → 200; a second 127.0.0.1 port → `EPERM`; https://example.com → `ENOTFOUND`. So app code cannot
read the seed, the pack or the site's files, cannot spawn tools, and cannot reach the site except through the
proxy — which is what makes §9's "an app never addresses the site" a measured fact, not an assumption. The same
profile runs the dev kit's invocations, so entrants meet it while building.

Bundling as `forge deploy`: esbuild per handler file under `src/`, `platform: node`, `format: cjs`,
`target: node22`, @forge packages resolved from the PRISTINE kit modules (never the workdir's). Handler
resolution follows @forge/bundler (`index.fn` → `src/index.{ts,tsx,js,jsx,mjs}`; WP1 confirms whether
`src/index.fn` is also accepted — the rovo-skill docs page example uses that form).

### 6.2 Proxy (from spike `forge-proxy.cjs`)

Routes `fpp/provider/{app|user|none}/remote/{jira|confluence|stargate}`, KVS capability, egress, logs. Must model
every endpoint `@forge/kvs` 2.0.7 can call (WP1 enumerates them from `@forge/kvs/out` and lists them in the kit
README): get/set/delete (+ `keyPolicy`, `ttl`, `returnValue`), secrets, query with `beginsWith` and cursors,
batch get/set/delete with `successfulKeys`/`failedKeys`, transactions with `check` (≤ 25 ops), entity
get/set/delete/query with declared indexes (partition match, range conditions, `Sort`, `Filter`, cursor,
limit), and the documented limits (key 500, value 240 KiB in bytes, depth 31). Queue publish → 201 (202/413/429
when scripted). Every call is logged `{t_virtual, invocationId, moduleType, provider, method, path, body,
status}` — the scorer grades from this log.

### 6.3 Event model

- `deliverProductEvent(change)`: the site has ALREADY applied the change (fields + changelog history at
  `created = now`); the emulator builds `avi:jira:updated:issue` (`issue{id,key,fields}`, `atlassianId`,
  `changelog{id,items}`, `eventType`) for every `trigger` module subscribed to it, honours `filter.ignoreSelf`,
  does NOT evaluate `filter.expression` (contract §8).
- Queues: `Queue.push` events are delivered to the `consumer` whose `queue` matches, as `AsyncEvent {body,
  queueName, jobId, eventId, retryContext}`. Delivery is sequential and drained to quiescence after each product
  event. A consumer result `{_retry: true, retryOptions}` (`InvocationError.toJSON`) → clock advances by
  `retryAfter`, redelivery with `retryContext.retryCount+1`. The async-events page documents no retry maximum:
  "Async Events are automatically retried within the retention window until they are successfully delivered.
  Retries use exponential backoff, with intervals reaching up to approximately 15 minutes between attempts", and
  the window "lasts for 24 hours"; app-level errors are retried and dropped when the window is exceeded. The
  harness therefore redelivers a thrown or timed-out consumer after 1, 2, 4, 8 virtual minutes, then every 15,
  until 24 virtual hours (the 1-minute start is the harness's choice, stated in contract §8). A throw is NOT an
  honoured `Retry-After` (`t_retry_after_honoured` = 0 for that fault) even though the redelivery lands the row.
- Scheduled: `runScheduled(moduleKey)` invokes with `{context:{cloudId, moduleKey}, contextToken}`, then drains.
- Rovo action: the documented action payload (inputs + context, per the action module page; WP1 quotes it), as
  the viewer.

### 6.4 Custom UI host (`forge/kit/lib/bridge-host.cjs`)

Serves `resources[].path` from a static server under a nested random prefix (absolute `/assets/…` 404s like
production) with the default Forge Custom UI CSP header (WP1 transcribes it from the custom-options and
permissions pages; `permissions.content.*` relaxes it as documented — the custom-options page: "To include
inline CSS in your app, follow the instructions on how to use custom content security policies"). A
`content.styles: [unsafe-inline]` relaxation is free; a `content.scripts` relaxation counts as an extra
permission in `l_scopes` (contract §7). React `style={{…}}` sets CSSOM properties, which style-src does not
govern; markup `style` attributes and `<style>` elements are blocked unless relaxed. An in-page init script installs
`globalThis.__bridge.callBridge` BEFORE the bundle (bridge 7.1.0 captures it at module load) and handles in the
page the ops whose arguments are functions (`on`, `getWidgetApi`, `getWidgetEditApi` return page-side objects);
the rest forward to Node via `exposeFunction`.

| op | host behaviour |
|---|---|
| `getContext` | seeded context: `extension` per module (widget: `{type, config, context:{dashboardId, widgetId}, layout:{width,height}, placement, filters: null, entryPoint?}` — `entryPoint: 'edit'` on the edit surface; after an edit-side `updateConfig`, `getContext` returns the new config; sprint action: `{type, project, board:{id,type}, sprint:{id,state}, location}`), `accountId` of the viewer, `theme`, `locale`, `timezone` |
| `invoke` | resolver function via §6.1, user-led as the viewer |
| `fetchProduct` | `requestJira` as the viewer through the proxy |
| `enableTheming` | injects the pinned @atlaskit/tokens light/dark theme CSS as a constructable stylesheet and sets `html[data-color-mode]` + `data-theme` as Atlassian does; without the op nothing is injected |
| `open`, `navigate`, `getUrl`, `reload` | recorded (router) |
| `close`, `submit`, `refresh`, `changeWindowTitle`, `emitReadyEvent`, `emitFrontendCustomMetric`, `initFeatureFlags`, `onClose` | recorded, inert |
| `showFlag`, `closeFlag` | recorded with their options |
| `on`, `emit` | page-side event bus; the host emits, with `{widgetId}`: `FORGE_DASHBOARDS_WIDGET_EDIT_CONFIG_CHANGED` after each edit-side `updateConfig`, `FORGE_DASHBOARDS_WIDGET_CONFIG_CHANGED` after a Save, `FORGE_DASHBOARDS_WIDGET_LAYOUT_CHANGED` after a resize — the events `@forge/hooks` 2.0.0 `useWidgetConfig.js`/`useWidgetContext.js` subscribe to. The probe renders each width as a fresh surface AND resizes one live surface |
| `getWidgetApi` | `{setPreviewConfig}` recorded |
| `getWidgetEditApi` | `{updateConfig, onSave, onProductSave, onSaveError}`. The host's Save (the probe; `window.__forgeHost.save()` in the dev kit): with `onProductSave` registered it stores ONLY that handler's return value and `null` stores nothing (widget docs: `return null; // return config to opt in to in-product save`; bridge page: `return config; // Return config to save in product`); with none registered — undocumented — it stores the last `updateConfig` value (stated in contract §4). Then `onSave(config, {widgetId})`, the config event, and the view re-renders with `extension.config` |
| anything else in bridge 7.1.0 | recorded + `harness_missing` (never silently answered) |

### 6.5 The dev kit the entrant runs (`forge/kit/bin/forge-dev.cjs`)

The same emulator and host as scoring (no divergence), pointed at the dev site (`FORGE_SITE_URL`), with dev KVS
state persisted under `.forge-dev/` in the workdir. Commands as in STARTER.md: `invoke --resolver <key>` builds
the resolver's `{call:{functionKey, payload}, context}`; `serve` accepts `--sprint <id>` (sprint-action context)
and `--config <json>` (a stored widget config), runs until stopped, and exposes `window.__forgeHost.save()` (the
dashboard's Save, §6.4); `reset` clears dev KVS and queues AND rewinds the dev site's live-update cursor, so
`events` replays from the start. It contains no fixtures, oracle,
fault schedule or checks; the site and the scorer live outside the sandbox's read set.

### 6.6 Screenshots

The probe saves `forge-shots/<surface>-<board>-<theme>-<w>x<h>.png` for: widget view (OPS, PAY) × light/dark ×
380/1180 px wide; widget edit × light/dark; sprint action for an OPS active sprint × light/dark at 800×600;
the after-comment flag state; the not-started state — plus `forge-shots/contact-sheet.png` composited by a
Playwright page. Screenshots are evidence for publication. Grading reads the DOM (computed styles, rects, text);
pixels decide only blank-surface and dark-mode dominance (§8.2 V).

## 7. The lint stage

Forge's own client-side linter (`@forge/lint` 6.3.0, the 16 linters, `mode: 'client-side'`, local OpenAPI via
`USE_LOCAL_SWAGGER`, a statsig stub) — the spike's `lint-offline.cjs`, now `forge/kit/bin/lint.cjs --json`. The
same file is the entrant's `npm run lint` (no divergence).

- The scorer runs it twice on the scored tree; the two JSON results must be identical, else every L row is
  `unavailable` (a harness defect, never the app's). This is the fixed point.
- Staging: `FullManifestLinter` hides later stages behind schema errors (RESEARCH §3, measured). The scorer reads
  the stage reached; unreached linters are unproven, never clean.
- **Errors**: critical `l_deployable` (severity 0) and the 0.499 band. **Warnings**: points only
  (`l_lint_warnings`, −0.2 per distinct warning message, floor 0) and an excellence-gate condition; never a cap.
- Blind spots lint misses (RESEARCH §3 list) are the scorer's own rules (`l_manifest_rules`) or runtime checks:
  resource dir contains `index.html` and is not under `src/`; no `layout: basic`; keys unique across modules,
  functions and resources; every referenced function exists and exports its handler; `storage:app` with KVS use;
  `/rest/api/3/search`, string paths, asUser in background, ADF, pagination and 429 handling are runtime checks.

## 8. Grading (WP2)

### 8.1 Composition

`earned = (0.88 · inner + 0.12 · gate_fraction · e_mean) × crit_mult`; `final = min(earned, ceiling)`; tier means
are plain means of their non-diagnostic, available rows (γ = 1); `unavailable` rows are excluded and make the
verdict unpublishable; `vacuous_root` rows price 0 and fire no multiplier. Report `inner`, `crit_mult` and the
unsuppressed criticals with every score.

Tier weights (inner, sum 1.00; asserted in score_forge.py):

| L lint | K currency | T pipeline | R reconcile | S storage | B resolvers | U UI function | V visual | A Rovo |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| .08 | .10 | .16 | .14 | .08 | .12 | .16 | .08 | .08 |

### 8.2 Check registry

55 weighted-or-diagnostic rows plus 4 E rows (§8.4). C = critical. Weight = the tier weight split equally across the tier's weighted rows. "Log" = the proxy/site
call log; "ev" = probe evidence; "oracle" = forge_oracle.py on the pack.

| id | tier | measured | how | precondition (else vacuous 0) | C |
|---|---|---|---|---|---|
| `l_deployable` | L | 0 lint errors, last stage reached | lint ×2 identical | ≥ 1 module declared | C |
| `l_bundles_load` | L | share of manifest functions that bundle and load with the handler exported | esbuild + require under the wrapper | ≥ 1 function declared | C |
| `l_lint_warnings` | L | distinct warnings (1 − 0.2·n) | lint | ≥ 1 function loads |  |
| `l_real_packages` | L | bundles resolve @forge/* at the kit pins; no imports outside the kit | esbuild metafile | ≥ 1 bundle imports `@forge/*` |  |
| `l_manifest_rules` | L | §7 blind-spot rules, fraction met | manifest + tree | ≥ 1 resource and ≥ 1 function |  |
| `l_scopes` | L | declared = required: per call the OAuth2 alternative it is satisfied by (classic where one exists, else the full granular set), plus `storage:app`; a declared scope is extra only when no call's chosen alternative uses it; a `permissions.content.scripts` relaxation is extra; 1 − 0.25 per missing or extra, floor 0; any `manage:`/admin-config scope counts 2 | lint PermissionLinter + log | ≥ 1 product or KVS call observed |  |
| `k_dashboard_widget` | K | `dashboards:widget` with `edit.resource`, Custom UI resources; no `jira:dashboardGadget` | manifest | — |  |
| `k_widget_edit_bridge` | K | edit saves via `getWidgetEditApi` (`updateConfig` or `onProductSave` observed) | ev bridge log | edit surface rendered |  |
| `k_rovo_skill` | K | `rovo:skill` `source.dir` exists with SKILL.md; frontmatter rules (contract §6); `dependencies.tools` = [`get-sprint-scope`]; `allowed-tools` ⊇ tools; a `rovo:agent` lists the skill | tree + manifest | — |  |
| `k_current_apis` | K | no `storage` from @forge/api, no `@forge/ui`, no `/rest/api/3/search` (static and runtime), every product call through `route` (no "You must create your route" throw), runtime `nodejs22.x`/`nodejs24.x` | AST + log | ≥ 1 product call observed |  |
| `k_consumer_shape` | K | consumer uses `function:`; its function serves no resolver; `timeoutSeconds` only on consumer/scheduled | manifest | a consumer exists |  |
| `k_entity_declared` | K | `app.storage.entities` declares the ledger entity with an index partitioned by sprint and ranged by time | manifest | — |  |
| `t_trigger_handoff` | T | irrelevant updates: 0 Jira calls, 0 pushes; relevant: ≥ 1 push, and the event path's ledger writes happen in consumer invocations (the scheduled job may write directly) | log by invocation module type | a trigger exists and ≥ 1 push observed |  |
| `t_event_rows` | T | rows for delivered live changes = oracle, keyed changelog id + sprint (issue, kind, at as instant, by, source=`event`); F1 | resolver/entity read + oracle | — |  |
| `t_no_double_count` | T | duplicated deliveries add no row and no points | rows + numbers | ≥ 1 ledger row | C |
| `t_out_of_order` | T | permuted pairs: rows and numbers = oracle | subset of the above | — |  |
| `t_multi_sprint_parse` | T | carry-over (multi-id) changes correct | subset | — |  |
| `t_reestimate_followed` | T | numbers after the estimate-change events = oracle (both boards' fields) | resolver read | — |  |
| `t_retry_after_honoured` | T | the consumer-path 429 (Retry-After 30): an `InvocationError` with `retryAfter` ≥ 30, or an in-invocation wait with the next attempt at ≥ 30 virtual s → 1.0; a throw (platform backoff) or an early retry → 0; the row must land once | site fault log | the fault fired |  |
| `t_no_user_in_async` | T | 0 `provider=user` calls and 0 `NeedsAuthenticationError` in trigger/consumer/scheduled | log | ≥ 1 background Jira call |  |
| `r_backfill_complete` | R | after the first scheduled run: historical rows = oracle (fraction) | entity read + oracle | — | C |
| `r_removals_found` | R | removed-to-backlog changes present | subset | — |  |
| `r_heal_dropped` | R | after the second run every dropped change has exactly one row; its `source` may be `reconcile` OR `event` (a consumer re-reading the full changelog while handling another delivery legitimately records it first — P7); no other row added | rows diff | — |  |
| `r_pagination` | R | every paginated read walked to its end in the endpoint's own style (`nextPageToken` for `/search/jql` and `/changelog/bulkfetch`, `startAt`/`isLast` for issue changelog and Agile), each page once; no `/rest/api/3/search`. Ids-only searches are legitimate — a missing field shows up in the numbers, not here | log | ≥ 1 paginated read |  |
| `r_rate_limit` | R | reconcile 429 (Retry-After 2): retried at ≥ 2 virtual s, run completes | fault log | the fault fired |  |
| `r_as_app` | R | reconcile reads are `provider=app` | log | ≥ 1 scheduled-run Jira read |  |
| `r_completes_in_timeout` | R | every scheduled invocation finishes inside its module timeout | emulator | a scheduled trigger made ≥ 1 Jira call |  |
| `s_storage_scope` | S | KVS used and no 403 from the KVS proxy | log | ≥ 1 KVS call |  |
| `s_entity_index_used` | S | ledger reads are entity queries on the declared index with the sprint as partition | log | ≥ 1 ledger read |  |
| `s_index_order` | S | rows read by the index come back in change-time order (default table order = oracle) | ev + log | table rows rendered |  |
| `s_limits` | S | no KVS limit errors (value bytes, key length, transaction ops) | log | ≥ 1 KVS write |  |
| `b_invoke_contract` | B | every invoked key is defined; no `undefined` results; bad input → structured error, not a throw | ev bridge log | ≥ 1 invoke observed |  |
| `b_no_permission_leak` | B | hidden issues' keys (whole-token match, `\b<KEY>\b`, so `OPS-12` never matches `OPS-120`) and summaries appear in no invoke/action response and no DOM text; change ids are compared only in `data-change-id` attributes and `changeId` fields | ev scan vs oracle | ≥ 1 person-facing change list returned or rendered | C |
| `b_hidden_count` | B | `hidden-count` and `hiddenChanges` = oracle per sprint | ev | — |  |
| `b_comment_adf_as_user` | B | comment body valid ADF (pinned @atlaskit/adf-schema JSON schema), contains issue key, sprint name, creep; author = viewer | site comments | ≥ 1 comment POST |  |
| `b_comment_exactly_once` | B | double click → exactly one comment; the 429-once fault → exactly one comment and one success flag | site comments | ≥ 1 comment POST | C |
| `u_widget_loads` | U | widget view renders ≥ 1 sprint with numbers (diagnostic: weight 0) | ev | — | C |
| `u_widget_numbers` | U | four numbers per active sprint of the configured board = oracle (Decimal, ROUND_HALF_UP to one decimal for creep, written `x.y%`), order by `startDate` | ev DOM | — |  |
| `u_widget_chart` | U | one rect per sprint × series; heights on one linear scale within 1 px | ev rects | — |  |
| `u_widget_edit_config` | U | pick board B → host Save → view shows B's sprints; reopening edit shows B pressed; no config → `needs-config` only; a SECOND widget instance (different `widgetId`, host-injected `extension.config` naming the other board) shows its own board | ev | — |  |
| `u_ledger_table` | U | rows = oracle visible changes for the context sprint, cells correct (`datetime` as instants), default order `at` then changelog id | ev DOM | — |  |
| `u_ledger_sort` | U | `at` toggles between default order and its exact reverse (ascending first after another sort); `points` descending, ties in default order; changelog ids compared numerically; `aria-sort` on the active header | ev | table rows rendered |  |
| `u_issue_router` | U | issue key click → bridge `open` or `navigate` (any `type`) to `/browse/<KEY>`; no popup or top navigation | ev | table rows rendered |  |
| `u_comment_flow` | U | select + post → success flag; forbidden issue → error flag, modal still sorts | ev | — |  |
| `u_modal_close` | U | close → bridge `close` | ev | modal rendered |  |
| `u_not_started` | U | future sprint context → `not-started` and no ledger, metrics or post button (a close button is allowed) | ev | — |  |
| `v_theme_tokens` | V | `enableTheming` called on every surface; metric and table text colours equal the mode's `--ds-text*` values (`--ds-link*` accepted on links); contrast ≥ 4.5:1 in both modes; disabled controls exempt | ev computed styles | a surface rendered app content |  |
| `v_dark_mode` | V | the host page is unpainted, so the app paints its own surface: dark screenshots' dominant colour is in the dark `--ds-surface*` family, light in the light family; no surface blank | ev pixels + tokens | a surface rendered app content |  |
| `v_csp_clean` | V | 0 `securitypolicyviolation`, 0 failed asset requests | ev | a surface rendered app content |  |
| `v_console_clean` | V | 0 console errors/page errors on nominal scenarios | ev | a surface rendered app content |  |
| `v_widget_sizes` | V | at 380 and 1180 px: no horizontal overflow, every sprint visible, no `data-metric` text clipped or ellipsized (names may ellipsize) | ev | widget rendered sprints |  |
| `a_action_result` | A | action JSON = oracle for each active sprint (numbers rounded as creep, visible changes in table order, `at` as instants) | emulator | — |  |
| `a_action_errors` | A | unknown and missing `sprintId` → `{error}`, no throw | emulator | the action exists |  |
| `a_action_permissions` | A | the per-person OUTCOME: `hiddenChanges` and the visible list are right for two users, whether read `asUser` or `asApp` + an explicit permission check (`asUser` in actions is undocumented) | emulator | — |  |
| `a_skill_instructions` | A | SKILL.md body ≤ 500 lines, names `get-sprint-scope` and `sprintId` | tree | SKILL.md exists |  |

Partial credit: every fraction-valued row is the measured fraction; an absent surface is 0 with `absent_surface`
(severity 0 when critical) — never a refusal (score_sb7 F9 doctrine).

Vacuity (G5): a "no defect" row cannot pay an app that never exercised the surface. When a row's precondition is
unmet it scores 0 with `vacuous_root` (priced 0, no multiplier of its own). Without this, an app that does
nothing collected ~0.08 from the L/K/R/S/B/V "nothing went wrong" rows. The oracle computes every number with
`Decimal` and `ROUND_HALF_UP` (half away from zero for these non-negative values), the rule contract §1 states.

### 8.3 Criticals (multiplier floor 0.6, `factor = 0.6 + 0.4 · severity_input`)

| critical | consequence | severity input |
|---|---|---|
| `l_deployable` | deploy blocked | 0 on any error |
| `l_bundles_load` | crash — the app does not run | 0 when < 0.4 of functions load, else 1 (SB `server_runs` transform) |
| `t_no_double_count` | wrong numbers — a change counted twice | cliff 0 |
| `r_backfill_complete` | data loss — changes silently missing | `min(s, 0.5)` when < 1 (SB data-loss transform) |
| `b_no_permission_leak` | data leak — hidden issues shown to a person | cliff 0 |
| `b_comment_exactly_once` | duplicate side effect on a customer's Jira | cliff 0 |
| `u_widget_loads` | dead primary flow — no data on the dashboard | 0 if 0 else 1 |

Admission rule (SB): every critical is a fact the golden achieves at exactly 1.0 on every calibration seed;
none is calibration-owned.

ROOT_BLOCKS (attribution + multiplier dedup, SB F3): `l_bundles_load` → every T, R, S, B, U, A row;
`r_backfill_complete` → `u_widget_numbers`, `a_action_result`, `u_ledger_table`, `r_removals_found`;
`u_widget_loads` → `u_widget_numbers`, `u_widget_chart`, `u_widget_edit_config`, `v_widget_sizes`. Vacuity:
`t_no_double_count`, `b_no_permission_leak` and `b_comment_exactly_once` cannot pass on nothing — with no rows,
no surface or no comment they carry `vacuous_root` (0, no multiplier of their own).

### 8.4 Excellence (0.12 slice)

Gate conditions (proportional, as `score_sb7.excellence`): `t_event_rows`, `r_heal_dropped`, `u_widget_numbers`,
`u_ledger_table`, `b_comment_adf_as_user`, every K row = 1.0; `v_console_clean`, `v_csp_clean`,
`l_lint_warnings` = 1.0 (zero warnings).

E rows (rungs are ratios of the oracle's optimum, the cut points calibration-owned in `forge-thresholds.json`
with golden receipts; initial values below are the rc defaults):

| id | measured | rc rungs |
|---|---|---|
| `e_reconcile_economy` | Jira calls of the backfill ÷ oracle optimum (field list + boards + configs + sprints + one ids-only search page set + changelog bulkfetch pages + issue bulkfetch) | ≤ 1.5× 1.0, ≤ 3× .75, ≤ 10× .5, ≤ 40× .25 |
| `e_event_economy` | Jira calls per relevant live change ÷ oracle optimum (1 read, sprint/field metadata cached in KVS) | same |
| `e_ui_round_trips` | invokes before first meaningful paint per surface | 1 → 1.0, 2 → .75, ≤ 4 → .5 |
| `e_idempotent_rerun` | entity writes by the third (no-change) scheduled run | 0 → 1.0, ≤ 1% of rows → .5, else 0 |

### 8.5 Admission bands (passing awards nothing; `passed` = exactly 1.0, available, not vacuous — sb71.passed)

| band | max | requires |
|---|---:|---|
| deployable | 0.499 | `l_deployable`, `l_bundles_load` |
| working ledger | 0.699 | `u_widget_loads`, `t_event_rows`, `r_backfill_complete`, `s_storage_scope`, `s_entity_index_used` |
| current platform, complete surfaces | 0.799 | `k_dashboard_widget`, `k_widget_edit_bridge`, `k_rovo_skill`, `u_widget_edit_config`, `u_ledger_table`, `a_action_result`, `v_theme_tokens`, `v_dark_mode` |
| production robustness | 0.899 | `t_no_double_count`, `t_out_of_order`, `t_retry_after_honoured`, `r_heal_dropped`, `r_rate_limit`, `r_pagination`, `b_no_permission_leak`, `b_comment_exactly_once`, `v_csp_clean`, `v_console_clean` |

The prompt's "Score bands" section states these four in words (parity test, §14).

### 8.6 Severity selftest (wired into `--reference`; an inversion refuses the freeze)

Synthetic row sets through the REAL composition: (1) every weighted row earns (0 → earned < 1); (2) one lint
warning costs points and never caps; (3) a permission leak scores below a missing chart; (4) a duplicate comment
scores below a missing comment; (5) a gadget-instead-of-widget app is held at 0.799; (6) the empty-starter row set
is scored (a verdict, never a refusal) at ≤ 0.05, and a one-function app (a trigger that does nothing) at ≤ 0.05
— the vacuity preconditions (§8.2) are what make both hold; (7) a dead bundle multiplies once (dedup) and lands ≤ 0.30; (8) a 0.9
backfill multiplies by exactly 0.8; (9) dominance: every band failure ≤ its ceiling; (10) the single-defect cost
table (WP2 computes it once from the composition and pins it in `test_score_forge.py`).

### 8.7 Scoring sequence (one fresh site, serial)

1. Clone the tree (excluding `node_modules`, `.forge-dev`, `forge-shots`); clone the kit's pristine modules in.
2. Lint ×2; bundle every function; load each under the wrapper.
3. Start the scoring site (`fixture_seed`, ephemeral port), the proxy and the emulator; empty KVS.
4. Backfill: run every `scheduledTrigger` once, drain queues.
5. Live: deliver the script (duplicates, permuted pairs, drops, irrelevant updates, estimate changes), draining
   after each delivery; the consumer 429 fires here.
6. Heal: scheduled run #2. Rerun: scheduled run #3 (no changes).
7. Rovo: the action as the viewer and as a second user, each active sprint, an unknown and a missing `sprintId`.
8. UI (Playwright, bundled Chromium): widget with no config → edit pick OPS → Save → view at 380/1180 light/dark
   → edit pick PAY → Save → view; sprint action for each active sprint (sort, router, select, double-click post,
   forbidden post, close) light/dark; future sprint; screenshots.
9. Oracle comparison, composition, bands, report.

Evidence: `forge-observations.json` (lint, bundles, call log, KVS snapshot after each phase, bridge log, DOM
reads, rects, computed styles, comments, harness_missing) + `forge-shots/`. Expected wall time 4–6 min (WP2
measures and records it).

### 8.8 Recall trace (report-only, weight 0)

From the isolated goose session database: the first written `manifest.yml`'s module types, the first backend
file's imports, and the number of lint runs and reference reads (`node_modules`, schema, OpenAPI) before the first
clean lint. Published beside the score as "recall vs research"; never part of the composition.

## 9. Hermetic scoring CLI (`bench/score_forge.py`)

`score_forge.py --tree <run tree> --seed <fixture_seed> --json-out v.json [--reference] [--runtime shim]`

- Refuses (exit 2) without `--seed`, on a malformed seed, and when `--seed` equals the tree's `dev_seed`
  (`trace.jsonl` header).
- Refuses (exit 3) when the preflight fails: Node with Playwright and the bundled Chromium
  (`GOOSE_SWARM_RENDER_NODE`), the kit cache present with the pinned lock sha, the wrapper sha.
- Serial: an exclusive lock file per host; the site binds an ephemeral port chosen by the scorer. The SB
  "advertised port" mechanism does not apply: a Forge app never addresses the site (all calls go through
  `route` → proxy); a literal `127.0.0.1` fetch is egress and refused.
- Never scores the workdir in place and never uses its `node_modules`.
- Verdict `status`: `scored` | `held` (§5.3 unmodelled policy). Verdict keys: `score`, `rawScore`, `inner`, `critical{multiplier, rows}`, `admission{ceiling, reasons}`,
  `excellence`, `tiers`, `checks`, `harness_missing`, `publishable`, `runtime`, `kit_lock_sha256`,
  `wrapper_sha256`, `fixture_seed`, `dev_seed`, `scorer_seconds`, `shots`, `recall_trace`.
- Reaps its own children on exit, SIGINT and SIGTERM, by pid.

## 10. Packaging

Measured closures (spike lockfile, `du -sk`, 2026-10-02): backend `@forge/api`+`kvs`+`events`+`resolver` 42.7 MB
(53 packages); + `@forge/bridge` 100.1 MB together (its deps include `@atlaskit/tokens` 13.9 MB,
`@atlaskit/adf-schema` 7.6 MB); `@forge/lint` 6.3.0 alone 174.3 MB (344 packages, `@forge/util` 44.9 MB,
`typescript` 23.4 MB); `esbuild` 10.2 MB; `@forge/react` 1,246 MB (excluded, D8); `@forge/cli` 377 MB (not
needed: `@forge/lint` and `@forge/cli-shared` 9.7.0 are published standalone).

**The kit** (`forge/kit/`, committed: `package.json`, `package-lock.json`, `bin/`, `lib/`, `openapi/`,
`runtime-pin.json`) is materialised by `bench/forge_kit.py ensure()` into
`~/Library/Application Support/Goose/benchmark/forge-kit/<lock-sha256[:16]>/` (override `FORGE_KIT_CACHE`):
`npm ci --ignore-scripts` from the committed lockfile (integrity hashes), the wrapper by sha, the manifest schema
copied out of @forge/manifest. Network is needed once per kit version, outside the sandbox. Two module trees:

| tree | contents | approx |
|---|---|---|
| `app-modules/` | @forge/api 8.2.0, kvs 2.0.7, events 3.0.7, resolver 2.0.0, bridge 7.1.0, dashboards-bridge 2.0.0, hooks 2.0.0, react + react-dom 18.3.1 (hooks peer `^18.2.0`), esbuild 0.28.2 | ~115 MB |
| `lint-modules/` | @forge/lint 6.3.0, @forge/cli-shared 9.7.0, yaml | ~175 MB |

The workdir gets `node_modules` as an APFS clone (`cp -cR`, zero bytes copied) of `app-modules/`; `$FORGE_KIT`
(read-only, added to the sandbox read set) holds `bin/`, `lib/`, `lint-modules/`, `schema/`, `openapi/`,
wrapper. No @forge package, wrapper or @atlaskit byte is committed or shipped in the app.

run_build changes this forces: `input-manifest.json` hashes the workdir EXCLUDING `node_modules` and records
`kit_lock_sha256` instead (15k files otherwise); the `_sb4trees` archive and the scorer clone ignore
`node_modules` and `.forge-dev`.

Shipped in the desktop payload: `forge/public/*`, `forge/starter/**`, `forge/kit/**` (no modules), `forge/site/*`,
`bench/score_forge.py`, `forge_oracle.py`, `forge_probe.mjs`, `forge_site.py`, `forge_kit.py`,
`forge-thresholds.json`. The OpenAPI files (7.1 MB, committed by the spike) ship in the payload — licence to be
confirmed by the owner (§15 R4).

## 11. Economics

OpenRouter list prices fetched 2026-10-02 (`/api/v1/models`), USD per million tokens (input / output / cache
read / cache write): GPT-6 Luna 0.10 / 0.50 / 0.01 / 0.125; GPT-6.1 Sol 2.00 / 10.00 / 0.10 / 2.50; Claude
Sonnet 5.5 2.00 / 10.00 / 0.20 / 2.50; GPT-6 Astra 10.00 / 50.00 / 1.00 / 12.50.

Token model, anchored on the measured Luna SB7.2 run on 3.0.87 (~/goose-builds/sb72-runs/20261002-gpt-6-luna-
b36032ee-trimmed/telemetry.jsonl: 150 calls, 12,910,157 prompt tokens, 171,324 completion tokens, final context
142,487; $0.23 billed, 99% cached): a strong model spends the full 150 calls; ~13.0 M cached prompt tokens, ~0.5 M
written to cache (context growth plus compactions), 0.25–0.45 M output (reasoning models write more than Luna).
Forge's public input is 16.9 KB vs SB7.2's 45.7 KB; reading typings and the OpenAPI adds back, so the prompt total
is kept at Luna's measured level.

| model | cached | cache writes | output | per run |
|---|---:|---:|---:|---:|
| GPT-6 Luna | $0.13 | $0.06 | $0.13–0.23 | **$0.32–0.42** |
| GPT-6.1 Sol | $1.30 | $1.25 | $2.50–4.50 | **$5.1–7.1** |
| Sonnet 5.5 | $2.60 | $1.25 | $2.50–4.50 | **$6.4–8.4** |
| GPT-6 Astra | $13.00 | $6.25 | $12.50–22.50 | **$32–42** |

A broken cache (the pre-3.0.87 GPT-6 failure) multiplies the input side by 10–20×: Astra uncached ≈ $150. Arm the
wallet guard (`BENCH_MAX_USD`, bench_budget.py) at $50 for Astra-class runs. The run config pins each model's
reasoning effort (the provider's `reasoning.effort`, `medium` unless the owner sets otherwise) and the verdict
records it — output tokens, and so cost, swing with it. The Sol, Sonnet and Astra rows are ASSUMPTIONS (Luna's
token profile at their prices) until a Sol run measures completion tokens per call. Scoring costs no model calls; ~5 min
of local CPU. These are estimates, not invoices; the first Luna run replaces them with a measurement.

## 12. Integration

- **isolated_tiers.py** — `IsolatedTier` gains `family='payments'`, `vendor='vendor_service_v3'`,
  `network='open'`, `kit=False` (defaults keep SB7.1/SB7.2 bytes-identical in behaviour); `FORGE10 =
  IsolatedTier('BENCH_FORGE10', 'forge-1.0', 'score_forge', 'forge/public/spec-build-forge.md', '',
  'forge/starter', (scorer files), (('FORGE-CONTRACT.md', 'forge/public/FORGE-CONTRACT.md'), ('STARTER.md',
  'forge/public/STARTER.md')), family='forge', vendor='forge_site', network='fenced', kit=True)`;
  `TIERS = (SB71, SB72, FORGE10)`.
- **run_build.py** — `--forge` sets `BENCH_FORGE10`; `_regime()` imports `tier.vendor` instead of hard-coding
  vendor_service_v3; for `tier.kit`: `forge_kit.ensure()` before the workdir, `node_modules` clone, env
  `FORGE_KIT` and `FORGE_SITE_URL`, the input-manifest and archive exclusions (§10); `serve_scoring_vendor` stops
  the dev site and lets `score_forge.gather` start its own scoring site. The budget, wallet guard, provider-error
  refusal, reaping, completion receipt and BROWSER-TESTING.md are unchanged.
- **forge_site.py** — the vendor interface run_build expects: `serve(port, trace, seed)` (starts
  `node forge/site/site.cjs` with a freshly drawn `dev_seed`, writes the trace header `{fixture_seed, dev_seed,
  tier, kit_lock_sha256}`), `mark_phase`, `DOCS_PATH = None`, `API_KEY = None`, `stop()`.
- **bench_isolation.py** — `prepare(..., network='fenced', extra_read=[kit])`: the fence rules of §4, the relay
  env, and the two-sided preflight; `open` keeps today's profile byte-identical.
- **bench_rescore.py** — receipts carry `tier`, `kit_lock_sha256`, `wrapper_sha256`; the retry replays
  score_forge.
- **release_manifest.py** — payload per family (`payload(root, family)`), so `forge/release-manifest.json` pins the
  forge payload and the SB7.x manifests keep their exact pins; `--check` per manifest.
- **ui/desktop** (after the freeze, panel-surgeon): `BenchTier` adds `'forge-1.0'`; a `BENCH_FAMILY` record;
  `DEFAULT_BENCHMARK_TIER` becomes per family; `benchmarkLaunchProblem` filters the catalog by family and accepts
  `^forge-\d+(\.\d+)*$`; `BENCH_SPEC_FILE`, `BENCH_RENDER_PROBE` (`forge_probe.mjs`), `BENCH_RUN_FLAG`
  (`--forge`); a family switch in the Benchmark view; `mirrorSwarmBenchPayload` copies the forge payload; the
  release-manifest copier verifies it.
- **leanzero.net** — catalog entries gain `family` (`payments` | `forge`); Forge gets its own leaderboard, not an
  SB era. (Site repository work, not goose.)

## 13. Build plan — three parallel packages

### 13.1 Ownership (one owner per file; nobody edits another package's files)

| package | owns |
|---|---|
| **WP1 — emulator + mock site** | `forge/kit/**` (package.json, lockfile, `bin/forge-dev.cjs`, `bin/lint.cjs`, `lib/emulator.cjs`, `lib/proxy.cjs`, `lib/bridge-host.cjs`, `lib/runtime.cjs`, `lib/csp.cjs`, `lib/tokens.cjs`, `openapi/`, `runtime-pin.json`), `forge/site/**` (`site.cjs`, `fixtures.cjs`, `jql.cjs`, `rest/*.cjs`), `forge/starter/**`, `bench/forge_kit.py`, `bench/forge_site.py`, `bench/bench_isolation.py` (fence + relay), `forge/kit/test/*` |
| **WP2 — scorer + probe** | `bench/score_forge.py`, `bench/forge_oracle.py`, `bench/forge_probe.mjs`, `bench/forge-thresholds.json`, `bench/forge_controls.py`, `bench/test_score_forge.py`, `bench/isolated_tiers.py`, `bench/run_build.py`, `bench/bench_rescore.py`, `bench/release_manifest.py`, `forge/release-manifest.json` |
| **WP3 — golden reference app** | `bench/golden-forge/**`, `forge/mutants/**` |

Public text (`forge/public/*`) changes only through the orchestrator, with §14's map updated in the same commit.

### 13.2 Interfaces

**I1 — the pack** (WP1 → WP2): §5.2 schema, `node forge/site/fixtures.cjs --seed S --out pack.json`.
Determinism test (WP1): two runs per seed byte-identical; three seeds differ in every id class.

**I2 — the emulator API** (WP1 → WP2; WP2 calls nothing else):

```js
const { createSite } = require('forge/site/site.cjs');            // private
const { createEmulator } = require('forge/kit/lib/emulator.cjs');  // public kit
const site = await createSite({ seed, port: 0, trace });           // {url, pack, clock, log, comments, stop}
const emu = await createEmulator({ appDir, kitDir, site, runtime: 'wrapper' | 'shim' });
emu.manifest; emu.functions; emu.modules(type);
await emu.build();                     // -> {functions: [{key, handler, bundled, loaded, error}]}
await emu.invoke(fnKey, { moduleKey, event, asUser });  // -> {ok, result, error, logs, ms, calls}
await emu.deliverProductEvent(change); // site applies, emulator delivers to matching triggers
await emu.drainQueues();               // -> deliveries [{eventId, attempt, result, retryAfter}]
await emu.runScheduled(moduleKey);     // invoke + drain
await emu.invokeAction(actionKey, inputs, { asUser });
await emu.openSurface(page, { moduleKey, entry: 'view' | 'edit', theme, layout, asUser, extension });
await emu.hostSave(page);              // dashboard Save for an open edit surface
emu.kvs.snapshot(); emu.log; emu.bridgeLog; emu.harnessMissing;
```

**I3 — the public contract** (WP3 implements, WP2 probes): FORGE-CONTRACT.md hooks only. WP2 never probes a hook
the contract does not name; WP3 never relies on harness internals.

**I4 — mutants** (WP3 → WP2): `forge/mutants/<id>.patch` against `bench/golden-forge` plus `<id>.expect.json`
`{"loses": [check ids], "critical": bool, "max_final": n}`, authored from THIS design's check table, not from
scorer output. `forge_controls.py` applies, rebuilds, scores, compares.

**I5 — evidence** (WP2 internal): `forge-observations.json` sections `lint`, `build`, `phases.{backfill, live,
heal, rerun}.{calls, kvs, deliveries}`, `rovo`, `ui.{widget, edit, sprintAction}`, `shots`, `harnessMissing`.

### 13.3 Order

Day 0, in parallel: WP1 starts from the spike (`forge-proxy.cjs`, `emulator.cjs`, `prove-frontend.cjs`); WP2
writes the registry, oracle and composition against a hand-made pack following I1 (replaced by WP1's generator
when it lands); WP3 builds the golden from `forge/public/` and the installed packages, testing with the spike
harness until WP1's `forge-dev` lands — WP3 must NOT read `forge/site`, `bench/score_forge.py` or
`forge_oracle.py` (it is the proof the public contract suffices). Then: WP1 kit + site → WP3 golden green on
`forge-dev` → WP2 scores the golden → controls → freeze.

### 13.4 Freeze gate (all must hold; `score_forge.py --reference` enforces 1–4)

1. Golden 1.000 on three seeds: every non-calibration check ≥ 0.95 and in fact 1.0, every critical exactly 1.0,
   no `unavailable`, no `harness_missing`, E gate open, every contract hook produced non-vacuous evidence.
2. Severity selftest passes (§8.6).
3. Lint double-run identical on the golden and on every mutant.
4. Thresholds file frozen with receipts (golden ×5 seeds) and its sha pinned in score_forge.py
   (`CALIB_SHA256`, the SB mechanism).
5. One-defect mutants (§13.5) each lose exactly their declared rows (± ROOT_BLOCKS attribution) and nothing else.
6. Empty-starter and one-function controls: scored, not refused, final ≤ 0.05.
7. Entrant-path control: one real run started from the Benchmark view (gate 3) with GPT-6 Luna — receipts for
   §11 and for the fence (§4) — before anything is published.
8. Second independent mini-app (`bench/golden-forge-alt/`, WP3, built by a separate agent session that never saw
   `golden-forge`, choosing different endpoints where the OpenAPI offers them — e.g. `?expand=changelog` and
   `/sprint/{id}/issue` instead of the bulk endpoints, asApp + `/permissions/check` instead of asUser) scores
   ≥ 0.95 with zero `harness_missing`.
9. A G3 control: the golden with the widget's board read from app storage instead of `extension.config` fails
   `u_widget_edit_config` (the second widget instance).

### 13.5 One-defect mutants (WP3)

Phase-stopping mutants (`m_storage_api`, `m_old_search`, `m_abs_assets`, `m_gadget`) list the rows this table
names; WP2's ROOT_BLOCKS must attribute every downstream row of the broken phase, and `forge_controls.py` accepts
exactly those attributions. The probe's double click (`m_double_post`) is two clicks dispatched to the same
element handle, so a button whose label changes width while posting cannot dodge it (WP3 measured that a
pointer double click can land on the container).

| id | defect | expected loss |
|---|---|---|
| `m_storage_api` | `import { storage } from '@forge/api'` (storage:app declared) | measured 2026-10-02: lint gives a WARNING only (`deprecated-api-storage`) — so `l_lint_warnings`, `k_current_apis`, and the runtime T/R/S rows (8.2.0 has no `storage`, the call throws); no lint cap |
| `m_runtime18` | `nodejs18.x` | `l_deployable`, ≤ 0.499 |
| `m_old_search` | `/rest/api/3/search` + `startAt` | `r_backfill_complete` (crit), `r_pagination`, `k_current_apis` |
| `m_ids_only` | `/search/jql` without `fields` | backfill rows/numbers |
| `m_open_sprints_only` | reconcile JQL `sprint in openSprints()` | `r_removals_found`, `r_backfill_complete` (crit 0.8) |
| `m_dedupe_event_id` | idempotency on queue `eventId` | `t_no_double_count` (crit) |
| `m_one_estimate_field` | one hard-coded estimate field | `t_reestimate_followed`, `u_widget_numbers`, `a_action_result` |
| `m_retry_now` | immediate retry on 429 | `t_retry_after_honoured`, `r_rate_limit` |
| `m_asapp_ui` | ledger resolver and action read Jira `asApp` | `b_no_permission_leak` (crit), `b_hidden_count`, `a_action_permissions` |
| `m_double_post` | no double-click guard | `b_comment_exactly_once` (crit) |
| `m_string_comment` | plain-string comment body | `b_comment_adf_as_user`, `u_comment_flow`, `b_comment_exactly_once` (no comment lands) |
| `m_gadget` | `jira:dashboardGadget` instead of `dashboards:widget` | K, U widget rows, ≤ 0.799 |
| `m_config_resolver` | edit saves config through a resolver | `k_widget_edit_bridge`, `u_widget_edit_config`, ≤ 0.799 |
| `m_no_theme` | no `theme.enable()`, hard-coded white | `v_theme_tokens`, `v_dark_mode`, ≤ 0.799 |
| `m_abs_assets` | `/assets/…` absolute paths | `u_widget_loads` (crit), `v_csp_clean` |
| `m_skill_name` | SKILL.md `name` ≠ directory | MEASURED by WP3 with the client-side lint: ERROR "Skill sprint-scope-analyst frontmatter field 'name' must match the parent directory name" → `l_deployable` (crit) + `k_rovo_skill`, ≤ 0.499 |
| `m_config_in_kvs` | widget board stored in KVS by a resolver, view ignores `extension.config` | `u_widget_edit_config` (second instance), `k_widget_edit_bridge` |
| `m_throw_on_429` | consumer throws on 429 instead of a retry request | `t_retry_after_honoured` (rows still land) |

## 14. Check ↔ contract map (measured is stated)

`test_score_forge.py` holds this map and fails when a check has no contract anchor or a contract hook has no
check. Anchors are FORGE-CONTRACT.md sections unless marked P (prompt) or S (STARTER).

| checks | anchor |
|---|---|
| `l_deployable`, `l_lint_warnings` | P done 1, S `npm run lint` |
| `l_bundles_load`, `l_real_packages`, `l_manifest_rules` | P done 1, §8 bundled from `src/`, S packages |
| `l_scopes` | §2 "Request only the scopes your calls need: per call, the OAuth2 scopes the shipped OpenAPI lists…", P done 2 |
| `k_dashboard_widget`, `k_widget_edit_bridge` | §2 table, §4 "dashboards widget edit API" |
| `k_rovo_skill`, `a_skill_instructions` | §2, §6 SKILL.md paragraph |
| `k_current_apis`, `k_consumer_shape` | §2 table (consumer, Custom UI only), P done 2; platform facts discoverable (§3 rule 3) |
| `k_entity_declared`, `s_entity_index_used`, `s_index_order` | §2 storage paragraph |
| `t_trigger_handoff` | §2 trigger/consumer rows, §3 "Updates that touch neither… (KVS reads are fine)" and "runs the scheduled job once before" |
| `t_event_rows`, `t_no_double_count`, `t_out_of_order`, `t_multi_sprint_parse` | §1 change, §3 exactly-once bullet |
| `t_reestimate_followed` | §1 estimate (sprint's `originBoardId` board field), §3 "estimate changes move the numbers" |
| `t_retry_after_honoured`, `r_rate_limit` | §3 429 bullet, §8 redelivery schedule and "a wait inside an invocation is real time" |
| `t_no_user_in_async`, `r_as_app` | §3 asApp bullet |
| `r_backfill_complete`, `r_removals_found` | §3 first bullet |
| `r_heal_dropped`, `e_idempotent_rerun` | §3 `source` bullet ("event work may also record other changes of the issue it reads"), "Later scheduled runs…" |
| `r_pagination` | P done 3; S dev-site paragraph (bulk endpoints, pagination, JQL subset, ISO dates) |
| `r_completes_in_timeout` | §8 "with the platform's timeouts" |
| `s_storage_scope`, `s_limits` | §2 storage, platform limits discoverable |
| `b_invoke_contract` | §6 errors, P done 7 |
| `b_no_permission_leak`, `b_hidden_count`, `a_action_permissions` | §1 last paragraph, §5 hidden-count, §6 |
| `b_comment_adf_as_user`, `b_comment_exactly_once`, `u_comment_flow` | §5 post-summary bullet (`Retry-After` ≤ 5 s on this path) |
| `u_widget_loads`, `u_widget_numbers`, `u_widget_chart`, `u_widget_edit_config` | §4 (`onProductSave` receives the last `updateConfig` value; ties by sprint id) |
| `u_ledger_table`, `u_ledger_sort`, `u_issue_router`, `u_modal_close`, `u_not_started` | §5 (order, numeric ids, reverse toggle, "active and future sprints", optional close) |
| `v_theme_tokens`, `v_dark_mode`, `v_csp_clean`, `v_console_clean` | §7 (disabled controls exempt) |
| `v_widget_sizes` | §4 last bullet (numbers never clipped; names may ellipsize) |
| `a_action_result`, `a_action_errors` | §6 |
| `e_reconcile_economy`, `e_event_economy`, `e_ui_round_trips` | P "Score bands": "A small excellence share rewards reaching Jira in few calls and rendering each surface in few round trips." (the ratios themselves are not published: they are calibration-owned) |
| bands | P "Score bands" (parity test pins the four maxima) |
| 150 calls | P budget sentence (bench_budget `stated_budgets` test) |

## 15. Risks (confidence that the design holds as written)

| # | risk | confidence | mitigation |
|---|---|---|---|
| R1 | The fence + provider relay: goose's provider calls through `HTTPS_PROXY`, nothing else goose needs (tokenizer download, session title call) breaks | MEDIUM — fence measured, relay not | preflight refuses; one cheap Luna call measures it before any build work depends on it (WP1 first task) |
| R2 | Emulator fidelity of the newest surfaces: the dashboards host protocol is inferred from `@forge/dashboards-bridge` 2.0.0 and `@forge/hooks` 2.0.0 sources (no host docs); the Rovo action event shape and async retry policy need doc quotes | MEDIUM-LOW | WP1 freezes each from package source + a quoted docs sentence; a mismatch found later changes the emulator, never the contract; the golden is built from public material only |
| R3 | Too hard for 150 calls: every model clusters below 0.5 and the top does not discriminate | LOW-MEDIUM that a frontier model completes everything | bands + partial credit spread scores; the first Luna and one frontier run decide; if the frontier lands < 0.3, cut the chart and the Rovo skill (−2 checks each), never the pipeline |
| R4 | Supply: the wrapper's timestamped CDN asset may disappear; OpenAPI files' licence for shipping; npm availability at first kit materialisation | MEDIUM | sha-pinned cache survives on each host; re-pin is one command; owner decides the OpenAPI shipping (fallback: fetch at kit time by sha) |
| R5 | Mock coverage: a valid Jira call or JQL the site does not model zeroes an honest app | MEDIUM | OpenAPI-driven 501 + `harness_missing` → the verdict is HELD and rescored after the gap is modelled, never voided or zeroed; WP1 models the documented alternatives per data need (§5.3) and the independent mini-app proves coverage (§13.4 item 8) |

Lower: Custom UI CSP header exactness (WP1 transcribes it; an over-strict header would fail an honest app —
the golden and a second independent mini-app must pass); scoring wall time if an app declares
`timeoutSeconds: 900` and blocks (platform-faithful; bounded by the script's ~40 deliveries).

## 16. Not in v1 (kept for revival, with the reason)

Forge SQL (needs a MySQL/TiDB engine); UI Kit (1.2 GB closure, no Atlassian renderer offline); `jira:jqlFunction`
(handler contract not captured); `apiRoute`, `global:fullPage`, `jira:command`, `objectStore`, app-managed
permissions (handler/host contracts unmeasured — candidates for forge-1.1 once RESEARCH covers them); webtrigger
with `hmacSharedSecret` (docs unconfirmed); `rovo:mcp`, `fifoConsumer`, `global:ui`, `dashboards:filter` (EAP or
undocumented, RESEARCH §5 "never demand"); Confluence (a second mock product; RESEARCH candidate C).

## 17. Red-team log (three independent red-teams on ac779fabd, verdict FIX-THEN-BUILD; 2026-10-02)

Every finding was checked against the primary source it cites before applying. 25 findings (P1–P7, G1–G9, E1–E7, two minor dates; G1 is P3 and E2 is G2): 25 applied, 0 rejected.

| finding | verified against | result |
|---|---|---|
| P1 `onProductSave` stores only its return; `null` stores nothing | dashboard-widget page (`return null; // return config to opt in to in-product save`), Dashboard UI bridge page (`return config; // Return config to save in product`) | applied §6.4, contract §4 (no-handler case stated) |
| P2 edit/layout events, new config from `getContext`, `filters: null` | @forge/hooks 2.0.0 `useWidgetConfig.js`, `useWidgetContext.js` (read 2026-10-02) | applied §6.4 |
| P3/G1 scope rule must ignore `basicAuth`/`{}` alternatives | shipped jira.json: `/search/jql` security `[{basicAuth}, {OAuth2:[read:jira-work]}, {}]`; `x-atlassian-oauth2-scopes` Current/Beta sets | applied §5.3, `l_scopes` |
| P4 no documented retry maximum | async-events page: retention 24 h, "exponential backoff … up to approximately 15 minutes" | applied §6.3, contract §8; a throw grades 0 on `t_retry_after_honoured` |
| P5 inline CSS needs a custom CSP; decide on relaxations | custom-options page "Inline styles … custom content security policies" | applied contract §7, §6.4, `l_scopes` (styles free, scripts extra) |
| P6 `asUser` in Rovo actions undocumented | rovo-action page: context carries the user's `accountId`; no `asUser` | applied: outcome graded, asApp + `/permissions/check` allowed |
| P7 `source` of a dropped change | design logic (a consumer reading the whole changelog records it first) | applied: oracle accepts `event` or `reconcile` for dropped changes |
| minor: dashboards-bridge publish time; gadget deprecation date | npm `time["2.0.0"]` 2026-09-28T02:45:05Z; CDAC 102826 "will be deprecated by 17 May 2027" | applied §1, §2.2 |
| G2 creep rounding/format | — (unstated rule) | applied contract §1/§6, oracle `Decimal` ROUND_HALF_UP |
| G3 config via KVS passes the edit check | design logic | applied: second widget instance + mutant `m_config_in_kvs` + freeze item 9 |
| G4 `storage` import is a warning | MEASURED 2026-10-02, spike `lint-offline.cjs`: `{"errors":0,"warnings":1}` `deprecated-api-storage` | applied §13.5 |
| G5 vacuous "no defect" rows pay an idle app | design arithmetic (~0.08 earned) | applied: precondition column §8.2, selftest (6), freeze item 6 |
| G6 leak matching false positives | design logic (`OPS-12` ⊂ `OPS-120`) | applied `b_no_permission_leak` |
| G7 function forks could read the seed / hit the site | MEASURED 2026-10-02 deny-default `sandbox-exec`: secret read EPERM, exec EPERM, proxy port 200, other port EPERM, internet ENOTFOUND | applied §6.1 |
| G8 unmodelled calls must not void a run | design logic + gate 1 | applied: `status: held` + rescore, alternatives list §5.3, mini-app freeze item 8 |
| G9 links, backdrop, tie-break unstated | contract text | applied contract §5/§7, `v_theme_tokens`, `v_dark_mode`, `u_ledger_table` |
| E1 keys and field names fixed though §8 says they differ | contract §8 vs §5.2 | applied §5.2: project keys and field-to-board seeded, cross-field decoys |
| E2 = G2 | — | applied with G2 |
| E3 comment on 429 | contract §5 | applied contract §5, `b_comment_exactly_once` |
| E4 change key | contract §1/§6 | applied: changelog id + sprint |
| E5 time format | contract §5/§6 | applied: ISO-8601 with offset, compared as instants |
| E6 dev-kit gaps (`--sprint`, `--config`, host Save, `reset`, resolver keys, backgrounding) | STARTER.md | applied STARTER.md, §6.5 |
| E7 wording, guard $50, pinned reasoning effort, Astra an assumption | prompt, §11 | applied |

Public input after the pass: 15,504 bytes (prompt 2,942 + contract 9,789 + starter 2,773), under the 25 KB target.

### 17.1 WP3's contract gaps (bench/golden-forge/CONTRACT-GAPS.md, 2026-10-02)

The golden was built from the public files only; each gap is resolved by STATING the rule (S), by declaring it
the entrant's judgement with grading that accepts every reasonable reading (J), or by routing a harness defect to
its package (R). Public input after this pass: 16,850 bytes.

| # | gap | resolution |
|---|---|---|
| 0a | site serves changelog `created` as epoch ms | R WP1: types as the OpenAPI declares (ISO-8601), §5.3; S STARTER "dates are ISO-8601 strings" |
| 0b | KVS `integer` is 32-bit; error names only the type | R WP1: error names the range, §5.3 (the documented rule stays a real-platform fact) |
| 0c | `/issue/{id}/changelog/list` unmodelled | R WP1: modelled, §5.3 |
| 0d | real wrapper throws on `InvocationError` without `retryData` when `timeoutSeconds` > 55 | J: real platform behaviour, reproduced by forge-dev (same wrapper); not stated |
| 0e | concurrent `forge-dev serve` corrupts `.forge-dev/state.json` | R WP1: atomic state writes (temp + rename) |
| 1 | JQL subset and endpoints | S STARTER: JQL fields/operators/functions named; REST v3 + Jira Software incl. bulk endpoints |
| 2 | what `onProductSave` receives | S §4: the last `updateConfig` value (stored config if none) |
| 3 | Retry-After longer than a resolver can wait | S §5: at most 5 s on the comment path |
| 4 | how a wait inside an invocation is measured | S §8: clock runs with real time, jumps only over redelivery waits |
| 5 | trigger work before config exists | S §3: KVS reads allowed; the scheduled job runs before any update |
| 6 | event path recording sibling changes | S §3 + J: allowed, `source` = `event`; oracle accepts either for those rows |
| 7 | which board's estimation field | S §1: the sprint's `originBoardId` board |
| 8 | sorting edge cases | S §5: reverse toggle, ascending after another sort, numeric ids, points ties in default order |
| 9 | ellipsis vs clipping | S §4: numbers never clipped or truncated; names may ellipsize |
| 10 | disabled-control contrast | S §7: exempt |
| 11 | scopes lint cannot check | S §2: OpenAPI OAuth2 scopes per call (classic else full granular set) |
| 12 | sprint action on a closed sprint | S §5: the harness opens active and future sprints only |
| 13 | close on the not-started screen | S §5: optional; `u_not_started` accepts it |
| 14 | Rovo action on a future sprint / non-numeric id | J: not graded beyond §6's unknown/missing rule (a non-numeric id is "unknown") |
| 15 | router `open` vs `navigate` | J: both accepted, any `type` (`u_issue_router`) |
| 16 | post-summary with nothing selected | J: not graded (the probe always selects first) |
| 17 | rerun after a sprint rename | J: the script renames nothing; `e_idempotent_rerun` measures only unchanged reruns |
| 18 | `selfGenerated` updates | J: the app writes no issue fields, so the harness generates none |
| 19 | equal sprint `startDate` | S §4 ties by sprint id; fixtures avoid equal dates (§5.2) |
| 20 | change exactly at `startDate` | S §1 "strictly after"; fixtures avoid it (§5.2) |
| 21 | `npm run lint` script in the starter | R WP1: `forge/starter/package.json` `"lint": "node $FORGE_KIT/bin/lint.cjs ."` |
| 22 | `m_skill_name` is a lint ERROR | applied §13.5: `l_deployable`, ≤ 0.499 |
| 23 | phase-stopping mutants need ROOT_BLOCKS attribution; extra rows for two mutants | applied §13.5 note + rows (R WP2: attribution) |
| 24 | double click dodged by a label that moves the button | R WP2: the probe clicks the same element handle twice (§13.5 note) |

